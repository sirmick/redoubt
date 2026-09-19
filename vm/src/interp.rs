//! The interpreter: executes one process's instructions until it yields, waits or ends.
//!
//! Each instruction is one arm of the `match` in [`step`], in the operand order of the compiler's
//! `genop.tab`. The loader has already validated table indices and labels. Anything else that
//! could be wrong with the code (a Y register outside the frame, an operand of the wrong kind) is
//! a [`Fault::BadCode`], which ends the process that ran it; it never panics the VM.

use alloc::rc::Rc;
use alloc::vec::Vec;
use core::cmp::Ordering;

use crate::bif::{Ctx, Native};
use crate::bits::{self, Builder};
use crate::loader::{MAX_Y_REGS, X_REGS};
use crate::module::{Arg, Instr, Module};
use crate::opcodes as op;
use crate::process::{Class, Cp, Exception, Frame, Handler, Process};
use crate::term::{Bits, FunView, Heap, Term};
use crate::vm::{System, Target};

pub enum Stop {
    /// Out of reductions; run again later.
    Yield,
    /// Blocked in `receive`.
    Wait,
    /// The process ended: the value its first function returned, or what killed it.
    Exit(Result<Term, Exception>),
}

/// Why an instruction could not complete.
pub enum Fault {
    /// An Erlang exception, which the process may catch.
    Raise(Exception),
    /// The code broke a rule the compiler never breaks. Ends the process; cannot be caught.
    BadCode(&'static str),
    /// The process exceeded a resource limit (its stack). Ends the process; cannot be caught.
    Limit(&'static str),
}

impl From<Exception> for Fault {
    fn from(e: Exception) -> Self {
        Fault::Raise(e)
    }
}

type R<T = ()> = Result<T, Fault>;

/// What an instruction did to control flow, beyond falling through to the next one.
enum Flow {
    Next,
    Stop(Stop),
}

/// Instructions a process may execute in one time slice, whatever it calls. Compiled Erlang
/// only loops through calls, which count as reductions, but corrupt code can loop with plain
/// jumps; this bound keeps such code from holding the scheduler forever.
pub const MAX_INSTRUCTIONS_PER_SLICE: usize = 200_000;

pub fn run(sys: &mut System, p: &mut Process) -> Stop {
    let mut instructions = 0usize;
    let mut module = p.pc.module.clone();
    loop {
        instructions += 1;
        if instructions > MAX_INSTRUCTIONS_PER_SLICE {
            return Stop::Yield;
        }
        // Hold the current module here, so fetching an instruction costs no reference-count
        // traffic; refresh it only when a call or return has moved to another module.
        if !Rc::ptr_eq(&module, &p.pc.module) {
            module = p.pc.module.clone();
            // Code loaded since the heap last looked brings literal chunks it must see.
            p.refresh(&sys.literals);
        }
        // Between instructions every term the process holds is a root, so this is a safe point.
        p.maybe_collect();
        let result = step(sys, p, &module);
        if let Some(reason) = p.pending_exit.take() {
            return Stop::Exit(Err(Exception::exit(reason)));
        }
        match result {
            Ok(Flow::Next) => {}
            Ok(Flow::Stop(s)) => return s,
            Err(Fault::Raise(e)) => {
                if let Some(stop) = raise(sys, p, e) {
                    return stop;
                }
            }
            Err(Fault::BadCode(what)) => {
                // Name the offending instruction, so a report points at the code.
                let op =
                    p.pc.pc
                        .checked_sub(1)
                        .and_then(|pc| p.pc.module.code.get(pc as usize))
                        .map(|i| i.op);
                let name = crate::opcodes::OPCODES[op.unwrap_or(0) as usize].name;
                let parts = [
                    Term::Atom(sys.atom("bad_code")),
                    Term::Atom(sys.atom(what)),
                    Term::Atom(p.pc.module.name),
                    Term::Atom(sys.atom(name)),
                ];
                let reason = p.heap.tuple(&parts);
                return Stop::Exit(Err(Exception::error(reason)));
            }
            Err(Fault::Limit(what)) => {
                let parts = [
                    Term::Atom(sys.atoms.system_limit),
                    Term::Atom(sys.atom(what)),
                ];
                let reason = p.heap.tuple(&parts);
                return Stop::Exit(Err(Exception::exit(reason)));
            }
        }
    }
}

// ---- operands ----

fn arg(ins: &Instr, i: usize) -> R<&Arg> {
    ins.args.get(i).ok_or(Fault::BadCode("missing operand"))
}

fn u(ins: &Instr, i: usize) -> R<usize> {
    match arg(ins, i)? {
        Arg::U(n) => usize::try_from(*n).map_err(|_| Fault::BadCode("operand too large")),
        _ => Err(Fault::BadCode("expected a number")),
    }
}

fn label(ins: &Instr, i: usize) -> R<Option<u32>> {
    match arg(ins, i)? {
        Arg::Label(l) => Ok(*l),
        _ => Err(Fault::BadCode("expected a label")),
    }
}

fn list(ins: &Instr, i: usize) -> R<&[Arg]> {
    match arg(ins, i)? {
        Arg::List(items) => Ok(items),
        _ => Err(Fault::BadCode("expected a list")),
    }
}

fn y_slot(p: &Process, y: u16) -> R<usize> {
    let f = p
        .frames
        .last()
        .ok_or(Fault::BadCode("Y register without a frame"))?;
    if (y as usize) < f.size {
        Ok(f.base + y as usize)
    } else {
        Err(Fault::BadCode("Y register outside the frame"))
    }
}

fn get(p: &Process, a: &Arg) -> R<Term> {
    match a {
        Arg::X(x) => Ok(p.x[*x as usize]),
        Arg::Y(y) => Ok(p.stack[y_slot(p, *y)?]),
        Arg::Const(t) => Ok(*t),
        _ => Err(Fault::BadCode("expected a source operand")),
    }
}

fn put(p: &mut Process, a: &Arg, t: Term) -> R {
    match a {
        Arg::X(x) => p.x[*x as usize] = t,
        Arg::Y(y) => {
            let i = y_slot(p, *y)?;
            p.stack[i] = t;
        }
        _ => return Err(Fault::BadCode("expected a destination register")),
    }
    Ok(())
}

fn src(p: &Process, ins: &Instr, i: usize) -> R<Term> {
    get(p, arg(ins, i)?)
}

/// A source operand (terms are copied freely: they are small values).
fn val(p: &Process, ins: &Instr, i: usize) -> R<Term> {
    src(p, ins, i)
}

fn dst(p: &mut Process, ins: &Instr, i: usize, t: Term) -> R {
    put(p, arg(ins, i)?, t)
}

/// A number that may be encoded either as an unsigned literal or as a source operand.
fn num_operand(p: &mut Process, a: &Arg) -> R<Term> {
    match a {
        Arg::U(n) => Ok(p.heap.from_i128(*n as i128)),
        _ => get(p, a),
    }
}

fn freg(ins: &Instr, i: usize) -> R<usize> {
    match arg(ins, i)? {
        Arg::FloatReg(r) => Ok(*r as usize),
        _ => Err(Fault::BadCode("expected a float register")),
    }
}

fn jump(p: &mut Process, target: Option<u32>) -> R {
    match target {
        Some(pc) => {
            p.pc.pc = pc;
            Ok(())
        }
        None => Err(Fault::BadCode("jump to no label")),
    }
}

/// A match context seen from outside the match: the bits not yet matched. BEAM lets code pass a
/// context to BIFs and type tests, which treat it this way (in OTP 28 a context *is* a
/// sub-bitstring whose start advances as matching proceeds).
fn as_value(heap: &mut Heap, t: Term) -> Term {
    match heap.as_match(t) {
        Some((bits, pos)) => {
            let b = heap.as_bits(bits).expect("a match state holds bits");
            heap.bits(b.slice(pos, b.len - pos))
        }
        None => t,
    }
}

/// The bits of a match state and its position.
fn match_bits(heap: &Heap, state: Term) -> Option<(Bits, usize)> {
    let (bits, pos) = heap.as_match(state)?;
    Some((heap.as_bits(bits)?, pos))
}

fn error_tuple(heap: &mut Heap, tag: &crate::atom::Atom, value: Term) -> Exception {
    Exception::error(heap.tuple(&[Term::Atom(*tag), value]))
}

// ---- calls, frames, returns ----

/// Count one reduction. Returns `true` when the time slice is used up.
fn reduce(p: &mut Process) -> bool {
    p.reductions += 1;
    p.budget = p.budget.saturating_sub(1);
    p.budget == 0
}

fn call_code(p: &mut Process, target: Cp, save_return: bool) -> Flow {
    if save_return {
        p.cp = Some(p.pc.clone());
    }
    p.pc = target;
    if reduce(p) {
        Flow::Stop(Stop::Yield)
    } else {
        Flow::Next
    }
}

fn do_return(p: &mut Process) -> Flow {
    match p.cp.take() {
        Some(cp) => {
            p.pc = cp;
            Flow::Next
        }
        None => Flow::Stop(Stop::Exit(Ok(p.x[0]))),
    }
}

fn allocate(p: &mut Process, size: usize, max_stack: usize) -> R {
    if size > MAX_Y_REGS {
        return Err(Fault::BadCode("frame too large"));
    }
    // Every frame counts, even one with no Y registers, so `allocate 0` in a loop is bounded too.
    if p.stack.len() + p.frames.len() + size > max_stack {
        return Err(Fault::Limit("stack"));
    }
    let base = p.stack.len();
    p.stack.resize(base + size, Term::Nil);
    p.frames.push(Frame {
        base,
        size,
        cp: p.cp.take(),
    });
    Ok(())
}

fn deallocate(p: &mut Process) -> R {
    let f = p
        .frames
        .pop()
        .ok_or(Fault::BadCode("deallocate without a frame"))?;
    p.stack.truncate(f.base);
    p.cp = f.cp;
    // A handler whose frame is gone can never be reached again.
    let depth = p.frames.len();
    p.handlers.retain(|h| h.depth <= depth);
    Ok(())
}

/// Call a native with `args`. If it fails, the native and its arguments head the stack trace,
/// as in BEAM (`{erlang,element,[5,foo],[]}`), except for the natives whose whole purpose is to
/// raise (`error/1` and friends), which BEAM leaves out.
fn run_native(
    sys: &mut System,
    p: &mut Process,
    n: Native,
    (m, f): (&crate::atom::Atom, &crate::atom::Atom),
    args: &[Term],
) -> R<Term> {
    let result = n(&mut Ctx { sys, p }, args);
    // A native may have made literals (`persistent_term:put`) or loaded code whose literals it
    // returns.
    p.refresh(&sys.literals);
    match result {
        Ok(t) => Ok(t),
        Err(mut e) => {
            let raiser = m == &sys.atoms.erlang
                && matches!(
                    f.as_str(),
                    "error" | "exit" | "throw" | "raise" | "nif_error"
                );
            if e.trace.is_none() && !raiser {
                // As BEAM does for its BIFs, name the module that can explain the error
                // (`erl_error` and Elixir turn `badarg` into "not a list" and the like).
                let location = match (e.class, error_formatter(m.as_str())) {
                    (crate::process::Class::Error, Some(formatter)) => {
                        let mut pairs = alloc::vec![(
                            Term::Atom(sys.atom("module")),
                            Term::Atom(sys.atom(formatter))
                        )];
                        if let Some(cause) = e.cause.take() {
                            pairs.push((Term::Atom(sys.atom("cause")), cause));
                        }
                        let error_info = Term::Atom(sys.atom("error_info"));
                        let h = &mut p.heap;
                        let info = h.map_from(pairs);
                        let item = h.tuple(&[error_info, info]);
                        h.list([item])
                    }
                    _ => Term::Nil,
                };
                let h = &mut p.heap;
                let args = h.list(args.iter().copied());
                let top = h.tuple(&[Term::Atom(*m), Term::Atom(*f), args, location]);
                let rest = stacktrace(sys, p, None);
                e.trace = Some(p.heap.cons(top, rest));
            }
            Err(Fault::Raise(e))
        }
    }
}

/// The module whose `format_error/2` explains errors from a native in `module`, as BEAM's
/// `beam_common.c` assigns them.
fn error_formatter(module: &str) -> Option<&'static str> {
    Some(match module {
        "erlang" | "erts_internal" | "atomics" | "counters" | "persistent_term" => {
            "erl_erts_errors"
        }
        "code" | "os" => "erl_kernel_errors",
        "binary" | "ets" | "lists" | "maps" | "math" | "re" | "unicode" => "erl_stdlib_errors",
        _ => return None,
    })
}

/// Call a native on x0.. and return its result.
fn call_native(
    sys: &mut System,
    p: &mut Process,
    n: Native,
    mf: (&crate::atom::Atom, &crate::atom::Atom),
    arity: usize,
) -> R<Term> {
    let mut args = [Term::Nil; 255];
    for (i, slot) in args.iter_mut().enumerate().take(arity) {
        *slot = as_value(&mut p.heap, p.x[i]);
    }
    run_native(sys, p, n, mf, &args[..arity])
}

/// The code and x registers for calling `fun` (a term of `heap`) with `args`.
pub(crate) fn fun_entry(
    sys: &mut System,
    heap: &mut Heap,
    fun: Term,
    mut args: Vec<Term>,
) -> Result<(Cp, Vec<Term>), Exception> {
    let Some(f) = heap.as_fun(fun) else {
        return Err(error_tuple(heap, &sys.atoms.badfun, fun));
    };
    if f.arity() as usize != args.len() {
        let args = heap.list(args);
        let info = heap.tuple(&[fun, args]);
        return Err(error_tuple(heap, &sys.atoms.badarity, info));
    }
    match f {
        FunView::Local {
            module,
            index,
            env,
            uniq,
            arity,
            ..
        } => {
            let env = env.to_vec();
            let Some(m) = sys.module(&module) else {
                return Err(Exception::error(Term::Atom(sys.atoms.undef)));
            };
            // A fun from another version of the module (or decoded from a binary) must match
            // this version's fun table, or it is a bad fun.
            let entry = m.funs.get(index as usize).filter(|e| {
                e.uniq == uniq && e.num_free as usize == env.len() && e.arity == arity + e.num_free
            });
            let Some(entry) = entry.map(|e| e.entry) else {
                return Err(error_tuple(heap, &sys.atoms.badfun, fun));
            };
            args.extend(env);
            Ok((
                Cp {
                    module: m,
                    pc: entry,
                },
                args,
            ))
        }
        FunView::Export {
            module,
            function,
            arity,
        } => match sys.resolve(&module, &function, arity) {
            Some(Target::Code(cp)) => Ok((cp, args)),
            _ => Err(Exception::error(Term::Atom(sys.atoms.undef))),
        },
    }
}

/// How a call continues: `Call` saves a return address, `Last` deallocates the frame first,
/// `Only` is a tail call from a function without a frame.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Call,
    Last,
    Only,
}

/// Enter `module:function/arity` with arguments already in x registers. `native` is the
/// implementation if the caller already knows it is a native (resolved at load time).
fn call_mfa(
    sys: &mut System,
    p: &mut Process,
    m: &crate::atom::Atom,
    f: &crate::atom::Atom,
    arity: usize,
    kind: Kind,
) -> R<Flow> {
    call_mfa_with(sys, p, m, f, arity, kind, None)
}

fn call_mfa_with(
    sys: &mut System,
    p: &mut Process,
    m: &crate::atom::Atom,
    f: &crate::atom::Atom,
    arity: usize,
    kind: Kind,
    native: Option<Native>,
) -> R<Flow> {
    if arity > 255 {
        return Err(Fault::BadCode("arity above 255"));
    }
    // erlang:apply/2,3 and erlang:hibernate/0,3 are control flow, not ordinary natives.
    if m == &sys.atoms.erlang && f.as_str() == "apply" && (arity == 2 || arity == 3) {
        return apply(sys, p, arity, kind);
    }
    if m == &sys.atoms.erlang && f.as_str() == "hibernate" && (arity == 0 || arity == 3) {
        return hibernate(sys, p, arity, kind);
    }
    // erlang:call_on_load_function(Module) calls the module's on_load function, which need not
    // be exported (for beamlet_code, as BEAM's code server uses it).
    if m == &sys.atoms.erlang && f.as_str() == "call_on_load_function" && arity == 1 {
        let entry = match p.x[0] {
            Term::Atom(name) => sys
                .module(&name)
                .and_then(|md| md.on_load_entry().map(|(_, pc)| Cp { module: md, pc })),
            _ => None,
        };
        let entry =
            entry.ok_or_else(|| Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg))))?;
        if kind == Kind::Last {
            deallocate(p)?;
        }
        return Ok(call_code(p, entry, kind == Kind::Call));
    }
    // code:load_binary/3 of a module with an on_load function continues in Erlang, which runs
    // it and unloads the module again if it fails.
    if m.as_str() == "code" && f.as_str() == "load_binary" && arity == 3 {
        let args: Vec<Term> = p.x[..3].to_vec();
        let loaded = run_native(sys, p, crate::bif::load_binary, (m, f), &args)?;
        let on_load = match p.heap.as_tuple(loaded) {
            Some(&[_, Term::Atom(name)]) => sys
                .module(&name)
                .and_then(|md| md.on_load())
                .map(|f| (name, f)),
            _ => None,
        };
        let Some((module, function)) = on_load else {
            p.x[0] = loaded;
            return Ok(match kind {
                Kind::Call => Flow::Next,
                Kind::Last => {
                    deallocate(p)?;
                    do_return(p)
                }
                Kind::Only => do_return(p),
            });
        };
        p.x[0] = Term::Atom(module);
        p.x[1] = Term::Atom(function);
        let (bm, bf) = (sys.atom("beamlet_code"), sys.atom("run_on_load"));
        return call_mfa(sys, p, &bm, &bf, 2, kind);
    }
    let target = match native {
        Some(n) => Some(Target::Native(n)),
        None => sys.resolve(m, f, arity as u32),
    };
    match target {
        Some(Target::Native(n)) => {
            let r = call_native(sys, p, n, (m, f), arity)?;
            p.x[0] = r;
            let yielded = reduce(p);
            let flow = match kind {
                Kind::Call => Flow::Next,
                Kind::Last => {
                    deallocate(p)?;
                    do_return(p)
                }
                Kind::Only => do_return(p),
            };
            if yielded && matches!(flow, Flow::Next) {
                return Ok(Flow::Stop(Stop::Yield));
            }
            Ok(flow)
        }
        Some(Target::Code(cp)) => {
            if kind == Kind::Last {
                deallocate(p)?;
            }
            Ok(call_code(p, cp, kind == Kind::Call))
        }
        None => {
            // A process with its own error handler gets `Handler:undefined_function(M, F, Args)`
            // instead (Elixir's parallel compiler waits for modules this way). Not when the
            // handler itself is what is missing.
            if let Some(h) = p.error_handler.filter(|h| h != m) {
                let undefined = sys.atom("undefined_function");
                if let Some(Target::Code(cp)) = sys.resolve(&h, &undefined, 3) {
                    let args = p.x[..arity].to_vec();
                    let args = p.heap.list(args);
                    p.x[0] = Term::Atom(*m);
                    p.x[1] = Term::Atom(*f);
                    p.x[2] = args;
                    if kind == Kind::Last {
                        deallocate(p)?;
                    }
                    return Ok(call_code(p, cp, kind == Kind::Call));
                }
            }
            // As in BEAM, the missing function heads the stack trace, with its arguments.
            let args = p.x[..arity].to_vec();
            let args = p.heap.list(args);
            let missing = p
                .heap
                .tuple(&[Term::Atom(*m), Term::Atom(*f), args, Term::Nil]);
            let mut e = Exception::error(Term::Atom(sys.atoms.undef));
            // A tail call has already left the calling function, so the trace starts at its
            // caller (as in BEAM).
            let rest = if kind == Kind::Call {
                stacktrace(sys, p, None)
            } else {
                continuations(sys, p, sys.backtrace_depth)
            };
            e.trace = Some(p.heap.cons(missing, rest));
            Err(Fault::Raise(e))
        }
    }
}

/// `erlang:hibernate()`: wait until the mailbox has a message (any message, consuming none),
/// then return `ok`. `erlang:hibernate(M, F, Args)`: the same, but the call stack is
/// discarded and the process continues with `M:F(Args...)`. (BEAM's loader turns the call in
/// `erlang:hibernate/0`'s own body into this; run literally, that body calls itself forever.)
fn hibernate(sys: &mut System, p: &mut Process, arity: usize, kind: Kind) -> R<Flow> {
    p.save = 0;
    if arity == 0 {
        p.x[0] = Term::Atom(sys.atoms.ok);
        let flow = match kind {
            Kind::Call => Flow::Next,
            Kind::Last => {
                deallocate(p)?;
                do_return(p)
            }
            Kind::Only => do_return(p),
        };
        return Ok(match flow {
            Flow::Next => Flow::Stop(Stop::Wait),
            other => other,
        });
    }
    let badarg = || Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg)));
    let (Term::Atom(m), Term::Atom(f)) = (p.x[0], p.x[1]) else {
        return Err(badarg());
    };
    let args = p
        .heap
        .to_vec(p.x[2])
        .filter(|a| a.len() <= 255)
        .ok_or_else(badarg)?;
    let Some(Target::Code(cp)) = sys.resolve(&m, &f, args.len() as u32) else {
        return Err(Fault::Raise(Exception::error(Term::Atom(sys.atoms.undef))));
    };
    p.stack.clear();
    p.frames.clear();
    p.handlers.clear();
    p.cp = None;
    for (i, a) in args.into_iter().enumerate() {
        p.x[i] = a;
    }
    p.pc = cp;
    Ok(Flow::Stop(Stop::Wait))
}

/// `badarg` for an apply whose module or function is not an atom. As in BEAM, the trace starts
/// with `erlang:apply/3` and its arguments.
fn bad_apply(sys: &mut System, p: &mut Process, m: Term, f: Term, args: Term) -> Fault {
    let mut e = Exception::error(Term::Atom(sys.atoms.badarg));
    let (erlang, apply) = (Term::Atom(sys.atoms.erlang), Term::Atom(sys.atom("apply")));
    let args = p.heap.list([m, f, args]);
    let head = p.heap.tuple(&[erlang, apply, args, Term::Nil]);
    let rest = stacktrace(sys, p, None);
    e.trace = Some(p.heap.cons(head, rest));
    Fault::Raise(e)
}

/// `erlang:apply(Fun, Args)` or `erlang:apply(M, F, Args)` with its arguments in x0..
fn apply(sys: &mut System, p: &mut Process, arity: usize, kind: Kind) -> R<Flow> {
    let args_term = p.x[arity - 1];
    let args = p
        .heap
        .to_vec(args_term)
        .ok_or_else(|| Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg))))?;
    if args.len() > 255 {
        return Err(Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg))));
    }
    if arity == 2 {
        let fun = p.x[0];
        return call_fun(sys, p, fun, args, kind);
    }
    let (Term::Atom(m), Term::Atom(f)) = (p.x[0], p.x[1]) else {
        let (m, f) = (p.x[0], p.x[1]);
        return Err(bad_apply(sys, p, m, f, args_term));
    };
    let n = args.len();
    for (i, a) in args.into_iter().enumerate() {
        p.x[i] = a;
    }
    call_mfa(sys, p, &m, &f, n, kind)
}

fn call_fun(sys: &mut System, p: &mut Process, fun: Term, args: Vec<Term>, kind: Kind) -> R<Flow> {
    // An export fun of a native: call the native directly.
    if let Some(FunView::Export {
        module,
        function,
        arity,
    }) = p.heap.as_fun(fun)
    {
        if arity as usize == args.len() {
            let n = args.len();
            for (i, a) in args.into_iter().enumerate() {
                p.x[i] = a;
            }
            return call_mfa(sys, p, &module, &function, n, kind);
        }
    }
    let (entry, regs) = fun_entry(sys, &mut p.heap, fun, args)?;
    if regs.len() > X_REGS {
        return Err(Fault::BadCode("too many arguments"));
    }
    for (i, a) in regs.into_iter().enumerate() {
        p.x[i] = a;
    }
    if kind == Kind::Last {
        deallocate(p)?;
    }
    Ok(call_code(p, entry, kind == Kind::Call))
}

// ---- exceptions ----

fn class_atom(sys: &System, c: Class) -> Term {
    Term::Atom(match c {
        Class::Error => sys.atoms.error,
        Class::Exit => sys.atoms.exit,
        Class::Throw => sys.atoms.throw,
    })
}

fn class_of(sys: &System, t: &Term) -> Option<Class> {
    let a = &sys.atoms;
    if t.is_atom(&a.error) {
        Some(Class::Error)
    } else if t.is_atom(&a.exit) {
        Some(Class::Exit)
    } else if t.is_atom(&a.throw) {
        Some(Class::Throw)
    } else {
        None
    }
}

/// The stack trace list inside a raw trace `{Class, Trace}`; anything else is already a list.
fn cooked(heap: &Heap, raw: Term) -> Term {
    match heap.as_tuple(raw) {
        Some(&[_, t]) => t,
        _ => raw,
    }
}

/// A trace entry `{M, F, ArityOrArgs, Location}` for code index `pc` of `m`, on `heap`. What it
/// holds comes from the module (atoms, and file names that are literals), so any heap will do.
fn trace_entry(
    atoms: &crate::atom::Atoms,
    heap: &mut Heap,
    m: &Module,
    pc: u32,
    args: Option<Term>,
) -> Option<Term> {
    let f = m.function_at(pc)?;
    let location = match m.location(pc) {
        Some((file, line)) => {
            let file = heap.tuple(&[Term::Atom(atoms.file), *file]);
            let line = heap.tuple(&[Term::Atom(atoms.line), Term::Int(line as i64)]);
            heap.list([file, line])
        }
        None => Term::Nil,
    };
    Some(heap.tuple(&[
        Term::Atom(m.name),
        Term::Atom(f.name),
        args.unwrap_or(Term::Int(f.arity as i64)),
        location,
    ]))
}

/// A stack trace: the current function, then the functions that will be returned to.
fn stacktrace(sys: &mut System, p: &mut Process, args: Option<Term>) -> Term {
    let here = p.pc.pc.saturating_sub(1);
    let module = p.pc.module.clone();
    let head = trace_entry(&sys.atoms, &mut p.heap, &module, here, args);
    let rest = continuations(
        sys,
        p,
        sys.backtrace_depth
            .saturating_sub(usize::from(head.is_some())),
    );
    match head {
        Some(h) => p.heap.cons(h, rest),
        None => rest,
    }
}

/// The stack trace of a native's caller, as an exception raised there would get it.
pub(crate) fn caller_stacktrace(sys: &mut System, p: &mut Process) -> Term {
    stacktrace(sys, p, None)
}

/// Up to `n` trace entries for the functions that will be returned to. Looks at no more frames
/// than entries it keeps: the cost of raising must not grow with the depth of the stack.
fn continuations(sys: &mut System, p: &mut Process, n: usize) -> Term {
    let points = continuation_points(p, n);
    trace_of(&sys.atoms, &mut p.heap, &points)
}

/// The places `p` will return to, innermost first, at most `n`.
fn continuation_points(p: &Process, n: usize) -> Vec<Cp> {
    let conts =
        p.cp.iter()
            .chain(p.frames.iter().rev().take(n).filter_map(|f| f.cp.as_ref()));
    conts.take(n).cloned().collect()
}

fn trace_of(atoms: &crate::atom::Atoms, heap: &mut Heap, points: &[Cp]) -> Term {
    let mut entries = Vec::new();
    for cp in points {
        entries.extend(trace_entry(
            atoms,
            heap,
            &cp.module,
            cp.pc.saturating_sub(1),
            None,
        ));
    }
    heap.list(entries)
}

/// Where `p` is, as text: its current function and up to `depth - 1` callers.
pub(crate) fn where_is(p: &Process, depth: usize) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    let conts = core::iter::once(&p.pc)
        .chain(p.cp.iter())
        .chain(p.frames.iter().rev().filter_map(|f| f.cp.as_ref()));
    for (i, cp) in conts.take(depth).enumerate() {
        let pc = if i == 0 {
            cp.pc
        } else {
            cp.pc.saturating_sub(1)
        };
        if let Some(f) = cp.module.function_at(pc) {
            if !out.is_empty() {
                out.push_str(" < ");
            }
            out.push_str(&alloc::format!(
                "{}:{}/{}",
                cp.module.name.as_str(),
                f.name.as_str(),
                f.arity
            ));
        }
    }
    out
}

/// The places `current_stacktrace` reports for `p`: where it is, then what it will return to,
/// at most `n`.
pub(crate) fn stacktrace_points(p: &Process, n: usize) -> Vec<Cp> {
    let mut points = alloc::vec![Cp {
        module: p.pc.module.clone(),
        pc: p.pc.pc
    }];
    points.extend(continuation_points(p, n.saturating_sub(1)));
    points
}

/// `process_info(P, current_stacktrace)` from [`stacktrace_points`], on `heap`, with source
/// locations.
pub(crate) fn current_stacktrace(
    atoms: &crate::atom::Atoms,
    heap: &mut Heap,
    points: &[Cp],
) -> Term {
    trace_of(atoms, heap, points)
}

/// Transfer control to the innermost handler, or end the process if there is none.
fn raise(sys: &mut System, p: &mut Process, mut e: Exception) -> Option<Stop> {
    if e.trace.is_none() {
        e.trace = Some(stacktrace(sys, p, None));
    }
    let Some(h) = p.handlers.pop() else {
        return Some(Stop::Exit(Err(e)));
    };
    p.frames.truncate(h.depth);
    let top = p.frames.last().map(|f| f.base + f.size).unwrap_or(0);
    p.stack.truncate(top);
    p.cp = None;
    // x2 is the "raw" stack trace: the class travels with it, so `raise` can rethrow it and
    // `build_stacktrace` turns it into the list Erlang code sees.
    let class = class_atom(sys, e.class);
    p.x[0] = class;
    p.x[1] = e.reason;
    p.x[2] = p.heap.tuple(&[class, e.trace.unwrap_or(Term::Nil)]);
    p.in_exception = true;
    p.pc = h.target;
    None
}

fn install_handler(p: &mut Process, ins: &Instr) -> R {
    let y = match arg(ins, 0)? {
        Arg::Y(y) => *y,
        _ => return Err(Fault::BadCode("try/catch needs a Y register")),
    };
    put(p, &Arg::Y(y), Term::Nil)?;
    let pc = label(ins, 1)?.ok_or(Fault::BadCode("try/catch without a label"))?;
    let depth = p.frames.len();
    p.handlers.push(Handler {
        depth,
        y,
        target: Cp {
            module: p.pc.module.clone(),
            pc,
        },
    });
    Ok(())
}

fn remove_handler(p: &mut Process, ins: &Instr) -> R {
    let y = match arg(ins, 0)? {
        Arg::Y(y) => *y,
        _ => return Err(Fault::BadCode("try_end needs a Y register")),
    };
    match p.handlers.last() {
        Some(h) if h.y == y && h.depth == p.frames.len() => {
            p.handlers.pop();
            Ok(())
        }
        _ => Err(Fault::BadCode("try_end does not match the innermost try")),
    }
}

// ---- the instruction loop ----

fn step(sys: &mut System, p: &mut Process, module: &Rc<Module>) -> R<Flow> {
    let here = p.pc.pc;
    let ins = module
        .code
        .get(here as usize)
        .ok_or(Fault::BadCode("pc outside the code"))?;
    p.pc.pc = here + 1;
    let a = &sys.atoms;

    match ins.op {
        // `on_load` marks the function BEAM runs after loading (usually to call load_nif). This
        // VM does not run it: natives it provides replace stub bodies at load instead.
        op::LABEL
        | op::LINE
        | op::EXECUTABLE_LINE
        | op::DEBUG_LINE
        | op::NIF_START
        | op::TEST_HEAP
        | op::ON_LOAD => {}

        crate::module::NATIVE_BODY => {
            let &(n, name, arity) = module
                .body_natives
                .get(u(ins, 0)?)
                .ok_or(Fault::BadCode("native body"))?;
            let r = call_native(sys, p, n, (&module.name, &name), arity as usize)?;
            p.x[0] = r;
            return Ok(do_return(p));
        }

        op::FUNC_INFO => {
            // Reached when no clause of the function matched.
            let arity = u(ins, 2)?;
            let args = p.x[..arity].to_vec();
            let args = p.heap.list(args);
            p.pc.pc = here + 1;
            let mut e = Exception::error(Term::Atom(a.function_clause));
            e.trace = Some(stacktrace(sys, p, Some(args)));
            return Err(Fault::Raise(e));
        }

        op::CALL | op::CALL_LAST | op::CALL_ONLY => {
            let target = label(ins, 1)?.ok_or(Fault::BadCode("call to no label"))?;
            let cp = Cp {
                module: module.clone(),
                pc: target,
            };
            if ins.op == op::CALL_LAST {
                deallocate(p)?;
            }
            return Ok(call_code(p, cp, ins.op == op::CALL));
        }

        op::CALL_EXT | op::CALL_EXT_LAST | op::CALL_EXT_ONLY => {
            let arity = u(ins, 0)?;
            let imp = &module.imports[u(ins, 1)?];
            let kind = match ins.op {
                op::CALL_EXT => Kind::Call,
                op::CALL_EXT_LAST => Kind::Last,
                _ => Kind::Only,
            };
            if imp.arity as usize != arity {
                return Err(Fault::BadCode("call arity does not match the import"));
            }
            let (m, f, native) = (imp.module, imp.function, imp.native);
            return call_mfa_with(sys, p, &m, &f, arity, kind, native);
        }

        op::BIF0 | op::BIF1 | op::BIF2 | op::GC_BIF1 | op::GC_BIF2 | op::GC_BIF3 => {
            // Operands: [Fail] [Live] Bif Args... Dst
            let (fail, first) = match ins.op {
                op::BIF0 => (None, 0),
                op::BIF1 | op::BIF2 => (label(ins, 0)?, 1),
                _ => (label(ins, 0)?, 2),
            };
            let nargs = match ins.op {
                op::BIF0 => 0,
                op::BIF1 | op::GC_BIF1 => 1,
                op::BIF2 | op::GC_BIF2 => 2,
                _ => 3,
            };
            let imp = &module.imports[u(ins, first)?];
            if imp.arity as usize != nargs {
                return Err(Fault::BadCode("BIF import arity"));
            }
            let n = imp
                .native
                .ok_or_else(|| Fault::Raise(Exception::error(Term::Atom(sys.atoms.undef))))?;
            // At most three arguments: a stack array, not an allocation per guard BIF.
            let mut args = [Term::Nil, Term::Nil, Term::Nil];
            for (i, slot) in args.iter_mut().enumerate().take(nargs) {
                let t = src(p, ins, first + 1 + i)?;
                *slot = as_value(&mut p.heap, t);
            }
            match run_native(sys, p, n, (&imp.module, &imp.function), &args[..nargs]) {
                Ok(t) => dst(p, ins, first + 1 + nargs, t)?,
                // A guard BIF with a fail label fails the guard instead of raising.
                Err(Fault::Raise(_)) if fail.is_some() => jump(p, fail)?,
                Err(e) => return Err(e),
            }
        }

        op::ALLOCATE | op::ALLOCATE_HEAP => allocate(p, u(ins, 0)?, sys.limits.max_stack_slots)?,
        op::DEALLOCATE => deallocate(p)?,
        op::RETURN => return Ok(do_return(p)),
        op::INIT_YREGS => {
            for y in list(ins, 0)? {
                put(p, y, Term::Nil)?;
            }
        }
        op::TRIM => {
            let n = u(ins, 0)?;
            let f = p
                .frames
                .last_mut()
                .ok_or(Fault::BadCode("trim without a frame"))?;
            if n > f.size {
                return Err(Fault::BadCode("trim more than the frame"));
            }
            let base = f.base;
            f.size -= n;
            p.stack.drain(base..base + n);
            // Handlers name Y registers of this frame; renumber them.
            let depth = p.frames.len();
            for h in p.handlers.iter_mut().filter(|h| h.depth == depth) {
                h.y =
                    h.y.checked_sub(n as u16)
                        .ok_or(Fault::BadCode("trim removed a try register"))?;
            }
        }

        op::MOVE => {
            let t = src(p, ins, 0)?;
            dst(p, ins, 1, t)?;
        }
        op::SWAP => {
            let (x, y) = (src(p, ins, 0)?, src(p, ins, 1)?);
            dst(p, ins, 0, y)?;
            dst(p, ins, 1, x)?;
        }

        // ---- lists and tuples ----
        op::PUT_LIST => {
            let (hd, tl) = (src(p, ins, 0)?, src(p, ins, 1)?);
            let t = p.heap.cons(hd, tl);
            dst(p, ins, 2, t)?;
        }
        op::GET_LIST => {
            let (hd, tl) = p
                .heap
                .as_cons(val(p, ins, 0)?)
                .ok_or(Fault::BadCode("get_list on a non-list"))?;
            dst(p, ins, 1, hd)?;
            dst(p, ins, 2, tl)?;
        }
        op::GET_HD | op::GET_TL => {
            let (hd, tl) = p
                .heap
                .as_cons(val(p, ins, 0)?)
                .ok_or(Fault::BadCode("get_hd/get_tl on a non-list"))?;
            dst(p, ins, 1, if ins.op == op::GET_HD { hd } else { tl })?;
        }
        op::PUT_TUPLE2 => {
            let mut elems = Vec::new();
            for e in list(ins, 1)? {
                elems.push(get(p, e)?);
            }
            let t = p.heap.tuple(&elems);
            dst(p, ins, 0, t)?;
        }
        op::GET_TUPLE_ELEMENT => {
            let i = u(ins, 1)?;
            let e = *p
                .heap
                .as_tuple(val(p, ins, 0)?)
                .and_then(|t| t.get(i))
                .ok_or(Fault::BadCode("get_tuple_element"))?;
            dst(p, ins, 2, e)?;
        }
        op::SET_TUPLE_ELEMENT => {
            // Builds a new tuple (BEAM updates in place: the compiler knows the old one is dead;
            // copying is the same to Erlang code).
            let v = src(p, ins, 0)?;
            let t = src(p, ins, 1)?;
            let i = u(ins, 2)?;
            let mut elems = p
                .heap
                .as_tuple(t)
                .ok_or(Fault::BadCode("set_tuple_element"))?
                .to_vec();
            *elems
                .get_mut(i)
                .ok_or(Fault::BadCode("set_tuple_element index"))? = v;
            let t = p.heap.tuple(&elems);
            dst(p, ins, 1, t)?;
        }
        op::UPDATE_RECORD => {
            let size = u(ins, 1)?;
            let t = src(p, ins, 2)?;
            let mut elems = match p.heap.as_tuple(t) {
                Some(e) if e.len() == size => e.to_vec(),
                _ => return Err(Fault::BadCode("update_record on a mismatched tuple")),
            };
            for pair in list(ins, 4)?.chunks(2) {
                let [Arg::U(pos), v] = pair else {
                    return Err(Fault::BadCode("update_record list"));
                };
                let pos = (*pos as usize)
                    .checked_sub(1)
                    .ok_or(Fault::BadCode("update_record index"))?;
                *elems
                    .get_mut(pos)
                    .ok_or(Fault::BadCode("update_record index"))? = get(p, v)?;
            }
            let t = p.heap.tuple(&elems);
            dst(p, ins, 3, t)?;
        }

        // ---- tests: jump to the label if the test fails ----
        op::IS_LT | op::IS_GE | op::IS_EQ | op::IS_NE | op::IS_EQ_EXACT | op::IS_NE_EXACT => {
            let (x, y) = (val(p, ins, 1)?, val(p, ins, 2)?);
            let h = &p.heap;
            let ok = match ins.op {
                op::IS_LT => h.cmp_term(x, y) == Ordering::Less,
                op::IS_GE => h.cmp_term(x, y) != Ordering::Less,
                op::IS_EQ => h.eq_arith(x, y),
                op::IS_NE => !h.eq_arith(x, y),
                op::IS_EQ_EXACT => h.eq_exact(x, y),
                _ => !h.eq_exact(x, y),
            };
            if !ok {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::IS_INTEGER
        | op::IS_FLOAT
        | op::IS_NUMBER
        | op::IS_ATOM
        | op::IS_PID
        | op::IS_REFERENCE
        | op::IS_PORT
        | op::IS_NIL
        | op::IS_BINARY
        | op::IS_LIST
        | op::IS_NONEMPTY_LIST
        | op::IS_TUPLE
        | op::IS_FUNCTION
        | op::IS_BOOLEAN
        | op::IS_MAP
        | op::IS_BITSTR => {
            let t = val(p, ins, 1)?;
            let heap = &p.heap;
            // A match context counts as the bitstring it is matching.
            let bits_like = |t: &Term, whole_bytes: bool| match t {
                Term::Bits(_) => {
                    !whole_bytes || heap.bit_len(*t).is_some_and(|n| n.is_multiple_of(8))
                }
                Term::Match(_) => {
                    !whole_bytes
                        || heap
                            .as_match(*t)
                            .and_then(|(b, pos)| Some(heap.bit_len(b)? - pos))
                            .is_some_and(|n| n.is_multiple_of(8))
                }
                _ => false,
            };
            let ok = match ins.op {
                op::IS_INTEGER => t.is_integer(),
                op::IS_FLOAT => matches!(t, Term::Float(_)),
                op::IS_NUMBER => t.is_number(),
                op::IS_ATOM => matches!(t, Term::Atom(_)),
                op::IS_PID => matches!(t, Term::Pid(p) if !p.port),
                op::IS_PORT => matches!(t, Term::Pid(p) if p.port),
                op::IS_REFERENCE => matches!(t, Term::Ref(_) | Term::Resource(_)),
                op::IS_NIL => matches!(t, Term::Nil),
                op::IS_BINARY => bits_like(&t, true),
                op::IS_LIST => matches!(t, Term::Nil | Term::Cons(_)),
                op::IS_NONEMPTY_LIST => matches!(t, Term::Cons(_)),
                op::IS_TUPLE => matches!(t, Term::Tuple(_)),
                op::IS_FUNCTION => matches!(t, Term::Fun(_)),
                op::IS_BOOLEAN => t.is_atom(&a.true_) || t.is_atom(&a.false_),
                op::IS_MAP => matches!(t, Term::Map(_)),
                _ => bits_like(&t, false),
            };
            if !ok {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::IS_FUNCTION2 => {
            let (f, n) = (src(p, ins, 1)?, src(p, ins, 2)?);
            let ok = matches!((p.heap.as_fun(f), n.as_usize()), (Some(f), Some(n)) if f.arity() as usize == n);
            if !ok {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::TEST_ARITY => {
            let n = u(ins, 2)?;
            if p.heap.as_tuple(val(p, ins, 1)?).map(|t| t.len()) != Some(n) {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::IS_TAGGED_TUPLE => {
            let n = u(ins, 2)?;
            let (t, tag) = (val(p, ins, 1)?, val(p, ins, 3)?);
            let ok = matches!(p.heap.as_tuple(t), Some(e) if e.len() == n && n > 0 && p.heap.eq_exact(e[0], tag));
            if !ok {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::SELECT_VAL => {
            let v = val(p, ins, 0)?;
            let mut target = label(ins, 1)?;
            for pair in list(ins, 2)?.chunks(2) {
                let [Arg::Const(c), Arg::Label(l)] = pair else {
                    return Err(Fault::BadCode("select_val list"));
                };
                if p.heap.eq_exact(v, *c) {
                    target = *l;
                    break;
                }
            }
            jump(p, target)?;
        }
        op::SELECT_TUPLE_ARITY => {
            let n = p
                .heap
                .as_tuple(val(p, ins, 0)?)
                .map(|t| t.len())
                .ok_or(Fault::BadCode("select_tuple_arity on a non-tuple"))?;
            let mut target = label(ins, 1)?;
            for pair in list(ins, 2)?.chunks(2) {
                let [Arg::U(arity), Arg::Label(l)] = pair else {
                    return Err(Fault::BadCode("select_tuple_arity list"));
                };
                if *arity as usize == n {
                    target = *l;
                    break;
                }
            }
            jump(p, target)?;
        }
        op::JUMP => jump(p, label(ins, 0)?)?,

        // ---- errors ----
        op::BADMATCH | op::CASE_END | op::TRY_CASE_END | op::BADRECORD => {
            let tag = match ins.op {
                op::BADMATCH => a.badmatch,
                op::CASE_END => a.case_clause,
                op::TRY_CASE_END => a.try_clause,
                _ => a.badrecord,
            };
            let v = src(p, ins, 0)?;
            return Err(Fault::Raise(error_tuple(&mut p.heap, &tag, v)));
        }
        op::IF_END => return Err(Fault::Raise(Exception::error(Term::Atom(a.if_clause)))),

        // ---- exceptions ----
        op::TRY | op::CATCH => install_handler(p, ins)?,
        op::TRY_END => remove_handler(p, ins)?,
        op::TRY_CASE => {
            if !p.in_exception {
                return Err(Fault::BadCode("try_case without an exception"));
            }
            p.in_exception = false;
            // x0..x2 already hold class, reason and stack trace.
        }
        op::CATCH_END => {
            if p.in_exception {
                p.in_exception = false;
                let (class, reason, trace) = (p.x[0], p.x[1], cooked(&p.heap, p.x[2]));
                let exit = Term::Atom(a.exit_upper);
                p.x[0] = if class.is_atom(&a.throw) {
                    reason
                } else if class.is_atom(&a.error) {
                    let inner = p.heap.tuple(&[reason, trace]);
                    p.heap.tuple(&[exit, inner])
                } else {
                    p.heap.tuple(&[exit, reason])
                };
            } else {
                remove_handler(p, ins)?;
            }
        }
        op::BUILD_STACKTRACE => p.x[0] = cooked(&p.heap, p.x[0]),
        op::RAISE => {
            // raise RawTrace Reason: rethrow with the class recorded in the raw trace.
            let raw = src(p, ins, 0)?;
            let reason = src(p, ins, 1)?;
            let (class, trace) = match p.heap.as_tuple(raw) {
                Some(&[c, t]) => (class_of(sys, &c).unwrap_or(Class::Error), t),
                _ => (Class::Error, raw),
            };
            return Err(Fault::Raise(Exception::with_trace(class, reason, trace)));
        }
        op::RAW_RAISE => {
            let class = if p.x[0].is_atom(&a.error) {
                Class::Error
            } else if p.x[0].is_atom(&a.exit) {
                Class::Exit
            } else if p.x[0].is_atom(&a.throw) {
                Class::Throw
            } else {
                p.x[0] = Term::Atom(a.badarg);
                return Ok(Flow::Next);
            };
            let e = Exception::with_trace(class, p.x[1], cooked(&p.heap, p.x[2]));
            return Err(Fault::Raise(e));
        }

        // ---- funs ----
        op::MAKE_FUN3 => {
            let index = u(ins, 0)?;
            let entry = &module.funs[index];
            let mut env = Vec::new();
            for e in list(ins, 2)? {
                env.push(get(p, e)?);
            }
            if env.len() != entry.num_free as usize {
                return Err(Fault::BadCode("make_fun3 environment size"));
            }
            let fun = p.heap.fun_local(
                module.name,
                index as u32,
                entry.arity - entry.num_free,
                entry.uniq,
                entry.function,
                &env,
            );
            dst(p, ins, 1, fun)?;
        }
        op::CALL_FUN => {
            let arity = u(ins, 0)?;
            if arity >= X_REGS {
                return Err(Fault::BadCode("call_fun arity"));
            }
            let fun = p.x[arity];
            let args = p.x[..arity].to_vec();
            return call_fun(sys, p, fun, args, Kind::Call);
        }
        op::CALL_FUN2 => {
            let arity = u(ins, 1)?;
            let fun = src(p, ins, 2)?;
            let args = p.x[..arity.min(X_REGS)].to_vec();
            return call_fun(sys, p, fun, args, Kind::Call);
        }
        op::APPLY | op::APPLY_LAST => {
            let arity = u(ins, 0)?;
            if arity + 2 > X_REGS {
                return Err(Fault::BadCode("apply arity"));
            }
            let (m, f) = (p.x[arity], p.x[arity + 1]);
            let (Term::Atom(m), Term::Atom(f)) = (m, f) else {
                let args = p.x[..arity].to_vec();
                let args = p.heap.list(args);
                return Err(bad_apply(sys, p, m, f, args));
            };
            let kind = if ins.op == op::APPLY {
                Kind::Call
            } else {
                Kind::Last
            };
            return call_mfa(sys, p, &m, &f, arity, kind);
        }

        // ---- messages ----
        op::SEND => {
            let (to, msg) = (p.x[0], p.x[1]);
            p.x[0] = crate::bif::send(&mut Ctx { sys, p }, &[to, msg])?;
        }
        op::LOOP_REC => {
            if p.save == p.mailbox.len() {
                sys.receive_pending(p);
            }
            if p.save < p.mailbox.len() {
                let m = p.mailbox[p.save];
                dst(p, ins, 1, m)?;
            } else {
                jump(p, label(ins, 0)?)?;
            }
        }
        op::LOOP_REC_END => {
            p.save += 1;
            jump(p, label(ins, 0)?)?;
        }
        op::REMOVE_MESSAGE => {
            if p.save < p.mailbox.len() {
                p.mailbox.remove(p.save);
            }
            p.save = 0;
            if let Some(t) = p.timer.take() {
                sys.cancel_timer(p.pid, t);
            }
            p.timed_out = false;
        }
        op::TIMEOUT => {
            p.save = 0;
            p.timed_out = false;
        }
        op::WAIT => {
            jump(p, label(ins, 0)?)?;
            return Ok(Flow::Stop(Stop::Wait));
        }
        op::WAIT_TIMEOUT => {
            if p.timed_out {
                // The timer fired: fall through to the `after` clause.
                p.timed_out = false;
                return Ok(Flow::Next);
            }
            let t = src(p, ins, 1)?;
            if t.is_atom(&a.infinity) {
                jump(p, label(ins, 0)?)?;
                return Ok(Flow::Stop(Stop::Wait));
            }
            let ms = match t {
                Term::Int(ms) if ms >= 0 => ms as u64,
                _ => return Err(Fault::Raise(Exception::error(Term::Atom(a.timeout_value)))),
            };
            if ms == 0 {
                return Ok(Flow::Next);
            }
            if p.timer.is_none() {
                let deadline = sys.now_us().saturating_add(ms.saturating_mul(1000));
                p.timer = Some(deadline);
                sys.arm_timer(p.pid, deadline);
            }
            p.timeout_pc = here + 1;
            jump(p, label(ins, 0)?)?;
            return Ok(Flow::Stop(Stop::Wait));
        }
        // Receive markers only speed up scanning the mailbox; scanning from the start is correct.
        op::RECV_MARKER_RESERVE => {
            let r = sys.make_ref();
            dst(p, ins, 0, Term::Ref(r))?;
        }
        op::RECV_MARKER_BIND | op::RECV_MARKER_CLEAR | op::RECV_MARKER_USE => {}

        // ---- maps ----
        op::PUT_MAP_ASSOC | op::PUT_MAP_EXACT => {
            let mut map = src(p, ins, 1)?;
            if !matches!(map, Term::Map(_)) {
                let (badmap, heap) = (a.badmap, &mut p.heap);
                return Err(Fault::Raise(error_tuple(heap, &badmap, map)));
            }
            for pair in list(ins, 4)?.chunks(2) {
                let [k, v] = pair else {
                    return Err(Fault::BadCode("map pairs"));
                };
                let (k, v) = (get(p, k)?, get(p, v)?);
                if ins.op == op::PUT_MAP_EXACT && p.heap.map_get(map, k).is_none() {
                    match label(ins, 0)? {
                        Some(l) => {
                            jump(p, Some(l))?;
                            return Ok(Flow::Next);
                        }
                        None => {
                            let badkey = sys.atoms.badkey;
                            return Err(Fault::Raise(error_tuple(&mut p.heap, &badkey, k)));
                        }
                    }
                }
                map = p.heap.map_put(map, k, v);
            }
            dst(p, ins, 2, map)?;
        }
        op::GET_MAP_ELEMENTS => {
            let map = src(p, ins, 1)?;
            if !matches!(map, Term::Map(_)) {
                return Err(Fault::BadCode("get_map_elements on a non-map"));
            }
            for pair in list(ins, 2)?.chunks(2) {
                let [k, d] = pair else {
                    return Err(Fault::BadCode("map pairs"));
                };
                let k = get(p, k)?;
                match p.heap.map_get(map, k) {
                    Some(v) => put(p, d, v)?,
                    None => {
                        jump(p, label(ins, 0)?)?;
                        return Ok(Flow::Next);
                    }
                }
            }
        }
        op::HAS_MAP_FIELDS => {
            let map = src(p, ins, 1)?;
            if !matches!(map, Term::Map(_)) {
                return Err(Fault::BadCode("has_map_fields on a non-map"));
            }
            for k in list(ins, 2)? {
                let k = get(p, k)?;
                if p.heap.map_get(map, k).is_none() {
                    jump(p, label(ins, 0)?)?;
                    break;
                }
            }
        }

        // ---- floats ----
        op::FMOVE => match (arg(ins, 0)?, arg(ins, 1)?) {
            (Arg::FloatReg(r), d) => {
                let f = p.f[*r as usize];
                let d = d.clone();
                put(p, &d, Term::Float(f))?;
            }
            (s, Arg::FloatReg(r)) => {
                let Term::Float(f) = get(p, s)? else {
                    return Err(Fault::BadCode("fmove of a non-float"));
                };
                p.f[*r as usize] = f;
            }
            _ => return Err(Fault::BadCode("fmove operands")),
        },
        op::FCONV => {
            let f = match src(p, ins, 0)? {
                Term::Float(f) => f,
                Term::Int(i) => i as f64,
                t @ Term::Big(_) => p
                    .heap
                    .as_big(t)
                    .and_then(num_traits::ToPrimitive::to_f64)
                    .filter(|f| f.is_finite())
                    .ok_or_else(|| Fault::Raise(Exception::error(Term::Atom(a.badarith))))?,
                _ => return Err(Fault::Raise(Exception::error(Term::Atom(a.badarith)))),
            };
            let r = freg(ins, 1)?;
            p.f[r] = f;
        }
        op::FADD | op::FSUB | op::FMUL | op::FDIV => {
            let (x, y) = (p.f[freg(ins, 1)?], p.f[freg(ins, 2)?]);
            let r = match ins.op {
                op::FADD => x + y,
                op::FSUB => x - y,
                op::FMUL => x * y,
                _ => x / y,
            };
            if !r.is_finite() {
                return Err(Fault::Raise(Exception::error(Term::Atom(a.badarith))));
            }
            let d = freg(ins, 3)?;
            p.f[d] = r;
        }
        op::FNEGATE => {
            let x = p.f[freg(ins, 1)?];
            let d = freg(ins, 2)?;
            p.f[d] = -x;
        }

        // ---- binaries ----
        op::BS_CREATE_BIN => return bs_create_bin(sys, p, ins, module),
        op::BS_INIT_WRITABLE => p.x[0] = p.heap.binary(&[]),
        op::BS_START_MATCH4 => {
            let t = src(p, ins, 2)?;
            let state = match t {
                Term::Match(_) => t,
                Term::Bits(_) => p.heap.match_state(t, 0),
                _ => match arg(ins, 0)? {
                    Arg::Label(l) => {
                        jump(p, *l)?;
                        return Ok(Flow::Next);
                    }
                    _ => return Err(Fault::BadCode("bs_start_match4 on a non-binary")),
                },
            };
            dst(p, ins, 3, state)?;
        }
        op::BS_GET_POSITION => {
            let (_, pos) = p
                .heap
                .as_match(src(p, ins, 0)?)
                .ok_or(Fault::BadCode("bs_get_position"))?;
            dst(p, ins, 1, Term::Int(pos as i64))?;
        }
        op::BS_SET_POSITION => {
            let state = src(p, ins, 0)?;
            let (bits, _) = p
                .heap
                .as_match(state)
                .ok_or(Fault::BadCode("bs_set_position"))?;
            let len = p
                .heap
                .bit_len(bits)
                .ok_or(Fault::BadCode("bs_set_position"))?;
            let pos = src(p, ins, 1)?
                .as_usize()
                .filter(|x| *x <= len)
                .ok_or(Fault::BadCode("bs_set_position"))?;
            p.heap.set_match_pos(state, pos);
        }
        op::BS_GET_TAIL => {
            let (bits, pos) =
                match_bits(&p.heap, src(p, ins, 0)?).ok_or(Fault::BadCode("bs_get_tail"))?;
            let t = p.heap.bits(bits.slice(pos, bits.len - pos));
            dst(p, ins, 1, t)?;
        }
        op::BS_MATCH => return bs_match(sys, p, ins),
        op::BS_START_MATCH3 => {
            let t = src(p, ins, 1)?;
            let state = match t {
                Term::Match(_) => t,
                Term::Bits(_) => p.heap.match_state(t, 0),
                _ => {
                    jump(p, label(ins, 0)?)?;
                    return Ok(Flow::Next);
                }
            };
            dst(p, ins, 3, state)?;
        }
        op::BS_GET_INTEGER2
        | op::BS_GET_FLOAT2
        | op::BS_GET_BINARY2
        | op::BS_SKIP_BITS2
        | op::BS_TEST_TAIL2
        | op::BS_TEST_UNIT
        | op::BS_MATCH_STRING
        | op::BS_GET_UTF8
        | op::BS_GET_UTF16
        | op::BS_GET_UTF32
        | op::BS_SKIP_UTF8
        | op::BS_SKIP_UTF16
        | op::BS_SKIP_UTF32 => return bs_get(sys, p, ins, module),

        _ => return Err(Fault::BadCode("opcode not implemented")),
    }
    Ok(Flow::Next)
}

// ---- binary construction ----

fn flags_little(sys: &System, heap: &Heap, flags: Term) -> bool {
    // Flags come as a list of atoms; `native` is little-endian on every target we support.
    heap.list_iter(flags)
        .flatten()
        .any(|f| f.is_atom(&sys.atoms.little) || f.is_atom(&sys.atoms.native))
}

fn flags_signed(sys: &System, heap: &Heap, flags: Term) -> bool {
    heap.list_iter(flags)
        .flatten()
        .any(|f| f.is_atom(&sys.atoms.signed))
}

fn bs_create_bin(sys: &mut System, p: &mut Process, ins: &Instr, module: &Module) -> R<Flow> {
    let fail = label(ins, 0)?;
    let segments = list(ins, 5)?;
    if segments.len() % 6 != 0 {
        return Err(Fault::BadCode("bs_create_bin segment list"));
    }
    let max_bits = sys.limits.max_binary_bits;
    let mut segments = segments;
    // `private_append` first: the compiler promises nothing else will use that binary again, so
    // its bytes may be appended to in place when nobody else holds them (see
    // `Heap::take_for_append`). This turns a binary comprehension from quadratic into linear.
    // The other segments are built first, so a failure leaves the base untouched.
    let mut base = None;
    if let [Arg::Const(Term::Atom(ty)), _, Arg::U(unit), _, src @ (Arg::X(_) | Arg::Y(_)), size, rest @ ..] =
        segments
    {
        // Only without a fail label: with one, the code there could still read the register.
        if fail.is_none()
            && ty.as_str() == "private_append"
            && matches!(size, Arg::Const(t) if t.is_atom(&sys.atoms.all))
        {
            let t = get(p, src)?;
            if p.heap
                .bit_len(t)
                .is_some_and(|len| *unit <= 1 || len % *unit as usize == 0)
            {
                base = Some(t);
                segments = rest;
            }
        }
    }
    let base_len = base.and_then(|b| p.heap.bit_len(b)).unwrap_or(0);
    let mut out = Builder::new();
    let mut ok = true;
    for seg in segments.chunks(6) {
        let [Arg::Const(Term::Atom(ty)), _seg, Arg::U(unit), flags, value, size] = seg else {
            return Err(Fault::BadCode("bs_create_bin segment"));
        };
        let unit = *unit as usize;
        let flags = match flags {
            Arg::Const(t) => *t,
            _ => Term::Nil,
        };
        let little = flags_little(sys, &p.heap, flags);
        let size_t = match size {
            Arg::Const(_) | Arg::X(_) | Arg::Y(_) => get(p, size)?,
            _ => Term::Nil,
        };
        let room = |out: &Builder, bits: usize| base_len + out.bit_len() + bits <= max_bits;
        let seg_ok = match ty.as_str() {
            "integer" => {
                let v = get(p, value)?;
                match (p.heap.as_bigint(v), size_t.as_usize()) {
                    // Check the size before building, so a huge segment fails without allocating.
                    (Some(v), Some(n)) if n.checked_mul(unit).is_some_and(|b| room(&out, b)) => {
                        out.push_integer(&v, n * unit, little);
                        true
                    }
                    _ => false,
                }
            }
            "float" => {
                let v = get(p, value)?;
                let f = match v {
                    Term::Float(f) => Some(f),
                    Term::Int(i) => Some(i as f64),
                    Term::Big(_) => p
                        .heap
                        .as_big(v)
                        .and_then(num_traits::ToPrimitive::to_f64)
                        .filter(|f| f.is_finite()),
                    _ => None,
                };
                match (f, size_t.as_usize()) {
                    (Some(f), Some(n)) => bits::push_float(&mut out, f, n * unit, little),
                    _ => false,
                }
            }
            "binary" | "append" | "private_append" => match p.heap.as_bits(get(p, value)?) {
                Some(b) => {
                    // `all` is only a size when written literally; a variable holding the atom
                    // `all` is a bad size, as in BEAM.
                    if matches!(size, Arg::Const(t) if t.is_atom(&sys.atoms.all)) {
                        if unit > 1 && b.len % unit != 0 {
                            false
                        } else {
                            out.push_bits(&b);
                            true
                        }
                    } else {
                        match size_t.as_usize().and_then(|n| n.checked_mul(unit)) {
                            Some(bits_wanted) if bits_wanted <= b.len => {
                                out.push_bits_prefix(&b, bits_wanted);
                                true
                            }
                            _ => false,
                        }
                    }
                }
                None => false,
            },
            "utf8" | "utf16" | "utf32" => match get(p, value)?.as_i64() {
                Some(cp) => match ty.as_str() {
                    "utf8" => bits::push_utf8(&mut out, cp),
                    "utf16" => bits::push_utf16(&mut out, cp, little),
                    _ => bits::push_utf32(&mut out, cp, little),
                },
                None => false,
            },
            "string" => {
                let (Arg::U(offset), Some(len)) = (value, size_t.as_usize()) else {
                    return Err(Fault::BadCode("bs_create_bin string"));
                };
                let start = *offset as usize;
                let bytes = start
                    .checked_add(len)
                    .and_then(|end| module.strings.get(start..end))
                    .ok_or(Fault::BadCode("bs_create_bin string range"))?;
                out.push_bytes(bytes);
                true
            }
            _ => return Err(Fault::BadCode("bs_create_bin segment type")),
        };
        if !seg_ok {
            ok = false;
            break;
        }
        if base_len + out.bit_len() > max_bits {
            return Err(Fault::Raise(Exception::error(Term::Atom(
                sys.atoms.system_limit,
            ))));
        }
    }
    if !ok {
        return match fail {
            Some(l) => {
                jump(p, Some(l))?;
                Ok(Flow::Next)
            }
            None => Err(Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg)))),
        };
    }
    let result = match base {
        None => out.finish(&mut p.heap),
        Some(base) => {
            let tail = out.into_bits();
            match p.heap.take_for_append(base) {
                Some((entry, bytes, len)) => {
                    let mut b = Builder::from_parts(bytes, len);
                    b.push_bits(&tail);
                    let (bytes, len) = b.into_parts();
                    p.heap.finish_append(entry, bytes, len)
                }
                None => {
                    let mut b = Builder::new();
                    b.push_bits(&p.heap.as_bits(base).expect("checked above"));
                    b.push_bits(&tail);
                    b.finish(&mut p.heap)
                }
            }
        }
    };
    dst(p, ins, 4, result)?;
    Ok(Flow::Next)
}

// ---- binary matching ----

/// Run the commands of a `bs_match` instruction; on the first failure, jump to its label.
fn bs_match(sys: &mut System, p: &mut Process, ins: &Instr) -> R<Flow> {
    let fail = label(ins, 0)?;
    let state = src(p, ins, 1)?;
    let (bits, mut pos) =
        match_bits(&p.heap, state).ok_or(Fault::BadCode("bs_match without a match state"))?;
    let bits = &bits;
    let cmds = list(ins, 2)?;
    let mut i = 0;
    let take = |i: &mut usize, n: usize| -> R<&[Arg]> {
        let s = cmds
            .get(*i..*i + n)
            .ok_or(Fault::BadCode("bs_match command list"))?;
        *i += n;
        Ok(s)
    };
    let failed = loop {
        let Some(cmd) = cmds.get(i) else { break false };
        i += 1;
        let Arg::Const(Term::Atom(name)) = cmd else {
            return Err(Fault::BadCode("bs_match command"));
        };
        let remaining = bits.len - pos;
        match name.as_str() {
            "ensure_at_least" => {
                let [Arg::U(stride), Arg::U(unit)] = take(&mut i, 2)? else {
                    return Err(Fault::BadCode("ensure_at_least"));
                };
                let (stride, unit) = (*stride as usize, (*unit as usize).max(1));
                if remaining < stride || !(remaining - stride).is_multiple_of(unit) {
                    break true;
                }
            }
            "ensure_exactly" => {
                let [Arg::U(stride)] = take(&mut i, 1)? else {
                    return Err(Fault::BadCode("ensure_exactly"));
                };
                if remaining != *stride as usize {
                    break true;
                }
            }
            "integer" | "binary" => {
                let [_live, flags, size, Arg::U(unit), d] = take(&mut i, 5)? else {
                    return Err(Fault::BadCode("bs_match integer/binary"));
                };
                let flags = match flags {
                    Arg::Const(t) => *t,
                    _ => Term::Nil,
                };
                let n = num_operand(p, size)?
                    .as_usize()
                    .and_then(|s| s.checked_mul(*unit as usize))
                    .ok_or(Fault::BadCode("bs_match size"))?;
                if n > remaining {
                    break true;
                }
                let t = if name.as_str() == "integer" {
                    let (signed, little) = (
                        flags_signed(sys, &p.heap, flags),
                        flags_little(sys, &p.heap, flags),
                    );
                    bits::read_integer(&mut p.heap, bits, pos, n, signed, little)
                } else {
                    p.heap.bits(bits.slice(pos, n))
                };
                pos += n;
                let d = d.clone();
                put(p, &d, t)?;
            }
            "get_tail" => {
                let [_live, _unit, d] = take(&mut i, 3)? else {
                    return Err(Fault::BadCode("get_tail"));
                };
                // The rest, without moving the position: code may go on matching from here
                // (a `with` keeps the tail for its `else` and reads on), as in BEAM.
                let t = p.heap.bits(bits.slice(pos, remaining));
                let d = d.clone();
                put(p, &d, t)?;
            }
            "=:=" => {
                let [_live, size, value] = take(&mut i, 3)? else {
                    return Err(Fault::BadCode("=:="));
                };
                let n = num_operand(p, size)?
                    .as_usize()
                    .ok_or(Fault::BadCode("=:= size"))?;
                if n > remaining {
                    break true;
                }
                let want = num_operand(p, value)?;
                let got = bits::read_integer(&mut p.heap, bits, pos, n, false, false);
                if !p.heap.eq_exact(got, want) {
                    break true;
                }
                pos += n;
            }
            "skip" => {
                let [Arg::U(stride)] = take(&mut i, 1)? else {
                    return Err(Fault::BadCode("skip"));
                };
                let n = *stride as usize;
                if n > remaining {
                    break true;
                }
                pos += n;
            }
            _ => return Err(Fault::BadCode("bs_match command not implemented")),
        }
    };
    if failed {
        jump(p, fail)?;
    } else {
        p.heap.set_match_pos(state, pos);
    }
    Ok(Flow::Next)
}

/// Segment flags, given either as a bit set (`field_flags`) or as a list of atoms (`bs_match`).
fn seg_flags(sys: &System, heap: &Heap, a: &Arg) -> (bool, bool) {
    const LITTLE: u64 = 0x02;
    const SIGNED: u64 = 0x04;
    const NATIVE: u64 = 0x10; // little-endian on every target we support
    match a {
        Arg::U(f) => (f & (LITTLE | NATIVE) != 0, f & SIGNED != 0),
        Arg::Const(t) => (flags_little(sys, heap, *t), flags_signed(sys, heap, *t)),
        _ => (false, false),
    }
}

/// The older single-segment matching instructions (`bs_get_integer2` and friends). Each reads
/// from the match state in operand 1 and jumps to operand 0 on failure.
fn bs_get(sys: &mut System, p: &mut Process, ins: &Instr, module: &Module) -> R<Flow> {
    let fail = label(ins, 0)?;
    let state = src(p, ins, 1)?;
    let (bits, pos) =
        match_bits(&p.heap, state).ok_or(Fault::BadCode("binary match without a match state"))?;
    let bits = &bits;
    let remaining = bits.len - pos;
    // Some(result, bits consumed) on success, None to take the fail label.
    let outcome: Option<(Option<Term>, usize)> = match ins.op {
        op::BS_GET_INTEGER2 | op::BS_GET_FLOAT2 | op::BS_GET_BINARY2 | op::BS_SKIP_BITS2 => {
            // get: Fail Ctx Live Size Unit Flags Dst; skip: Fail Ctx Size Unit Flags
            let skip = ins.op == op::BS_SKIP_BITS2;
            let (size_i, unit_i, flags_i) = if skip { (2, 3, 4) } else { (3, 4, 5) };
            let size = num_operand(p, arg(ins, size_i)?)?;
            let unit = u(ins, unit_i)?;
            let (little, signed) = seg_flags(sys, &p.heap, arg(ins, flags_i)?);
            let n = if size.is_atom(&sys.atoms.all) {
                // `binary` with no size: the rest, which must be a whole number of units.
                if unit > 1 && remaining % unit != 0 {
                    None
                } else {
                    Some(remaining)
                }
            } else {
                match size.as_usize().and_then(|s| s.checked_mul(unit)) {
                    Some(n) => Some(n),
                    // A negative or non-integer size: BEAM fails the match (or raises badarg).
                    None if size.is_integer() => None,
                    None => {
                        return Err(Fault::Raise(Exception::error(Term::Atom(sys.atoms.badarg))))
                    }
                }
            };
            match n {
                Some(n) if n <= remaining => match ins.op {
                    op::BS_GET_INTEGER2 => Some((
                        Some(bits::read_integer(
                            &mut p.heap,
                            bits,
                            pos,
                            n,
                            signed,
                            little,
                        )),
                        n,
                    )),
                    op::BS_GET_FLOAT2 => {
                        bits::read_float(bits, pos, n, little).map(|f| (Some(Term::Float(f)), n))
                    }
                    op::BS_GET_BINARY2 => Some((Some(p.heap.bits(bits.slice(pos, n))), n)),
                    _ => Some((None, n)),
                },
                _ => None,
            }
        }
        op::BS_TEST_TAIL2 => {
            let want = u(ins, 2)?;
            (remaining == want).then_some((None, 0))
        }
        op::BS_TEST_UNIT => {
            let unit = u(ins, 2)?.max(1);
            (remaining % unit == 0).then_some((None, 0))
        }
        op::BS_MATCH_STRING => {
            // Fail Ctx Bits Offset: compare the next Bits bits with the string table at Offset.
            let n = u(ins, 2)?;
            let offset = u(ins, 3)?;
            let bytes = offset
                .checked_add(n.div_ceil(8))
                .and_then(|end| module.strings.get(offset..end))
                .ok_or(Fault::BadCode("bs_match_string range"))?;
            let want = Bits {
                data: alloc::sync::Arc::new(bytes.to_vec()),
                offset: 0,
                len: n,
            };
            let ok = n <= remaining && (0..n).all(|i| bits.bit(pos + i) == want.bit(i));
            ok.then_some((None, n))
        }
        op::BS_GET_UTF8 | op::BS_SKIP_UTF8 => bits::read_utf8(bits, pos).map(|(cp, n)| {
            (
                (ins.op == op::BS_GET_UTF8).then_some(Term::Int(cp as i64)),
                n,
            )
        }),
        op::BS_GET_UTF16 | op::BS_SKIP_UTF16 | op::BS_GET_UTF32 | op::BS_SKIP_UTF32 => {
            let get = matches!(ins.op, op::BS_GET_UTF16 | op::BS_GET_UTF32);
            let (little, _) = seg_flags(sys, &p.heap, arg(ins, 3)?);
            let decoded = if matches!(ins.op, op::BS_GET_UTF16 | op::BS_SKIP_UTF16) {
                read_utf16(&mut p.heap, bits, pos, remaining, little)
            } else if remaining >= 32 {
                bits::read_integer(&mut p.heap, bits, pos, 32, false, little)
                    .as_i64()
                    .and_then(|v| u32::try_from(v).ok())
                    .and_then(char::from_u32)
                    .map(|c| (c as u32, 32))
            } else {
                None
            };
            decoded.map(|(cp, n)| (get.then_some(Term::Int(cp as i64)), n))
        }
        _ => return Err(Fault::BadCode("binary match opcode")),
    };
    match outcome {
        None => jump(p, fail)?,
        Some((value, n)) => {
            p.heap.set_match_pos(state, pos + n);
            if let Some(v) = value {
                // The destination is the last operand.
                dst(p, ins, ins.args.len() - 1, v)?;
            }
        }
    }
    Ok(Flow::Next)
}

fn read_utf16(
    heap: &mut Heap,
    bits: &Bits,
    pos: usize,
    remaining: usize,
    little: bool,
) -> Option<(u32, usize)> {
    if remaining < 16 {
        return None;
    }
    let mut unit = |at: usize| {
        bits::read_integer(heap, bits, at, 16, false, little)
            .as_i64()
            .map(|v| v as u16)
    };
    let first = unit(pos)?;
    if !(0xD800..0xE000).contains(&first) {
        return char::from_u32(first as u32).map(|c| (c as u32, 16));
    }
    if remaining < 32 {
        return None;
    }
    let second = unit(pos + 16)?;
    let c = char::decode_utf16([first, second]).next()?.ok()?;
    Some((c as u32, 32))
}
