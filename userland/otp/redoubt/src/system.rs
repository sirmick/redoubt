//! The system's calls on Redoubt (docs/userland/beamlet.md, "Natives"): the VM's [`System`], over
//! the client library and the runtime's kernel calls.
//!
//! - **A handle is a [`Cap`]**, the value behind its resource term: the handle, owned once (closed when its
//!   last copy goes, the namespace's and the named handles' never while the VM runs), and what kind it is,
//!   which each call checks before the kernel is asked. The kind is the platform's knowledge: a namespace
//!   entry is a 9P connection, the named handle `budget` is the VM's own budget, a budget made here is a
//!   budget, and any other handle is an endpoint.
//! - **The namespace is the files' own** ([`crate::files`]): a bind here is a bind there. A handle is
//!   attached as a 9P connection at most once (a second `Tversion` on one handle would end the first's fids),
//!   so a handle bound twice is one connection.
//! - **A call goes out on a thread of the pool** ([`crate::pool`]), waiting at most its timeout: no hub
//!   carries a typed call. Its reply is the wire's words, the buffer the reply's word 1 says (a lent
//!   message's), and the handles it brought, each a new resource; the native decodes nothing of a protocol.

use alloc::collections::VecDeque;
use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::num::NonZeroU64;

use beamlet_vm::platform::{BudgetSpec, Entry, Event, Launch, Message, Object, Refused, System, Usage};
use redoubt_client::Error;
use redoubt_client::file::Connection;
use redoubt_rt::abi::{self, FOREVER, Handle, Labels};
use redoubt_rt::handle::{Budget, Endpoint};
use redoubt_rt::ipc::{Buffer, Delivery};
use redoubt_rt::path;
use redoubt_rt::startup::Startup;

use crate::Redoubt;

/// The named handle that is the VM's own budget: what `budget_create/1` carves from.
pub const BUDGET: &str = "budget";

/// How long a `send/2` waits for its receiver to take it (µs): no longer than a hub's submit does.
pub const SEND_TIMEOUT_US: u64 = 1_000;

/// How long the VM waits for a thread of its own to take work it hands it (µs): a free thread
/// waits in its receive, so it takes it as soon as it runs; one that has not in a second has ended,
/// and is set aside.
pub const HAND_US: u64 = 1_000_000;

/// How long a bind waits for a server to answer each of the attach's two calls (µs), on the VM's
/// thread: one short call a live server answers at once, as a launcher's release is.
pub const ATTACH_US: u64 = 1_000_000;

/// A handle the VM holds: one it was given since its start is closed when its last [`Cap`] goes;
/// one it was started with is the process's for its life, and the namespace's, so never.
pub(crate) struct Owned {
    pub(crate) handle: Handle,
    close: bool,
}

impl Owned {
    /// A handle a reply, a request or a carve brought: closed when its last copy goes.
    pub(crate) fn new(handle: Handle) -> Owned { Owned { handle, close: true } }

    /// A handle of the startup block: kept as long as the process runs.
    fn kept(handle: Handle) -> Owned { Owned { handle, close: false } }
}

impl Drop for Owned {
    fn drop(&mut self) {
        if self.close {
            let _ = redoubt_rt::handle::close(self.handle);
        }
    }
}

/// What a handle is.
#[derive(Clone)]
pub(crate) enum Kind {
    /// A 9P connection, attached.
    Connection(Connection),
    /// A budget.
    Budget,
    /// Any other handle: an endpoint or a receive right.
    Endpoint,
}

/// The value behind a handle's resource term.
pub(crate) struct Cap {
    pub(crate) owned: Arc<Owned>,
    pub(crate) kind: Kind,
}

impl Cap {
    fn object(owned: Arc<Owned>, kind: Kind) -> Object { Arc::new(Cap { owned, kind }) }

    /// A handle a reply or a request brought: an endpoint, closed when its last copy goes.
    pub(crate) fn received(handle: Handle) -> Object {
        Cap::object(Arc::new(Owned::new(handle)), Kind::Endpoint)
    }

    pub(crate) fn handle(&self) -> Handle { self.owned.handle }
}

/// The [`Cap`] behind `object`; any other is not one of the platform's.
pub(crate) fn cap(object: &Object) -> Result<&Cap, Refused> {
    object.downcast_ref::<Cap>().ok_or(Refused("bad_handle"))
}

/// A handle the VM knows by its start or a bind: owned once, and its connection once attached.
struct Known {
    owned: Arc<Owned>,
    /// Its name, if it came as a named handle.
    name: Option<String>,
    kind: Kind,
}

/// The system calls' state.
pub(crate) struct Sys {
    known: Vec<Known>,
    /// The named handles, in block order: (name, handle).
    named: Vec<(String, Handle)>,
    labels: Vec<u64>,
    events: VecDeque<(u64, Event)>,
    /// The typed calls' threads.
    pool: crate::pool::Pool,
    /// The endpoints served.
    served: crate::serve::Served,
    /// The jobs running.
    jobs: crate::jobs::Jobs,
}

impl Sys {
    /// The handles `startup` gave the VM, and its label set, `labels`, read by `pool`'s start.
    pub(crate) fn new(
        startup: &Startup,
        ns: &redoubt_client::ns::Namespace,
        labels: Vec<u64>,
        pool: crate::pool::Pool,
    ) -> Sys {
        let mut sys = Sys {
            known: Vec::new(),
            named: Vec::new(),
            labels,
            events: VecDeque::new(),
            pool,
            served: crate::serve::Served::default(),
            jobs: crate::jobs::Jobs::default(),
        };
        for (_, conn) in ns.list() {
            let handle = conn.endpoint().handle();
            if sys.find(handle).is_none() {
                sys.known.push(Known {
                    owned: Arc::new(Owned::kept(handle)),
                    name: None,
                    kind: Kind::Connection(conn.clone()),
                });
            }
        }
        for (name, handle) in startup.handles() {
            sys.named.push((name.to_string(), handle));
            match sys.find(handle) {
                Some(i) => sys.known[i].name = sys.known[i].name.take().or_else(|| Some(name.to_string())),
                None => sys.known.push(Known {
                    owned: Arc::new(Owned::kept(handle)),
                    name: Some(name.to_string()),
                    kind: if name == BUDGET { Kind::Budget } else { Kind::Endpoint },
                }),
            }
        }
        sys
    }

    /// Whether anything the system calls began is still to end: a call, a served endpoint or a job.
    pub(crate) fn busy(&self) -> bool { self.pool.busy() || self.served.busy() || self.jobs.busy() }

    /// Whether an event waits for the VM to poll it.
    pub(crate) fn has_events(&self) -> bool { !self.events.is_empty() }

    /// Takes a wake-up of the system calls' threads; anything else is handed back.
    pub(crate) fn deliver(&mut self, wake: &Endpoint, delivery: Delivery) -> Option<Delivery> {
        let delivery = self.pool.deliver(wake, delivery)?;
        let delivery = match self.served.deliver(delivery) {
            Ok(event) => {
                self.events.push_back(event);
                return None;
            }
            Err(delivery) => delivery,
        };
        match self.jobs.deliver(delivery) {
            Ok(event) => {
                self.events.push_back(event);
                None
            }
            Err(delivery) => Some(delivery),
        }
    }

    /// Moves what has ended to the events the VM polls.
    pub(crate) fn collect(&mut self) {
        while let Some((asker, call, result)) = self.pool.ended() {
            self.events.push_back((asker, Event::Reply { call, result }));
        }
    }

    fn find(&self, handle: Handle) -> Option<usize> {
        self.known.iter().position(|k| k.owned.handle == handle)
    }

    /// The resource value for the known handle `i`.
    fn object(&self, i: usize) -> Object {
        Cap::object(Arc::clone(&self.known[i].owned), self.known[i].kind.clone())
    }
}

/// A refusal's name: the kernel's error, the server's, or the library's.
pub(crate) fn name(e: Error) -> Refused {
    Refused(match e {
        Error::Sys(e) => kernel(e),
        Error::Disconnected => "disconnected",
        Error::Wire(_) | Error::Unexpected => "protocol",
        Error::Server(_) => "refused",
        Error::Rerror(name) => name.as_str(),
        Error::Refused(_) => "bad_name",
    })
}

/// The kernel's error by its name in kernel/abi.md, in snake case.
pub(crate) fn kernel(e: abi::Error) -> &'static str {
    match e {
        abi::Error::BadHandle => "bad_handle",
        abi::Error::WrongObject => "wrong_object",
        abi::Error::InvalidArgument => "invalid_argument",
        abi::Error::OutOfMemory => "out_of_memory",
        abi::Error::OutOfProcesses => "out_of_processes",
        abi::Error::TooManyThreads => "too_many_threads",
        abi::Error::NotPermitted => "not_permitted",
        abi::Error::ClassDenied => "class_denied",
        abi::Error::LabelDenied => "label_denied",
        abi::Error::Busy => "busy",
        abi::Error::Refused => "refused",
        abi::Error::TooLarge => "too_large",
        abi::Error::Timeout => "timeout",
        abi::Error::Dead => "disconnected",
    }
}

/// Starts a thread of the platform on a stack of `stack` pages: it runs `body` with an endpoint of
/// its own to receive on and a handle that wakes the VM on `wake` with `badge`; the VM's handle to
/// send it work through comes back.
pub(crate) fn worker(
    wake: &Endpoint,
    badge: u64,
    stack: usize,
    body: impl FnOnce(Endpoint, Endpoint) + Send + 'static,
) -> Result<Endpoint, Refused> {
    let refused = |e| Refused(kernel(e));
    let receive = Endpoint::create().map_err(refused)?;
    let go = receive.mint(NonZeroU64::MIN, None).map_err(refused)?;
    let done = wake.mint(NonZeroU64::new(badge).ok_or(Refused("protocol"))?, None).map_err(refused)?;
    redoubt_rt::thread::spawn(alloc::boxed::Box::new(move || body(receive, done)), stack).map_err(refused)?;
    Ok(go)
}

/// A budget's handle, or `wrong_object` before any kernel call.
fn budget(object: &Object) -> Result<Handle, Refused> {
    let cap = cap(object)?;
    match cap.kind {
        Kind::Budget => Ok(cap.handle()),
        _ => Err(Refused("wrong_object")),
    }
}

/// An endpoint's handle (a connection's is its server's endpoint), or `wrong_object`.
fn endpoint(object: &Object) -> Result<Handle, Refused> {
    let cap = cap(object)?;
    match cap.kind {
        Kind::Budget => Err(Refused("wrong_object")),
        _ => Ok(cap.handle()),
    }
}

/// The handles `objects` hold, each one of the platform's.
fn handles(objects: &[Object]) -> Result<Vec<Handle>, Refused> {
    objects.iter().map(|o| cap(o).map(Cap::handle)).collect()
}

impl System for Redoubt {
    fn lookup(&mut self, path: &str) -> Result<(Object, String), Refused> {
        if !path.starts_with('/') {
            let &(_, handle) =
                self.sys.named.iter().find(|(name, _)| name == path).ok_or(Refused("not_found"))?;
            let i = self.sys.find(handle).ok_or(Refused("not_found"))?;
            return Ok((self.sys.object(i), String::new()));
        }
        if !path::is_clean_absolute(path) {
            return Err(Refused("bad_name"));
        }
        let (conn, rest) = self.files.ns.lookup(path).ok_or(Refused("not_found"))?;
        let rest = rest.to_string();
        let i = self.sys.find(conn.endpoint().handle()).ok_or(Refused("not_found"))?;
        Ok((self.sys.object(i), rest))
    }

    fn bind(&mut self, prefix: &str, handle: &Object) -> Result<(), Refused> {
        if !path::is_clean_absolute(prefix) {
            return Err(Refused("bad_name"));
        }
        let cap = cap(handle)?;
        let conn = match &cap.kind {
            Kind::Connection(conn) => conn.clone(),
            Kind::Budget => return Err(Refused("not_a_connection")),
            Kind::Endpoint => match self.sys.find(cap.handle()) {
                Some(i) if matches!(self.sys.known[i].kind, Kind::Connection(_)) => {
                    let Kind::Connection(conn) = &self.sys.known[i].kind else { unreachable!("matched") };
                    conn.clone()
                }
                // Attached once, here: a server that does not speak 9P is no connection.
                found => {
                    let conn = Connection::attach_within(
                        Endpoint::from_handle(cap.handle()),
                        &mut self.lend,
                        ATTACH_US,
                    )
                    .map_err(|_| Refused("not_a_connection"))?;
                    match found {
                        Some(i) => self.sys.known[i].kind = Kind::Connection(conn.clone()),
                        None => self.sys.known.push(Known {
                            owned: Arc::clone(&cap.owned),
                            name: None,
                            kind: Kind::Connection(conn.clone()),
                        }),
                    }
                    conn
                }
            },
        };
        self.files.ns.bind(prefix, conn).map_err(name)
    }

    fn table(&mut self) -> Vec<Entry> {
        let mut out = Vec::new();
        for (prefix, conn) in self.files.ns.list() {
            if let Some(i) = self.sys.find(conn.endpoint().handle()) {
                out.push(Entry {
                    path: prefix.to_string(),
                    name: self.sys.known[i].name.clone(),
                    handle: self.sys.object(i),
                });
            }
        }
        for (name, handle) in &self.sys.named {
            if let Some(i) = self.sys.find(*handle) {
                out.push(Entry { path: name.clone(), name: Some(name.clone()), handle: self.sys.object(i) });
            }
        }
        out
    }

    fn call(
        &mut self,
        asker: u64,
        call: u64,
        to: &Object,
        message: Message,
        timeout_us: u64,
    ) -> Result<(), Refused> {
        let target = endpoint(to)?;
        let carried = handles(&message.handles)?;
        let Message { words, buffer, handles: mut held } = message;
        held.push(Object::clone(to));
        let call =
            crate::pool::Call { asker, id: call, to: target, carried, words, buffer, timeout_us, held };
        self.sys.pool.call(self.io.wake(), call)?;
        self.sys.collect();
        Ok(())
    }

    fn send(&mut self, to: &Object, message: Message) -> Result<(), Refused> {
        let to = Endpoint::from_handle(endpoint(to)?);
        let carried = handles(&message.handles)?;
        let transfer = match &message.buffer {
            None => None,
            Some(bytes) => {
                let mut pages =
                    Buffer::new(bytes.len().max(1).div_ceil(abi::PAGE_SIZE)).map_err(|e| name(e.into()))?;
                pages[..bytes.len()].copy_from_slice(bytes);
                Some(pages)
            }
        };
        to.send(&message.words, &carried, transfer, SEND_TIMEOUT_US).map_err(|(e, _)| name(e.into()))
    }

    fn serve(&mut self, asker: u64, served: &Object) -> Result<(), Refused> {
        let handle = endpoint(served)?;
        self.sys.served.serve(self.io.wake(), asker, handle, Object::clone(served))
    }

    fn reply(&mut self, request: &Object, reply: Message) -> Result<(), Refused> {
        let carried = handles(&reply.handles)?;
        crate::serve::reply(request, &reply, &carried)
    }

    fn budget_create(&mut self, spec: &BudgetSpec) -> Result<Object, Refused> {
        let &(_, parent) = self.sys.named.iter().find(|(n, _)| n == BUDGET).ok_or(Refused("no_budget"))?;
        let too_large = |_| Refused("too_large");
        let spec = abi::BudgetSpec {
            pages: spec.pages,
            processes: u32::try_from(spec.processes).map_err(too_large)?,
            weight: u32::try_from(spec.weight).map_err(too_large)?,
            // Left out, the VM's own: what the kernel stamps on its sends, so its own budget's
            // set, which a user-class caller's child must carry exactly (docs/kernel/budgets.md,
            // "Labels on budgets"). Given, they go to the kernel as they are, for its verdict.
            labels: Labels::from_slice(spec.labels.as_deref().unwrap_or(&self.sys.labels))
                .map_err(|_| Refused("too_large"))?,
            account: spec.account,
            deadline: spec.deadline.unwrap_or(FOREVER),
        };
        let child = Budget::from_handle(parent).create_child(&spec).map_err(|e| name(e.into()))?;
        Ok(Cap::object(Arc::new(Owned::new(child.handle())), Kind::Budget))
    }

    fn budget_destroy(&mut self, b: &Object) -> Result<(), Refused> {
        Budget::from_handle(budget(b)?).destroy().map_err(|e| name(e.into()))
    }

    fn budget_usage(&mut self, b: &Object) -> Result<Usage, Refused> {
        let u = Budget::from_handle(budget(b)?).usage().map_err(|e| name(e.into()))?;
        Ok(Usage {
            pages: (u.pages_limit, u.pages_usage),
            processes: (u.processes_limit.into(), u.processes_usage.into()),
            weight: (u.weight_limit.into(), u.weight_carved.into()),
        })
    }

    fn labels(&mut self) -> Vec<u64> { self.sys.labels.clone() }

    fn launch(&mut self, asker: u64, job: u64, launch: Launch) -> Result<(), Refused> {
        let resolved = crate::jobs::Resolved {
            budget: budget(&launch.budget)?,
            namespace: launch.namespace.iter().map(|(_, o)| endpoint(o)).collect::<Result<_, _>>()?,
            handles: launch.handles.iter().map(|(_, o)| cap(o).map(Cap::handle)).collect::<Result<_, _>>()?,
        };
        self.sys.jobs.launch(self.io.wake(), asker, job, launch, resolved)
    }

    fn poll(&mut self) -> Option<(u64, Event)> { self.sys.events.pop_front() }
}
