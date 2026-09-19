//! Exceptions, processes, messages, links, monitors, names, the process dictionary and time.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::interp;
use crate::process::{Class, Exception, MaxHeap};
use crate::term::{OwnedTerm, Pid, Term};

type R = Result<Term, Exception>;

// ---- exceptions ----

pub fn error(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::error(a[0]))
}

/// `error/2` and `error/3`: the extra arguments only annotate the stack trace.
/// `error(Reason, Args)` and `error(Reason, Args, Options)`. As in BEAM, the trace starts at the
/// calling function, showing `Args` (a list, or `none` for just the arity), with the
/// `error_info` option (used by `erl_error` and Elixir to explain the error) added to its
/// location.
pub fn error2(c: &mut Ctx, a: &[Term]) -> R {
    let mut e = Exception::error(a[0]);
    let trace = crate::interp::caller_stacktrace(&mut c.sys(), c.p);
    let h = c.heap();
    let error_info = a.get(2).and_then(|opts| h.to_vec(*opts)).and_then(|opts| {
        opts.into_iter().find(
            |o| matches!(h.as_tuple(*o), Some(&[Term::Atom(k), _]) if k.as_str() == "error_info"),
        )
    });
    let args = h.to_vec(a[1]);
    let top = h
        .as_cons(trace)
        .and_then(|(head, tail)| Some((h.as_tuple(head)?.to_vec(), tail)));
    e.trace = Some(match top {
        Some((entry, tail)) if entry.len() == 4 => {
            let (m, f, arity, location) = (entry[0], entry[1], entry[2], entry[3]);
            let args = match args {
                Some(list) => c.list(list),
                None => arity,
            };
            let mut loc = c.heap().to_vec(location).unwrap_or_default();
            loc.extend(error_info);
            let loc = c.list(loc);
            let head = c.tuple(&[m, f, args, loc]);
            c.cons(head, tail)
        }
        _ => trace,
    });
    Err(e)
}

pub fn exit(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::exit(a[0]))
}

pub fn throw(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::throw(a[0]))
}

/// `erlang:raise(Class, Reason, Stacktrace)`. An invalid class makes it return `badarg`.
pub fn raise(c: &mut Ctx, a: &[Term]) -> R {
    let class = match &a[0] {
        t if t.is_atom(&c.atoms.error) => Class::Error,
        t if t.is_atom(&c.atoms.exit) => Class::Exit,
        t if t.is_atom(&c.atoms.throw) => Class::Throw,
        _ => return Ok(Term::Atom(c.atoms.badarg)),
    };
    Err(Exception::with_trace(class, a[1], a[2]))
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
    Ok(Term::Ref(c.sys().make_ref()))
}

// ---- spawning ----

fn do_spawn(c: &mut Ctx, entry: crate::process::Cp, args: Vec<Term>, link: bool) -> R {
    spawn_set_up(c, entry, args, link, |_| {})
}

/// Spawn a process and, before any scheduler can run it (under the same lock), make it a
/// child of the caller and apply `setup`.
fn spawn_set_up(
    c: &mut Ctx,
    entry: crate::process::Cp,
    args: Vec<Term>,
    link: bool,
    setup: impl FnOnce(&mut crate::process::Process),
) -> R {
    let mut sys = c.sys();
    let pid = sys.spawn_copy(entry, &c.p.heap, &args, false)?;
    if let Some(p) = sys.procs.get_mut(pid) {
        // A child inherits its parent's group leader.
        p.group_leader = c.p.group_leader.or(Some(c.p.pid));
        if link {
            p.links.insert(c.p.pid);
        }
        setup(p);
    }
    sys.hold_back(c.p, pid);
    drop(sys);
    if link {
        c.p.links.insert(pid);
    }
    Ok(Term::Pid(pid))
}

fn mfa_entry(c: &mut Ctx, a: &[Term]) -> Result<(crate::process::Cp, Vec<Term>), Exception> {
    let (Term::Atom(m), Term::Atom(f)) = (a[0], a[1]) else {
        return Err(c.badarg());
    };
    let args = c.list_arg(a[2])?;
    let found = c.sys().resolve(&m, &f, args.len() as u32);
    match found {
        Some(crate::vm::Target::Code(cp)) => Ok((cp, args)),
        // Spawning straight into a native, or into code that does not exist. BEAM spawns a
        // process that immediately fails with `undef`; so do we, by pointing it at nothing.
        _ => Err(Exception::error(Term::Atom(c.atoms.undef))),
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
    let (entry, args) = interp::fun_entry(&mut *c.sys(), &mut c.p.heap, a[0], Vec::new())?;
    do_spawn(c, entry, args, false)
}

pub fn spawn_link_fun(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(&mut *c.sys(), &mut c.p.heap, a[0], Vec::new())?;
    do_spawn(c, entry, args, true)
}

// ---- messages ----

/// Where a send goes: `Ok(Some(pid))`, or `Ok(None)` for `{Name, Node}` naming no process here
/// (another node, which is never connected, or a name nobody has): such messages are dropped,
/// as BEAM drops them. A bare name nobody has is `badarg`, as in BEAM.
fn destination(c: &mut Ctx, t: &Term) -> Result<Option<Pid>, Exception> {
    match *t {
        Term::Pid(p) => Ok(Some(p)),
        Term::Atom(name) => c
            .sys()
            .registered
            .get(name.as_str())
            .copied()
            .map(Some)
            .ok_or_else(|| c.badarg()),
        Term::Tuple(_) => match c.heap().as_tuple(*t) {
            Some(&[Term::Atom(name), Term::Atom(node)]) if node.as_str() == crate::etf::NODE => {
                Ok(c.sys().registered.get(name.as_str()).copied())
            }
            Some(&[Term::Atom(_), Term::Atom(_)]) => Ok(None),
            _ => Err(c.badarg()),
        },
        _ => Err(c.badarg()),
    }
}

/// Whether `t` is `{Name, Node}` for a node other than this one.
fn remote(c: &Ctx, t: &Term) -> bool {
    matches!(c.heap().as_tuple(*t), Some(&[Term::Atom(_), Term::Atom(n)]) if n.as_str() != crate::etf::NODE)
}

/// Send `msg` to `to`, which may be the running process itself. Every message goes through the
/// receiver's inbox, a message to itself too, as on BEAM: a process that keeps sending itself
/// messages must not starve the ones others sent before (the mailbox takes in the inbox only
/// when a receive has looked at everything in it).
pub fn send_to(c: &mut Ctx, to: Pid, msg: Term) {
    // Copied before the lock is taken: other schedulers need not wait for a big message.
    let fragment = OwnedTerm::new(&c.p.heap, msg);
    let delivered = c.sys().send_owned(to, fragment);
    if !delivered && to == c.p.pid {
        // Its own mailbox overflowed: it ends now, not at the end of its time slice.
        let reason = crate::vm::mailbox_full(&mut c.sys().atom_table, c.atoms);
        c.p.pending_exit = Some(reason.copy_into(&mut c.p.heap));
    }
}

pub fn send(c: &mut Ctx, a: &[Term]) -> R {
    // A reference is a destination while it is an active alias; otherwise the message is
    // dropped, as for a dead pid.
    if let Term::Ref(r) = &a[0] {
        let found = {
            let mut sys = c.sys();
            let found = sys.aliases.get(r).copied();
            // A reply alias takes one message: the first sender to get here.
            if found.is_some_and(|al| al.mode == crate::vm::AliasMode::ReplyDemonitor) {
                sys.aliases.remove(r);
            }
            found
        };
        if let Some(alias) = found {
            send_to(c, alias.owner, a[1]);
        }
        return Ok(a[1]);
    }
    if let Some(to) = destination(c, &a[0])? {
        send_to(c, to, a[1]);
    }
    Ok(a[1])
}

/// `send(Dest, Msg, Options)`: `noconnect` and `nosuspend` only matter between nodes; a send
/// to another node with `noconnect` returns `noconnect` (no node is ever connected).
pub fn send3(c: &mut Ctx, a: &[Term]) -> R {
    let opts = c.list_arg(a[2])?;
    if remote(c, &a[0])
        && opts
            .iter()
            .any(|o| matches!(o, Term::Atom(x) if x.as_str() == "noconnect"))
    {
        return Ok(c.atom("noconnect"));
    }
    send(c, &a[..2])?;
    Ok(c.ok())
}

// ---- aliases ----

fn monitor_options(
    c: &Ctx,
    opts: &Term,
) -> Result<(Option<crate::vm::AliasMode>, Option<Term>), Exception> {
    use crate::vm::AliasMode;
    let (mut mode, mut tag) = (None, None);
    for o in c.list_arg(*opts)? {
        match c.heap().as_tuple(o) {
            Some(&[Term::Atom(k), t]) if k.as_str() == "tag" => tag = Some(t),
            Some(&[Term::Atom(k), Term::Atom(v)]) if k.as_str() == "alias" => {
                mode = Some(match v.as_str() {
                    "explicit_unalias" => AliasMode::Explicit,
                    "demonitor" => AliasMode::Demonitor,
                    "reply_demonitor" => AliasMode::ReplyDemonitor,
                    _ => return Err(c.badarg()),
                });
            }
            _ => return Err(c.badarg()),
        }
    }
    Ok((mode, tag))
}

/// `alias()` and `alias(Options)`: a reference that routes messages to the caller.
pub fn alias(c: &mut Ctx, a: &[Term]) -> R {
    let mode = match a.first() {
        Some(opts) => {
            let opts = c.list_arg(*opts)?;
            // alias/1 takes plain atoms: explicit_unalias or reply.
            if opts
                .iter()
                .any(|o| matches!(o, Term::Atom(x) if x.as_str() == "reply"))
            {
                crate::vm::AliasMode::ReplyDemonitor
            } else if opts
                .iter()
                .all(|o| matches!(o, Term::Atom(x) if x.as_str() == "explicit_unalias"))
            {
                crate::vm::AliasMode::Explicit
            } else {
                return Err(c.badarg());
            }
        }
        None => crate::vm::AliasMode::Explicit,
    };
    let r = c.sys().make_ref();
    c.sys().aliases.insert(
        r,
        crate::vm::Alias {
            owner: c.p.pid,
            mode,
        },
    );
    Ok(Term::Ref(r))
}

pub fn unalias(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else {
        return Err(c.badarg());
    };
    let owned = c
        .sys()
        .aliases
        .get(&r)
        .is_some_and(|al| al.owner == c.p.pid);
    if owned {
        c.sys().aliases.remove(&r);
    }
    Ok(c.bool(owned))
}

/// `monitor(process, Target, Options)`: supports `{alias, Mode}`.
/// `monitor(process, Target, Options)`: `{alias, Mode}` and `{tag, Tag}` (the first element of
/// the message instead of `'DOWN'`).
pub fn monitor3(c: &mut Ctx, a: &[Term]) -> R {
    let (mode, tag) = monitor_options(c, &a[2])?;
    let r = monitor_tagged(c, &a[..2], tag)?;
    if let (Some(mode), Term::Ref(r)) = (mode, &r) {
        c.sys().aliases.insert(
            *r,
            crate::vm::Alias {
                owner: c.p.pid,
                mode,
            },
        );
    }
    Ok(r)
}

pub fn monitor(c: &mut Ctx, a: &[Term]) -> R {
    monitor_tagged(c, a, None)
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
    let kill = reason.is_atom(&c.atoms.kill);
    if c.p.trap_exit && !kill {
        let msg = c.tuple(&[Term::Atom(c.atoms.exit_upper), Term::Pid(c.p.pid), reason]);
        send_to(c, c.p.pid, msg);
    } else {
        c.p.pending_exit = Some(if kill {
            Term::Atom(c.atoms.killed)
        } else {
            reason
        });
    }
}

pub fn link(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    if pid == c.p.pid {
        return Ok(Term::Atom(c.atoms.true_));
    }
    let me = c.p.pid;
    let alive = c.sys().procs.update(pid, move |other| {
        other.links.insert(me);
    });
    match alive {
        true => {
            c.p.links.insert(pid);
        }
        false => {
            let noproc = Term::Atom(c.atoms.noproc);
            if c.p.trap_exit {
                let msg = c.tuple(&[Term::Atom(c.atoms.exit_upper), Term::Pid(pid), noproc]);
                send_to(c, c.p.pid, msg);
            } else {
                exit_self(c, noproc);
            }
        }
    }
    Ok(Term::Atom(c.atoms.true_))
}

pub fn unlink(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    c.p.links.remove(&pid);
    let me = c.p.pid;
    c.sys().procs.update(pid, move |other| {
        other.links.remove(&me);
    });
    Ok(Term::Atom(c.atoms.true_))
}

fn monitor_tagged(c: &mut Ctx, a: &[Term], tag: Option<Term>) -> R {
    // `process` or `port`: the kind of what is monitored, which must match.
    let port = match &a[0] {
        Term::Atom(k) if k.as_str() == "process" => false,
        Term::Atom(k) if k.as_str() == "port" => true,
        _ => return Err(c.badarg()),
    };
    if matches!(&a[1], Term::Pid(p) if p.port != port) {
        return Err(c.badarg());
    }
    let kind = a[0];
    let r = c.sys().make_ref();
    // A monitor by name reports `{Name, Node}` in its 'DOWN' message, as BEAM does.
    // By name: `Name` or `{Name, Node}`. This node is not distributed, so naming another node
    // is `badarg`, as in BEAM.
    let by_name = match a[1] {
        Term::Atom(name) => Some(name),
        Term::Tuple(_) => match c.heap().as_tuple(a[1]) {
            Some(&[Term::Atom(name), Term::Atom(node)]) if node.as_str() == crate::etf::NODE => {
                Some(name)
            }
            _ => return Err(c.badarg()),
        },
        _ => None,
    };
    let object = match (a[1], by_name) {
        (Term::Tuple(_), _) | (Term::Pid(_), _) => a[1],
        _ => {
            let node = c.atom(crate::etf::NODE);
            c.tuple(&[a[1], node])
        }
    };
    // Kept by the monitored process, outside both heaps.
    let monitor = crate::process::Monitor {
        watcher: c.p.pid,
        object: c.own(object),
        tag: tag.map(|t| c.own(t)),
    };
    // Finding the target and installing the monitor happen under one lock, so a target that
    // dies meanwhile cannot miss it.
    let installed = {
        let mut sys = c.sys();
        let target = match (a[1], by_name) {
            (Term::Pid(p), _) => Some(p),
            (_, Some(name)) => match sys.registered.get(name.as_str()) {
                Some(p) if p.port == port => Some(*p),
                Some(_) => return Err(c.badarg()),
                None => None,
            },
            _ => return Err(c.badarg()),
        };
        match target {
            Some(pid) if pid == c.p.pid => Some(None), // monitoring yourself never fires
            Some(pid) => sys
                .procs
                .update(pid, move |t| {
                    t.monitored_by.insert(r, monitor);
                })
                .then_some(Some(pid)),
            None => None,
        }
    };
    match installed {
        Some(Some(pid)) => {
            c.p.monitors.insert(r, pid);
        }
        Some(None) => {}
        None => {
            let parts = [
                tag.unwrap_or(Term::Atom(c.atoms.down)),
                Term::Ref(r),
                kind,
                object,
                Term::Atom(c.atoms.noproc),
            ];
            let msg = c.tuple(&parts);
            send_to(c, c.p.pid, msg);
        }
    }
    Ok(Term::Ref(r))
}

pub fn demonitor(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else {
        return Err(c.badarg());
    };
    if let Some(pid) = c.p.monitors.remove(&r) {
        c.sys().procs.update(pid, move |t| {
            t.monitored_by.remove(&r);
        });
    }
    let me = c.p.pid;
    let mut sys = c.sys();
    if sys
        .aliases
        .get(&r)
        .is_some_and(|al| al.owner == me && al.mode != crate::vm::AliasMode::Explicit)
    {
        sys.aliases.remove(&r);
    }
    drop(sys);
    Ok(Term::Atom(c.atoms.true_))
}

/// `demonitor(Ref, Options)`: `flush` drops a down message already queued; `info` makes the result
/// say whether the monitor was still active (`true`) or had already fired or gone (`false`).
pub fn demonitor2(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else {
        return Err(c.badarg());
    };
    let (mut flush, mut info) = (false, false);
    for o in c.list_arg(a[1])? {
        match &o {
            Term::Atom(x) if x.as_str() == "flush" => flush = true,
            Term::Atom(x) if x.as_str() == "info" => info = true,
            _ => return Err(c.badarg()),
        }
    }
    let active = c.p.monitors.contains_key(&r);
    demonitor(c, a)?;
    if flush {
        c.sys().receive_pending(c.p);
        // Any `{_, Ref, _, _, _}`: the first element may be a custom tag (`monitor/3`).
        let heap = &c.p.heap;
        c.p.mailbox
            .retain(|m| !matches!(heap.as_tuple(*m), Some(&[_, Term::Ref(x), _, _, _]) if x == r));
        c.p.save = 0;
    }
    Ok(c.bool(!info || active))
}

pub fn exit2(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    if pid == c.p.pid {
        exit_self(c, a[1]);
    } else {
        let reason = alloc::sync::Arc::new(c.own(a[1]));
        c.sys().signal_exit(crate::vm::ExitSignal {
            target: pid,
            from: c.p.pid,
            reason,
            from_link: false,
            forced: false,
        });
        // End the caller's time slice: the signal is delivered before it runs again, so what
        // it does next (process_info, is_process_alive, ...) sees the effect, as BEAM's signal
        // ordering between two processes guarantees.
        c.p.budget = c.p.budget.min(1);
    }
    Ok(Term::Atom(c.atoms.true_))
}

pub fn process_flag(c: &mut Ctx, a: &[Term]) -> R {
    if a[0].is_atom(&c.atoms.trap_exit) {
        let new = if a[1].is_atom(&c.atoms.true_) {
            true
        } else if a[1].is_atom(&c.atoms.false_) {
            false
        } else {
            return Err(c.badarg());
        };
        let old = core::mem::replace(&mut c.p.trap_exit, new);
        c.sys().procs.set_traps(c.p.pid, new);
        return Ok(c.bool(old));
    }
    // Flags that tune BEAM's implementation (distribution buffering, heap sizing, call
    // saving, trace hiding) and change nothing here: accepted, with BEAM's default as the old
    // value.
    if let Term::Atom(f) = &a[0] {
        let old = match f.as_str() {
            "async_dist" | "sensitive" => Some(c.bool(false)),
            "save_calls" => Some(Term::Int(0)),
            "message_queue_data" => Some(c.atom("on_heap")),
            "min_heap_size" => Some(Term::Int(233)),
            "min_bin_vheap_size" => Some(Term::Int(46422)),
            "fullsweep_after" => Some(Term::Int(65535)),
            _ => None,
        };
        if let Some(old) = old {
            return Ok(old);
        }
    }
    if matches!(&a[0], Term::Atom(f) if f.as_str() == "priority") {
        let new = match &a[1] {
            Term::Atom(p) => {
                crate::process::Priority::from_name(p.as_str()).ok_or_else(|| c.badarg())?
            }
            _ => return Err(c.badarg()),
        };
        let old = core::mem::replace(&mut c.p.priority, new);
        return Ok(c.atom(old.name()));
    }
    if matches!(&a[0], Term::Atom(f) if f.as_str() == "error_handler") {
        let Term::Atom(new) = &a[1] else {
            return Err(c.badarg());
        };
        let new = (new.as_str() != "error_handler").then_some(*new);
        let old = core::mem::replace(&mut c.p.error_handler, new);
        return Ok(match old {
            Some(m) => Term::Atom(m),
            None => c.atom("error_handler"),
        });
    }
    if matches!(&a[0], Term::Atom(f) if f.as_str() == "max_heap_size") {
        let new = parse_max_heap(c, &a[1])?;
        let old = core::mem::replace(&mut c.p.max_heap, new);
        return Ok(max_heap_term(
            &mut c.sys().atom_table,
            c.atoms,
            &mut c.p.heap,
            old,
        ));
    }
    Err(c.badarg())
}

/// A `max_heap_size` setting: a size in words, or a map of `size`, `kill`, `error_logger` and
/// `include_shared_binaries` (keys left out take their defaults).
fn parse_max_heap(c: &Ctx, v: &Term) -> Result<MaxHeap, Exception> {
    let size = |t: &Term| t.as_usize().map(|n| n as u64).ok_or_else(|| c.badarg());
    let flag = |t: &Term| match t {
        Term::Atom(a) if *a == c.atoms.true_ => Ok(true),
        Term::Atom(a) if *a == c.atoms.false_ => Ok(false),
        _ => Err(c.badarg()),
    };
    let mut m = MaxHeap::default();
    match v {
        Term::Map(_) => {
            for (k, v) in c.heap().map_entries(*v).expect("a map") {
                match k {
                    Term::Atom(a) if a.as_str() == "size" => m.size = size(&v)?,
                    Term::Atom(a) if a.as_str() == "kill" => m.kill = flag(&v)?,
                    Term::Atom(a) if a.as_str() == "error_logger" => m.error_logger = flag(&v)?,
                    Term::Atom(a) if a.as_str() == "include_shared_binaries" => {
                        m.include_shared_binaries = flag(&v)?
                    }
                    _ => return Err(c.badarg()),
                }
            }
        }
        other => m.size = size(other)?,
    }
    Ok(m)
}

/// A `max_heap_size` setting as `process_flag/2` and `process_info/2` report it.
pub(crate) fn max_heap_term(
    table: &mut crate::atom::AtomTable,
    atoms: &crate::atom::Atoms,
    heap: &mut crate::term::Heap,
    m: MaxHeap,
) -> Term {
    let b = |v: bool| Term::Atom(if v { atoms.true_ } else { atoms.false_ });
    let pairs: Vec<(Term, Term)> = [
        ("error_logger", b(m.error_logger)),
        ("include_shared_binaries", b(m.include_shared_binaries)),
        ("kill", b(m.kill)),
        ("size", Term::Int(m.size as i64)),
    ]
    .into_iter()
    .map(|(k, v)| (Term::Atom(table.intern(k).expect("short atom")), v))
    .collect();
    heap.map_from(pairs)
}

pub fn is_process_alive(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])
        .ok()
        .filter(|p| !p.port)
        .ok_or_else(|| c.badarg())?;
    let alive = pid == c.p.pid || c.sys().procs.is_alive(pid);
    Ok(c.bool(alive))
}

// ---- registered names ----

pub fn register(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = a[0] else {
        return Err(c.badarg());
    };
    let pid = pid_arg(c, &a[1])?;
    let mut sys = c.sys();
    // A name names one process, and a process has at most one name (the table says which).
    if name == c.atoms.undefined
        || sys.registered.contains_key(name.as_str())
        || sys.registered.values().any(|&p| p == pid)
    {
        return Err(c.badarg());
    }
    if pid == c.p.pid {
        c.p.registered_name = Some(name);
    } else if !sys
        .procs
        .update(pid, move |p| p.registered_name = Some(name))
    {
        return Err(c.badarg());
    }
    sys.registered.insert(name.as_str().into(), pid);
    Ok(Term::Atom(c.atoms.true_))
}

pub fn unregister(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else {
        return Err(c.badarg());
    };
    let pid = c
        .sys()
        .registered
        .remove(name.as_str())
        .ok_or_else(|| c.badarg())?;
    if pid == c.p.pid {
        c.p.registered_name = None;
    } else {
        c.sys().procs.update(pid, |p| p.registered_name = None);
    }
    Ok(Term::Atom(c.atoms.true_))
}

pub fn whereis(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else {
        return Err(c.badarg());
    };
    Ok(match c.sys().registered.get(name.as_str()) {
        Some(p) => Term::Pid(*p),
        None => Term::Atom(c.atoms.undefined),
    })
}

// ---- process dictionary ----

pub fn put(c: &mut Ctx, a: &[Term]) -> R {
    let old = c.p.dictionary.put(&c.p.heap, a[0], a[1]);
    Ok(old.unwrap_or(Term::Atom(c.atoms.undefined)))
}

pub fn get(c: &mut Ctx, a: &[Term]) -> R {
    let v = c.p.dictionary.get(&c.p.heap, a[0]);
    Ok(v.unwrap_or(Term::Atom(c.atoms.undefined)))
}

pub fn get_all(c: &mut Ctx, _a: &[Term]) -> R {
    let entries = c.p.dictionary.entries().to_vec();
    let v: Vec<Term> = entries.into_iter().map(|(k, v)| c.tuple(&[k, v])).collect();
    Ok(c.list(v))
}

pub fn erase(c: &mut Ctx, a: &[Term]) -> R {
    let old = c.p.dictionary.remove(&c.p.heap, a[0]);
    Ok(old.unwrap_or(Term::Atom(c.atoms.undefined)))
}

pub fn group_leader(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Pid(c.p.group_leader.unwrap_or(c.p.pid)))
}

/// `group_leader(Leader, Pid)`: make `Leader` the group leader of `Pid`.
pub fn set_group_leader(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Pid(leader), Term::Pid(pid)) = (&a[0], &a[1]) else {
        return Err(c.badarg());
    };
    if *pid == c.p.pid {
        c.p.group_leader = Some(*leader);
    } else {
        let leader = *leader;
        if !c
            .sys()
            .procs
            .update(*pid, move |p| p.group_leader = Some(leader))
        {
            return Err(c.badarg());
        }
    }
    Ok(Term::Atom(c.atoms.true_))
}

// ---- timers ----

/// The deadline for `Time` (milliseconds, relative unless `{abs, true}`), in platform time.
fn timer_deadline(c: &mut Ctx, time: &Term, opts: Option<&Term>) -> Result<u64, Exception> {
    let ms = time
        .as_i64()
        .filter(|t| (0..=u32::MAX as i64).contains(t))
        .ok_or_else(|| c.badarg())? as u64;
    let mut abs = false;
    if let Some(opts) = opts {
        for o in c.list_arg(*opts)? {
            match c.heap().as_tuple(o) {
                Some(&[Term::Atom(k), v]) if k.as_str() == "abs" => abs = v.is_atom(&c.atoms.true_),
                _ => return Err(c.badarg()),
            }
        }
    }
    let now = c.sys().now_us();
    Ok(if abs {
        ms.saturating_mul(1000)
    } else {
        now.saturating_add(ms * 1000)
    })
}

fn timer_target(c: &Ctx, t: &Term) -> Result<Term, Exception> {
    match t {
        Term::Pid(_) | Term::Atom(_) => Ok(*t),
        _ => Err(c.badarg()),
    }
}

/// `start_timer(Time, Dest, Msg[, Opts])`: after `Time` ms, `Dest` gets `{timeout, Ref, Msg}`.
pub fn start_timer(c: &mut Ctx, a: &[Term]) -> R {
    let deadline = timer_deadline(c, &a[0], a.get(3))?;
    let to = timer_target(c, &a[1])?;
    // The message carries the timer's own reference, so reserve it first.
    let placeholder = crate::term::OwnedTerm::immediate(Term::Nil);
    let r = c
        .sys()
        .start_message_timer(deadline, to, placeholder)
        .ok_or_else(|| c.system_limit())?;
    let timeout = c.atom("timeout");
    let msg = c.tuple(&[timeout, Term::Ref(r), a[2]]);
    let msg = c.own(msg);
    c.sys().message_timers.insert(r, (deadline, to, msg));
    Ok(Term::Ref(r))
}

/// `send_after(Time, Dest, Msg[, Opts])`: after `Time` ms, `Dest` gets `Msg`.
pub fn send_after(c: &mut Ctx, a: &[Term]) -> R {
    let deadline = timer_deadline(c, &a[0], a.get(3))?;
    let to = timer_target(c, &a[1])?;
    let msg = c.own(a[2]);
    let r = c
        .sys()
        .start_message_timer(deadline, to, msg)
        .ok_or_else(|| c.system_limit())?;
    Ok(Term::Ref(r))
}

fn remaining_ms(c: &mut Ctx, deadline: Option<u64>) -> Term {
    match deadline {
        Some(d) => Term::Int((d.saturating_sub(c.sys().now_us()) / 1000) as i64),
        None => c.bool(false),
    }
}

/// `cancel_timer(Ref[, Opts])`: milliseconds that were left, or `false`. With `{async, true}` the
/// answer comes as a message `{cancel_timer, Ref, Result}`; with `{info, false}` there is none.
pub fn cancel_timer(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else {
        return Err(c.badarg());
    };
    let (mut asynchronous, mut info) = (false, true);
    if let Some(opts) = a.get(1) {
        for o in c.list_arg(*opts)? {
            match c.heap().as_tuple(o) {
                Some(&[Term::Atom(k), v]) if k.as_str() == "async" => {
                    asynchronous = v.is_atom(&c.atoms.true_)
                }
                Some(&[Term::Atom(k), v]) if k.as_str() == "info" => {
                    info = v.is_atom(&c.atoms.true_)
                }
                _ => return Err(c.badarg()),
            }
        }
    }
    let left = c.sys().cancel_message_timer(r);
    let result = remaining_ms(c, left);
    if asynchronous {
        if info {
            let tag = c.atom("cancel_timer");
            let msg = c.tuple(&[tag, Term::Ref(r), result]);
            send_to(c, c.p.pid, msg);
        }
        return Ok(c.ok());
    }
    Ok(if info { result } else { c.ok() })
}

pub fn read_timer(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else {
        return Err(c.badarg());
    };
    let deadline = c.sys().message_timers.get(&r).map(|(d, _, _)| *d);
    Ok(remaining_ms(c, deadline))
}

/// `erts_internal:time_unit()` and `perf_counter_unit()`: the native time unit is the
/// nanosecond.
pub fn time_unit(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Int(1_000_000_000))
}

// ---- persistent_term ----

/// Most keys `persistent_term` may hold; it is VM-wide state any process can grow.
const MAX_PERSISTENT_TERMS: usize = 1 << 16;

pub fn pt_put(c: &mut Ctx, a: &[Term]) -> R {
    let key = c.own(a[0]);
    let full = {
        let sys = c.sys();
        !sys.persistent.contains_key(&key) && sys.persistent.len() >= MAX_PERSISTENT_TERMS
    };
    if full {
        return Err(c.system_limit());
    }
    // The value becomes a literal, as in BEAM: reading it copies nothing. (A value replaced is
    // not freed; BEAM frees it once no process refers to it.)
    let value = c.sys().make_literal(&c.p.heap, a[1]);
    c.sys().persistent.insert(key, value);
    Ok(c.ok())
}

/// `get(Key)` (`badarg` if absent) and `get(Key, Default)`.
pub fn pt_get(c: &mut Ctx, a: &[Term]) -> R {
    let key = c.own(a[0]);
    let found = c.sys().persistent.get(&key).copied();
    match found {
        Some(v) => {
            c.p.refresh(&c.sys().literals);
            Ok(v)
        }
        None => a.get(1).copied().ok_or_else(|| c.badarg()),
    }
}

pub fn pt_get_all(c: &mut Ctx, _a: &[Term]) -> R {
    c.p.refresh(&c.sys().literals);
    let entries: Vec<(crate::term::OwnedTerm, Term)> = c
        .sys()
        .persistent
        .iter()
        .map(|(k, v)| (k.clone(), *v))
        .collect();
    let v: Vec<Term> = entries
        .into_iter()
        .map(|(k, v)| {
            let k = c.copy_in(&k);
            c.tuple(&[k, v])
        })
        .collect();
    Ok(c.list(v))
}

pub fn pt_erase(c: &mut Ctx, a: &[Term]) -> R {
    let key = c.own(a[0]);
    let existed = c.sys().persistent.remove(&key).is_some();
    Ok(c.bool(existed))
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

fn scaled(c: &mut Ctx, us: u64, per_second: u64) -> Term {
    c.from_i128(us as i128 * per_second as i128 / 1_000_000)
}

/// The native time unit is the nanosecond, as on a typical BEAM.
pub fn monotonic_time(c: &mut Ctx, _a: &[Term]) -> R {
    let now = c.sys().now_us();
    Ok(scaled(c, now, 1_000_000_000))
}

pub fn monotonic_time1(c: &mut Ctx, a: &[Term]) -> R {
    let per = unit_per_second(c, &a[0])?;
    let now = c.sys().now_us();
    Ok(scaled(c, now, per))
}

fn wall_us(c: &mut Ctx) -> Result<u64, Exception> {
    c.platform().system_time_us().ok_or_else(|| c.badarg())
}

pub fn system_time(c: &mut Ctx, _a: &[Term]) -> R {
    let now = wall_us(c)?;
    Ok(scaled(c, now, 1_000_000_000))
}

pub fn system_time1(c: &mut Ctx, a: &[Term]) -> R {
    let per = unit_per_second(c, &a[0])?;
    let now = wall_us(c)?;
    Ok(scaled(c, now, per))
}

pub fn yield_(c: &mut Ctx, _a: &[Term]) -> R {
    c.p.budget = 0;
    Ok(Term::Atom(c.atoms.true_))
}

// ---- code ----

pub fn function_exported(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Atom(m), Term::Atom(f), Some(arity)) = (&a[0], &a[1], a[2].as_usize()) else {
        return Err(c.badarg());
    };
    let arity = arity as u32;
    let exported = {
        let mut sys = c.sys();
        sys.native(m, f, arity).is_some()
            || (sys.is_loaded(m)
                && sys
                    .module(m)
                    .is_some_and(|md| md.export(f, arity).is_some()))
    };
    Ok(c.bool(exported))
}

pub fn module_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    let loaded = c.sys().is_loaded(m);
    Ok(c.bool(loaded))
}

// ---- spawn_opt ----

/// `spawn_opt(Fun, Options)` and `spawn_opt(M, F, A, Options)`. Supported options: `link`,
/// `monitor` and `max_heap_size`. Tuning options (`min_heap_size`, `priority`, ...) are accepted
/// and ignored: this VM has no per-process heaps and one priority.
fn spawn_with(c: &mut Ctx, entry: crate::process::Cp, args: Vec<Term>, opts: &Term) -> R {
    let opts = c.list_arg(*opts)?;
    let (mut link, mut monitor, mut max_heap, mut priority) = (false, false, None, None);
    for o in &opts {
        if let Term::Atom(a) = o {
            match a.as_str() {
                "link" => link = true,
                "monitor" => monitor = true,
                _ => return Err(c.badarg()),
            }
            continue;
        }
        let t = c.tuple_elems(*o).ok_or_else(|| c.badarg())?;
        let key = match t.first() {
            Some(Term::Atom(a)) => a.as_str(),
            _ => "",
        };
        match (key, &t[..]) {
            ("monitor", _) => monitor = true,
            ("max_heap_size", &[_, v]) => max_heap = Some(parse_max_heap(c, &v)?),
            ("priority", &[_, v]) => {
                let Term::Atom(p) = v else {
                    return Err(c.badarg());
                };
                priority = Some(
                    crate::process::Priority::from_name(p.as_str()).ok_or_else(|| c.badarg())?,
                );
            }
            (_, &[_, _]) => {}
            _ => return Err(c.badarg()),
        }
    }
    let r = monitor.then(|| c.sys().make_ref());
    let watcher = c.p.pid;
    let spawned = spawn_set_up(c, entry, args, link, |p| {
        if let Some(m) = max_heap {
            p.max_heap = m;
        }
        if let Some(pr) = priority {
            p.priority = pr;
        }
        if let Some(r) = r {
            let object = OwnedTerm::immediate(Term::Pid(p.pid));
            p.monitored_by.insert(
                r,
                crate::process::Monitor {
                    watcher,
                    object,
                    tag: None,
                },
            );
        }
    })?;
    let (Term::Pid(pid), Some(r)) = (spawned, r) else {
        return Ok(spawned);
    };
    c.p.monitors.insert(r, pid);
    Ok(c.tuple(&[Term::Pid(pid), Term::Ref(r)]))
}

/// `spawn_monitor(Fun)` and `spawn_monitor(M, F, A)`: `{Pid, Ref}`.
pub fn spawn_monitor1(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(&mut *c.sys(), &mut c.p.heap, a[0], Vec::new())?;
    let m = c.atom("monitor");
    let monitor = c.list([m]);
    spawn_with(c, entry, args, &monitor)
}

pub fn spawn_monitor3(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = mfa_entry(c, a)?;
    let m = c.atom("monitor");
    let monitor = c.list([m]);
    spawn_with(c, entry, args, &monitor)
}

pub fn spawn_opt2(c: &mut Ctx, a: &[Term]) -> R {
    let (entry, args) = interp::fun_entry(&mut *c.sys(), &mut c.p.heap, a[0], Vec::new())?;
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

/// `erlang:system_info/1` for the keys portable code asks about. `machine` is `"BEAM"`: this VM
/// implements BEAM semantics, and code that checks it should take its BEAM path.
pub fn system_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(key) = &a[0] else {
        return Err(c.badarg());
    };
    Ok(match key.as_str() {
        "machine" => c.string("BEAM"),
        "otp_release" => c.string(OTP_RELEASE),
        "version" => c.string(ERTS_VERSION),
        "wordsize" => Term::Int(8),
        "process_count" => Term::Int(c.sys().procs.count() as i64),
        "process_limit" => Term::Int(crate::vm::MAX_PROCESSES as i64),
        "atom_count" => Term::Int(c.sys().atom_table.len() as i64),
        "atom_limit" => Term::Int(crate::atom::MAX_ATOMS as i64),
        "port_count" => Term::Int(0),
        "schedulers" | "logical_processors" => Term::Int(c.sys().schedulers as i64),
        "schedulers_online" => Term::Int(c.sys().schedulers_online as i64),
        "emu_flavor" => c.atom("emu"),
        "system_architecture" => c.string("beamlet"),
        "system_version" => return super::info::system_version(c, a),
        "os_type" => {
            let e = [c.atom("unix"), c.atom("beamlet")];
            c.tuple(&e)
        }
        // Every target (x86-64, AArch64, RISC-V) is little-endian; binaries are portable anyway.
        "endian" => c.atom("little"),
        "build_type" => c.atom("opt"),
        "debug_compiled" | "kernel_poll" | "dynamic_trace_probes" => c.bool(false),
        "threads" | "smp_support" => c.bool(true),
        "dynamic_trace" => c.atom("none"),
        "thread_pool_size"
        | "dirty_cpu_schedulers"
        | "dirty_io_schedulers"
        | "dirty_cpu_schedulers_online" => Term::Int(0),
        "compat_rel" => Term::Int(28),
        "nif_version" => c.string("2.17"),
        "driver_version" => c.string("3.3"),
        "time_warp_mode" => c.atom("no_time_warp"),
        "time_offset" => c.atom("final"),
        "min_heap_size" => {
            let e = [c.atom("min_heap_size"), Term::Int(233)];
            c.tuple(&e)
        }
        "fullsweep_after" => {
            let e = [c.atom("fullsweep_after"), Term::Int(65535)];
            c.tuple(&e)
        }
        "max_heap_size" => {
            let m = max_heap_term(
                &mut c.sys().atom_table,
                c.atoms,
                &mut c.p.heap,
                MaxHeap::default(),
            );
            let k = c.atom("max_heap_size");
            c.tuple(&[k, m])
        }
        "ets_limit" => Term::Int(crate::ets::MAX_TABLES as i64),
        "backtrace_depth" => Term::Int(c.sys().backtrace_depth as i64),
        _ => return Err(c.badarg()),
    })
}

/// `net_kernel:dflag_unicode_io(Pid)`: whether an I/O server understands Unicode requests.
/// Asked by `io` before every request; all servers here are local and Unicode-capable.
pub fn dflag_unicode_io(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Atom(c.atoms.true_))
}

/// `io:printable_range()`: which characters `~p` prints as text. BEAM's default is `latin1`.
pub fn printable_range(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Atom(c.atoms.latin1))
}

// ---- the VM as a whole ----

/// `halt()`, `halt(Status)`, `halt(Status, Options)`: stop the VM. A string status (a crash
/// slogan) or `abort` stops it with status 1.
pub fn halt(c: &mut Ctx, a: &[Term]) -> R {
    let status = match a.first() {
        None => 0,
        Some(Term::Int(n)) if *n >= 0 => *n,
        Some(Term::Atom(x)) if x.as_str() == "abort" => 1,
        Some(t) if c.heap().to_vec(*t).is_some() => 1,
        _ => return Err(c.badarg()),
    };
    if let Some(opts) = a.get(1) {
        c.list_arg(*opts)?;
    }
    c.sys().halted = Some(status);
    c.sys().wake_all = true;
    // Nothing more of this process runs.
    c.p.pending_exit = Some(c.atom("kill"));
    Ok(c.ok())
}

/// `erlang:statistics(Item)` for the items that mean something here. Run time is wall time
/// since the VM started: there is no separate CPU clock.
pub fn statistics(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(item) = a[0] else {
        return Err(c.badarg());
    };
    let pair = |c: &mut Ctx, x: u64, y: u64| c.tuple(&[Term::Int(x as i64), Term::Int(y as i64)]);
    let start = c.sys().stats.start_us;
    let now = c.platform().monotonic_us() - start;
    let queue = c.sys().run_queue.len() as i64;
    Ok(match item.as_str() {
        "runtime" | "wall_clock" => {
            let last = if item.as_str() == "runtime" {
                &mut c.sys().stats.last_runtime_us
            } else {
                &mut c.sys().stats.last_wall_us
            };
            let since = now - core::mem::replace(last, now);
            pair(c, now / 1000, since / 1000)
        }
        "reductions" | "exact_reductions" => {
            // The running process's current slice is counted too.
            let total = c.sys().stats.reductions
                + (crate::vm::TIME_SLICE - c.p.budget.min(crate::vm::TIME_SLICE)) as u64;
            let since = total - core::mem::replace(&mut c.sys().stats.last_reductions, total);
            pair(c, total, since)
        }
        "context_switches" => {
            let n = c.sys().stats.context_switches;
            pair(c, n, 0)
        }
        "garbage_collection" => c.tuple(&[Term::Int(0), Term::Int(0), Term::Int(0)]),
        "io" => {
            let (i, o) = (c.atom("input"), c.atom("output"));
            let (i, o) = (c.tuple(&[i, Term::Int(0)]), c.tuple(&[o, Term::Int(0)]));
            c.tuple(&[i, o])
        }
        "run_queue" | "total_run_queue_lengths" | "total_run_queue_lengths_all" => Term::Int(queue),
        "run_queue_lengths" | "run_queue_lengths_all" => c.list([Term::Int(queue)]),
        "total_active_tasks" | "total_active_tasks_all" => Term::Int(queue + 1),
        "active_tasks" | "active_tasks_all" => c.list([Term::Int(queue + 1)]),
        "scheduler_wall_time" | "scheduler_wall_time_all" | "microstate_accounting" => {
            Term::Atom(c.atoms.undefined)
        }
        _ => return Err(c.badarg()),
    })
}

pub fn registered(c: &mut Ctx, _a: &[Term]) -> R {
    let names: Vec<String> = c.sys().registered.keys().cloned().collect();
    let v: Vec<Term> = names.iter().map(|n| c.atom(n)).collect();
    Ok(c.list(v))
}

/// `get_keys()`: the process dictionary's keys; `get_keys(Value)`: those whose value is
/// exactly `Value`.
pub fn get_keys(c: &mut Ctx, a: &[Term]) -> R {
    let h = &c.p.heap;
    let keys: Vec<Term> =
        c.p.dictionary
            .entries()
            .iter()
            .filter(|(_, v)| a.first().is_none_or(|w| h.eq_exact(*v, *w)))
            .map(|(k, _)| *k)
            .collect();
    Ok(c.list(keys))
}

/// `now()`: `{MegaSecs, Secs, MicroSecs}` of the system clock, strictly increasing.
pub fn now(c: &mut Ctx, _a: &[Term]) -> R {
    let us = wall_us(c)?.max(c.sys().stats.last_now_us + 1);
    c.sys().stats.last_now_us = us;
    Ok(c.tuple(&[
        Term::Int((us / 1_000_000_000_000) as i64),
        Term::Int((us / 1_000_000 % 1_000_000) as i64),
        Term::Int((us % 1_000_000) as i64),
    ]))
}

/// `time_offset()` and `time_offset(Unit)`: system time minus monotonic time.
pub fn time_offset(c: &mut Ctx, a: &[Term]) -> R {
    let per = match a.first() {
        Some(u) => unit_per_second(c, u)?,
        None => 1_000_000_000,
    };
    let offset = wall_us(c)? as i128 - c.sys().now_us() as i128;
    Ok(c.from_i128(offset * per as i128 / 1_000_000))
}

/// `bump_reductions(N)`: use up `N` reductions of this time slice.
pub fn bump_reductions(c: &mut Ctx, a: &[Term]) -> R {
    let n = a[0].as_usize().ok_or_else(|| c.badarg())?;
    c.p.budget = c.p.budget.saturating_sub(n).max(1);
    Ok(c.bool(true))
}

/// `link(Pid, Options)`: the options (`priority`, OTP 28) do not change anything here.
pub fn link2(c: &mut Ctx, a: &[Term]) -> R {
    c.heap().to_vec(a[1]).ok_or_else(|| c.badarg())?;
    link(c, &a[..1])
}

pub fn pre_loaded(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Nil)
}

/// `persistent_term:put_new(Key, Value)`: store a new key. A key already there is `ok` if it
/// holds the same value, `badarg` otherwise.
pub fn pt_put_new(c: &mut Ctx, a: &[Term]) -> R {
    let key = c.own(a[0]);
    let found = c.sys().persistent.get(&key).copied();
    match found {
        Some(v) => {
            c.p.refresh(&c.sys().literals);
            if c.heap().eq_exact(v, a[1]) {
                Ok(c.ok())
            } else {
                Err(c.badarg())
            }
        }
        None => pt_put(c, a),
    }
}

/// `persistent_term:info()`: `#{count, memory}`.
pub fn pt_info(c: &mut Ctx, _a: &[Term]) -> R {
    let count = c.sys().persistent.len() as i64;
    // Keys, and values (literals: counted by their own chunks, a cell each at least).
    let words: u64 = c.sys().persistent.keys().map(|k| k.words() + 2).sum();
    let (count_k, memory_k) = (c.atom("count"), c.atom("memory"));
    Ok(c.map_from([
        (count_k, Term::Int(count)),
        (memory_k, Term::Int((words * 8) as i64)),
    ]))
}

/// `erlang:system_flag(Flag, Value)`: `backtrace_depth` takes effect; the other flags that
/// portable code sets are accepted and report BEAM's defaults as their old values.
pub fn system_flag(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(flag) = &a[0] else {
        return Err(c.badarg());
    };
    Ok(match flag.as_str() {
        "backtrace_depth" => {
            let n = a[1].as_usize().ok_or_else(|| c.badarg())?.min(1024);
            Term::Int(core::mem::replace(&mut c.sys().backtrace_depth, n) as i64)
        }
        "schedulers_online" => {
            let mut sys = c.sys();
            let n = a[1]
                .as_usize()
                .filter(|n| (1..=sys.schedulers).contains(n))
                .ok_or_else(|| c.badarg())?;
            let old = core::mem::replace(&mut sys.schedulers_online, n);
            // Parked helpers look again.
            sys.wake_all = true;
            Term::Int(old as i64)
        }
        "dirty_cpu_schedulers_online" => Term::Int(1),
        "multi_scheduling" => c.atom("enabled"),
        "min_heap_size" | "min_bin_vheap_size" => Term::Int(233),
        "fullsweep_after" => Term::Int(65535),
        "trace_control_word" => Term::Int(0),
        "time_offset" => c.atom("final"),
        "scheduler_wall_time" | "microstate_accounting" | "system_logger" => c.bool(false),
        "max_heap_size" => {
            let m = max_heap_term(
                &mut c.sys().atom_table,
                c.atoms,
                &mut c.p.heap,
                MaxHeap::default(),
            );
            let k = c.atom("max_heap_size");
            c.tuple(&[k, m])
        }
        _ => return Err(c.badarg()),
    })
}

pub fn ok_any(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.ok())
}

pub fn nil_any(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Nil)
}

/// `monitor_node(Node, Flag)`: with no distribution every other node is down at once, so the
/// caller gets `{nodedown, Node}` straight away (BEAM does the same for an unreachable node).
pub fn monitor_node(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(_) = &a[0] else {
        return Err(c.badarg());
    };
    if a[1].is_atom(&c.atoms.true_) {
        let tag = c.atom("nodedown");
        let msg = c.tuple(&[tag, a[0]]);
        send_to(c, c.p.pid, msg);
    }
    Ok(c.bool(true))
}

pub fn ok_2(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.ok())
}

pub fn false_1(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(false))
}

pub fn nif_error(_c: &mut Ctx, a: &[Term]) -> R {
    Err(Exception::error(a[0]))
}

pub fn garbage_collect(c: &mut Ctx, _a: &[Term]) -> R {
    // Collection happens between instructions; ask for one at the next (this native's own
    // locals are not roots, so it cannot collect here).
    c.p.gc_at = 0;
    Ok(Term::Atom(c.atoms.true_))
}

pub fn erase_all(c: &mut Ctx, _a: &[Term]) -> R {
    let old = c.p.dictionary.take();
    let v: Vec<Term> = old.into_iter().map(|(k, v)| c.tuple(&[k, v])).collect();
    Ok(c.list(v))
}

/// `unique_integer()` and `unique_integer([positive | monotonic])`. References come from a
/// VM-wide increasing counter, so every result is unique, positive and monotonic.
pub fn unique_integer(c: &mut Ctx, a: &[Term]) -> R {
    if let Some(opts) = a.first() {
        for o in c.list_arg(*opts)? {
            match &o {
                Term::Atom(x) if matches!(x.as_str(), "positive" | "monotonic") => {}
                _ => return Err(c.badarg()),
            }
        }
    }
    Ok(Term::Int(c.sys().make_ref().0 as i64))
}

pub fn timestamp(c: &mut Ctx, _a: &[Term]) -> R {
    let us = wall_us(c)?;
    Ok(c.tuple(&[
        Term::Int((us / 1_000_000_000_000) as i64),
        Term::Int((us / 1_000_000 % 1_000_000) as i64),
        Term::Int((us % 1_000_000) as i64),
    ]))
}

pub fn make_fun(c: &mut Ctx, a: &[Term]) -> R {
    let (Term::Atom(m), Term::Atom(f), Some(arity)) = (a[0], a[1], a[2].as_usize()) else {
        return Err(c.badarg());
    };
    if arity > 255 {
        return Err(c.badarg());
    }
    Ok(c.heap_mut().fun_export(m, f, arity as u32))
}

pub fn fun_info(c: &mut Ctx, a: &[Term]) -> R {
    use crate::term::FunView;
    let Term::Atom(item) = a[1] else {
        return Err(c.badarg());
    };
    let f = c.heap().as_fun(a[0]).ok_or_else(|| c.badarg())?;
    // Copy out what the answer needs, so the heap is free for building it.
    let (arity, local) = (
        f.arity(),
        match f {
            FunView::Local {
                module,
                index,
                uniq,
                name,
                env,
                ..
            } => Some((module, index, uniq, name, env.to_vec())),
            FunView::Export { .. } => None,
        },
    );
    let (module, function) = match f {
        FunView::Local { module, .. } => (module, None),
        FunView::Export {
            module, function, ..
        } => (module, Some(function)),
    };
    let value = match (local, item.as_str()) {
        (_, "module") => Term::Atom(module),
        (_, "arity") => Term::Int(arity as i64),
        (None, "name") => Term::Atom(function.expect("an export fun")),
        (Some((module, index, uniq, name, _)), "name") => {
            // From the module's fun table when this is still its fun (decoded funs do not
            // carry a name), else the name recorded when the fun was made.
            let current = c.sys().module(&module).and_then(|m| {
                m.funs
                    .get(index as usize)
                    .filter(|e| e.uniq == uniq)
                    .map(|e| e.function)
            });
            Term::Atom(current.unwrap_or(name))
        }
        (None, "type") => c.atom("external"),
        (Some(_), "type") => c.atom("local"),
        (None, "env") => Term::Nil,
        (Some((_, _, _, _, env)), "env") => c.list(env),
        (Some((_, index, ..)), "index" | "new_index") => Term::Int(index as i64),
        (Some((_, _, uniq, ..)), "uniq") => Term::Int(uniq as i64),
        (Some((module, ..)), "new_uniq") => {
            let md5 = c.sys().loaded_md5(&module).unwrap_or([0; 16]);
            c.binary(&md5)
        }
        // Funs do not record their creator; BEAM reports the same for funs it did not track.
        (Some(_), "pid") => Term::Pid(Pid::process(0, 0)),
        (Some(_), "refc") => Term::Int(1),
        (None, "pid" | "index" | "new_index" | "uniq" | "new_uniq" | "refc") => {
            Term::Atom(c.atoms.undefined)
        }
        _ => return Err(c.badarg()),
    };
    Ok(c.tuple(&[a[1], value]))
}

/// `erts_internal:cmp_term/2`: the exact term order (`1` and `1.0` differ), as -1, 0 or 1.
/// `fun_info_mfa(Fun)`: `{Module, Function, Arity}` of the code behind a fun.
pub fn fun_info_mfa(c: &mut Ctx, a: &[Term]) -> R {
    use crate::term::FunView;
    let f = c.heap().as_fun(a[0]).ok_or_else(|| c.badarg())?;
    let arity = f.arity();
    let (m, name) = match f {
        FunView::Export {
            module, function, ..
        } => (module, function),
        FunView::Local { module, index, .. } => {
            let md = c.sys().module(&module).ok_or_else(|| c.badarg())?;
            (
                module,
                md.funs
                    .get(index as usize)
                    .ok_or_else(|| c.badarg())?
                    .function,
            )
        }
    };
    Ok(c.tuple(&[Term::Atom(m), Term::Atom(name), Term::Int(arity as i64)]))
}

pub fn cmp_term(c: &mut Ctx, a: &[Term]) -> R {
    Ok(Term::Int(match c.heap().cmp_exact(a[0], a[1]) {
        core::cmp::Ordering::Less => -1,
        core::cmp::Ordering::Equal => 0,
        core::cmp::Ordering::Greater => 1,
    }))
}
