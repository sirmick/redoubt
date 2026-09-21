//! A minimal fake kernel for host tests, installed behind `redoubt_rt::HostKernel`.
//!
//! Each fake process is a host thread with its own handle table, account and labels; they
//! share the host's address space, so a lend is "mapped" into the server by passing its
//! address, and records are read and written where the runtime put them. It models what the
//! runtime and the echo pair use (endpoints, badges, `mint`, the four IPC calls, `serve`,
//! abandoned calls and their notices, `map_anon`, `unmap`, `handle_close`, `time_now`, `random`,
//! `process_exit`) and not the rest: no budgets
//! or charging, no label check between user budgets (R1), no fair waiting (R2), no lend
//! unmapping from the caller. The executable model (`redoubt/model`, WP-M0) should replace it
//! once merged. Calls it does not model panic, so a test cannot rely on them by accident.

#![allow(dead_code)]

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use redoubt_rt::abi::{
    BODY_SLOTS, Body, Call, Error, FOREVER, Handle, Handles, Labels, Message, MessageKind, MintSource,
    PAGE_SIZE, Pages, RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, Return,
};

/// What a handle names. Only endpoints are modelled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Endpoint {
    id: usize,
    badge: u64,
}

struct Process {
    account: u64,
    labels: Labels,
    /// Index 0 is never a handle.
    handles: Vec<Option<Endpoint>>,
    /// Mapped ranges: address -> length.
    mappings: HashMap<usize, usize>,
}

struct Pending {
    id: NonZeroU64,
    call: bool,
    badge: u64,
    account: u64,
    labels: Labels,
    words: [usize; 4],
    handles: Vec<Endpoint>,
    pages: Option<Pages>,
    from: usize,
}

#[derive(Default)]
struct EndpointState {
    queue: VecDeque<Pending>,
    dead: bool,
}

#[derive(Default)]
struct State {
    processes: Vec<Process>,
    endpoints: Vec<EndpointState>,
    next_id: u64,
    /// Calls taken and not replied to: id -> (receiving process, endpoint).
    open: HashMap<u64, (usize, usize)>,
    /// Replies waiting for their caller: id -> (words, handles).
    replies: HashMap<u64, ([usize; 4], Vec<Endpoint>)>,
    /// Sends a receiver has taken.
    taken: HashSet<u64>,
    exits: HashMap<usize, u32>,
    rng: u64,
    /// Abandoned-call notices not yet received: (receiving process, endpoint, message id).
    notices: VecDeque<(usize, usize, u64)>,
    /// Taken calls whose caller gave up: their reply reaches nobody.
    abandoned: HashSet<u64>,
    /// `serve` and `reply` as they happened: (process, call, message id).
    log: Vec<(usize, &'static str, u64)>,
}

pub struct Fake {
    state: Mutex<State>,
    changed: Condvar,
    boot: Instant,
}

thread_local! {
    static CURRENT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The unwind payload of `process_exit`.
struct Exited(u32);

/// Installs the fake for this test binary (once) and returns it.
pub fn fake() -> &'static Fake {
    static FAKE: std::sync::OnceLock<&'static Fake> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let fake: &'static Fake = Box::leak(Box::new(Fake {
            state: Mutex::new(State { rng: 0x2545_f491_4f6c_dd1d, next_id: 1, ..State::default() }),
            changed: Condvar::new(),
            boot: Instant::now(),
        }));
        redoubt_rt::install_host_kernel(fake);
        fake
    })
}

fn current() -> usize {
    CURRENT.with(|c| c.get()).expect("a system call from a thread that is no fake process")
}

impl Fake {
    fn lock(&self) -> MutexGuard<'_, State> { self.state.lock().unwrap_or_else(|e| e.into_inner()) }

    /// A new process (no handles yet).
    pub fn process(&self, account: u64, labels: &[u64]) -> usize {
        let mut s = self.lock();
        let labels = Labels::from_slice(labels).unwrap();
        s.processes.push(Process { account, labels, handles: vec![None], mappings: HashMap::new() });
        s.processes.len() - 1
    }

    /// A new endpoint whose receive right (badge 0) goes into `owner`'s table.
    pub fn endpoint(&self, owner: usize) -> Handle {
        let mut s = self.lock();
        s.endpoints.push(EndpointState::default());
        let id = s.endpoints.len() - 1;
        install(&mut s, owner, Endpoint { id, badge: 0 })
    }

    /// Gives `to` a handle to the endpoint `from_handle` names in `from`, with `badge`: what a
    /// parent does with `mint` and `process_start`.
    pub fn grant(&self, from: usize, from_handle: Handle, to: usize, badge: u64) -> Handle {
        let mut s = self.lock();
        let ep = lookup(&s, from, from_handle).expect("grant from a handle that exists");
        install(&mut s, to, Endpoint { badge, ..ep })
    }

    /// Copies `from`'s handle `from_handle` into `to`, badge and all: what `process_start` does
    /// with the handles it installs.
    pub fn copy(&self, from: usize, from_handle: Handle, to: usize) -> Handle {
        let mut s = self.lock();
        let ep = lookup(&s, from, from_handle).expect("copy a handle that exists");
        install(&mut s, to, ep)
    }

    /// Destroys the endpoint: its receivers and blocked callers get `Dead`.
    pub fn destroy(&self, owner: usize, handle: Handle) {
        let mut s = self.lock();
        let ep = lookup(&s, owner, handle).unwrap();
        s.endpoints[ep.id].dead = true;
        self.changed.notify_all();
    }

    /// How many handles `pid` holds, and how many pages it has mapped.
    pub fn held(&self, pid: usize) -> (usize, usize) {
        let s = self.lock();
        let p = &s.processes[pid];
        (p.handles.iter().flatten().count(), p.mappings.values().map(|len| len / PAGE_SIZE).sum())
    }

    /// The `serve` and `reply` calls `pid` made, in order: ("serve" or "reply", message id).
    pub fn log(&self, pid: usize) -> Vec<(&'static str, u64)> {
        self.lock().log.iter().filter(|(p, _, _)| *p == pid).map(|(_, call, id)| (*call, *id)).collect()
    }

    /// Calls taken and not yet replied to, by any process.
    pub fn open_calls(&self, pid: usize) -> usize {
        self.lock().open.values().filter(|(p, _)| *p == pid).count()
    }

    /// Runs `body` as process `pid` on its own thread; the handle yields its exit code.
    pub fn run<F: FnOnce() -> u32 + Send + 'static>(
        &'static self,
        pid: usize,
        body: F,
    ) -> std::thread::JoinHandle<u32> {
        std::thread::spawn(move || {
            CURRENT.with(|c| c.set(Some(pid)));
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)) {
                Ok(code) => code,
                Err(payload) => match payload.downcast::<Exited>() {
                    Ok(exited) => exited.0,
                    Err(payload) => std::panic::resume_unwind(payload),
                },
            }
        })
    }

    /// Runs `body` as `pid` on this thread.
    pub fn as_process<T>(&self, pid: usize, body: impl FnOnce() -> T) -> T {
        let old = CURRENT.with(|c| c.replace(Some(pid)));
        let result = body();
        CURRENT.with(|c| c.set(old));
        result
    }

    /// Waits for a change or the deadline; false once the deadline has passed.
    fn wait<'a>(&self, guard: &mut Option<MutexGuard<'a, State>>, deadline: Option<Instant>) -> bool {
        let g = guard.take().unwrap();
        match deadline {
            None => *guard = Some(self.changed.wait(g).unwrap_or_else(|e| e.into_inner())),
            Some(deadline) => {
                let now = Instant::now();
                if now >= deadline {
                    *guard = Some(g);
                    return false;
                }
                let (g, _) = self.changed.wait_timeout(g, deadline - now).unwrap_or_else(|e| e.into_inner());
                *guard = Some(g);
            }
        }
        true
    }
}

fn install(s: &mut State, pid: usize, ep: Endpoint) -> Handle {
    let table = &mut s.processes[pid].handles;
    let index = match table.iter().skip(1).position(Option::is_none) {
        Some(free) => free + 1,
        None => {
            table.push(None);
            table.len() - 1
        }
    };
    table[index] = Some(ep);
    Handle::new(index as u32).unwrap()
}

fn lookup(s: &State, pid: usize, h: Handle) -> Result<Endpoint, Error> {
    s.processes[pid].handles.get(h.index() as usize).copied().flatten().ok_or(Error::BadHandle)
}

fn deadline(timeout: u64) -> Option<Instant> {
    (timeout != FOREVER).then(|| Instant::now() + Duration::from_micros(timeout))
}

/// Whether `pages` lies inside one of `pid`'s mappings.
fn owns(s: &State, pid: usize, pages: Pages) -> bool {
    let len = pages.npages.get() * PAGE_SIZE;
    pages.addr.is_multiple_of(PAGE_SIZE)
        && s.processes[pid].mappings.iter().any(|(&a, &l)| a <= pages.addr && pages.addr + len <= a + l)
}

fn read_body(addr: usize) -> Result<Body, Error> {
    // SAFETY: the runtime passes the address of a live, 8-aligned `[u64; BODY_SLOTS]` record
    // it owns for the duration of the call (redoubt-rt `sys::Record`), in this address space.
    let slots = unsafe { (addr as *const [u64; BODY_SLOTS]).read() };
    Body::decode(&slots)
}

fn write_body(addr: usize, body: &Body) {
    // SAFETY: as in `read_body`; `call` passes its record from a unique borrow, for writing.
    unsafe { (addr as *mut [u64; BODY_SLOTS]).write(body.encode()) }
}

impl redoubt_rt::HostKernel for Fake {
    fn syscall(&self, call: &Call) -> Result<Return, Error> {
        let pid = current();
        match *call {
            Call::MapAnon { len, .. } => {
                if len == 0 || len % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgument);
                }
                let layout = Layout::from_size_align(len, PAGE_SIZE).map_err(|_| Error::OutOfMemory)?;
                // SAFETY: the layout has a non-zero size.
                let addr = unsafe { alloc_zeroed(layout) } as usize;
                if addr == 0 {
                    return Err(Error::OutOfMemory);
                }
                self.lock().processes[pid].mappings.insert(addr, len);
                Ok(Return::Addr(addr))
            }
            Call::Unmap { addr, len } => {
                let mut s = self.lock();
                if s.processes[pid].mappings.get(&addr) != Some(&len) {
                    return Err(Error::InvalidArgument);
                }
                s.processes[pid].mappings.remove(&addr);
                // SAFETY: `addr` was returned by `alloc_zeroed` with exactly this layout
                // (MapAnon, or a transfer of such a mapping) and is unmapped only once.
                unsafe { dealloc(addr as *mut u8, Layout::from_size_align(len, PAGE_SIZE).unwrap()) };
                Ok(Return::Nothing)
            }
            Call::EndpointCreate => {
                let mut s = self.lock();
                s.endpoints.push(EndpointState::default());
                let id = s.endpoints.len() - 1;
                Ok(Return::Handle(install(&mut s, pid, Endpoint { id, badge: 0 })))
            }
            Call::Mint { source, badge, budget: None } => {
                let mut s = self.lock();
                let ep = match source {
                    MintSource::Handle(h) => {
                        let ep = lookup(&s, pid, h)?;
                        if ep.badge != 0 {
                            return Err(Error::NotPermitted);
                        }
                        ep.id
                    }
                    MintSource::Message(id) => match s.open.get(&id.get()) {
                        Some(&(receiver, ep)) if receiver == pid => ep,
                        _ => return Err(Error::InvalidArgument),
                    },
                };
                Ok(Return::Handle(install(&mut s, pid, Endpoint { id: ep, badge: badge.get() })))
            }
            Call::HandleClose { handle } => {
                let mut s = self.lock();
                lookup(&s, pid, handle)?;
                s.processes[pid].handles[handle.index() as usize] = None;
                Ok(Return::Nothing)
            }
            Call::Call { endpoint, body_rec, lend, timeout } => {
                self.message(pid, endpoint, body_rec, lend, timeout, true)
            }
            Call::Send { endpoint, body_rec, transfer, timeout } => {
                self.message(pid, endpoint, body_rec, transfer, timeout, false)
            }
            Call::Receive { from, timeout, received_rec, .. } => {
                self.receive(pid, from, timeout, received_rec)
            }
            Call::Reply { msg_id, body_rec } => {
                let body = read_body(body_rec)?;
                let mut s = self.lock();
                match s.open.get(&msg_id.get()) {
                    Some(&(receiver, _)) if receiver == pid => {}
                    _ => return Err(Error::InvalidArgument),
                }
                let handles = body
                    .handles
                    .as_slice()
                    .iter()
                    .map(|h| lookup(&s, pid, *h))
                    .collect::<Result<Vec<_>, _>>()?;
                s.open.remove(&msg_id.get());
                s.log.push((pid, "reply", msg_id.get()));
                // An abandoned call's reply is discarded (R3).
                if !s.abandoned.remove(&msg_id.get()) {
                    s.replies.insert(msg_id.get(), (body.words, handles));
                }
                self.changed.notify_all();
                Ok(Return::Nothing)
            }
            Call::TimeNow => Ok(Return::Time(self.boot.elapsed().as_micros() as u64)),
            Call::Random => {
                let mut s = self.lock();
                s.rng ^= s.rng << 13;
                s.rng ^= s.rng >> 7;
                s.rng ^= s.rng << 17;
                Ok(Return::Random(s.rng))
            }
            Call::ProcessExit { code } => {
                self.lock().exits.insert(pid, code);
                std::panic::resume_unwind(Box::new(Exited(code)))
            }
            Call::Serve { msg_id } => {
                let mut s = self.lock();
                match s.open.get(&msg_id.get()) {
                    Some(&(receiver, _)) if receiver == pid => {
                        s.log.push((pid, "serve", msg_id.get()));
                        Ok(Return::Nothing)
                    }
                    _ => Err(Error::InvalidArgument),
                }
            }
            other => panic!("the fake kernel does not model {:?}", other.number().name()),
        }
    }
}

impl Fake {
    /// `call` (`is_call`) or `send`: queue the message, then wait for the reply or the taking.
    fn message(
        &self,
        pid: usize,
        endpoint: Handle,
        body_rec: usize,
        pages: Option<Pages>,
        timeout: u64,
        is_call: bool,
    ) -> Result<Return, Error> {
        let body = read_body(body_rec)?;
        let mut s = self.lock();
        let ep = lookup(&s, pid, endpoint)?;
        let handles =
            body.handles.as_slice().iter().map(|h| lookup(&s, pid, *h)).collect::<Result<Vec<_>, _>>()?;
        if pages.is_some_and(|p| !owns(&s, pid, p)) {
            return Err(Error::InvalidArgument);
        }
        // The fake transfers whole mappings only (a Buffer always is one).
        let whole = |p: Pages| s.processes[pid].mappings.get(&p.addr) == Some(&(p.npages.get() * PAGE_SIZE));
        if !is_call && pages.is_some_and(|p| !whole(p)) {
            panic!("the fake kernel transfers only whole mappings");
        }
        let id = NonZeroU64::new(s.next_id).unwrap();
        s.next_id += 1;
        let (account, labels) = (s.processes[pid].account, s.processes[pid].labels);
        let words = body.words;
        let pending =
            Pending { id, call: is_call, badge: ep.badge, account, labels, words, handles, pages, from: pid };
        s.endpoints[ep.id].queue.push_back(pending);
        self.changed.notify_all();
        let deadline = deadline(timeout);
        let mut guard = Some(s);
        loop {
            let s = guard.as_mut().unwrap();
            if is_call {
                if let Some((words, handles)) = s.replies.remove(&id.get()) {
                    let handles: Vec<Handle> = handles.into_iter().map(|e| install(s, pid, e)).collect();
                    write_body(body_rec, &Body { words, handles: Handles::from_slice(&handles).unwrap() });
                    return Ok(Return::Nothing);
                }
            } else if s.taken.remove(&id.get()) {
                return Ok(Return::Nothing);
            }
            let queued = s.endpoints[ep.id].queue.iter().position(|p| p.id == id);
            if s.endpoints[ep.id].dead {
                if let Some(i) = queued {
                    s.endpoints[ep.id].queue.remove(i);
                }
                return Err(Error::Dead);
            }
            if !self.wait(&mut guard, deadline) {
                let s = guard.as_mut().unwrap();
                if let Some(i) = s.endpoints[ep.id].queue.iter().position(|p| p.id == id) {
                    s.endpoints[ep.id].queue.remove(i);
                    return Err(Error::Timeout);
                }
                // A call the server took is abandoned (R3): it stays open there until the
                // server replies, and the server is told.
                if let Some(&(receiver, endpoint)) = s.open.get(&id.get()) {
                    s.abandoned.insert(id.get());
                    s.notices.push_back((receiver, endpoint, id.get()));
                    self.changed.notify_all();
                    return Err(Error::Timeout);
                }
                if !is_call {
                    return Err(Error::Timeout);
                }
            }
        }
    }

    fn receive(&self, pid: usize, from: Option<Handle>, timeout: u64, rec: usize) -> Result<Return, Error> {
        let deadline = deadline(timeout);
        let mut guard = Some(self.lock());
        let Some(from) = from else {
            while self.wait(&mut guard, deadline) {}
            return Err(Error::Timeout);
        };
        let ep = lookup(guard.as_ref().unwrap(), pid, from)?;
        if ep.badge != 0 {
            return Err(Error::NotPermitted);
        }
        loop {
            let s = guard.as_mut().unwrap();
            if s.endpoints[ep.id].dead {
                return Err(Error::Dead);
            }
            // Notices before messages.
            if let Some(i) = s.notices.iter().position(|(p, e, _)| *p == pid && *e == ep.id) {
                let (_, _, id) = s.notices.remove(i).unwrap();
                let notice = Received::Abandoned(NonZeroU64::new(id).unwrap());
                // SAFETY: as below, the runtime's live, 8-aligned receive record.
                unsafe { (rec as *mut [u64; RECEIVED_SLOTS]).write(notice.encode()) };
                return Ok(Return::Nothing);
            }
            if let Some(p) = s.endpoints[ep.id].queue.pop_front() {
                let handles: Vec<Handle> = p.handles.iter().map(|e| install(s, pid, *e)).collect();
                let kind = if p.call {
                    s.open.insert(p.id.get(), (pid, ep.id));
                    MessageKind::Call { lend: p.pages }
                } else {
                    // A transfer changes owner: the mapping moves to the receiver.
                    if let Some(pages) = p.pages {
                        let len = s.processes[p.from].mappings.remove(&pages.addr).unwrap();
                        s.processes[pid].mappings.insert(pages.addr, len);
                    }
                    s.taken.insert(p.id.get());
                    MessageKind::Send { transfer: p.pages }
                };
                self.changed.notify_all();
                let handles: Vec<Option<Handle>> = handles.into_iter().map(Some).collect();
                let body =
                    ReceivedBody { words: p.words, handles: ReceivedHandles::from_slice(&handles).unwrap() };
                let message = Message {
                    kind,
                    msg_id: p.id,
                    badge: p.badge,
                    account: p.account,
                    labels: p.labels,
                    body,
                };
                // SAFETY: the runtime passes the address of a live, 8-aligned
                // `[u64; RECEIVED_SLOTS]` record it borrows mutably for the call.
                unsafe { (rec as *mut [u64; RECEIVED_SLOTS]).write(Received::Message(message).encode()) };
                return Ok(Return::Nothing);
            }
            if !self.wait(&mut guard, deadline) {
                return Err(Error::Timeout);
            }
        }
    }
}
