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
fn info_item(table: &mut AtomTable, atoms: &Atoms, p: &Process, running: bool, item: &Term) -> Option<Term> {
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
        // The current function, then the functions that will be returned to (no locations).
        "current_stacktrace" => {
            let conts = core::iter::once(&p.pc).chain(p.cp.iter()).chain(p.frames.iter().rev().filter_map(|f| f.cp.as_ref()));
            let entries: Vec<Term> = conts
                .take(8)
                .filter_map(|cp| {
                    let f = cp.module.function_at(cp.pc.saturating_sub(1))?;
                    Some(Term::tuple(alloc::vec![
                        Term::Atom(cp.module.name.clone()),
                        Term::Atom(f.name.clone()),
                        Term::Int(f.arity as i64),
                        Term::Nil,
                    ]))
                })
                .collect();
            Term::list(entries)
        }
        "stack_size" => Term::Int((p.stack.len() + p.frames.len()) as i64),
        // Measured now (see `memory`): the words the process holds, each shared term once.
        "heap_size" | "total_heap_size" => Term::Int(crate::memory::process(p, u64::MAX).words as i64),
        "memory" => Term::Int((crate::memory::process(p, u64::MAX).words * 8) as i64),
        "min_heap_size" => Term::Int(233),
        "max_heap_size" => super::proc::max_heap_term(table, atoms, p.max_heap),
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
        let value = info_item(table, atoms, p, running, item)
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
            _ => return None,
        })
    };
    match a.get(1) {
        Some(Term::Atom(key)) => item(c, key.as_str()).ok_or_else(|| c.badarg()),
        Some(_) => Err(c.badarg()),
        None => {
            let keys = ["module", "exports", "attributes", "compile"];
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

/// `universaltime()` as `{{Y, M, D}, {H, Mi, S}}`. `localtime()` is the same: the VM has no time
/// zone (the platform could supply one later).
pub fn universaltime(c: &mut Ctx, _a: &[Term]) -> R {
    let us = c.sys.platform.system_time_us().ok_or_else(|| c.badarg())?;
    let secs = (us / 1_000_000) as i64;
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    Ok(Term::tuple(alloc::vec![
        Term::tuple(alloc::vec![Term::Int(y), Term::Int(m as i64), Term::Int(d as i64)]),
        Term::tuple(alloc::vec![Term::Int(t / 3600), Term::Int(t / 60 % 60), Term::Int(t % 60)]),
    ]))
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
    }
}
