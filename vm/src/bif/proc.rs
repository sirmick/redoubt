//! Exceptions, processes, messages, links, monitors, names, the process dictionary and time.

use alloc::vec::Vec;

use super::Ctx;
use crate::interp;
use crate::process::{Class, Exception};
use crate::term::{MapKey, Pid, Term};

type R = Result<Term, Exception>;

// ---- exceptions ----

pub fn error(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::error(a[0].clone()))
}

/// `error/2` and `error/3`: the extra arguments only annotate the stack trace.
pub fn error2(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::error(a[0].clone()))
}

pub fn exit(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::exit(a[0].clone()))
}

pub fn throw(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::throw(a[0].clone()))
}

/// `erlang:raise(Class, Reason, Stacktrace)`. An invalid class makes it return `badarg`.
pub fn raise(c: &mut Ctx, a: &[Term]) -> R {
    let class = match &a[0] {
        t if t.is_atom(&c.sys.atoms.error) => Class::Error,
        t if t.is_atom(&c.sys.atoms.exit) => Class::Exit,
        t if t.is_atom(&c.sys.atoms.throw) => Class::Throw,
        _ => return Ok(Term::Atom(c.sys.atoms.badarg.clone())),
    };
    Err(Exception { class, reason: a[1].clone(), trace: Some(a[2].clone()) })
}

// ---- identity ----

pub fn self_(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Pid(c.p.pid))
}

pub fn node(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.atom("nonode@nohost"))
}

pub fn node1(c: &mut Ctx, a: &[Term]) -> R {
    match a[0] {
        Term::Pid(_) | Term::Ref(_) => Ok(c.atom("nonode@nohost")),
        _ => Err(c.badarg()),
    }
}

pub fn make_ref(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Ref(c.sys.make_ref()))
}

// ---- spawning ----

fn do_spawn(c: &mut Ctx, entry: crate::process::Cp, args: Vec<Term>, link: bool) -> R {
    let pid = c.sys.spawn_at(entry, args)?;
    if let Some(p) = c.sys.procs.get_mut(pid) {
        p.group_leader = c.p.group_leader.or(Some(c.p.pid));
        if link {
            p.links.insert(c.p.pid);
        }
    }
    if link {
        c.p.links.insert(pid);
    }
    Ok(Term::Pid(pid))
}

fn mfa_entry(c: &mut Ctx, a: &[Term]) -> Result<(crate::process::Cp, Vec<Term>), Exception> {
    let (Term::Atom(m), Term::Atom(f)) = (&a[0], &a[1]) else { return Err(c.badarg()) };
    let args = a[2].to_vec().ok_or_else(|| c.badarg())?;
    match c.sys.resolve(m, f, args.len() as u32) {
        Some(crate::vm::Target::Code(cp)) => Ok((cp, args)),
        // Spawning straight into a native, or into code that does not exist. BEAM spawns a
        // process that immediately fails with `undef`; so do we, by pointing it at nothing.
        _ => Err(Exception::error(Term::Atom(c.sys.atoms.undef.clone()))),
    }
}

pub fn spawn(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = mfa_entry(c, a)?;
    do_spawn(c, entry, args, false)
}

pub fn spawn_link(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = mfa_entry(c, a)?;
    do_spawn(c, entry, args, true)
}

pub fn spawn_fun(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(c.sys, &a[0], Vec::new())?;
    do_spawn(c, entry, args, false)
}

pub fn spawn_link_fun(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(c.sys, &a[0], Vec::new())?;
    do_spawn(c, entry, args, true)
}

// ---- messages ----

/// Resolve a send destination: a pid, a registered name, or `{Name, Node}` for this node.
fn destination(c: &mut Ctx, t: &Term) -> Result<Pid, Exception> {
    match t {
        Term::Pid(p) => Ok(*p),
        Term::Atom(name) => c.sys.registered.get(name.as_str()).copied().ok_or_else(|| c.badarg()),
        Term::Tuple(t) if t.len() == 2 => {
            let local = t[1].as_tuple().is_none() && matches!(&t[1], Term::Atom(n) if n.as_str() == "nonode@nohost");
            if local {
                destination(c, &t[0])
            } else {
                Err(c.badarg())
            }
        }
        _ => Err(c.badarg()),
    }
}

/// Send `msg` to `to`, which may be the running process itself.
pub fn send_to(c: &mut Ctx, to: Pid, msg: Term) {
    if to == c.p.pid {
        crate::vm::deliver(c.p, msg, &mut c.sys.run_queue);
    } else {
        c.sys.send(to, msg);
    }
}

pub fn send(c: &mut Ctx, a: &[Term]) -> R {
    let to = destination(c, &a[0])?;
    send_to(c, to, a[1].clone());
    Ok(a[1].clone())
}

// ---- links, monitors, exit signals ----

fn pid_arg(c: &Ctx, t: &Term) -> Result<Pid, Exception> {
    match t {
        Term::Pid(p) => Ok(*p),
        _ => Err(c.badarg()),
    }
}

/// Deliver an exit signal from the running process to itself, immediately.
fn exit_self(c: &mut Ctx, reason: Term) {
    let kill = reason.is_atom(&c.sys.atoms.kill);
    if c.p.trap_exit && !kill {
        let msg = Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.exit_upper.clone()), Term::Pid(c.p.pid), reason]);
        send_to(c, c.p.pid, msg);
    } else {
        c.p.pending_exit = Some(if kill { Term::Atom(c.sys.atoms.killed.clone()) } else { reason });
    }
}

pub fn link(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    if pid == c.p.pid {
        return Ok(Term::Atom(c.sys.atoms.true_.clone()));
    }
    match c.sys.procs.get_mut(pid) {
        Some(other) => {
            other.links.insert(c.p.pid);
            c.p.links.insert(pid);
        }
        None => {
            let noproc = Term::Atom(c.sys.atoms.noproc.clone());
            if c.p.trap_exit {
                let msg = Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.exit_upper.clone()), Term::Pid(pid), noproc]);
                send_to(c, c.p.pid, msg);
            } else {
                exit_self(c, noproc);
            }
        }
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn unlink(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    c.p.links.remove(&pid);
    if let Some(other) = c.sys.procs.get_mut(pid) {
        other.links.remove(&c.p.pid);
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn monitor(c: &mut Ctx, a: &[Term]) -> R {
    if !a[0].is_atom(&c.sys.atoms.process) {
        return Err(c.badarg());
    }
    let r = c.sys.make_ref();
    // A monitor by name reports `{Name, Node}` in its 'DOWN' message, as BEAM does.
    let (target, alive, object) = match &a[1] {
        Term::Pid(p) => (Some(*p), *p == c.p.pid || c.sys.procs.is_alive(*p), a[1].clone()),
        Term::Atom(name) => {
            let object = Term::tuple(alloc::vec![a[1].clone(), c.atom(crate::etf::NODE)]);
            match c.sys.registered.get(name.as_str()) {
                Some(p) => (Some(*p), true, object),
                None => (None, false, object),
            }
        }
        _ => return Err(c.badarg()),
    };
    match target {
        Some(pid) if alive && pid != c.p.pid => {
            if let Some(t) = c.sys.procs.get_mut(pid) {
                t.monitored_by.insert(r, (c.p.pid, object));
            }
            c.p.monitors.insert(r, pid);
        }
        Some(pid) if pid == c.p.pid => {} // monitoring yourself never fires
        _ => {
            let msg = Term::tuple(alloc::vec![
                Term::Atom(c.sys.atoms.down.clone()),
                Term::Ref(r),
                Term::Atom(c.sys.atoms.process.clone()),
                object,
                Term::Atom(c.sys.atoms.noproc.clone()),
            ]);
            send_to(c, c.p.pid, msg);
        }
    }
    Ok(Term::Ref(r))
}

pub fn demonitor(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else { return Err(c.badarg()) };
    if let Some(pid) = c.p.monitors.remove(&r) {
        if let Some(t) = c.sys.procs.get_mut(pid) {
            t.monitored_by.remove(&r);
        }
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

/// `demonitor(Ref, Options)`: `flush` drops a `'DOWN'` already queued; `info` makes the result
/// say whether the monitor was still active (`true`) or had already fired or gone (`false`).
pub fn demonitor2(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else { return Err(c.badarg()) };
    let (mut flush, mut info) = (false, false);
    for o in a[1].to_vec().ok_or_else(|| c.badarg())? {
        match &o {
            Term::Atom(x) if x.as_str() == "flush" => flush = true,
            Term::Atom(x) if x.as_str() == "info" => info = true,
            _ => return Err(c.badarg()),
        }
    }
    let active = c.p.monitors.contains_key(&r);
    demonitor(c, a)?;
    if flush {
        let down = c.sys.atoms.down.clone();
        c.p.mailbox.retain(|m| match m.as_tuple() {
            Some([tag, Term::Ref(x), ..]) => !(tag.is_atom(&down) && *x == r),
            _ => true,
        });
        c.p.save = 0;
    }
    Ok(c.bool(!info || active))
}

pub fn exit2(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    if pid == c.p.pid {
        exit_self(c, a[1].clone());
    } else {
        c.sys.exits.push_back(crate::vm::ExitSignal { target: pid, from: c.p.pid, reason: a[1].clone(), from_link: false });
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn process_flag(c: &mut Ctx, a: &[Term]) -> R {
    if a[0].is_atom(&c.sys.atoms.trap_exit) {
        let new = if a[1].is_atom(&c.sys.atoms.true_) {
            true
        } else if a[1].is_atom(&c.sys.atoms.false_) {
            false
        } else {
            return Err(c.badarg());
        };
        let old = core::mem::replace(&mut c.p.trap_exit, new);
        return Ok(c.bool(old));
    }
    Err(c.badarg())
}

pub fn is_process_alive(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    let alive = pid == c.p.pid || c.sys.procs.is_alive(pid);
    Ok(c.bool(alive))
}

// ---- registered names ----

pub fn register(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else { return Err(c.badarg()) };
    let pid = pid_arg(c, &a[1])?;
    if name == &c.sys.atoms.undefined || c.sys.registered.contains_key(name.as_str()) {
        return Err(c.badarg());
    }
    let target = if pid == c.p.pid { Some(&mut *c.p) } else { c.sys.procs.get_mut(pid) };
    match target {
        Some(p) if p.registered_name.is_none() => p.registered_name = Some(name.clone()),
        _ => return Err(c.badarg()),
    }
    c.sys.registered.insert(name.as_str().into(), pid);
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn unregister(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else { return Err(c.badarg()) };
    let pid = c.sys.registered.remove(name.as_str()).ok_or_else(|| c.badarg())?;
    let target = if pid == c.p.pid { Some(&mut *c.p) } else { c.sys.procs.get_mut(pid) };
    if let Some(p) = target {
        p.registered_name = None;
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn whereis(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else { return Err(c.badarg()) };
    Ok(match c.sys.registered.get(name.as_str()) {
        Some(p) => Term::Pid(*p),
        None => Term::Atom(c.sys.atoms.undefined.clone()),
    })
}

// ---- process dictionary ----

pub fn put(c: &mut Ctx, a: &[Term]) -> R {
    let old = c.p.dictionary.insert(MapKey(a[0].clone()), a[1].clone());
    Ok(old.unwrap_or_else(|| Term::Atom(c.sys.atoms.undefined.clone())))
}

pub fn get(c: &mut Ctx, a: &[Term]) -> R {
    let v = c.p.dictionary.get(&MapKey(a[0].clone())).cloned();
    Ok(v.unwrap_or_else(|| Term::Atom(c.sys.atoms.undefined.clone())))
}

pub fn get_all(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(
        c.p.dictionary.iter().map(|(k, v)| Term::tuple(alloc::vec![k.0.clone(), v.clone()])).collect::<Vec<_>>(),
    ))
}

pub fn erase(c: &mut Ctx, a: &[Term]) -> R {
    let old = c.p.dictionary.remove(&MapKey(a[0].clone()));
    Ok(old.unwrap_or_else(|| Term::Atom(c.sys.atoms.undefined.clone())))
}

pub fn group_leader(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Pid(c.p.group_leader.unwrap_or(c.p.pid)))
}

// ---- time ----

/// Parts per second of a time unit, as `erlang:convert_time_unit/3` understands them.
fn unit_per_second(c: &Ctx, t: &Term) -> Result<u64, Exception> {
    match t {
        Term::Atom(a) => match a.as_str() {
            "second" | "seconds" => Ok(1),
            "millisecond" | "milli_seconds" => Ok(1_000),
            "microsecond" | "micro_seconds" => Ok(1_000_000),
            "nanosecond" | "nano_seconds" | "native" | "perf_counter" => Ok(1_000_000_000),
            _ => Err(c.badarg()),
        },
        Term::Int(n) if *n > 0 => Ok(*n as u64),
        _ => Err(c.badarg()),
    }
}

fn scaled(us: u64, per_second: u64) -> Term {
    Term::from_i128(us as i128 * per_second as i128 / 1_000_000)
}

/// The native time unit is the nanosecond, as on a typical BEAM.
pub fn monotonic_time(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(scaled(c.sys.now_us(), 1_000_000_000))
}

pub fn monotonic_time1(c: &mut Ctx, a: &[Term]) -> R {
    let per = unit_per_second(c, &a[0])?;
    Ok(scaled(c.sys.now_us(), per))
}

fn wall_us(c: &mut Ctx) -> Result<u64, Exception> {
    c.sys.platform.system_time_us().ok_or_else(|| c.badarg())
}

pub fn system_time(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(scaled(wall_us(c)?, 1_000_000_000))
}

pub fn system_time1(c: &mut Ctx, a: &[Term]) -> R {
    let per = unit_per_second(c, &a[0])?;
    Ok(scaled(wall_us(c)?, per))
}

pub fn yield_(c: &mut Ctx, _a: &[Term]) -> R {
    c.p.budget = 0;
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

// ---- code ----

pub fn function_exported(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Atom(m), Term::Atom(f), Some(arity)) = (&a[0], &a[1], a[2].as_usize()) else {
        return Err(c.badarg());
    };
    let arity = arity as u32;
    let exported = c.sys.native(m, f, arity).is_some()
        || (c.sys.is_loaded(m) && c.sys.module(m).is_some_and(|md| md.export(f, arity).is_some()));
    Ok(c.bool(exported))
}

pub fn module_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    let loaded = c.sys.is_loaded(m);
    Ok(c.bool(loaded))
}

// ---- spawn_opt ----

/// `spawn_opt(Fun, Options)` and `spawn_opt(M, F, A, Options)`. Supported options: `link` and
/// `monitor`. Tuning options (`min_heap_size`, `priority`, ...) are accepted and ignored: this
/// VM has no per-process heaps and one priority.
fn spawn_with(c: &mut Ctx, entry: crate::process::Cp, args: Vec<Term>, opts: &Term) -> R {
    let opts = opts.to_vec().ok_or_else(|| c.badarg())?;
    let (mut link, mut monitor) = (false, false);
    for o in &opts {
        match o {
            Term::Atom(a) if a.as_str() == "link" => link = true,
            Term::Atom(a) if a.as_str() == "monitor" => monitor = true,
            Term::Tuple(t) if !t.is_empty() && matches!(&t[0], Term::Atom(a) if a.as_str() == "monitor") => monitor = true,
            Term::Tuple(t) if t.len() == 2 => {}
            _ => return Err(c.badarg()),
        }
    }
    let pid = match do_spawn(c, entry, args, link)? {
        Term::Pid(p) => p,
        _ => unreachable!("do_spawn returns a pid"),
    };
    if !monitor {
        return Ok(Term::Pid(pid));
    }
    let r = c.sys.make_ref();
    if let Some(t) = c.sys.procs.get_mut(pid) {
        t.monitored_by.insert(r, (c.p.pid, Term::Pid(pid)));
    }
    c.p.monitors.insert(r, pid);
    Ok(Term::tuple(alloc::vec![Term::Pid(pid), Term::Ref(r)]))
}

pub fn spawn_opt2(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(c.sys, &a[0], Vec::new())?;
    spawn_with(c, entry, args, &a[1])
}

pub fn spawn_opt4(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = mfa_entry(c, a)?;
    spawn_with(c, entry, args, &a[3])
}

// ---- system information ----

/// The OTP release and runtime version this VM mimics: those of the pinned toolchain.
pub const OTP_RELEASE: &str = "28";
pub const ERTS_VERSION: &str = "16.4.0.6";

fn string(s: &str) -> Term {
    Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>())
}

/// `erlang:system_info/1` for the keys portable code asks about. `machine` is `"BEAM"`: this VM
/// implements BEAM semantics, and code that checks it should take its BEAM path.
pub fn system_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(key) = &a[0] else { return Err(c.badarg()) };
    Ok(match key.as_str() {
        "machine" => string("BEAM"),
        "otp_release" => string(OTP_RELEASE),
        "version" => string(ERTS_VERSION),
        "wordsize" => Term::Int(8),
        "process_count" => Term::Int(c.sys.procs.count() as i64),
        "process_limit" => Term::Int(crate::vm::MAX_PROCESSES as i64),
        "atom_count" => Term::Int(c.sys.atom_table.len() as i64),
        "atom_limit" => Term::Int(crate::atom::MAX_ATOMS as i64),
        "port_count" => Term::Int(0),
        "schedulers" | "schedulers_online" | "logical_processors" => Term::Int(1),
        "emu_flavor" => c.atom("emu"),
        "system_architecture" => string("beamlet"),
        "system_version" => return super::info::system_version(c, a),
        "os_type" => Term::tuple(alloc::vec![c.atom("unix"), c.atom("beamlet")]),
        _ => return Err(c.badarg()),
    })
}

pub fn nif_error(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::error(a[0].clone()))
}

pub fn garbage_collect(c: &mut Ctx, _a: &[Term]) -> R {
    // Reference counting frees garbage as soon as it is created; there is nothing to collect.
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn erase_all(c: &mut Ctx, _a: &[Term]) -> R {
    let old = core::mem::take(&mut c.p.dictionary);
    Ok(Term::list(old.into_iter().map(|(k, v)| Term::tuple(alloc::vec![k.0, v])).collect::<Vec<_>>()))
}

/// `unique_integer()` and `unique_integer([positive | monotonic])`. References come from a
/// VM-wide increasing counter, so every result is unique, positive and monotonic.
pub fn unique_integer(c: &mut Ctx, a: &[Term]) -> R {
    if let Some(opts) = a.first() {
        for o in opts.to_vec().ok_or_else(|| c.badarg())? {
            match &o {
                Term::Atom(x) if matches!(x.as_str(), "positive" | "monotonic") => {}
                _ => return Err(c.badarg()),
            }
        }
    }
    Ok(Term::Int(c.sys.make_ref().0 as i64))
}

pub fn timestamp(c: &mut Ctx, _a: &[Term]) -> R {
    let us = wall_us(c)?;
    Ok(Term::tuple(alloc::vec![
        Term::Int((us / 1_000_000_000_000) as i64),
        Term::Int((us / 1_000_000 % 1_000_000) as i64),
        Term::Int((us % 1_000_000) as i64),
    ]))
}

pub fn make_fun(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Atom(m), Term::Atom(f), Some(arity)) = (&a[0], &a[1], a[2].as_usize()) else {
        return Err(c.badarg());
    };
    if arity > 255 {
        return Err(c.badarg());
    }
    Ok(Term::Fun(alloc::rc::Rc::new(crate::term::Fun::Export {
        module: m.clone(),
        function: f.clone(),
        arity: arity as u32,
    })))
}

pub fn fun_info(c: &mut Ctx, a: &[Term]) -> R {
    use crate::term::Fun;
    let (Term::Fun(f), Term::Atom(item)) = (&a[0], &a[1]) else { return Err(c.badarg()) };
    let value = match (&**f, item.as_str()) {
        (Fun::Export { module, .. } | Fun::Local { module, .. }, "module") => Term::Atom(module.clone()),
        (_, "arity") => Term::Int(f.arity() as i64),
        (Fun::Export { function, .. }, "name") => Term::Atom(function.clone()),
        (Fun::Local { module, index, .. }, "name") => {
            let m = c.sys.module(module).ok_or_else(|| c.badarg())?;
            Term::Atom(m.funs.get(*index as usize).ok_or_else(|| c.badarg())?.function.clone())
        }
        (Fun::Export { .. }, "type") => c.atom("external"),
        (Fun::Local { .. }, "type") => c.atom("local"),
        (Fun::Export { .. }, "env") => Term::Nil,
        (Fun::Local { env, .. }, "env") => Term::list(env.clone()),
        (Fun::Local { index, .. }, "index") => Term::Int(*index as i64),
        _ => return Err(c.badarg()),
    };
    Ok(Term::tuple(alloc::vec![a[1].clone(), value]))
}

/// `erts_internal:cmp_term/2`: the exact term order (`1` and `1.0` differ), as -1, 0 or 1.
pub fn cmp_term(_c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(match a[0].cmp_exact(&a[1]) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }))
}
