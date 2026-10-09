//! The system's natives, module `redoubt` (docs/userland/beamlet.md, "Natives"), over the
//! platform's [`System`]: the namespace, calls and serving, budgets, labels and launching.
//!
//! What is here is only the boundary: each argument checked for its type and its size before the
//! platform sees it, and each result built as a term. Everything the calls do is the platform's.
//!
//! - **Handles are resource terms** holding the platform's [`Object`]: unforgeable, collected (the platform
//!   closes a handle when its last copy goes), and never serialisable (`term_to_binary` writes a resource as
//!   the plain reference it is, which no native takes).
//! - **Every refusal is a name:** a term of the wrong type is `badarg`, as for any native; a well-formed
//!   request the platform refuses is `{error, Name}`, a Redoubt name.
//! - **Every call's work is bounded:** no list is walked past its cap ([`MAX_LIST`] and the others below),
//!   and no binary is copied past its own.

use alloc::string::String;
use alloc::vec::Vec;

use super::Ctx;
use crate::platform::{BudgetSpec, Event, Launch, Message, Object, Refused, System, Usage};
use crate::process::Exception;
use crate::term::{Heap, Ref, Resource, Term};

type R = Result<Term, Exception>;

/// The most handles one message carries (the kernel's `MAX_MSG_HANDLES`).
pub const MAX_MESSAGE_HANDLES: usize = 4;
/// The longest buffer one message carries: the wire's largest message (64 KiB).
pub const MAX_BUFFER: usize = 65536;
/// The most entries a launch's namespace and named handles have together (`MAX_START_HANDLES`).
pub const MAX_START_HANDLES: usize = 128;
/// The most arguments a launch gives its program.
pub const MAX_ARGS: usize = 128;
/// The most labels in a budget's spec (the kernel's `MAX_LABELS`).
pub const MAX_LABELS: usize = 16;
/// The longest path, name or argument, in bytes.
pub const MAX_NAME: usize = 4096;
/// The longest program image a launch copies (8 MiB).
pub const MAX_IMAGE: usize = 8 << 20;
/// The longest a call waits for its reply, in milliseconds: its pool thread's deadline.
pub const MAX_CALL_MS: u64 = 5_000;

/// Runs `f` on the platform's system calls, with only the platform locked while it runs.
fn with_system<T>(c: &mut Ctx, f: impl FnOnce(&mut dyn System) -> Result<T, Refused>) -> Result<T, Refused> {
    let mut platform = c.platform();
    let system = platform.system().ok_or(Refused("not_supported"))?;
    f(system)
}

fn refused(c: &mut Ctx, Refused(name): Refused) -> Term {
    let reason = c.atom(name);
    c.error_tuple(reason)
}

fn done(c: &mut Ctx, r: Result<(), Refused>) -> R {
    Ok(match r {
        Ok(()) => c.ok(),
        Err(e) => refused(c, e),
    })
}

// ---- arguments ----

/// The handle resource `t` holds, or `badarg`. A reference read back from `term_to_binary` is not
/// one: it holds nothing.
fn handle(c: &Ctx, t: Term) -> Result<Object, Exception> {
    c.resource::<Object>(t).map(|held| Object::clone(&*held)).ok_or_else(|| c.badarg())
}

/// The elements of the proper list `t`, at most `max` of them; otherwise `badarg` (the walk stops
/// one past `max`, so a long list costs no more than a short one).
fn list(c: &Ctx, t: Term, max: usize) -> Result<Vec<Term>, Exception> {
    let mut out = Vec::new();
    for item in c.heap().list_iter(t).take(max.saturating_add(1)) {
        out.push(item.map_err(|_| c.badarg())?);
    }
    if out.len() > max {
        return Err(c.badarg());
    }
    Ok(out)
}

/// The bytes of the binary `t`, at most `max`; otherwise `badarg`.
fn bytes(c: &Ctx, t: Term, max: usize) -> Result<Vec<u8>, Exception> {
    let bits = c.heap().as_bits(t).filter(|b| b.is_binary() && b.len / 8 <= max).ok_or_else(|| c.badarg())?;
    Ok(bits.to_bytes().into_owned())
}

/// The UTF-8 text of the binary `t`, at most [`MAX_NAME`] bytes, with no NUL; otherwise `badarg`.
fn text(c: &Ctx, t: Term) -> Result<String, Exception> {
    let b = bytes(c, t, MAX_NAME)?;
    if b.contains(&0) {
        return Err(c.badarg());
    }
    String::from_utf8(b).map_err(|_| c.badarg())
}

/// The non-negative integer `t`, at most `u64::MAX`; otherwise `badarg`.
fn unsigned(c: &Ctx, t: Term) -> Result<u64, Exception> {
    match t {
        Term::Int(i) if i >= 0 => Ok(i as u64),
        _ => c.heap().as_big(t).and_then(|b| u64::try_from(b).ok()).ok_or_else(|| c.badarg()),
    }
}

/// `{Words, Buffer, Handles}`: four non-negative words, a binary or `nil` (an inline message lends
/// nothing) and at most four handles.
fn message(c: &Ctx, t: Term) -> Result<Message, Exception> {
    let Some([w, b, h]) = c.tuple_elems(t).and_then(|e| <[Term; 3]>::try_from(e).ok()) else {
        return Err(c.badarg());
    };
    let words = list(c, w, 4)?;
    let words: [Term; 4] = words.try_into().map_err(|_| c.badarg())?;
    let mut out = [0; 4];
    for (slot, word) in out.iter_mut().zip(words) {
        *slot = unsigned(c, word)?;
    }
    let buffer = match b {
        Term::Atom(a) if a.as_str() == "nil" => None,
        _ => Some(bytes(c, b, MAX_BUFFER)?),
    };
    let handles =
        list(c, h, MAX_MESSAGE_HANDLES)?.into_iter().map(|t| handle(c, t)).collect::<Result<_, _>>()?;
    Ok(Message { words: out, buffer, handles })
}

/// The value of `key` in the map `m`, if it is there.
fn field(c: &mut Ctx, m: Term, key: &str) -> Option<Term> {
    let key = c.atom(key);
    c.heap().map_get(m, key)
}

/// A map with only keys of `known`; otherwise `badarg`.
fn fields(c: &mut Ctx, m: Term, known: &[&str]) -> Result<Vec<Option<Term>>, Exception> {
    let size = c.heap().map_len(m).ok_or_else(|| c.badarg())?;
    let found: Vec<Option<Term>> = known.iter().map(|k| field(c, m, k)).collect();
    if found.iter().flatten().count() != size {
        return Err(c.badarg());
    }
    Ok(found)
}

// ---- results ----

// ---- the natives ----

/// `ns_lookup(Path)`: `{ok, Handle, Rest}`, the connection the longest matching prefix names and
/// the rest of the path; for a name with no `/`, the named handle and `<<>>`.
pub fn ns_lookup(c: &mut Ctx, a: &[Term]) -> R {
    let path = text(c, a[0])?;
    Ok(match with_system(c, |s| s.lookup(&path)) {
        Ok((h, rest)) => {
            let h = c.new_resource(h);
            let rest = c.binary(rest.as_bytes());
            let ok = c.ok();
            c.tuple(&[ok, h, rest])
        }
        Err(e) => refused(c, e),
    })
}

/// `bind(Prefix, Handle)`: the connection at `Prefix` as well; `ok`.
pub fn bind(c: &mut Ctx, a: &[Term]) -> R {
    let prefix = text(c, a[0])?;
    let h = handle(c, a[1])?;
    let r = with_system(c, |s| s.bind(&prefix, &h));
    done(c, r)
}

/// `ns()`: `[{Path, Name | nil, Handle}]`, the namespace then the named handles.
pub fn ns(c: &mut Ctx, _a: &[Term]) -> R {
    let table = with_system(c, |s| Ok(s.table())).unwrap_or_default();
    let nil = c.atom("nil");
    let entries: Vec<Term> = table
        .into_iter()
        .map(|e| {
            let path = c.binary(e.path.as_bytes());
            let name = e.name.map_or(nil, |n| c.binary(n.as_bytes()));
            let h = c.new_resource(e.handle);
            c.tuple(&[path, name, h])
        })
        .collect();
    Ok(c.list(entries))
}

/// `call(Handle, {Words, Buffer, Handles}, TimeoutMs)`: `{ok, Ref}`, and the reply arrives as
/// `{reply, Ref, {ok, {Words, Buffer, Handles}} | {error, Name}}`. The platform makes the call
/// while the caller goes on: it waits at most `TimeoutMs`, itself at most [`MAX_CALL_MS`].
pub fn call(c: &mut Ctx, a: &[Term]) -> R {
    let to = handle(c, a[0])?;
    let m = message(c, a[1])?;
    let ms = unsigned(c, a[2]).ok().filter(|&ms| ms <= MAX_CALL_MS).ok_or_else(|| c.badarg())?;
    let asker = super::asker(c.p.pid);
    let id = c.sys().make_ref();
    match with_system(c, |s| s.call(asker, id.0, &to, m, ms * 1000)) {
        Ok(()) => c.sys().system_waits += 1,
        // Refused before it was made: the reply says so, as for any other end.
        Err(e) => {
            let result = refused(c, e);
            let tag = c.atom("reply");
            let msg = c.tuple(&[tag, Term::Ref(id), result]);
            let me = c.p.pid;
            super::proc::send_to(c, me, msg);
        }
    }
    Ok(c.ok_tuple(Term::Ref(id)))
}

/// `send(Handle, {Words, Buffer, Handles})`: one way; `ok`.
pub fn send(c: &mut Ctx, a: &[Term]) -> R {
    let to = handle(c, a[0])?;
    let m = message(c, a[1])?;
    let r = with_system(c, |s| s.send(&to, m));
    done(c, r)
}

/// `serve(Endpoint)`: its requests come to the caller as `{request, Request, Badge, Account,
/// Labels, {Words, Buffer, Handles}}`; `ok`.
pub fn serve(c: &mut Ctx, a: &[Term]) -> R {
    let endpoint = handle(c, a[0])?;
    let asker = super::asker(c.p.pid);
    let r = with_system(c, |s| s.serve(asker, &endpoint));
    if r.is_ok() {
        c.sys().system_waits += 1;
    }
    done(c, r)
}

/// `reply(Request, {Words, Buffer, Handles})`: answers a request once; `ok`.
pub fn reply(c: &mut Ctx, a: &[Term]) -> R {
    let request = handle(c, a[0])?;
    let m = message(c, a[1])?;
    let r = with_system(c, |s| s.reply(&request, m));
    done(c, r)
}

/// `budget_create(#{pages, processes, weight, labels, account, deadline})`: `{ok, Budget}`, carved
/// from this VM's own budget. `labels` (default this VM's own), `account` (default 0) and
/// `deadline` (the clock's microseconds, default none) may be left out.
pub fn budget_create(c: &mut Ctx, a: &[Term]) -> R {
    let f = fields(c, a[0], &["pages", "processes", "weight", "labels", "account", "deadline"])?;
    let need = |c: &Ctx, t: Option<Term>| t.ok_or_else(|| c.badarg()).and_then(|t| unsigned(c, t));
    let spec = BudgetSpec {
        pages: need(c, f[0])?,
        processes: need(c, f[1])?,
        weight: need(c, f[2])?,
        labels: f[3]
            .map(|t| list(c, t, MAX_LABELS)?.into_iter().map(|l| unsigned(c, l)).collect())
            .transpose()?,
        account: f[4].map(|t| unsigned(c, t)).transpose()?.unwrap_or(0),
        deadline: f[5].map(|t| unsigned(c, t)).transpose()?,
    };
    Ok(match with_system(c, |s| s.budget_create(&spec)) {
        Ok(b) => {
            let b = c.new_resource(b);
            c.ok_tuple(b)
        }
        Err(e) => refused(c, e),
    })
}

/// `budget_destroy(Budget)`: destroys it and everything in it; `ok`.
pub fn budget_destroy(c: &mut Ctx, a: &[Term]) -> R {
    let b = handle(c, a[0])?;
    let r = with_system(c, |s| s.budget_destroy(&b));
    done(c, r)
}

/// `budget_usage(Budget)`: `{ok, #{pages => {Limit, Used}, processes => {Limit, Used}, weight =>
/// {Limit, Carved}}}`.
pub fn budget_usage(c: &mut Ctx, a: &[Term]) -> R {
    let b = handle(c, a[0])?;
    Ok(match with_system(c, |s| s.budget_usage(&b)) {
        Ok(Usage { pages, processes, weight }) => {
            let mut pairs = Vec::new();
            for (key, (limit, used)) in [("pages", pages), ("processes", processes), ("weight", weight)] {
                let k = c.atom(key);
                let (l, u) = (c.heap_mut().from_u64(limit), c.heap_mut().from_u64(used));
                let v = c.tuple(&[l, u]);
                pairs.push((k, v));
            }
            let m = c.map_from(pairs);
            c.ok_tuple(m)
        }
        Err(e) => refused(c, e),
    })
}

/// `labels()`: this VM's label set, fixed when its budget was made.
pub fn labels(c: &mut Ctx, _a: &[Term]) -> R {
    let labels = with_system(c, |s| Ok(s.labels())).unwrap_or_default();
    let terms: Vec<Term> = labels.into_iter().map(|l| c.heap_mut().from_u64(l)).collect();
    Ok(c.list(terms))
}

/// `identity()`: `{ok, #{principal => Name, labels => [{Name, Id}], context => Name | nil}}`, what
/// the steward told this VM of itself when it launched it as a session; `{error, not_found}` for a
/// VM that is no session.
pub fn identity(c: &mut Ctx, _a: &[Term]) -> R {
    Ok(match with_system(c, |s| Ok(s.identity())) {
        Ok(Some(i)) => {
            let principal = c.binary(i.principal.as_bytes());
            let labels: Vec<Term> = i
                .labels
                .iter()
                .map(|(name, id)| {
                    let name = c.binary(name.as_bytes());
                    let id = c.heap_mut().from_u64(*id);
                    c.tuple(&[name, id])
                })
                .collect();
            let labels = c.list(labels);
            let context = match &i.context {
                Some(name) => c.binary(name.as_bytes()),
                None => c.atom("nil"),
            };
            let keys = [c.atom("principal"), c.atom("labels"), c.atom("context")];
            let m = c.map_from(keys.into_iter().zip([principal, labels, context]));
            c.ok_tuple(m)
        }
        Ok(None) => refused(c, Refused("not_found")),
        Err(e) => refused(c, e),
    })
}

/// `launch(#{image, budget, namespace, handles, args, stack_pages, heap_pages, serve})`: `{ok, Job}`,
/// and the job's end arrives as `{exit, Job, Cause, Code}`. `image` is the program's bytes and
/// `budget` one the caller carved; `namespace` is `[{Path, Connection}]` and `handles` `[{Name,
/// Handle}]`, at most [`MAX_START_HANDLES`] together with `serve`'s; `args` is `[Binary]`. With
/// `serve => Name`, the child is given a new endpoint's receive right as the named handle `Name`, and
/// the answer is `{ok, Job, Connection}`, the caller's send right to it.
pub fn launch(c: &mut Ctx, a: &[Term]) -> R {
    let f = fields(
        c,
        a[0],
        &["image", "budget", "namespace", "handles", "args", "stack_pages", "heap_pages", "serve"],
    )?;
    let image = bytes(c, f[0].ok_or_else(|| c.badarg())?, MAX_IMAGE)?;
    let budget = handle(c, f[1].ok_or_else(|| c.badarg())?)?;
    let pairs = |c: &Ctx, t: Option<Term>| -> Result<Vec<(String, Object)>, Exception> {
        let Some(t) = t else { return Ok(Vec::new()) };
        list(c, t, MAX_START_HANDLES)?
            .into_iter()
            .map(|e| match c.tuple_elems(e).as_deref() {
                Some(&[name, h]) => Ok((text(c, name)?, handle(c, h)?)),
                _ => Err(c.badarg()),
            })
            .collect()
    };
    let namespace = pairs(c, f[2])?;
    let handles = pairs(c, f[3])?;
    let args = match f[4] {
        Some(t) => list(c, t, MAX_ARGS)?.into_iter().map(|t| text(c, t)).collect::<Result<_, _>>()?,
        None => Vec::new(),
    };
    let stack_pages = f[5].map(|t| unsigned(c, t)).transpose()?;
    let heap_pages = f[6].map(|t| unsigned(c, t)).transpose()?;
    let serve = f[7].map(|t| text(c, t)).transpose()?;
    if namespace.len() + handles.len() + usize::from(serve.is_some()) > MAX_START_HANDLES {
        return Ok(refused(c, Refused("too_many")));
    }
    let launch = Launch { image, budget, namespace, handles, args, stack_pages, heap_pages, serve };
    let asker = super::asker(c.p.pid);
    let job = c.sys().make_ref();
    let r = with_system(c, |s| s.launch(asker, job.0, launch));
    Ok(match r {
        Ok(served) => {
            c.sys().system_waits += 1;
            let ok = c.ok();
            match served {
                Some(conn) => {
                    let conn = c.new_resource(conn);
                    c.tuple(&[ok, Term::Ref(job), conn])
                }
                None => c.tuple(&[ok, Term::Ref(job)]),
            }
        }
        Err(e) => refused(c, e),
    })
}

/// A message with each handle's resource id chosen: built off any heap ([`body`]).
type Numbered = ([u64; 4], Option<Vec<u8>>, Vec<(u64, Object)>);

/// `{Words, Buffer, Handles}` on `h`, each handle a new resource with its id; `nil` is the atom
/// an inline message's buffer is.
fn body(h: &mut Heap, (words, buffer, handles): Numbered, nil: Term) -> Term {
    let words: Vec<Term> = words.iter().map(|&w| h.from_u64(w)).collect();
    let words = h.list(words);
    let buffer = match &buffer {
        Some(b) => h.binary(b),
        None => nil,
    };
    let handles: Vec<Term> =
        handles.into_iter().map(|(id, o)| h.resource(Resource::new(id, alloc::boxed::Box::new(o)))).collect();
    let handles = h.list(handles);
    h.tuple(&[words, buffer, handles])
}

impl crate::vm::System {
    /// `message` with a fresh resource id for each handle it brought.
    fn numbered(&mut self, message: Message) -> Numbered {
        let handles = message.handles.into_iter().map(|o| (self.make_ref().0, o)).collect();
        (message.words, message.buffer, handles)
    }

    /// Sends each process what the platform's system calls have for it: requests on endpoints it
    /// serves, its calls' replies and its jobs' ends. Asked only while one of those is to come.
    pub(crate) fn poll_system(&mut self) {
        while self.system_waits > 0 {
            let polled = self.platform.lock().system().and_then(|s| s.poll());
            let Some((asker, event)) = polled else { return };
            let to = super::asker_pid(asker);
            let nil = Term::Atom(self.atom("nil"));
            match event {
                Event::Request { request, badge, account, labels, message } => {
                    let request = request.map(|r| (self.make_ref().0, r));
                    let message = self.numbered(message);
                    let tag = Term::Atom(self.atom("request"));
                    self.send_with(to, |h| {
                        let request = match request {
                            Some((id, r)) => h.resource(Resource::new(id, alloc::boxed::Box::new(r))),
                            None => nil,
                        };
                        let (badge, account) = (h.from_u64(badge), h.from_u64(account));
                        let labels: Vec<Term> = labels.into_iter().map(|l| h.from_u64(l)).collect();
                        let labels = h.list(labels);
                        let body = body(h, message, nil);
                        h.tuple(&[tag, request, badge, account, labels, body])
                    });
                }
                Event::Reply { call, result } => {
                    self.system_waits -= 1;
                    let result = match result {
                        Ok(m) => Ok(self.numbered(m)),
                        Err(Refused(name)) => Err(Term::Atom(self.atom(name))),
                    };
                    let [tag, ok, err] = ["reply", "ok", "error"].map(|n| Term::Atom(self.atom(n)));
                    self.send_with(to, |h| {
                        let result = match result {
                            Ok(m) => {
                                let body = body(h, m, nil);
                                h.tuple(&[ok, body])
                            }
                            Err(name) => h.tuple(&[err, name]),
                        };
                        h.tuple(&[tag, Term::Ref(Ref(call)), result])
                    });
                }
                Event::Exit { job, cause, code } => {
                    self.system_waits -= 1;
                    let [tag, cause] = ["exit", cause].map(|n| Term::Atom(self.atom(n)));
                    self.send_with(to, |h| {
                        let code = h.from_u64(code);
                        h.tuple(&[tag, Term::Ref(Ref(job)), cause, code])
                    });
                }
            }
        }
    }
}
