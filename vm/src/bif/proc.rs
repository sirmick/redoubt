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
    let (target, alive) = match &a[1] {
        Term::Pid(p) => (Some(*p), *p == c.p.pid || c.sys.procs.is_alive(*p)),
        Term::Atom(name) => match c.sys.registered.get(name.as_str()) {
            Some(p) => (Some(*p), true),
            None => (None, false),
        },
        _ => return Err(c.badarg()),
    };
    match target {
        Some(pid) if alive && pid != c.p.pid => {
            if let Some(t) = c.sys.procs.get_mut(pid) {
                t.monitored_by.insert(r, c.p.pid);
            }
            c.p.monitors.insert(r, pid);
        }
        Some(pid) if pid == c.p.pid => {} // monitoring yourself never fires
        _ => {
            let msg = Term::tuple(alloc::vec![
                Term::Atom(c.sys.atoms.down.clone()),
                Term::Ref(r),
                Term::Atom(c.sys.atoms.process.clone()),
                a[1].clone(),
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

/// `demonitor(Ref, Options)`; supports `flush` (drop a `'DOWN'` already queued).
pub fn demonitor2(c: &mut Ctx, a: &[Term]) -> R {
    let options = a[1].to_vec().ok_or_else(|| c.badarg())?;
    demonitor(c, a)?;
    let Term::Ref(r) = a[0] else { return Err(c.badarg()) };
    if options.iter().any(|o| matches!(o, Term::Atom(x) if x.as_str() == "flush")) {
        let down = c.sys.atoms.down.clone();
        c.p.mailbox.retain(|m| match m.as_tuple() {
            Some([tag, Term::Ref(x), ..]) => !(tag.is_atom(&down) && *x == r),
            _ => true,
        });
        c.p.save = 0;
    }
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn exit2(c: &mut Ctx, a: &[Term]) -> R {
    let pid = pid_arg(c, &a[0])?;
    if pid == c.p.pid {
        exit_self(c, a[1].clone());
    } else {
        c.sys.exits.push_back((pid, c.p.pid, a[1].clone()));
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
