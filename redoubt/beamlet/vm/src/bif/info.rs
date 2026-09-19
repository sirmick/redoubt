//! Introspection: processes, loaded code, text forms of pids and funs, time of day, checksums.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::atom::{AtomTable, Atoms};
use crate::process::{Exception, Process, State};
use crate::term::{compare, copy, Heap, Pid, Term};

type R = Result<Term, Exception>;

fn text_of(c: &Ctx, t: &Term) -> Result<String, Exception> {
    let mut s = String::new();
    for item in c.heap().list_iter(*t) {
        let ch = item
            .ok()
            .and_then(|x| x.as_i64())
            .and_then(|i| u32::try_from(i).ok())
            .and_then(char::from_u32);
        s.push(ch.ok_or_else(|| c.badarg())?);
    }
    Ok(s)
}

// ---- processes ----

pub fn processes(c: &mut Ctx, _a: &[Term]) -> R {
    let pids: Vec<Term> = c
        .sys()
        .procs
        .pids()
        .into_iter()
        .filter(|p| !p.port)
        .map(Term::Pid)
        .collect();
    Ok(c.list(pids))
}

/// One `process_info` item of `p`, built on `out`, or `None` for an item this VM does not
/// track. `item` is a term of `item_heap` (the caller's). Terms of `p` are copied to `out`,
/// so this works for another process as for the caller itself.
#[allow(clippy::too_many_arguments)]
fn info_item(
    table: &mut AtomTable,
    atoms: &Atoms,
    p: &Process,
    running: bool,
    depth: usize,
    item_heap: &Heap,
    item: Term,
    out: &mut Heap,
) -> Option<Term> {
    // `{dictionary, Key}`: one entry of the process dictionary.
    if let Some(&[Term::Atom(k), key]) = item_heap.as_tuple(item) {
        if k.as_str() == "dictionary" {
            let v = p
                .dictionary
                .entries()
                .iter()
                .find(|(k, _)| compare(&p.heap, *k, item_heap, key, true).is_eq())
                .map(|e| e.1);
            return Some(match v {
                Some(v) => copy(&p.heap, v, out),
                None => Term::Atom(atoms.undefined),
            });
        }
        return None;
    }
    let Term::Atom(item) = item else { return None };
    let item = item.as_str();
    let pids = |out: &mut Heap, set: &mut dyn Iterator<Item = Pid>| {
        out.list(set.map(Term::Pid).collect::<Vec<_>>())
    };
    let bool = |b: bool| Term::Atom(if b { atoms.true_ } else { atoms.false_ });
    let mut atom = |name: &str| Term::Atom(table.intern(name).expect("short atom"));
    Some(match item {
        "links" => pids(out, &mut p.links.iter().copied()),
        "monitored_by" => pids(out, &mut p.monitored_by.values().map(|m| m.watcher)),
        "monitors" => {
            let items: Vec<Term> = p
                .monitors
                .values()
                .map(|pid| out.tuple(&[Term::Atom(atoms.process), Term::Pid(*pid)]))
                .collect();
            out.list(items)
        }
        "trap_exit" => bool(p.trap_exit),
        "registered_name" => match &p.registered_name {
            Some(n) => Term::Atom(*n),
            None => Term::Nil,
        },
        "message_queue_len" => Term::Int(p.mailbox.len() as i64),
        "messages" => {
            let msgs: Vec<Term> = p.mailbox.iter().map(|m| copy(&p.heap, *m, out)).collect();
            out.list(msgs)
        }
        "dictionary" => {
            let items: Vec<Term> = p
                .dictionary
                .entries()
                .iter()
                .map(|(k, v)| {
                    let (k, v) = (copy(&p.heap, *k, out), copy(&p.heap, *v, out));
                    out.tuple(&[k, v])
                })
                .collect();
            out.list(items)
        }
        "group_leader" => Term::Pid(p.group_leader.unwrap_or(p.pid)),
        "reductions" => Term::Int(p.reductions as i64),
        // The current function, then the functions that will be returned to.
        "current_stacktrace" => {
            let points = crate::interp::stacktrace_points(p, depth);
            crate::interp::current_stacktrace(atoms, out, &points)
        }
        "stack_size" => Term::Int((p.stack.len() + p.frames.len()) as i64),
        // Read off the heap: what the process holds, garbage not yet collected included.
        // As BEAM: the heap block, which changes only when the heap is collected (the
        // collection threshold, two words a cell). `memory` is what is in use.
        "heap_size" | "total_heap_size" => Term::Int((p.gc_at * 2) as i64),
        "memory" => Term::Int((crate::memory::process(p).total_words() * 8) as i64),
        "min_heap_size" => Term::Int(233),
        "max_heap_size" => super::proc::max_heap_term(table, atoms, out, p.max_heap),
        "priority" => atom(p.priority.name()),
        "error_handler" => match &p.error_handler {
            Some(m) => Term::Atom(*m),
            None => atom("error_handler"),
        },
        "current_function" => match p.pc.module.function_at(p.pc.pc) {
            Some(f) => out.tuple(&[
                Term::Atom(p.pc.module.name),
                Term::Atom(f.name),
                Term::Int(f.arity as i64),
            ]),
            None => Term::Atom(atoms.undefined),
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
pub fn process_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Pid(pid) = a[0] else {
        return Err(c.badarg());
    };
    if pid.port {
        return Err(c.badarg());
    }
    let single = matches!(a[1], Term::Atom(_) | Term::Tuple(_));
    let items: Vec<Term> = if single {
        alloc::vec![a[1]]
    } else {
        c.heap().to_vec(a[1]).ok_or_else(|| c.badarg())?
    };
    // One lock for all of it: the process read cannot start running meanwhile.
    let mut guard = c.sys();
    let sys = &mut *guard;
    let running = pid == c.p.pid;
    if !running && sys.procs.is_running(pid) {
        drop(guard);
        return c.retry();
    }
    // Messages still in the inbox count, and are listed, as queued.
    if running {
        sys.receive_pending(c.p);
    } else {
        sys.receive_pending_of(pid);
    }
    let depth = sys.backtrace_depth;
    let registered_name = Term::Atom(sys.atom("registered_name"));
    // Built on a heap of its own, then copied to the caller's (which may be the process read).
    let mut out = Heap::new(&sys.literals);
    let result = {
        let (table, atoms) = (&mut sys.atom_table, &sys.atoms);
        let p: &Process = if running {
            c.p
        } else {
            match sys.procs.get_mut(pid) {
                Some(p) => p,
                None => return Ok(Term::Atom(atoms.undefined)),
            }
        };
        let mut pairs = Vec::new();
        let mut result = None;
        for &item in &items {
            let value = info_item(table, atoms, p, running, depth, &c.p.heap, item, &mut out)
                .ok_or_else(|| Exception::error(Term::Atom(atoms.badarg)))?;
            let item_here = copy(&c.p.heap, item, &mut out);
            // Asked for alone, an unnamed process's `registered_name` is just `[]`.
            if single && c.p.heap.eq_exact(item, registered_name) && matches!(value, Term::Nil) {
                result = Some(Term::Nil);
                break;
            }
            pairs.push(out.tuple(&[item_here, value]));
        }
        match result {
            Some(r) => r,
            None if single => pairs.pop().expect("one item"),
            None => out.list(pairs),
        }
    };
    Ok(copy(&out, result, &mut c.p.heap))
}

pub fn process_info1(c: &mut Ctx, a: &[Term]) -> R {
    let items = [
        "registered_name",
        "status",
        "message_queue_len",
        "links",
        "dictionary",
        "trap_exit",
        "group_leader",
    ];
    let items: Vec<Term> = items.iter().map(|i| c.atom(i)).collect();
    let list = c.list(items);
    process_info(c, &[a[0], list])
}

// ---- code ----

pub fn loaded(c: &mut Ctx, _a: &[Term]) -> R {
    Ok({
        let v = c
            .sys()
            .loaded_modules()
            .into_iter()
            .map(Term::Atom)
            .collect::<Vec<_>>();
        c.list(v)
    })
}

/// `code:ensure_loaded(M)`: load through the platform if needed.
pub fn ensure_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    let module = c.sys().module(m);
    Ok(match module {
        Some(_) => {
            let e = [c.atom("module"), a[0]];
            c.tuple(&e)
        }
        None => {
            let e = [Term::Atom(c.atoms.error), c.atom("nofile")];
            c.tuple(&e)
        }
    })
}

/// `code:is_loaded(M)`: `{file, Where}` or `false`. Modules come from the platform, so there is
/// no file name to report.
pub fn is_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    Ok(if c.sys().is_loaded(m) {
        {
            let e = [Term::Atom(c.atoms.file), c.atom("loaded")];
            c.tuple(&e)
        }
    } else {
        c.bool(false)
    })
}

/// `code:ensure_modules_loaded(Modules)`: `ok`, or `{error, [{Module, nofile}]}`.
pub fn ensure_modules_loaded(c: &mut Ctx, a: &[Term]) -> R {
    let mut missing = Vec::new();
    for m in c.heap().to_vec(a[0]).ok_or_else(|| c.badarg())? {
        let Term::Atom(name) = &m else {
            return Err(c.badarg());
        };
        if c.sys().module(name).is_none() {
            missing.push({
                let e = [m, c.atom("nofile")];
                c.tuple(&e)
            });
        }
    }
    Ok(if missing.is_empty() {
        c.ok()
    } else {
        {
            let e = [Term::Atom(c.atoms.error), {
                let v = missing;
                c.list(v)
            }];
            c.tuple(&e)
        }
    })
}

/// `code:load_binary(Module, File, Binary)`: load a module from bytes (e.g. fresh from the
/// compiler). The same loader checks apply as for modules from the platform.
pub fn load_binary(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = a[0] else {
        return Err(c.badarg());
    };
    let bytes = c.heap().iodata_bytes(a[2]).ok_or_else(|| c.badarg())?;
    let error = Term::Atom(c.atoms.error);
    let found = c.sys().load_bytes(&bytes);
    match found {
        Ok(name) if name == m => {
            let file = c.own(a[1]);
            c.sys().module_files.insert(String::from(m.as_str()), file);
            let module = c.atom("module");
            Ok(c.tuple(&[module, a[0]]))
        }
        Ok(_) | Err(_) => {
            let badfile = c.atom("badfile");
            Ok(c.tuple(&[error, badfile]))
        }
    }
}

/// `code:delete(Module)`: `true` if it was loaded. There is no separate "old code": deleting
/// unloads at once (see `System::delete_module`), so `purge` has nothing left to do.
pub fn code_delete(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    let gone = c.sys().delete_module(m);
    Ok(c.bool(gone))
}

/// `erlang:delete_module(Module)`: `true`, or `undefined` if it was not loaded.
pub fn delete_module(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    Ok(if c.sys().delete_module(m) {
        c.bool(true)
    } else {
        Term::Atom(c.atoms.undefined)
    })
}

/// `code:purge/1`: `false` (no old code is ever kept); `code:soft_purge/1`: `true`.
pub fn purge(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(false))
}

pub fn soft_purge(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.bool(true))
}

/// `code:get_object_code(Module)`: `{Module, Beam, Filename}` from the platform (with a nominal
/// file name, `Module.beam`: where the platform keeps it is its own business) or else from the
/// VM's code path, or `error`.
pub fn get_object_code(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(m) = &a[0] else {
        return Err(c.badarg());
    };
    if crate::vm::RUNTIME_MODULES.contains(&m.as_str()) {
        return Ok(Term::Atom(c.atoms.error));
    }
    let name = String::from(m.as_str());
    let located = c.sys().locate_module(&name);
    let found = match located {
        Some(crate::vm::Found::Platform(bytes)) => Some((alloc::format!("{name}.beam"), bytes)),
        Some(crate::vm::Found::Path(path, bytes)) => Some((path, bytes)),
        None => None,
    };
    Ok(match found {
        Some((file, bytes)) => {
            let e = [
                a[0],
                {
                    let v = &bytes;
                    c.binary(v)
                },
                c.string(&file),
            ];
            c.tuple(&e)
        }
        None => Term::Atom(c.atoms.error),
    })
}

pub fn all_loaded(c: &mut Ctx, _a: &[Term]) -> R {
    let file = c.atom("loaded");
    let mods = c.sys().loaded_modules();
    let v: Vec<Term> = mods
        .into_iter()
        .map(|m| c.tuple(&[Term::Atom(m), file]))
        .collect();
    Ok(c.list(v))
}

/// `erlang:get_module_info(M)` and `get_module_info(M, Key)`, behind every `M:module_info/0,1`.
pub fn get_module_info(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(name) = &a[0] else {
        return Err(c.badarg());
    };
    let m = c.sys().module(name).ok_or_else(|| c.badarg())?;
    let decode = |c: &mut Ctx, bytes: &[u8]| -> Term {
        if bytes.is_empty() {
            return Term::Nil;
        }
        crate::etf::decode(bytes, &mut c.sys().atom_table, &mut c.p.heap).unwrap_or(Term::Nil)
    };
    let item = |c: &mut Ctx, key: &str| -> Option<Term> {
        Some(match key {
            "module" => Term::Atom(m.name),
            "exports" => {
                let v = m
                    .exports
                    .iter()
                    .map(|e| {
                        let e = [Term::Atom(e.function), Term::Int(e.arity as i64)];
                        c.tuple(&e)
                    })
                    .collect::<Vec<_>>();
                c.list(v)
            }
            "functions" => {
                let v = m
                    .functions
                    .iter()
                    .map(|f| {
                        let e = [Term::Atom(f.name), Term::Int(f.arity as i64)];
                        c.tuple(&e)
                    })
                    .collect::<Vec<_>>();
                c.list(v)
            }
            "attributes" => decode(c, &m.attributes),
            "compile" => decode(c, &m.compile_info),
            "nifs" => Term::Nil,
            "md5" => {
                let v = &m.md5;
                c.binary(v)
            }
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
                out.push({
                    let e = [c.atom(k), v];
                    c.tuple(&e)
                });
            }
            Ok({
                let v = out;
                c.list(v)
            })
        }
    }
}

// ---- text forms ----

pub fn pid_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Pid(p) = a[0] else {
        return Err(c.badarg());
    };
    if p.port {
        return Err(c.badarg());
    }
    Ok(c.string(&alloc::format!("<0.{}.{}>", p.index, p.serial)))
}

/// `list_to_pid("<0.I.S>")`. Pids are not capabilities inside one VM (see DESIGN.md), so making
/// one from text grants nothing new.
pub fn list_to_pid(c: &mut Ctx, a: &[Term]) -> R {
    let s = text_of(c, &a[0])?;
    let parts: Option<Vec<u32>> = s
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .and_then(|s| {
            s.split('.')
                .map(|n| n.parse().ok())
                .collect::<Option<Vec<u32>>>()
        });
    match parts.as_deref() {
        Some([0, index, serial]) => Ok(Term::Pid(Pid::process(*index, *serial))),
        _ => Err(c.badarg()),
    }
}

pub fn ref_to_list(c: &mut Ctx, a: &[Term]) -> R {
    // A resource is a reference to Erlang code, and prints as one.
    let id = match a[0] {
        Term::Ref(r) => r.0,
        Term::Resource(_) => c.heap().as_resource(a[0]).map_or(0, |r| r.id),
        _ => return Err(c.badarg()),
    };
    Ok(c.string(&alloc::format!("#Ref<0.0.0.{id}>")))
}

pub fn fun_to_list(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Fun(_) = &a[0] else {
        return Err(c.badarg());
    };
    let text = c.show(a[0]);
    Ok(c.string(&text))
}

/// `display_string(String)` / `display_string(Device, String)`: raw text to the console.
pub fn display_string(c: &mut Ctx, a: &[Term]) -> R {
    let s = a.last().expect("one or two arguments");
    let text = match c.heap().as_bits(*s) {
        Some(b) if b.is_binary() => b.to_bytes().into_owned(),
        _ => text_of(c, s)?.into_bytes(),
    };
    c.platform().console_write(&text);
    Ok(Term::Atom(c.atoms.true_))
}

pub fn system_version(c: &mut Ctx, _a: &[Term]) -> R {
    let _ = c;
    Ok(c.string(&alloc::format!(
        "Erlang/OTP {} [erts-{}] [beamlet] [64-bit]\n",
        super::proc::OTP_RELEASE,
        super::proc::ERTS_VERSION
    )))
}

// ---- environment (the VM's own; see `System::env`) ----

pub fn getenv(c: &mut Ctx, a: &[Term]) -> R {
    let name = text_of(c, &a[0])?;
    let value = c.sys().env.get(&name).cloned();
    Ok(match value {
        Some(v) => c.string(&v),
        None => a.get(1).copied().unwrap_or(Term::Atom(c.atoms.false_)),
    })
}

pub fn getenv_all(c: &mut Ctx, _a: &[Term]) -> R {
    let vars: Vec<String> = c
        .sys()
        .env
        .iter()
        .map(|(k, v)| alloc::format!("{k}={v}"))
        .collect();
    let v: Vec<Term> = vars.iter().map(|s| c.string(s)).collect();
    Ok(c.list(v))
}

pub fn putenv(c: &mut Ctx, a: &[Term]) -> R {
    let (name, value) = (text_of(c, &a[0])?, text_of(c, &a[1])?);
    if name.is_empty() || name.contains('=') {
        return Err(c.badarg());
    }
    c.sys().env.insert(name, value);
    Ok(Term::Atom(c.atoms.true_))
}

pub fn unsetenv(c: &mut Ctx, a: &[Term]) -> R {
    let name = text_of(c, &a[0])?;
    c.sys().env.remove(&name);
    Ok(Term::Atom(c.atoms.true_))
}

// ---- beamlet: the VM's own API ----

/// `beamlet:app_spec(App)`: the `.app` file of `App` from the platform, or `error`.
pub fn app_spec(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Atom(app) = &a[0] else {
        return Err(c.badarg());
    };
    let name = String::from(app.as_str());
    let app = c.platform().load_app(&name);
    Ok(match app {
        Some(b) => c.binary(&b),
        None => Term::Atom(c.atoms.error),
    })
}

/// `inet:gethostname()`: a VM does not learn its host's name (that would be ambient
/// information); it is `localhost` unless an embedder's native says otherwise.
pub fn gethostname(c: &mut Ctx, _a: &[Term]) -> R {
    let name = c.string("localhost");
    Ok(c.ok_tuple(name))
}

/// `net_adm:localhost()`: the host name without a resolver domain (there is no resolver).
pub fn localhost(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.string("localhost"))
}

// ---- init (a preloaded module in BEAM; its queries answered here) ----

/// A VM has no command line: no arguments, no flags.
pub fn init_get_arguments(_c: &mut Ctx, _a: &[Term]) -> R {
    Ok(Term::Nil)
}

/// `init:get_argument(Flag)`: `home` is the VM's `HOME`, `root` its OTP root (see
/// `code:root_dir/0`); there are no other command-line flags.
pub fn init_get_argument(c: &mut Ctx, a: &[Term]) -> R {
    let value = match &a[0] {
        Term::Atom(f) if f.as_str() == "home" => c.sys().env.get("HOME").cloned(),
        Term::Atom(f) if f.as_str() == "root" && !c.sys().lib_roots.is_empty() => {
            let root = super::code::root_dir(c, &[])?;
            c.heap().to_vec(root).map(|chars| {
                chars
                    .iter()
                    .filter_map(|t| t.as_i64().and_then(|i| char::from_u32(i as u32)))
                    .collect()
            })
        }
        _ => None,
    };
    Ok(match value {
        Some(v) => {
            let s = c.string(&v);
            let inner = c.list([s]);
            let outer = c.list([inner]);
            c.ok_tuple(outer)
        }
        None => Term::Atom(c.atoms.error),
    })
}

pub fn init_get_status(c: &mut Ctx, _a: &[Term]) -> R {
    let started = c.atom("started");
    Ok(c.tuple(&[started, started]))
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
fn datetime(c: &mut Ctx, secs: i64) -> Term {
    let (y, m, d) = civil(secs.div_euclid(86_400));
    let t = secs.rem_euclid(86_400);
    let date = c.tuple(&[Term::Int(y), Term::Int(m as i64), Term::Int(d as i64)]);
    let time = c.tuple(&[
        Term::Int(t / 3600),
        Term::Int(t / 60 % 60),
        Term::Int(t % 60),
    ]);
    c.tuple(&[date, time])
}

/// `universaltime()` as `{{Y, M, D}, {H, Mi, S}}`. `localtime()` is the same: the VM has no time
/// zone (the platform could supply one later).
pub fn universaltime(c: &mut Ctx, _a: &[Term]) -> R {
    let us = c.platform().system_time_us().ok_or_else(|| c.badarg())?;
    Ok(datetime(c, (us / 1_000_000) as i64))
}

/// Years BEAM's calendar conversions accept.
const YEARS: core::ops::RangeInclusive<i64> = 0..=9999;

pub fn posixtime_to_universaltime(c: &mut Ctx, a: &[Term]) -> R {
    let Term::Int(secs) = a[0] else {
        return Err(c.badarg());
    };
    let (y, _, _) = civil(secs.div_euclid(86_400));
    if !YEARS.contains(&y) {
        return Err(c.badarg());
    }
    Ok(datetime(c, secs))
}

/// `universaltime_to_posixtime({{Y, M, D}, {H, Mi, S}})`, checking that the date exists.
pub fn universaltime_to_posixtime(c: &mut Ctx, a: &[Term]) -> R {
    let h = c.heap();
    let field = |t: &Term, i: usize| {
        h.as_tuple(*t)
            .filter(|x| x.len() == 3)
            .and_then(|x| match x[i] {
                Term::Int(n) => Some(n),
                _ => None,
            })
    };
    let parts = h.as_tuple(a[0]).filter(|x| x.len() == 2).and_then(|dt| {
        Some([
            field(&dt[0], 0)?,
            field(&dt[0], 1)?,
            field(&dt[0], 2)?,
            field(&dt[1], 0)?,
            field(&dt[1], 1)?,
            field(&dt[1], 2)?,
        ])
    });
    let Some([y, m, d, h, mi, s]) = parts else {
        return Err(c.badarg());
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
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
    Ok(c.heap().as_tuple(now).expect("{Date, Time}")[0])
}

pub fn time(c: &mut Ctx, a: &[Term]) -> R {
    let now = universaltime(c, a)?;
    Ok(c.heap().as_tuple(now).expect("{Date, Time}")[1])
}

/// Local time is UTC here, so conversions between the two are the identity (after checking
/// the argument has the right shape).
pub fn same_datetime(c: &mut Ctx, a: &[Term]) -> R {
    let h = c.heap();
    let three = |t: Term| h.as_tuple(t).is_some_and(|x| x.len() == 3);
    match h.as_tuple(a[0]) {
        Some(&[d, t]) if three(d) && three(t) => {
            Ok(if a.len() == 1 { a[0] } else { c.list([a[0]]) })
        }
        _ => Err(c.badarg()),
    }
}

// ---- checksums ----

/// CRC-32 (IEEE 802.3, as zlib), bit by bit: short and obviously correct.
pub(crate) fn crc32_update(mut crc: u32, bytes: &[u8]) -> u32 {
    crc = !crc;
    for &b in bytes {
        crc ^= b as u32;
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    !crc
}

/// `crc32(IoData)` and `crc32(OldCrc, IoData)`.
pub fn crc32(c: &mut Ctx, a: &[Term]) -> R {
    let (old, data) = match a {
        [data] => (0, data),
        [old, data] => (
            old.as_i64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| c.badarg())?,
            data,
        ),
        _ => return Err(c.badarg()),
    };
    let bytes = c.heap().iodata_bytes(*data).ok_or_else(|| c.badarg())?;
    Ok(Term::Int(crc32_update(old, &bytes) as i64))
}

/// `erlang:md5(IoData)`.
pub fn md5(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let data = c.heap().iodata_bytes(a[0]).ok_or_else(|| c.badarg())?;
    Ok(c.binary(&md5::Md5::digest(&data)))
}

/// An MD5 context as `md5_init/0` hands it out: the hash's serialized state, a binary.
fn md5_context(c: &Ctx, t: &Term) -> Result<md5::Md5, Exception> {
    use md5::digest::common::hazmat::{SerializableState, SerializedState};
    let b = c.heap().as_bits(*t).ok_or_else(|| c.badarg())?;
    let bytes = b.to_bytes();
    let state = SerializedState::<md5::Md5>::try_from(&bytes[..]).map_err(|_| c.badarg())?;
    md5::Md5::deserialize(&state).map_err(|_| c.badarg())
}

fn md5_state(c: &mut Ctx, h: &md5::Md5) -> Term {
    use md5::digest::common::hazmat::SerializableState;
    c.binary(&h.serialize())
}

pub fn md5_init(c: &mut Ctx, _a: &[Term]) -> R {
    use md5::Digest;
    Ok(md5_state(c, &md5::Md5::new()))
}

pub fn md5_update(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let mut h = md5_context(c, &a[0])?;
    h.update(c.heap().iodata_bytes(a[1]).ok_or_else(|| c.badarg())?);
    Ok(md5_state(c, &h))
}

pub fn md5_final(c: &mut Ctx, a: &[Term]) -> R {
    use md5::Digest;
    let h = md5_context(c, &a[0])?;
    Ok(c.binary(&h.finalize()))
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
        [old, data] => (
            old.as_i64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| c.badarg())?,
            data,
        ),
        _ => return Err(c.badarg()),
    };
    let bytes = c.heap().iodata_bytes(*data).ok_or_else(|| c.badarg())?;
    Ok(Term::Int(adler32_update(old, &bytes) as i64))
}

/// Arguments of the `*_combine` functions: two checksums and a length.
fn combine_args(c: &Ctx, a: &[Term]) -> Result<(u32, u32, u64), Exception> {
    let word = |t: &Term| t.as_i64().and_then(|v| u32::try_from(v).ok());
    match (
        word(&a[0]),
        word(&a[1]),
        a[2].as_i64().and_then(|v| u64::try_from(v).ok()),
    ) {
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
        b = if b & 1 != 0 {
            (b >> 1) ^ 0xEDB8_8320
        } else {
            b >> 1
        };
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
    Ok(Term::Int(
        (crc_multmodp(crc_x2nmodp(len2, 3), c1) ^ c2) as i64,
    ))
}

/// `os:getpid()`: the VM has no host process number of its own to report.
pub fn os_getpid(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(c.string("1"))
}

/// `os:env()`: the VM's environment as `{Name, Value}` pairs.
pub fn os_env(c: &mut Ctx, _a: &[Term]) -> R {
    let vars: Vec<(String, String)> = c
        .sys()
        .env
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let v: Vec<Term> = vars
        .iter()
        .map(|(k, v)| {
            let (k, v) = (c.string(k), c.string(v));
            c.tuple(&[k, v])
        })
        .collect();
    Ok(c.list(v))
}

/// `binary:referenced_byte_size(Bin)`: the size of the buffer `Bin` is a window into. As in
/// BEAM, a binary of 64 bytes or less counts as its own copy (BEAM copies such small ones).
pub fn referenced_byte_size(c: &mut Ctx, a: &[Term]) -> R {
    match c.heap().as_bits(a[0]) {
        Some(b) if b.is_binary() && b.len / 8 <= 64 => Ok(Term::Int((b.len / 8) as i64)),
        Some(b) if b.is_binary() => Ok(Term::Int(b.data.len() as i64)),
        _ => Err(c.badarg()),
    }
}

/// `beamlet:console_subscribe()`: make the caller the receiver of console input, as
/// `{beamlet_console, Bytes}` messages and finally `{beamlet_console, eof}`. For the `user` I/O
/// server; there is one reader per VM, and the last caller wins.
pub fn console_subscribe(c: &mut Ctx, _a: &[Term]) -> R {
    c.sys().console_reader = Some(c.p.pid);
    Ok(c.ok())
}

// ---- erlang:memory ----

/// The categories of `erlang:memory/0`, in its order.
const MEMORY_TYPES: [&str; 9] = [
    "total",
    "processes",
    "processes_used",
    "system",
    "atom",
    "atom_used",
    "binary",
    "code",
    "ets",
];

/// Bytes in use per category of `erlang:memory/0`, read off each process's heap, and the ETS
/// tables' running totals. `code` is not tracked (0), and
/// atoms are estimated from their count.
fn memory_values(c: &mut Ctx) -> [u64; 9] {
    let current = crate::memory::process(c.p);
    let (mut procs, mut binary) = (current.words, current.binary_bytes);
    let sys = c.sys();
    for pid in sys.procs.pids() {
        if let Some(usage) = sys.procs.usage(pid).filter(|_| pid != c.p.pid) {
            procs += usage.words;
            binary += usage.binary_bytes;
        }
    }
    let processes = procs * 8;
    let atom = sys.atom_table.len() as u64 * 16;
    let ets = sys.ets.words() * 8;
    drop(sys);
    let code = 0;
    let system = atom + binary + code + ets;
    [
        processes + system,
        processes,
        processes,
        system,
        atom,
        atom,
        binary,
        code,
        ets,
    ]
}

pub fn memory0(c: &mut Ctx, _a: &[Term]) -> R {
    let values = memory_values(c);
    let items: Vec<Term> = MEMORY_TYPES
        .iter()
        .zip(values)
        .map(|(k, v)| {
            let e = [c.atom(k), Term::Int(v as i64)];
            c.tuple(&e)
        })
        .collect();
    Ok({
        let v = items;
        c.list(v)
    })
}

/// `erlang:memory(Type)` and `erlang:memory([Type])`.
pub fn memory1(c: &mut Ctx, a: &[Term]) -> R {
    let values = memory_values(c);
    let value = |c: &Ctx, t: &Term| match t {
        Term::Atom(k) => MEMORY_TYPES
            .iter()
            .position(|m| *m == k.as_str())
            .map(|i| Term::Int(values[i] as i64))
            .ok_or_else(|| c.badarg()),
        _ => Err(c.badarg()),
    };
    if let Term::Atom(_) = &a[0] {
        return value(c, &a[0]);
    }
    let types = c.heap().to_vec(a[0]).ok_or_else(|| c.badarg())?;
    let mut out = Vec::new();
    for t in &types {
        out.push({
            let e = [*t, value(c, t)?];
            c.tuple(&e)
        });
    }
    Ok({
        let v = out;
        c.list(v)
    })
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
