//! A minimal fake kernel for host tests, installed as `redoubt_rt`'s `Transport`.
//!
//! Each fake process is a host thread with its own handle table, account and labels; they
//! share the host's address space, so a lend is "mapped" into the server by passing its
//! address, and records are read and written where the runtime put them. It models what the
//! runtime and the echo pair use (endpoints, badges, `mint`, the four IPC calls, `serve`,
//! abandoned calls and their notices, `map_anon`, `unmap`, `handle_close`, `time_now`, `random`,
//! `process_exit`) and not the rest: no charging, no label check between user budgets (R1), no fair waiting
//! (R2). A lend's or a transfer's pages are given up for the message, as the kernel's are
//! (docs/kernel/ipc.md, Messages): a given-up range is refused to every call (a second lend or
//! transfer, `unmap`, `process_map`, and a record inside it, all `InvalidArgument`) until the
//! reply, or the take; a message that fails while still queued gives it back. A direct touch is
//! prevented only by the runtime's types. The executable model (`model/`) should replace it. Calls it does
//! not model panic, so a test cannot rely on them by accident.
//!
//! For launchers it models budgets as handles (`Fake::budget`), `process_create`, `process_map`,
//! `process_start` and `budget_destroy`, and keeps what each child was given for a test to read
//! back (`Fake::launched`); a child runs nothing, and a test ends it (`Fake::exit`), which sends
//! its one exit notice. Every call is logged by name (`Fake::calls`), a test can have the next
//! call of a name refused (`Fake::refuse`), or a later one (`Fake::refuse_after`), and it can read
//! the most pages a process held at once (`Fake::held_peak`).
//!
//! It also has what a driver needs: device objects (`Fake::mmio` and `Fake::irq`), `map_device`
//! over a page of host memory a test can read and write as if it were registers, `receive` on an
//! IRQ handle (R5: the source is unmasked when the receive begins and `fired` is cleared when it
//! returns), and `thread_create`, which runs the new thread as the same fake process.
//!
//! Beside it: [`scripted`], a one-call ABI seam for the outcome tests, and [`vectors`], the 9P
//! conformance runner every 9P server's tests use. Dev-only: host tests depend on this crate, and
//! nothing that runs on the machine does.

pub mod scripted;
pub mod vectors;

use std::alloc::{Layout, alloc_zeroed, dealloc};
use std::cell::Cell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::num::NonZeroU64;
use std::sync::atomic::AtomicPtr;
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use redoubt_rt::abi::{
    BODY_SLOTS, Body, Call, CallOutcome, Cause, Error, ExitNotice, FOREVER, Handle, Handles, Labels,
    LendDisposition, MAX_START_HANDLES, MemFlags, Message, MessageKind, MintSource, PAGE_SIZE, Pages,
    RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, ReplyOutcome, Return,
};
use redoubt_rt::ipc::{Request, Words};
use redoubt_rt::server::typed::{Outcome, finish};

/// What a handle names: an endpoint, or one of a device object's two forms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Object {
    Endpoint(Endpoint),
    /// A device's registers: the index of a [`State::devices`] entry.
    Mmio(usize),
    /// A device's interrupt: the index of a [`State::devices`] entry.
    Irq(usize),
    /// A budget: the index of a [`State::budgets`] entry (true once destroyed).
    Budget(usize),
    /// A launched process: the index of a [`State::launched`] entry.
    Process(usize),
}

impl Object {
    fn endpoint(self) -> Result<Endpoint, Error> {
        match self {
            Object::Endpoint(ep) => Ok(ep),
            _ => Err(Error::WrongObject),
        }
    }
}

/// One device object: a page standing in for its registers, and its interrupt's `fired` and
/// `masked` flags (kernel/devices.md R5).
struct Device {
    /// The registers, as bytes of this (host) process's memory; `map_device` returns its address.
    registers: usize,
    len: usize,
    fired: bool,
    masked: bool,
}

/// What a launcher gave one child, as `process_map` and `process_start` received it.
#[derive(Clone, Debug, Default)]
pub struct Launched {
    /// The budget it runs in (a [`State::budgets`] index).
    budget: usize,
    /// The endpoint its exit notice goes to.
    exit: usize,
    /// Each `process_map`: destination, flags, and the bytes the pages held.
    pub maps: Vec<(usize, MemFlags, Vec<u8>)>,
    /// `process_start`'s entry, stack pointer and argument, once started.
    pub start: Option<(usize, usize, usize)>,
    /// The handles `process_start` installed, as copies in this fake's own table: the launcher's
    /// handle for each, found again by [`Fake::installed`].
    handles: Vec<Object>,
    ended: bool,
}

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
    handles: Vec<Option<Object>>,
    /// Mapped ranges: address -> length.
    mappings: HashMap<usize, usize>,
    /// The most pages mapped at once since [`Fake::held_peak`] last read it.
    peak: usize,
}

impl Process {
    /// Maps `len` bytes at `addr`, raising the peak if it passes it.
    fn map(&mut self, addr: usize, len: usize) {
        self.mappings.insert(addr, len);
        self.peak = self.peak.max(self.held());
    }

    fn held(&self) -> usize { self.mappings.values().map(|len| len / PAGE_SIZE).sum() }
}

struct Pending {
    id: NonZeroU64,
    call: bool,
    badge: u64,
    account: u64,
    labels: Labels,
    words: [usize; 4],
    handles: Vec<Object>,
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
    replies: HashMap<u64, ([usize; 4], Vec<Object>)>,
    /// Sends a receiver has taken.
    taken: HashSet<u64>,
    exits: HashMap<usize, u32>,
    rng: u64,
    /// Abandoned-call notices not yet received: (receiving process, endpoint, message id).
    notices: VecDeque<(usize, usize, u64)>,
    /// Taken calls whose caller gave up, with the lend each left with its server (address,
    /// length): the reply reaches nobody and frees the lend (R3).
    abandoned: HashMap<u64, Option<(usize, usize)>>,
    /// Pages given up to a message not yet settled: message id -> (giving process, address,
    /// length). Every call of that process naming one of them is refused.
    given_up: HashMap<u64, (usize, usize, usize)>,
    /// `serve` and `reply` as they happened: (process, call, message id).
    log: Vec<(usize, &'static str, u64)>,
    /// Device objects, by the index their handles carry.
    devices: Vec<Device>,
    /// Budgets, by the index their handles carry: true once destroyed.
    budgets: Vec<bool>,
    /// The pointer `MapAnon` or `device` allocated at each address, so memory nothing frees (a
    /// heap's pages, a device's registers) stays reachable from here and Miri does not report it
    /// leaked. An entry is replaced when its address is allocated again; after an `Unmap` it is
    /// left dangling, which is harmless because nothing reads it.
    anon: HashMap<usize, AtomicPtr<u8>>,
    /// Launched processes, by the index their handles carry.
    launched: Vec<Launched>,
    /// Exit notices not yet received: (endpoint, notice).
    exits_due: VecDeque<(usize, ExitNotice)>,
    /// Every system call, by process: its name.
    calls: Vec<(usize, &'static str)>,
    /// Calls to refuse once: (process, call name, matching calls to let through first, error).
    refusals: Vec<(usize, &'static str, usize, Error)>,
}

pub struct Fake {
    state: Mutex<State>,
    changed: Condvar,
    boot: Instant,
    /// Whether a wait with no deadline may last for ever ([`Fake::never_stuck`]).
    patient: std::sync::atomic::AtomicBool,
}

thread_local! {
    static CURRENT: Cell<Option<usize>> = const { Cell::new(None) };
}

/// The unwind payload of `process_exit`.
struct Exited(u32);

/// Answers `request` with `words` and no handles, the public way (`finish`).
pub fn answer(request: Request, words: Words) -> Result<ReplyOutcome, Error> {
    finish(request, &Outcome { words, send: Handles::new(), close: Handles::new() })
}

/// Installs the fake for this test binary (once) and returns it.
pub fn fake() -> &'static Fake {
    static FAKE: std::sync::OnceLock<&'static Fake> = std::sync::OnceLock::new();
    FAKE.get_or_init(|| {
        let fake: &'static Fake = Box::leak(Box::new(Fake {
            state: Mutex::new(State { rng: 0x2545_f491_4f6c_dd1d, next_id: 1, ..State::default() }),
            changed: Condvar::new(),
            boot: Instant::now(),
            patient: std::sync::atomic::AtomicBool::new(false),
        }));
        redoubt_rt::install_transport(fake);
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
        s.processes.push(Process { account, labels, handles: vec![None], mappings: HashMap::new(), peak: 0 });
        s.processes.len() - 1
    }

    /// A new endpoint whose receive right (badge 0) goes into `owner`'s table.
    pub fn endpoint(&self, owner: usize) -> Handle {
        let mut s = self.lock();
        s.endpoints.push(EndpointState::default());
        let id = s.endpoints.len() - 1;
        install(&mut s, owner, Object::Endpoint(Endpoint { id, badge: 0 }))
    }

    /// A new device object whose two handles (registers, interrupt) go into `owner`'s table,
    /// with `len` bytes of registers, zeroed. What `init` hands a driver (servers/init.md).
    pub fn device(&self, owner: usize, len: usize) -> (Handle, Handle) {
        let mut s = self.lock();
        let layout = Layout::from_size_align(len.max(1), PAGE_SIZE).unwrap();
        // SAFETY: the layout has a non-zero size; the allocation lives as long as the fake,
        // which is leaked, so the addresses `map_device` hands out stay valid.
        let bytes = unsafe { alloc_zeroed(layout) };
        let registers = bytes as usize;
        s.anon.insert(registers, AtomicPtr::new(bytes));
        s.devices.push(Device { registers, len, fired: false, masked: false });
        let index = s.devices.len() - 1;
        let mmio = install(&mut s, owner, Object::Mmio(index));
        let irq = install(&mut s, owner, Object::Irq(index));
        (mmio, irq)
    }

    /// The device's registers, as a test reads and writes them: the bytes behind `map_device`.
    /// A test plays the part of the hardware here.
    pub fn registers(&self, owner: usize, mmio: Handle) -> &'static mut [u8] {
        let s = self.lock();
        let Ok(Object::Mmio(index)) = lookup(&s, owner, mmio) else { panic!("not an mmio handle") };
        let d = &s.devices[index];
        // SAFETY: `device` allocated `len` bytes at this address and never frees them. Tests
        // are single-threaded around each use; the borrow is `'static` because the allocation is.
        unsafe { std::slice::from_raw_parts_mut(d.registers as *mut u8, d.len) }
    }

    /// The device raises its interrupt (kernel/devices.md R5: the kernel masks the source and
    /// sets `fired`).
    pub fn fire(&self, owner: usize, irq: Handle) {
        let mut s = self.lock();
        let Ok(Object::Irq(index)) = lookup(&s, owner, irq) else { panic!("not an irq handle") };
        s.devices[index].fired = true;
        s.devices[index].masked = true;
        self.changed.notify_all();
    }

    /// Whether the device's interrupt source is masked, which only `receive` unmasks (R5).
    pub fn masked(&self, owner: usize, irq: Handle) -> bool {
        let s = self.lock();
        let Ok(Object::Irq(index)) = lookup(&s, owner, irq) else { panic!("not an irq handle") };
        s.devices[index].masked
    }

    /// Gives `to` a handle to the endpoint `from_handle` names in `from`, with `badge`: what a
    /// parent does with `mint` and `process_start`.
    pub fn grant(&self, from: usize, from_handle: Handle, to: usize, badge: u64) -> Handle {
        let mut s = self.lock();
        let ep = as_endpoint(&s, from, from_handle).expect("grant from an endpoint that exists");
        install(&mut s, to, Object::Endpoint(Endpoint { badge, ..ep }))
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
        let ep = as_endpoint(&s, owner, handle).unwrap();
        s.endpoints[ep.id].dead = true;
        self.changed.notify_all();
    }

    /// How many handles `pid` holds, and how many pages it has mapped.
    pub fn held(&self, pid: usize) -> (usize, usize) {
        let s = self.lock();
        let p = &s.processes[pid];
        (p.handles.iter().flatten().count(), p.held())
    }

    /// The most pages `pid` had mapped at once since the last read (or since it was made); the
    /// next read starts from what it holds now.
    pub fn held_peak(&self, pid: usize) -> usize {
        let mut s = self.lock();
        let p = &mut s.processes[pid];
        let held = p.held();
        std::mem::replace(&mut p.peak, held)
    }

    /// The `serve` and `reply` calls `pid` made, in order: ("serve" or "reply", message id).
    pub fn log(&self, pid: usize) -> Vec<(&'static str, u64)> {
        self.lock().log.iter().filter(|(p, _, _)| *p == pid).map(|(_, call, id)| (*call, *id)).collect()
    }

    /// Messages queued on the endpoint `owner`'s `handle` names, not yet taken.
    pub fn queued(&self, owner: usize, handle: Handle) -> usize {
        let s = self.lock();
        let ep = as_endpoint(&s, owner, handle).expect("an endpoint");
        s.endpoints[ep.id].queue.len()
    }

    /// Calls taken and not yet replied to, by any process.
    pub fn open_calls(&self, pid: usize) -> usize {
        self.lock().open.values().filter(|(p, _)| *p == pid).count()
    }

    /// A new budget whose handle goes into `owner`'s table: what a launcher carves for a child.
    pub fn budget(&self, owner: usize) -> Handle {
        let mut s = self.lock();
        s.budgets.push(false);
        let index = s.budgets.len() - 1;
        install(&mut s, owner, Object::Budget(index))
    }

    /// Whether the budget `owner` holds as `budget` was destroyed.
    pub fn destroyed(&self, owner: usize, budget: Handle) -> bool {
        let s = self.lock();
        let Ok(Object::Budget(index)) = lookup(&s, owner, budget) else { panic!("not a budget handle") };
        s.budgets[index]
    }

    /// What `owner`'s child `process` was given.
    pub fn launched(&self, owner: usize, process: Handle) -> Launched {
        let s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        s.launched[index].clone()
    }

    /// What each child made in the budget `owner` holds as `budget` was given, oldest first: read
    /// even after the launcher closed the child's handle.
    pub fn launched_in(&self, owner: usize, budget: Handle) -> Vec<Launched> {
        let s = self.lock();
        let Ok(Object::Budget(index)) = lookup(&s, owner, budget) else { panic!("not a budget handle") };
        s.launched.iter().filter(|child| child.budget == index).cloned().collect()
    }

    /// Whether the `slot`th handle (from 0) `owner`'s child `process` was started with is the object
    /// `owner`'s `handle` names.
    pub fn installed(&self, owner: usize, process: Handle, slot: usize, handle: Handle) -> bool {
        let s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        lookup(&s, owner, handle).ok() == s.launched[index].handles.get(slot).copied()
    }

    /// `owner`'s child `process` exits with `code`: its exit notice goes to its exit endpoint.
    pub fn exit(&self, owner: usize, process: Handle, code: u32) {
        let mut s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        end(&mut s, index, Cause::Exited, code);
        self.changed.notify_all();
    }

    /// The system calls `pid` made, by name, in order.
    pub fn calls(&self, pid: usize) -> Vec<&'static str> {
        self.lock().calls.iter().filter(|(p, _)| *p == pid).map(|(_, call)| *call).collect()
    }

    /// Refuses `pid`'s next call named `call` with `error`, before it does anything.
    pub fn refuse(&self, pid: usize, call: &'static str, error: Error) {
        self.refuse_after(pid, call, 0, error)
    }

    /// Lets `skip` of `pid`'s calls named `call` through, then refuses the next with `error`.
    pub fn refuse_after(&self, pid: usize, call: &'static str, skip: usize, error: Error) {
        self.lock().refusals.push((pid, call, skip, error));
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

    /// Lets a wait with no deadline last for ever, for a program run on this fake that waits on a
    /// person rather than a test: a shell whose user is thinking changes nothing for as long as
    /// they think. Tests never call it, so a test that stalls still fails after [`STUCK`].
    pub fn never_stuck(&self) { self.patient.store(true, std::sync::atomic::Ordering::Relaxed); }

    /// Seeds the kernel's `random`, for a program run on this fake for a person, whose keys must
    /// not repeat from run to run. Tests never call it, so their randomness repeats, as a test's
    /// should.
    pub fn seed_random(&self, seed: u64) { self.lock().rng = seed | 1; }

    /// Waits for a change or the deadline; false once the deadline has passed. A wait with no
    /// deadline (`FOREVER`) that sees nothing change anywhere in this kernel for [`STUCK`] panics:
    /// the test is stuck (a server that ended early, a reply never sent), and fails naming it
    /// rather than hanging the run. Load slows a test's progress; it does not stop every change.
    fn wait<'a>(&self, guard: &mut Option<MutexGuard<'a, State>>, deadline: Option<Instant>) -> bool {
        let g = guard.take().unwrap();
        match deadline {
            None if self.patient.load(std::sync::atomic::Ordering::Relaxed) => {
                *guard = Some(self.changed.wait(g).unwrap_or_else(|e| e.into_inner()));
            }
            None => {
                let (g, waited) = self.changed.wait_timeout(g, STUCK).unwrap_or_else(|e| e.into_inner());
                if waited.timed_out() {
                    drop(g);
                    panic!(
                        "a FOREVER wait saw no change in the fake kernel for {STUCK:?}: the test is stuck"
                    );
                }
                *guard = Some(g);
            }
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

fn install(s: &mut State, pid: usize, ep: Object) -> Handle {
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

fn lookup(s: &State, pid: usize, h: Handle) -> Result<Object, Error> {
    s.processes[pid].handles.get(h.index() as usize).copied().flatten().ok_or(Error::BadHandle)
}

/// The endpoint `h` names in `pid`, or `WrongObject` if it names a device.
fn as_endpoint(s: &State, pid: usize, h: Handle) -> Result<Endpoint, Error> { lookup(s, pid, h)?.endpoint() }

/// Ends launched process `index`, once: its exit notice is queued on its exit endpoint.
fn end(s: &mut State, index: usize, cause: Cause, code: u32) {
    let child = &mut s.launched[index];
    if child.ended {
        return;
    }
    child.ended = true;
    let notice =
        ExitNotice { pid: 1000 + index as u32, cause, code, blamed_account: 0, blamed_labels: Labels::new() };
    let exit = child.exit;
    s.exits_due.push_back((exit, notice));
}

/// Frees a mapping this kernel made, which its process no longer holds.
fn free(addr: usize, len: usize) {
    // SAFETY: `addr` was returned by `alloc_zeroed` with exactly this layout (MapAnon, or a
    // transfer or lend of such a mapping), and its caller has just taken it out of its process's
    // mappings, so it is freed only once.
    unsafe { dealloc(addr as *mut u8, Layout::from_size_align(len, PAGE_SIZE).unwrap()) };
}

/// How long a wait with no deadline may see nothing change before the test is called stuck: far
/// beyond any step of any test here, however loaded the machine.
const STUCK: Duration = Duration::from_secs(60);

fn deadline(timeout: u64) -> Option<Instant> {
    (timeout != FOREVER).then(|| Instant::now() + Duration::from_micros(timeout))
}

/// Whether `pages` lies inside one of `pid`'s mappings.
fn owns(s: &State, pid: usize, pages: Pages) -> bool {
    let len = pages.npages.get() * PAGE_SIZE;
    pages.addr.is_multiple_of(PAGE_SIZE)
        && s.processes[pid].mappings.iter().any(|(&a, &l)| a <= pages.addr && pages.addr + len <= a + l)
}

/// `InvalidArgument` if any byte of `addr..addr + len` lies in pages `pid` has given up.
fn not_given_up(s: &State, pid: usize, addr: usize, len: usize) -> Result<(), Error> {
    // An end that overflows lies past every other, so it overlaps.
    let ends_after = |start: usize, len: usize, at: usize| start.checked_add(len).is_none_or(|end| at < end);
    if s.given_up.values().any(|&(p, a, l)| p == pid && ends_after(a, l, addr) && ends_after(addr, len, a)) {
        return Err(Error::InvalidArgument);
    }
    Ok(())
}

/// The taken call `id` of `pid`'s loses its caller (R3), which gets `status`: it stays open at
/// `receiver` until the reply, which reaches nobody and frees the lend. Until then the lend is the
/// server's alone and stays given up by its caller; the reply releases both.
fn abandon(
    s: &mut State,
    pid: usize,
    receiver: usize,
    id: NonZeroU64,
    pages: Option<Pages>,
    status: Error,
) -> Return {
    let lend = pages.map(|pages| {
        let len = s.processes[pid].mappings.remove(&pages.addr).unwrap();
        s.processes[receiver].map(pages.addr, len);
        (pages.addr, len)
    });
    s.abandoned.insert(id.get(), lend);
    Return::Call(CallOutcome {
        status: Err(status),
        lend: if pages.is_some() { LendDisposition::Consumed } else { LendDisposition::None },
        reply_present: false,
    })
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

// SAFETY: it returns only memory it allocated and keeps alive for the process, or pages a caller
// lent or transferred, which that caller's runtime gave up for the call.
unsafe impl redoubt_rt::Transport for Fake {
    fn call(&self, call: &Call) -> Result<Return, Error> {
        let pid = current();
        {
            let name = call.number().name();
            let mut s = self.lock();
            s.calls.push((pid, name));
            if let Some(i) = s.refusals.iter().position(|(p, c, _, _)| *p == pid && *c == name) {
                if s.refusals[i].2 == 0 {
                    return Err(s.refusals.remove(i).3);
                }
                s.refusals[i].2 -= 1;
            }
        }
        match *call {
            Call::MapAnon { len, .. } => {
                if len == 0 || len % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgument);
                }
                let layout = Layout::from_size_align(len, PAGE_SIZE).map_err(|_| Error::OutOfMemory)?;
                // SAFETY: the layout has a non-zero size.
                let pages = unsafe { alloc_zeroed(layout) };
                let addr = pages as usize;
                if addr == 0 {
                    return Err(Error::OutOfMemory);
                }
                let mut s = self.lock();
                s.processes[pid].map(addr, len);
                s.anon.insert(addr, AtomicPtr::new(pages));
                Ok(Return::Addr(addr))
            }
            Call::Unmap { addr, len } => {
                let mut s = self.lock();
                if s.processes[pid].mappings.get(&addr) != Some(&len) {
                    return Err(Error::InvalidArgument);
                }
                not_given_up(&s, pid, addr, len)?;
                s.processes[pid].mappings.remove(&addr);
                free(addr, len);
                Ok(Return::Nothing)
            }
            Call::EndpointCreate => {
                let mut s = self.lock();
                s.endpoints.push(EndpointState::default());
                let id = s.endpoints.len() - 1;
                Ok(Return::Handle(install(&mut s, pid, Object::Endpoint(Endpoint { id, badge: 0 }))))
            }
            Call::MapDevice { device } => {
                let s = self.lock();
                let Object::Mmio(index) = lookup(&s, pid, device)? else {
                    return Err(Error::WrongObject);
                };
                let d = &s.devices[index];
                Ok(Return::Mapping { addr: d.registers, len: d.len })
            }
            Call::ThreadCreate { entry, arg, .. } => {
                // The new thread is the same fake process: it shares its handle table, as a
                // thread does on the machine.
                // SAFETY (host test code only): `entry` is a `fn(usize) -> !` the caller cast
                // to a `usize`, which is what `process_start` and `thread_create` take.
                let entry: extern "C" fn(usize) -> ! = unsafe { std::mem::transmute(entry) };
                std::thread::spawn(move || {
                    CURRENT.with(|c| c.set(Some(pid)));
                    // The body never returns; `process_exit` unwinds, which ends this thread.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| entry(arg)));
                });
                Ok(Return::Tid(1))
            }
            Call::Mint { source, badge, budget: None } => {
                let mut s = self.lock();
                let ep = match source {
                    MintSource::Handle(h) => {
                        let ep = as_endpoint(&s, pid, h)?;
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
                Ok(Return::Handle(install(
                    &mut s,
                    pid,
                    Object::Endpoint(Endpoint { id: ep, badge: badge.get() }),
                )))
            }
            Call::HandleClose { handle } => {
                let mut s = self.lock();
                lookup(&s, pid, handle)?;
                s.processes[pid].handles[handle.index() as usize] = None;
                Ok(Return::Nothing)
            }
            Call::Call { endpoint, body_rec, lend, timeout } => {
                match self.message(pid, endpoint, body_rec, lend, timeout, true) {
                    Ok(result) => Ok(result),
                    Err(error) => Ok(Return::Call(CallOutcome {
                        status: Err(error),
                        lend: if lend.is_some() { LendDisposition::Returned } else { LendDisposition::None },
                        reply_present: false,
                    })),
                }
            }
            Call::Send { endpoint, body_rec, transfer, timeout } => {
                self.message(pid, endpoint, body_rec, transfer, timeout, false)
            }
            Call::Receive { from, timeout, received_rec, .. } => {
                self.receive(pid, from, timeout, received_rec)
            }
            Call::Reply { msg_id, body_rec } => {
                let mut s = self.lock();
                not_given_up(&s, pid, body_rec, BODY_SLOTS * 8)?;
                let body = read_body(body_rec)?;
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
                // The lend is the caller's again (or, abandoned, the server's to free).
                s.given_up.remove(&msg_id.get());
                s.log.push((pid, "reply", msg_id.get()));
                // An abandoned call's reply is discarded, and frees its lend (R3).
                let abandoned = s.abandoned.remove(&msg_id.get());
                if let Some(Some((addr, len))) = abandoned {
                    s.processes[pid].mappings.remove(&addr);
                    free(addr, len);
                }
                let delivered = abandoned.is_none();
                let installed = if delivered { (1 << handles.len()) - 1 } else { 0 };
                if delivered {
                    s.replies.insert(msg_id.get(), (body.words, handles));
                }
                self.changed.notify_all();
                Ok(Return::Reply(ReplyOutcome { delivered, installed }))
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
            Call::ProcessCreate { budget, exit_endpoint } => {
                let mut s = self.lock();
                let Object::Budget(budget) = lookup(&s, pid, budget)? else { return Err(Error::WrongObject) };
                if s.budgets[budget] {
                    return Err(Error::BadHandle);
                }
                let exit = as_endpoint(&s, pid, exit_endpoint)?;
                if exit.badge != 0 {
                    return Err(Error::NotPermitted);
                }
                s.launched.push(Launched { budget, exit: exit.id, ..Launched::default() });
                let index = s.launched.len() - 1;
                Ok(Return::Handle(install(&mut s, pid, Object::Process(index))))
            }
            Call::ProcessMap { process, src, dst, len, flags } => {
                let mut s = self.lock();
                let Object::Process(index) = lookup(&s, pid, process)? else {
                    return Err(Error::WrongObject);
                };
                if s.launched[index].start.is_some() {
                    return Err(Error::NotPermitted);
                }
                if s.processes[pid].mappings.get(&src) != Some(&len) {
                    return Err(Error::InvalidArgument);
                }
                not_given_up(&s, pid, src, len)?;
                s.processes[pid].mappings.remove(&src);
                // SAFETY: `src..src + len` is a whole mapping this kernel made for `pid` (checked
                // just above) and has just left its table, so nothing else reads or frees it.
                let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) }.to_vec();
                free(src, len);
                s.launched[index].maps.push((dst, flags, bytes));
                Ok(Return::Nothing)
            }
            Call::ProcessStart { process, entry, sp, arg, handles_rec, count } => {
                let mut s = self.lock();
                not_given_up(&s, pid, handles_rec, MAX_START_HANDLES * 8)?;
                // SAFETY: the runtime passes the address of a live, 8-aligned
                // `[u64; MAX_START_HANDLES]` record it owns for the call (`Process::start`).
                let raw = unsafe { (handles_rec as *const [u64; MAX_START_HANDLES]).read() };
                let Object::Process(index) = lookup(&s, pid, process)? else {
                    return Err(Error::WrongObject);
                };
                if s.launched[index].start.is_some() {
                    return Err(Error::NotPermitted);
                }
                let handles = raw
                    .get(..count as usize)
                    .ok_or(Error::TooLarge)?
                    .iter()
                    .map(|raw| lookup(&s, pid, Handle::from_raw(*raw)?))
                    .collect::<Result<Vec<_>, _>>()?;
                let child = &mut s.launched[index];
                child.handles = handles;
                child.start = Some((entry, sp, arg));
                Ok(Return::Nothing)
            }
            Call::BudgetDestroy { budget } => {
                let mut s = self.lock();
                let Object::Budget(index) = lookup(&s, pid, budget)? else { return Err(Error::WrongObject) };
                s.budgets[index] = true;
                let children: Vec<usize> =
                    (0..s.launched.len()).filter(|&i| s.launched[i].budget == index).collect();
                for child in children {
                    end(&mut s, child, Cause::Killed, 0);
                }
                self.changed.notify_all();
                Ok(Return::Nothing)
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
        let mut s = self.lock();
        not_given_up(&s, pid, body_rec, BODY_SLOTS * 8)?;
        let body = read_body(body_rec)?;
        let ep = as_endpoint(&s, pid, endpoint)?;
        let handles =
            body.handles.as_slice().iter().map(|h| lookup(&s, pid, *h)).collect::<Result<Vec<_>, _>>()?;
        if let Some(p) = pages {
            if !owns(&s, pid, p) {
                return Err(Error::InvalidArgument);
            }
            not_given_up(&s, pid, p.addr, p.npages.get() * PAGE_SIZE)?;
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
        // From here until the message settles, the pages are given up (docs/kernel/ipc.md,
        // Messages).
        if let Some(p) = pages {
            s.given_up.insert(id.get(), (pid, p.addr, p.npages.get() * PAGE_SIZE));
        }
        self.changed.notify_all();
        let deadline = deadline(timeout);
        let mut guard = Some(s);
        let result = loop {
            let s = guard.as_mut().unwrap();
            if is_call {
                if let Some((words, handles)) = s.replies.remove(&id.get()) {
                    let handles: Vec<Handle> = handles.into_iter().map(|e| install(s, pid, e)).collect();
                    write_body(body_rec, &Body { words, handles: Handles::from_slice(&handles).unwrap() });
                    break Ok(Return::Call(CallOutcome {
                        status: Ok(()),
                        lend: if pages.is_some() { LendDisposition::Returned } else { LendDisposition::None },
                        reply_present: true,
                    }));
                }
            } else if s.taken.remove(&id.get()) {
                break Ok(Return::Nothing);
            }
            let queued = s.endpoints[ep.id].queue.iter().position(|p| p.id == id);
            if s.endpoints[ep.id].dead {
                if let Some(i) = queued {
                    s.endpoints[ep.id].queue.remove(i);
                    break Err(Error::Dead);
                }
                // A call already taken is abandoned by the endpoint's destruction, and no notice
                // follows: there is no endpoint left to receive one on (R3).
                if let Some(&(receiver, _)) = s.open.get(&id.get()) {
                    return Ok(abandon(s, pid, receiver, id, pages, Error::Dead));
                }
                break Err(Error::Dead);
            }
            if !self.wait(&mut guard, deadline) {
                let s = guard.as_mut().unwrap();
                if let Some(i) = s.endpoints[ep.id].queue.iter().position(|p| p.id == id) {
                    s.endpoints[ep.id].queue.remove(i);
                    break Err(Error::Timeout);
                }
                // A call the server took is abandoned (R3): it stays open there until the
                // server replies, and the server is told.
                if let Some(&(receiver, endpoint)) = s.open.get(&id.get()) {
                    s.notices.push_back((receiver, endpoint, id.get()));
                    self.changed.notify_all();
                    return Ok(abandon(s, pid, receiver, id, pages, Error::Timeout));
                }
                if !is_call {
                    break Err(Error::Timeout);
                }
            }
        };
        // Settled: refused or timed out while queued, replied to, or taken (a transfer). The pages
        // are the sender's again, or the receiver's. An abandoned call returned above: its pages
        // stay given up until the server's reply.
        guard.as_mut().unwrap().given_up.remove(&id.get());
        result
    }

    fn receive(&self, pid: usize, from: Option<Handle>, timeout: u64, rec: usize) -> Result<Return, Error> {
        let deadline = deadline(timeout);
        let mut guard = Some(self.lock());
        not_given_up(guard.as_ref().unwrap(), pid, rec, RECEIVED_SLOTS * 8)?;
        let Some(from) = from else {
            while self.wait(&mut guard, deadline) {}
            return Err(Error::Timeout);
        };
        // An IRQ handle: unmask the source (R5), then wait for it to fire.
        if let Object::Irq(index) = lookup(guard.as_ref().unwrap(), pid, from)? {
            guard.as_mut().unwrap().devices[index].masked = false;
            self.changed.notify_all();
            loop {
                let s = guard.as_mut().unwrap();
                if std::mem::take(&mut s.devices[index].fired) {
                    // SAFETY: the runtime's live, 8-aligned receive record.
                    unsafe { (rec as *mut [u64; RECEIVED_SLOTS]).write(Received::Interrupt.encode()) };
                    return Ok(Return::Nothing);
                }
                if !self.wait(&mut guard, deadline) {
                    return Err(Error::Timeout);
                }
            }
        }
        let ep = as_endpoint(guard.as_ref().unwrap(), pid, from)?;
        if ep.badge != 0 {
            return Err(Error::NotPermitted);
        }
        loop {
            let s = guard.as_mut().unwrap();
            if s.endpoints[ep.id].dead {
                return Err(Error::Dead);
            }
            // Notices before messages.
            if let Some(i) = s.exits_due.iter().position(|(e, _)| *e == ep.id) {
                let (_, notice) = s.exits_due.remove(i).unwrap();
                // SAFETY: as below, the runtime's live, 8-aligned receive record.
                unsafe { (rec as *mut [u64; RECEIVED_SLOTS]).write(Received::Exit(notice).encode()) };
                return Ok(Return::Nothing);
            }
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
                        s.processes[pid].map(pages.addr, len);
                        s.given_up.remove(&p.id.get());
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
