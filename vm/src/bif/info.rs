//! Introspection: processes, loaded code, text forms of pids and funs, time of day, checksums.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::atom::{AtomTable, Atoms};
use crate::process::{Exception, Process, State};
use crate::term::{Pid, Term};

type R = Result<Term, Exception>;

fn string(s: &str) -> Term {
    Term::list(s.chars().map(|ch| Term::Int(ch as i64)).collect::<Vec<_>>())
}

fn text_of(c: &Ctx, t: &Term) -> Result<String, Exception> {
    let mut s = String::new();
    for item in t.list_iter() {
        let ch = item.ok().and_then(|x| x.as_i64()).and_then(|i| u32::try_from(i).ok()).and_then(char::from_u32);
        s.push(ch.ok_or_else(|| c.badarg())?);
    }
    Ok(s)
}

// ---- processes ----

pub fn processes(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(c.sys.procs.pids().into_iter().map(Term::Pid).collect::<Vec<_>>()))
}

/// One `process_info` item, or `None` for an item this VM does not track. Takes the atom
/// table and the process separately so it works both for the running process (borrowed by the
/// caller) and for one in the process table.
fn info_item(table: &mut AtomTable, atoms: &Atoms, p: &Process, running: bool, depth: usize, item: &Term) -> Option<Term> {
    // `{dictionary, Key}`: one entry of the process dictionary.
    if let Some([Term::Atom(k), key]) = item.as_tuple() {
        if k.as_str() == "dictionary" {
            let v = p.dictionary.get(&crate::term::MapKey(key.clone())).cloned();
            return Some(v.unwrap_or_else(|| Term::Atom(atoms.undefined.clone())));
        }
        return None;
    }
    let Term::Atom(item) = item else { return None };
    let item = item.as_str();
    let pids = |set: &mut dyn Iterator<Item = Pid>| Term::list(set.map(Term::Pid).collect::<Vec<_>>());
    let bool = |b: bool| Term::Atom(if b { atoms.true_.clone() } else { atoms.false_.clone() });
    let mut atom = |name: &str| Term::Atom(table.intern(name).expect("short atom"));
    Some(match item {
        "links" => pids(&mut p.links.iter().copied()),
        "monitored_by" => pids(&mut p.monitored_by.values().map(|(w, _)| *w)),
        "monitors" => Term::list(
            p.monitors
                .values()
                .map(|pid| Term::tuple(alloc::vec![Term::Atom(atoms.process.clone()), Term::Pid(*pid)]))
                .collect::<Vec<_>>(),
        ),
        "trap_exit" => bool(p.trap_exit),
        "registered_name" => match &p.registered_name {
            Some(n) => Term::Atom(n.clone()),
            None => Term::Nil,
        },
        "message_queue_len" => Term::Int(p.mailbox.len() as i64),
        "messages" => Term::list(p.mailbox.iter().cloned().collect::<Vec<_>>()),
        "dictionary" => Term::list(
            p.dictionary.iter().map(|(k, v)| Term::tuple(alloc::vec![k.0.clone(), v.clone()])).collect::<Vec<_>>(),
        ),
        "group_leader" => Term::Pid(p.group_leader.unwrap_or(p.pid)),
        "reductions" => Term::Int(p.reductions as i64),
        // The current function, then the functions that will be returned to.
        "current_stacktrace" => crate::interp::current_stacktrace(atoms, p, depth),
        "stack_size" => Term::Int((p.stack.len() + p.frames.len()) as i64),
        // Measured now (see `memory`): the words the process holds, each shared term once.
        "heap_size" | "total_heap_size" => Term::Int(crate::memory::process(p, u64::MAX).words as i64),
        "memory" => Term::Int((crate::memory::process(p, u64::MAX).words * 8) as i64),
        "min_heap_size" => Term::Int(233),
        "max_heap_size" => super::proc::max_heap_term(table, atoms, p.max_heap),
        "priority" => atom(p.priority.name()),
        "error_handler" => match &p.error_handler {
            Some(m) => Term::Atom(m.clone()),
            None => atom("error_handler"),
        },
        "current_function" => match p.pc.module.function_at(p.pc.pc) {
            Some(f) => Term::tuple(alloc::vec![
                Term::Atom(p.pc.module.name.clone()),
                Term::Atom(f.name.clone()),
                Term::Int(f.arity as i64),
            ]),
            None => Term::Atom(atoms.undefined.clone()),
        },
        "status" => atom(if running {
            "running"
        } else if p.state == State::Waiting {
            "waiting"
        } else {
            "runnable"
        }),
        _ => return None,
    })
}

/// `process_info(Pid, Item)` and `process_info(Pid, [Item])`. `undefined` for a dead process.
/// Memory items (`memory`, `heap_size`, ...) are measured when asked for: see `memory.rs`.
pub fn process_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Pid(pid) = a[0] else { return Err(c.badarg()) };
    let single = matches!(a[1], Term::Atom(_) | Term::Tuple(_));
    let items: Vec<Term> = if single { alloc::vec![a[1].clone()] } else { a[1].to_vec().ok_or_else(|| c.badarg())? };
    let running = pid == c.p.pid;
    let depth = c.sys.backtrace_depth;
    let sys = &mut *c.sys;
    let (table, atoms) = (&mut sys.atom_table, &sys.atoms);
    let p: &Process = if running {
        c.p
    } else {
        match sys.procs.get_mut(pid) {
            Some(p) => p,
            None => return Ok(Term::Atom(atoms.undefined.clone())),
        }
    };
    let mut out = Vec::new();
    for item in &items {
        let value = info_item(table, atoms, p, running, depth, item)
            .ok_or_else(|| Exception::error(Term::Atom(atoms.badarg.clone())))?;
        // Asked for alone, an unnamed process's `registered_name` is just `[]`.
        if single && item.is_atom(&table.intern("registered_name").expect("short atom")) && matches!(value, Term::Nil) {
            return Ok(Term::Nil);
        }
        out.push(Term::tuple(alloc::vec![item.clone(), value]));
    }
    if single {
        return Ok(out.pop().expect("one item"));
    }
    Ok(Term::list(out))
}

pub fn process_info1(c: &mut Ctx, a: &[Term]) -> R {
    let items = ["registered_name", "status", "message_queue_len", "links", "dictionary", "trap_exit", "group_leader"];
    let list = Term::list(items.iter().map(|i| c.atom(i)).collect::<Vec<_>>());
    process_info(c, &[a[0].clone(), list])
}

// ---- code ----

pub fn loaded(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(c.sys.loaded_modules().into_iter().map(Term::Atom).collect::<Vec<_>>()))
}

/// `code:ensure_loaded(M)`: load through the platform if needed.
pub fn ensure_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    Ok(match c.sys.module(m) {
        Some(_) => Term::tuple(alloc::vec![c.atom("module"), a[0].clone()]),
        None => Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), c.atom("nofile")]),
    })
}

/// `code:is_loaded(M)`: `{file, Where}` or `false`. Modules come from the platform, so there is
/// no file name to report.
pub fn is_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    Ok(if c.sys.is_loaded(m) {
        Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.file.clone()), c.atom("loaded")])
    } else {
        c.bool(false)
    })
}

/// `code:ensure_modules_loaded(Modules)`: `ok`, or `{error, [{Module, nofile}]}`.
pub fn ensure_modules_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let mut missing = Vec::new();
    for m in a[0].to_vec().ok_or_else(|| c.badarg())? {
        let Term::Atom(name) = &m else { return Err(c.badarg()) };
        if c.sys.module(name).is_none() {
            missing.push(Term::tuple(alloc::vec![m.clone(), c.atom("nofile")]));
        }
    }
    Ok(if missing.is_empty() {
        c.ok()
    } else {
        Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), Term::list(missing)])
    })
}

/// `code:load_binary(Module, File, Binary)`: load a module from bytes (e.g. fresh from the
/// compiler). The same loader checks apply as for modules from the platform.
pub fn load_binary(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    let bytes = a[2].iodata_bytes().ok_or_else(|| c.badarg())?;
    match c.sys.load_bytes(&bytes) {
        Ok(name) if &name == m => Ok(Term::tuple(alloc::vec![c.atom("module"), a[0].clone()])),
        Ok(_) => Ok(Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), c.atom("badfile")])),
        Err(_) => Ok(Term::tuple(alloc::vec![Term::Atom(c.sys.atoms.error.clone()), c.atom("badfile")])),
    }
}

/// `code:delete(Module)`: `true` if it was loaded. There is no separate "old code": deleting
/// unloads at once (see `System::delete_module`), so `purge` has nothing left to do.
pub fn code_delete(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    let gone = c.sys.delete_module(m);
    Ok(c.bool(gone))
}

/// `erlang:delete_module(Module)`: `true`, or `undefined` if it was not loaded.
pub fn delete_module(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    Ok(if c.sys.delete_module(m) { c.bool(true) } else { Term::Atom(c.sys.atoms.undefined.clone()) })
}

/// `code:purge/1`: `false` (no old code is ever kept); `code:soft_purge/1`: `true`.
pub fn purge(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(false))
}

pub fn soft_purge(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(true))
}

/// `code:get_object_code(Module)`: `{Module, Beam, Filename}` from the platform, or `error`.
/// The file name is nominal (`Module.beam`): where the platform keeps it is its own business.
pub fn get_object_code(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else { return Err(c.badarg()) };
    if crate::vm::RUNTIME_MODULES.contains(&m.as_str()) {
        return Ok(Term::Atom(c.sys.atoms.error.clone()));
    }
    Ok(match c.sys.platform.load_module(m.as_str()) {
        Some(bytes) => Term::tuple(alloc::vec![a[0].clone(), Term::binary(&bytes), string(&alloc::format!("{}.beam", m.as_str()))]),
        None => Term::Atom(c.sys.atoms.error.clone()),
    })
}

pub fn all_loaded(c: &mut Ctx, _a: &[Term]) -> R {
    let file = c.atom("loaded");
    Ok(Term::list(
        c.sys.loaded_modules().into_iter().map(|m| Term::tuple(alloc::vec![Term::Atom(m), file.clone()])).collect::<Vec<_>>(),
    ))
}

/// `erlang:get_module_info(M)` and `get_module_info(M, Key)`, behind every `M:module_info/0,1`.
pub fn get_module_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else { return Err(c.badarg()) };
    let m = c.sys.module(name).ok_or_else(|| c.badarg())?;
    let decode = |c: &mut Ctx, bytes: &[u8]| -> Term {
        if bytes.is_empty() {
            return Term::Nil;
        }
        crate::etf::decode(bytes, &mut c.sys.atom_table).unwrap_or(Term::Nil)
    };
    let item = |c: &mut Ctx, key: &str| -> Option<Term> {
        Some(match key {
            "module" => Term::Atom(m.name.clone()),
            "exports" => Term::list(
                m.exports
                    .iter()
                    .map(|e| Term::tuple(alloc::vec![Term::Atom(e.function.clone()), Term::Int(e.arity as i64)]))
                    .collect::<Vec<_>>(),
            ),
            "functions" => Term::list(
                m.functions
                    .iter()
                    .map(|f| Term::tuple(alloc::vec![Term::Atom(f.name.clone()), Term::Int(f.arity as i64)]))
                    .collect::<Vec<_>>(),
            ),
            "attributes" => decode(c, &m.attributes),
            "compile" => decode(c, &m.compile_info),
            "nifs" => Term::Nil,
            "md5" => Term::binary(&m.md5),
            _ => return None,
        })
    };
    match a.get(1) {
        Some(Term::Atom(key)) => item(c, key.as_str()).ok_or_else(|| c.badarg()),
        Some(_) => Err(c.badarg()),
        None => {
            let keys = ["module", "exports", "attributes", "compile", "md5"];
            let mut out = Vec::new();
            for k in keys {
                let v = item(c, k).expect("known key");
                out.push(Term::tuple(alloc::vec![c.atom(k), v]));
            }
            Ok(Term::list(out))
        }
    }
}

// ---- text forms ----

pub fn pid_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Pid(p) = a[0] else { return Err(c.badarg()) };
    Ok(string(&alloc::format!("<0.{}.{}>", p.index, p.serial)))
}

/// `list_to_pid("<0.I.S>")`. Pids are not capabilities inside one VM (see DESIGN.md), so making
/// one from text grants nothing new.
pub fn list_to_pid(c: &mut Ctx, a: &[Term]) -> R {
    let s = text_of(c, &a[0])?;
    let parts: Option<Vec<u32>> = s
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .and_then(|s| s.split('.').map(|n| n.parse().ok()).collect::<Option<Vec<u32>>>());
    match parts.as_deref() {
        Some([0, index, serial]) => Ok(Term::Pid(Pid { index: *index, serial: *serial })),
        _ => Err(c.badarg()),
    }
}

pub fn ref_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Ref(r) = a[0] else { return Err(c.badarg()) };
    Ok(string(&alloc::format!("#Ref<0.0.0.{}>", r.0)))
}

pub fn fun_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Fun(_) = &a[0] else { return Err(c.badarg()) };
    Ok(string(&alloc::format!("{}", a[0])))
}

/// `display_string(String)` / `display_string(Device, String)`: raw text to the console.
pub fn display_string(c: &mut Ctx, a: &[Term]) -> R {
    let s = a.last().expect("one or two arguments");
    let text = match s {
        Term::Bits(b) if b.is_binary() => b.to_bytes().into_owned(),
        _ => text_of(c, s)?.into_bytes(),
    };
    c.sys.platform.console_write(&text);
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn system_version(c: &mut Ctx, _a: &[Term]) -> R {
    let _ = c;
    Ok(string(&alloc::format!(
        "Erlang/OTP {} [erts-{}] [beamlet] [64-bit]\n",
        super::proc::OTP_RELEASE,
        super::proc::ERTS_VERSION
    )))
}

// ---- environment (the VM's own; see `System::env`) ----

pub fn getenv(c: &mut Ctx, a: &[Term]) -> R {
    let name = text_of(c, &a[0])?;
    Ok(match c.sys.env.get(&name) {
        Some(v) => string(v),
        None => a.get(1).cloned().unwrap_or_else(|| Term::Atom(c.sys.atoms.false_.clone())),
    })
}

pub fn getenv_all(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(c.sys.env.iter().map(|(k, v)| string(&alloc::format!("{k}={v}"))).collect::<Vec<_>>()))
}

pub fn putenv(c: &mut Ctx, a: &[Term]) -> R {
    let (name, value) = (text_of(c, &a[0])?, text_of(c, &a[1])?);
    if name.is_empty() || name.contains('=') {
        return Err(c.badarg());
    }
    c.sys.env.insert(name, value);
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

pub fn unsetenv(c: &mut Ctx, a: &[Term]) -> R {
    let name = text_of(c, &a[0])?;
    c.sys.env.remove(&name);
    Ok(Term::Atom(c.sys.atoms.true_.clone()))
}

// ---- beamlet: the VM's own API ----

/// `beamlet:app_spec(App)`: the `.app` file of `App` from the platform, or `error`.
pub fn app_spec(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(app) = &a[0] else { return Err(c.badarg()) };
    let name = String::from(app.as_str());
    Ok(match c.sys.platform.load_app(&name) {
        Some(b) => Term::binary(&b),
        None => Term::Atom(c.sys.atoms.error.clone()),
    })
}

/// `inet:gethostname()`: a VM does not learn its host's name (that would be ambient
/// information); it is `localhost` unless an embedder's native says otherwise.
pub fn gethostname(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::tuple(alloc::vec![c.ok(), string("localhost")]))
}

/// `net_adm:localhost()`: the host name without a resolver domain (there is no resolver).
pub fn localhost(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(string("localhost"))
}

// ---- init (a preloaded module in BEAM; its queries answered here) ----

/// A VM has no command line: no arguments, no flags.
pub fn init_get_arguments(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Nil)
}

pub fn init_get_argument(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Atom(c.sys.atoms.error.clone()))
}

pub fn init_get_status(c: &mut Ctx, _a: &[Term]) -> R {
    let started = c.atom("started");
    Ok(Term::tuple(alloc::vec![started.clone(), started]))
}

// ---- time of day ----

/// Civil date from days since 1970-01-01 (Howard Hinnant's algorithm, proleptic Gregorian).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// Days since 1970-01-01 of a civil date (the inverse of [`civil`]).
fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `{{Y, M, D}, {H, Mi, S}}` for seconds since the Unix epoch.
fn datetime(secs: i64) -> Term {
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    Term::tuple(alloc::vec![
        Term::tuple(alloc::vec![Term::Int(y), Term::Int(m as i64), Term::Int(d as i64)]),
        Term::tuple(alloc::vec![Term::Int(t / 3600), Term::Int(t / 60 % 60), Term::Int(t % 60)]),
    ])
}

/// `universaltime()` as `{{Y, M, D}, {H, Mi, S}}`. `localtime()` is the same: the VM has no time
/// zone (the platform could supply one later).
pub fn universaltime(c: &mut Ctx, _a: &[Term]) -> R {
    let us = c.sys.platform.system_time_us().ok_or_else(|| c.badarg())?;
    Ok(datetime((us / 1_000_000) as i64))
}

/// Years BEAM's calendar conversions accept.
const YEARS: core::ops::RangeInclusive<i64> = 0..=9999;

pub fn posixtime_to_universaltime(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Int(secs) = a[0] else { return Err(c.badarg()) };
    let t = datetime(secs);
    match t.as_tuple().and_then(|dt| dt[0].as_tuple().map(|d| d[0].clone())) {
        Some(Term::Int(y)) if YEARS.contains(&y) => Ok(t),
        _ => Err(c.badarg()),
    }
}

/// `universaltime_to_posixtime({{Y, M, D}, {H, Mi, S}})`, checking that the date exists.
pub fn universaltime_to_posixtime(c: &mut Ctx, a: &[Term]) -> R {
    let field = |t: &Term, i: usize| t.as_tuple().filter(|x| x.len() == 3).and_then(|x| match x[i] {
        Term::Int(n) => Some(n),
        _ => None,
    });
    let parts = a[0].as_tuple().filter(|x| x.len() == 2).and_then(|dt| {
        Some([field(&dt[0], 0)?, field(&dt[0], 1)?, field(&dt[0], 2)?, field(&dt[1], 0)?, field(&dt[1], 1)?, field(&dt[1], 2)?])
    });
    let Some([y, m, d, h, mi, s]) = parts else { return Err(c.badarg()) };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let valid = YEARS.contains(&y)
        && (1..=12).contains(&m)
        && d >= 1
        && d <= month_days[(m - 1) as usize]
        && (0..24).contains(&h)
        && (0..60).contains(&mi)
        && (0..60).contains(&s);
    if !valid {
        return Err(c.badarg());
    }
    let days = days_from_civil(y, m as u32, d as u32);
    Ok(Term::Int(days * 86_400 + h * 3600 + mi * 60 + s))
}

pub fn date(c: &mut Ctx, a: &[Term]) -> R {
    let now = universaltime(c, a)?;
    Ok(now.as_tuple().expect("{Date, Time}")[0].clone())
}

pub fn time(c: &mut Ctx, a: &[Term]) -> R {
    let now = universaltime(c, a)?;
    Ok(now.as_tuple().expect("{Date, Time}")[1].clone())
}

/// Local time is UTC here, so conversions between the two are the identity (after checking
/// the argument has the right shape).
pub fn same_datetime(c: &mut Ctx, a: &[Term]) -> R {
    match a[0].as_tuple() {
        Some([d, t]) if d.as_tuple().is_some_and(|x| x.len() == 3) && t.as_tuple().is_some_and(|x| x.len() == 3) => {
            Ok(if a.len() == 1 { a[0].clone() } else { Term::list(alloc::vec![a[0].clone()]) })
        }
        _ => Err(c.badarg()),
    }
}

// ---- checksums ----

/// CRC-32 (IEEE 802.3, as zlib), bit by bit: short and obviously correct.
fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    crc = !crc;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// `crc32(IoData)` and `crc32(OldCrc, IoData)`.
pub fn crc32(c: &mut Ctx, a: &[Term]) -> R {
    let (old, data) = match a {
        [data] => (0, data),
        [old, data] => (old.as_i64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| c.badarg())?, data),
        _ => return Err(c.badarg()),
    };
    let bin = super::erlang::iolist_to_binary(c, core::slice::from_ref(data))?;
    let Term::Bits(b) = bin else { return Err(c.badarg()) };
    Ok(Term::Int(crc32_update(old, &b.to_bytes()) as i64))
}

/// `erlang:md5(IoData)`.
pub fn md5(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let data = a[0].iodata_bytes().ok_or_else(|| c.badarg())?;
    Ok(Term::binary(&md5::Md5::digest(&data)))
}

/// An MD5 context as `md5_init/0` hands it out: the hash's serialized state, a binary.
fn md5_context(c: &Ctx, t: &Term) -> Result<md5::Md5, Exception> {
    use md5::digest::common::hazmat::{SerializableState, SerializedState};
    let Term::Bits(b) = t else { return Err(c.badarg()) };
    let bytes = b.to_bytes();
    let state = SerializedState::<md5::Md5>::try_from(&bytes[..]).map_err(|_| c.badarg())?;
    md5::Md5::deserialize(&state).map_err(|_| c.badarg())
}

fn md5_state(h: &md5::Md5) -> Term {
    use md5::digest::common::hazmat::SerializableState;
    Term::binary(&h.serialize())
}

pub fn md5_init(_c: &mut Ctx, _a: &[Term]) -> R {
    use md5::Digest;
    Ok(md5_state(&md5::Md5::new()))
}

pub fn md5_update(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let mut h = md5_context(c, &a[0])?;
    h.update(a[1].iodata_bytes().ok_or_else(|| c.badarg())?);
    Ok(md5_state(&h))
}

pub fn md5_final(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let h = md5_context(c, &a[0])?;
    Ok(Term::binary(&h.finalize()))
}

/// Adler-32 (RFC 1950), as zlib.
fn adler32_update(adler: u32, bytes: &[u8]) -> u32 {
    const BASE: u32 = 65521;
    let (mut a, mut b) = (adler & 0xffff, adler >> 16);
    // 5552 bytes at a time keep the sums from overflowing (zlib's NMAX).
    for chunk in bytes.chunks(5552) {
        for &x in chunk {
            a += x as u32;
            b += a;
        }
        a %= BASE;
        b %= BASE;
    }
    (b << 16) | a
}

pub fn adler32(c: &mut Ctx, a: &[Term]) -> R {
    let (old, data) = match a {
        [data] => (1, data),
        [old, data] => (old.as_i64().and_then(|v| u32::try_from(v).ok()).ok_or_else(|| c.badarg())?, data),
        _ => return Err(c.badarg()),
    };
    let bin = super::erlang::iolist_to_binary(c, core::slice::from_ref(data))?;
    let Term::Bits(b) = bin else { return Err(c.badarg()) };
    Ok(Term::Int(adler32_update(old, &b.to_bytes()) as i64))
}

/// Arguments of the `*_combine` functions: two checksums and a length.
fn combine_args(c: &Ctx, a: &[Term]) -> Result<(u32, u32, u64), Exception> {
    let word = |t: &Term| t.as_i64().and_then(|v| u32::try_from(v).ok());
    match (word(&a[0]), word(&a[1]), a[2].as_i64().and_then(|v| u64::try_from(v).ok())) {
        (Some(x), Some(y), Some(n)) => Ok((x, y, n)),
        _ => Err(c.badarg()),
    }
}

/// `adler32_combine(A1, A2, Size2)`: the checksum of two concatenated inputs (zlib's method).
pub fn adler32_combine(c: &mut Ctx, a: &[Term]) -> R {
    const BASE: u64 = 65521;
    let (a1, a2, len2) = combine_args(c, a)?;
    let (a1, a2) = (a1 as u64, a2 as u64);
    let rem = len2 % BASE;
    let mut sum1 = a1 & 0xffff;
    let mut sum2 = rem * sum1 % BASE;
    sum1 += (a2 & 0xffff) + BASE - 1;
    sum2 += ((a1 >> 16) & 0xffff) + ((a2 >> 16) & 0xffff) + BASE - rem;
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum1 >= BASE {
        sum1 -= BASE;
    }
    if sum2 >= BASE << 1 {
        sum2 -= BASE << 1;
    }
    if sum2 >= BASE {
        sum2 -= BASE;
    }
    Ok(Term::Int((sum1 | (sum2 << 16)) as i64))
}

/// `a * b` modulo the CRC-32 polynomial, in its reflected representation (zlib's `multmodp`).
fn crc_multmodp(a: u32, mut b: u32) -> u32 {
    let mut m: u32 = 1 << 31;
    let mut p = 0;
    loop {
        if a & m != 0 {
            p ^= b;
            if a & (m - 1) == 0 {
                return p;
            }
        }
        m >>= 1;
        b = if b & 1 != 0 { (b >> 1) ^ 0xEDB8_8320 } else { b >> 1 };
    }
}

/// `x^(n * 2^k)` modulo the polynomial (zlib's `x2nmodp`).
fn crc_x2nmodp(mut n: u64, mut k: u32) -> u32 {
    let mut p: u32 = 1 << 31;
    while n != 0 {
        if n & 1 != 0 {
            // x^(2^k), by squaring x (which is 1 << 30) k times.
            let mut x2k: u32 = 1 << 30;
            for _ in 0..(k % 32) {
                x2k = crc_multmodp(x2k, x2k);
            }
            p = crc_multmodp(x2k, p);
        }
        n >>= 1;
        k += 1;
    }
    p
}

/// `crc32_combine(C1, C2, Size2)`: the CRC of two concatenated inputs (zlib's method).
pub fn crc32_combine(c: &mut Ctx, a: &[Term]) -> R {
    let (c1, c2, len2) = combine_args(c, a)?;
    Ok(Term::Int((crc_multmodp(crc_x2nmodp(len2, 3), c1) ^ c2) as i64))
}

/// `os:getpid()`: the VM has no host process number of its own to report.
pub fn os_getpid(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(string("1"))
}

/// `os:env()`: the VM's environment as `{Name, Value}` pairs.
pub fn os_env(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::list(c.sys.env.iter().map(|(k, v)| Term::tuple(alloc::vec![string(k), string(v)])).collect::<Vec<_>>()))
}

/// `binary:referenced_byte_size(Bin)`: the size of the buffer `Bin` is a window into. As in
/// BEAM, a binary of 64 bytes or less counts as its own copy (BEAM copies such small ones).
pub fn referenced_byte_size(c: &mut Ctx, a: &[Term]) -> R {
    match &a[0] {
        Term::Bits(b) if b.is_binary() && b.len / 8 <= 64 => Ok(Term::Int((b.len / 8) as i64)),
        Term::Bits(b) if b.is_binary() => Ok(Term::Int(b.data.len() as i64)),
        _ => Err(c.badarg()),
    }
}

/// `beamlet:console_subscribe()`: make the caller the receiver of console input, as
/// `{beamlet_console, Bytes}` messages and finally `{beamlet_console, eof}`. For the `user` I/O
/// server; there is one reader per VM, and the last caller wins.
pub fn console_subscribe(c: &mut Ctx, _a: &[Term]) -> R {
    c.sys.console_reader = Some(c.p.pid);
    Ok(c.ok())
}

// ---- erlang:memory ----

/// The categories of `erlang:memory/0`, in its order.
const MEMORY_TYPES: [&str; 9] =
    ["total", "processes", "processes_used", "system", "atom", "atom_used", "binary", "code", "ets"];

/// Bytes in use per category of `erlang:memory/0`, from each process's last measurement (the
/// caller's is taken now) and the ETS tables' running totals. `code` is not tracked (0), and
/// atoms are estimated from their count.
fn memory_values(c: &mut Ctx) -> [u64; 9] {
    let current = crate::memory::process(c.p, u64::MAX);
    let (mut procs, mut binary) = (current.words, current.binary_bytes);
    for pid in c.sys.procs.pids() {
        if let Some(p) = c.sys.procs.get_mut(pid) {
            procs += p.usage.words.max(crate::memory::PROCESS_WORDS);
            binary += p.usage.binary_bytes;
        }
    }
    let processes = procs * 8;
    let atom = c.sys.atom_table.len() as u64 * 16;
    let ets = c.sys.ets.words() * 8;
    let code = 0;
    let system = atom + binary + code + ets;
    [processes + system, processes, processes, system, atom, atom, binary, code, ets]
}

pub fn memory0(c: &mut Ctx, _a: &[Term]) -> R {
    let values = memory_values(c);
    let items: Vec<Term> =
        MEMORY_TYPES.iter().zip(values).map(|(k, v)| Term::tuple(alloc::vec![c.atom(k), Term::Int(v as i64)])).collect();
    Ok(Term::list(items))
}

/// `erlang:memory(Type)` and `erlang:memory([Type])`.
pub fn memory1(c: &mut Ctx, a: &[Term]) -> R {
    let values = memory_values(c);
    let value = |c: &Ctx, t: &Term| match t {
        Term::Atom(k) => MEMORY_TYPES.iter().position(|m| *m == k.as_str()).map(|i| Term::Int(values[i] as i64)).ok_or_else(|| c.badarg()),
        _ => Err(c.badarg()),
    };
    if let Term::Atom(_) = &a[0] {
        return value(c, &a[0]);
    }
    let types = a[0].to_vec().ok_or_else(|| c.badarg())?;
    let mut out = Vec::new();
    for t in &types {
        out.push(Term::tuple(alloc::vec![t.clone(), value(c, t)?]));
    }
    Ok(Term::list(out))
}

#[cfg(test)]
mod tests {
    #[test]
    fn crc32_matches_zlib() {
        // Values from erlang:crc32/1,2 on OTP 28.
        assert_eq!(super::crc32_update(0, b"Hello, World!"), 3_964_322_768);
        assert_eq!(super::crc32_update(0, b"abc"), 891_568_578);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(super::civil(0), (1970, 1, 1));
        assert_eq!(super::civil(20_714), (2026, 9, 18));
        assert_eq!(super::civil(-1), (1969, 12, 31));
        for days in [-800_000, -1, 0, 59, 11_016, 20_714, 2_000_000] {
            let (y, m, d) = super::civil(days);
            assert_eq!(super::days_from_civil(y, m, d), days);
        }
    }
}
