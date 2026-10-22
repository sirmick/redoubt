//! A minimal fake kernel for host tests, installed as `redoubt_rt`'s `Transport`.
//!
//! Each fake process is a host thread with its own handle table, account and labels; they
//! share the host's address space, so a lend is "mapped" into the server by passing its
//! address, and records are read and written where the runtime put them. It models what the
//! runtime and the echo pair use (endpoints, badges, `mint`, the four IPC calls and the two turns,
//! each a reply or send and then a receive in one call, `serve`,
//! abandoned calls and their notices, `map_anon`, `map_fixed` (host pages at the address named,
//! for a spawned thread's stack), `unmap`, `handle_close`, `time_now`, `random`,
//! `process_exit`) and not the rest: no charging, no label check between user budgets (R1), no fair waiting
//! (R2). A lend's or a transfer's pages are given up for the message, as the kernel's are
//! (docs/kernel/ipc.md, Messages): a given-up range is refused to every call (a second lend or
//! transfer, `unmap`, `process_map`, and a record inside it, all `InvalidArgument`) until the
//! reply, or the take; a message that fails while still queued gives it back. A direct touch is
//! prevented only by the runtime's types. The executable model (`model/`) should replace it. Calls it does
//! not model panic, so a test cannot rely on them by accident.
//!
//! For launchers it models budgets as handles with labels (`Fake::budget`, which carries its
//! owner's), `budget_create` (the kernel's label rule for a user-class caller, nothing charged),
//! `process_create`, `process_map`, `process_start`, `endpoint_create` naming a budget (owned by
//! it, the caller's own or below it), and `budget_destroy`, which destroys the endpoints the budget
//! and those below it own and leaves every handle to any of them, and every handle a process
//! running in one made (`Fake::play`), a dead entry in every table: `Dead` to every call, its index
//! held until its holder closes it, as the kernel's are (I17). It kills the processes running in
//! them too: their open calls fail their callers `Dead` (R4b), and every call a killed process's
//! host thread makes after fails `Dead`, which ends a played server's loop. It keeps what each
//! child was given for a test to read back (`Fake::launched`); a child runs nothing, and a test
//! ends it (`Fake::exit`), which sends its one exit notice. Every call is logged by name
//! (`Fake::calls`), a test can have the next call of a name refused (`Fake::refuse`), or a later
//! one (`Fake::refuse_after`), and it can read the most pages a process held at once
//! (`Fake::held_peak`). Its clock is the host's until a test
//! holds it (`Fake::hold_clock`) and moves it itself (`Fake::tick`), so a deadline, a server's own
//! or a call's timeout, is a step the test takes and never a race against the host. A tick is a
//! quiescent step: it is taken only once every other thread of the fake is in a wait (a thread a
//! change has woken is not, until it waits again), so it ends the waits the test wrote it to end
//! and never falls between a server's reading of the time and the receive that reading gave its
//! deadline, nor between a change and the reaction of the thread it woke.
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
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use redoubt_rt::abi::{
    BODY_SLOTS, BUDGET_SPEC_SLOTS, Body, BudgetSpec, Call, CallOutcome, Cause, Error, ExitNotice,
    FIRST_SLOTS, FOREVER, FirstDone, FirstHalf, Handle, Handles, Hold, Labels, LendDisposition,
    MAX_RANGE_PAGES, MAX_START_HANDLES, MemFlags, Message, MessageKind, MintSource, PAGE_SIZE, Pages,
    RECEIVED_SLOTS, Received, ReceivedBody, ReceivedHandles, ReplyOutcome, Return, TurnOutcome, range_cap,
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
    /// A budget: the index of a [`State::budgets`] entry.
    Budget(usize),
    /// A launched process: the index of a [`State::launched`] entry.
    Process(usize),
    /// A dead entry: a handle a destruction swept, which names nothing and answers `Dead` until
    /// its holder closes it (kernel/invariants.md I17).
    Dead,
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

/// One budget: its labels, fixed when it was made, the budget it was carved from, and whether it
/// was destroyed.
#[derive(Clone, Copy, Default)]
struct Budget {
    labels: Labels,
    parent: Option<usize>,
    destroyed: bool,
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

/// What a handle names. Only endpoints are modelled. Its stamp is the budget the process that made
/// it runs in ([`Fake::play`]), if it was given one: destroying that budget revokes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Endpoint {
    id: usize,
    badge: u64,
    stamp: Option<usize>,
}

struct Process {
    account: u64,
    labels: Labels,
    /// The budget it runs in, if a test said so ([`Fake::play`]): what stamps the handles it makes.
    runs_in: Option<usize>,
    /// Killed by its budget's destruction: every call it makes fails `Dead`.
    killed: bool,
    /// Index 0 is never a handle.
    handles: Vec<Option<Object>>,
    /// Mapped ranges: address -> length.
    mappings: HashMap<usize, usize>,
    /// Where its `MapAnon` places pages ([`Area`]): its own, as each address space's is.
    area: Area,
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

    /// Takes `addr..addr + len` out of the one mapping that holds it, as the kernel unmaps or moves
    /// any part of a mapping: what is left of it on either side stays mapped, each part a mapping
    /// of its own. The holding mapping's start and length, or `None` if no one mapping holds it.
    fn carve(&mut self, addr: usize, len: usize) -> Option<(usize, usize)> {
        let (&at, &whole) = self.mappings.iter().find(|&(&a, &l)| a <= addr && addr + len <= a + l)?;
        self.mappings.remove(&at);
        if addr > at {
            self.mappings.insert(at, addr - at);
        }
        if addr + len < at + whole {
            self.mappings.insert(addr + len, at + whole - addr - len);
        }
        Some((at, whole))
    }

    /// Whether one mapping holds all of `addr..addr + len`.
    fn holds(&self, addr: usize, len: usize) -> bool {
        self.mappings.iter().any(|(&a, &l)| a <= addr && addr + len <= a + l)
    }
}

struct Pending {
    id: NonZeroU64,
    call: bool,
    badge: u64,
    /// The stamp of the handle it was sent through: what a `mint` from the call stamps with.
    stamp: Option<usize>,
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
    /// The budget that owns it, whose destruction destroys it; none for the fake's root.
    owner: Option<usize>,
}

#[derive(Default)]
struct State {
    processes: Vec<Process>,
    endpoints: Vec<EndpointState>,
    next_id: u64,
    /// Calls taken and not replied to: id -> (receiving process, endpoint).
    /// Each open call: its receiver, the endpoint it arrived on and the stamp of the handle it
    /// came through (the default stamp of a `mint` from it, kernel/objects.md R9).
    open: HashMap<u64, (usize, usize, Option<usize>)>,
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
    /// Budgets, by the index their handles carry.
    budgets: Vec<Budget>,
    /// The handles to budgets a destruction swept, by (process, index): the budget each named,
    /// for [`Fake::destroyed`] to read once its slot is a dead entry or closed.
    freed: HashMap<(usize, usize), usize>,
    /// The pointer `MapAnon` or `device` allocated at each address, so memory nothing frees (a
    /// heap's pages, a device's registers) stays reachable from here and Miri does not report it
    /// leaked. An entry is replaced when its address is allocated again; after an `Unmap` it is
    /// left dangling, which is harmless because nothing reads it (under Miri a run's entry goes
    /// with its allocation).
    anon: HashMap<usize, AtomicPtr<u8>>,
    /// The addresses `MapFixed` mapped outside every [`Area`], host pages from `mmap` at the
    /// address the runtime named (a spawned thread's stack above its guard page), which `Unmap`
    /// gives back with `munmap`.
    fixed: HashSet<usize>,
    /// Launched processes, by the index their handles carry.
    launched: Vec<Launched>,
    /// Exit notices not yet received: (endpoint, notice).
    exits_due: VecDeque<(usize, ExitNotice)>,
    /// Every system call, by process: its name.
    calls: Vec<(usize, &'static str)>,
    /// Calls to refuse once: (process, call name, matching calls to let through first, error).
    refusals: Vec<(usize, &'static str, usize, Error)>,
    /// Every `console_hold`, by process: what it asked and what a take was answered.
    holds: Vec<(usize, Hold, bool)>,
    /// Processes whose next take of the console's hold is told a kernel line ended their
    /// unfinished line ([`Fake::console_line_ended`]).
    console_ended: HashSet<usize>,
    /// The threads of the fake ([`Counted`]) alive, and how many of them are in `wait`: a tick
    /// of the held clock is taken only when every one but the ticker's is ([`Fake::tick`]). A
    /// change ([`Fake::change`]) wakes every waiter, so it counts none of them until each waits
    /// again; `changes` counts the changes, so a wait that ended for another reason (its
    /// deadline, a spurious wake) can tell it is still counted and discount itself.
    alive: usize,
    waiting: usize,
    changes: u64,
}

pub struct Fake {
    state: Mutex<State>,
    changed: Condvar,
    /// Signalled when a thread of the fake enters a wait or ends, for a tick waiting on them.
    quiet: Condvar,
    boot: Instant,
    /// Whether a test holds the clock ([`Fake::hold_clock`]), and where it stands then, in
    /// microseconds since boot.
    held: AtomicBool,
    clock: AtomicU64,
    /// Whether a wait with no deadline may last for ever ([`Fake::never_stuck`]).
    patient: AtomicBool,
}

thread_local! {
    static CURRENT: Cell<Option<usize>> = const { Cell::new(None) };
    /// Whether this thread is one of the fake's ([`Counted`]).
    static COUNTED: Cell<bool> = const { Cell::new(false) };
}

/// A thread of the fake, counted in `State::alive` while it lives: one `run`, `as_process` or
/// `thread_create` made. A tick of the held clock waits for every one but the ticker's own to be
/// in `wait` ([`Fake::tick`]). A thread counted once stays counted once, however the three nest.
struct Counted<'a>(Option<&'a Fake>);

impl<'a> Counted<'a> {
    fn new(fake: &'a Fake) -> Self {
        if COUNTED.with(Cell::get) {
            return Counted(None);
        }
        COUNTED.with(|c| c.set(true));
        fake.lock().alive += 1;
        Counted(Some(fake))
    }
}

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        if let Some(fake) = self.0 {
            fake.uncount();
        }
    }
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
            quiet: Condvar::new(),
            boot: Instant::now(),
            held: AtomicBool::new(false),
            clock: AtomicU64::new(0),
            patient: AtomicBool::new(false),
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

    /// A new process (no handles yet), whose labels are the set `labels` names: sorted and without
    /// repeats, as the kernel keeps a budget's.
    pub fn process(&self, account: u64, labels: &[u64]) -> usize {
        let mut s = self.lock();
        let labels = Labels::from_slice(&set(labels)).unwrap();
        s.processes.push(Process {
            account,
            labels,
            runs_in: None,
            killed: false,
            handles: vec![None],
            mappings: HashMap::new(),
            area: Area::default(),
            peak: 0,
        });
        s.processes.len() - 1
    }

    /// A new endpoint whose receive right (badge 0) goes into `owner`'s table, owned by the budget
    /// `owner` runs in.
    pub fn endpoint(&self, owner: usize) -> Handle {
        let mut s = self.lock();
        let stamp = s.processes[owner].runs_in;
        new_endpoint(&mut s, owner, stamp)
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
        self.change(&mut s);
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
        let stamp = s.processes[from].runs_in;
        install(&mut s, to, Object::Endpoint(Endpoint { badge, stamp, ..ep }))
    }

    /// Copies `from`'s handle `from_handle` into `to`, badge and all: what `process_start` does
    /// with the handles it installs.
    pub fn copy(&self, from: usize, from_handle: Handle, to: usize) -> Handle {
        let mut s = self.lock();
        let ep = lookup(&s, from, from_handle).expect("copy a handle that exists");
        install(&mut s, to, ep)
    }

    /// Destroys the endpoint: its receivers and blocked callers get `Dead`. One already destroyed
    /// with the budget that owned it, its handle a dead entry, is left as it is.
    pub fn destroy(&self, owner: usize, handle: Handle) {
        let mut s = self.lock();
        if s.processes[owner].killed && lookup(&s, owner, handle).is_err() {
            return;
        }
        let ep = as_endpoint(&s, owner, handle).unwrap();
        s.endpoints[ep.id].dead = true;
        self.change(&mut s);
    }

    /// How many handles `pid` holds, and how many pages it has mapped.
    pub fn held(&self, pid: usize) -> (usize, usize) {
        let s = self.lock();
        let p = &s.processes[pid];
        (p.handles.iter().flatten().count(), p.held())
    }

    /// What `pid` has mapped: each range's address and length, in address order.
    pub fn mappings(&self, pid: usize) -> Vec<(usize, usize)> {
        let s = self.lock();
        let mut mappings: Vec<(usize, usize)> =
            s.processes[pid].mappings.iter().map(|(&a, &l)| (a, l)).collect();
        mappings.sort_unstable();
        mappings
    }

    /// Waits until `pid` holds exactly `handles` handles, at most `timeout`: `true` when it does.
    /// For a test that knows what a thread of the process is about to close or make, and waits for
    /// that rather than for a count sampled at some moment.
    pub fn wait_held(&self, pid: usize, handles: usize, timeout: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + timeout;
        let mut s = self.lock();
        loop {
            if s.processes[pid].handles.iter().flatten().count() == handles {
                return true;
            }
            let now = std::time::Instant::now();
            if now >= deadline {
                return false;
            }
            s = self.changed.wait_timeout(s, deadline - now).unwrap_or_else(|e| e.into_inner()).0;
        }
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
        self.lock().open.values().filter(|(p, ..)| *p == pid).count()
    }

    /// A new budget whose handle goes into `owner`'s table: what a launcher carves for a child. It
    /// carries `owner`'s labels, as the budget a process runs in does.
    pub fn budget(&self, owner: usize) -> Handle {
        let mut s = self.lock();
        let labels = s.processes[owner].labels;
        s.budgets.push(Budget { labels, parent: None, destroyed: false });
        let index = s.budgets.len() - 1;
        install(&mut s, owner, Object::Budget(index))
    }

    /// Whether the budget `owner` holds as `budget` was destroyed (its handle is then gone, as the
    /// kernel's is).
    pub fn destroyed(&self, owner: usize, budget: Handle) -> bool {
        let s = self.lock();
        if let Some(index) = s.freed.get(&(owner, budget.index() as usize)) {
            return s.budgets[*index].destroyed;
        }
        let Ok(Object::Budget(index)) = lookup(&s, owner, budget) else { panic!("not a budget handle") };
        s.budgets[index].destroyed
    }

    /// A test says `pid` runs in the budget it holds as `budget`: the endpoints it makes and the
    /// handles it grants from now on are stamped with it, as [`Fake::play`] says of a child's.
    pub fn runs_in(&self, pid: usize, budget: Handle) {
        let mut s = self.lock();
        let Ok(Object::Budget(index)) = lookup(&s, pid, budget) else { panic!("not a budget handle") };
        s.processes[pid].runs_in = Some(index);
    }

    /// A test plays `child`, which the fake does not run, as the process `pid`: `pid` runs in the
    /// child's budget, so the handles it makes from now on are stamped with it and go when it is
    /// destroyed (kernel/objects.md R9), and it holds a copy of each handle the child was started
    /// with, returned in the child's order.
    pub fn play(&self, child: &Launched, pid: usize) -> Vec<Handle> {
        let mut s = self.lock();
        s.processes[pid].runs_in = Some(child.budget);
        child.handles.iter().map(|object| install(&mut s, pid, *object)).collect()
    }

    /// What the child launched last, by any launcher, was given: for a launcher whose handles the
    /// test cannot name, as a VM's.
    pub fn last_launched(&self) -> Option<Launched> { self.lock().launched.last().cloned() }

    /// What the child launched last to serve `owner` was given: the latest whose first handle is a
    /// receive right to an endpoint `owner` holds a handle to. For a launcher whose handles the test
    /// cannot name, beside other tests' launchers in the same fake.
    pub fn served_child(&self, owner: usize) -> Option<Launched> {
        let s = self.lock();
        let held: Vec<usize> = s.processes[owner]
            .handles
            .iter()
            .flatten()
            .filter_map(|o| o.endpoint().ok())
            .map(|e| e.id)
            .collect();
        s.launched
            .iter()
            .rev()
            .find(|child| {
                matches!(child.handles.first(), Some(Object::Endpoint(ep)) if ep.badge == 0 && held.contains(&ep.id))
            })
            .cloned()
    }

    /// What `owner`'s child `process` was given.
    pub fn launched(&self, owner: usize, process: Handle) -> Launched {
        let s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        s.launched[index].clone()
    }

    /// What each child made in the budget `owner` holds as `budget` was given, oldest first: read
    /// even after the launcher closed the child's handle, or destroyed the budget.
    pub fn launched_in(&self, owner: usize, budget: Handle) -> Vec<Launched> {
        let s = self.lock();
        let index = match s.freed.get(&(owner, budget.index() as usize)) {
            Some(index) => *index,
            None => {
                let Ok(Object::Budget(index)) = lookup(&s, owner, budget) else {
                    panic!("not a budget handle")
                };
                index
            }
        };
        s.launched.iter().filter(|child| child.budget == index).cloned().collect()
    }

    /// What each child made in a budget carved from the one `owner` holds as `parent` was given,
    /// oldest first: a launcher's children in budgets it carved, whose handles the test never
    /// sees, even after it destroyed them.
    pub fn launched_under(&self, owner: usize, parent: Handle) -> Vec<Launched> {
        let s = self.lock();
        let Ok(Object::Budget(index)) = lookup(&s, owner, parent) else { panic!("not a budget handle") };
        s.launched.iter().filter(|child| s.budgets[child.budget].parent == Some(index)).cloned().collect()
    }

    /// Whether the `slot`th handle (from 0) `owner`'s child `process` was started with is the object
    /// `owner`'s `handle` names.
    pub fn installed(&self, owner: usize, process: Handle, slot: usize, handle: Handle) -> bool {
        let s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        lookup(&s, owner, handle).ok() == s.launched[index].handles.get(slot).copied()
    }

    /// If the `slot`th handle (from 0) `child` was started with is an endpoint's receive right (badge
    /// 0): the badges of the handles to that endpoint `owner` holds, 0 for a receive right of its own.
    pub fn served(&self, owner: usize, child: &Launched, slot: usize) -> Option<Vec<u64>> {
        let s = self.lock();
        let Some(Object::Endpoint(ep)) = child.handles.get(slot).copied() else { return None };
        if ep.badge != 0 {
            return None;
        }
        let held = s.processes[owner].handles.iter().flatten();
        Some(held.filter_map(|o| o.endpoint().ok()).filter(|e| e.id == ep.id).map(|e| e.badge).collect())
    }

    /// `owner`'s child `process` exits with `code`: its exit notice goes to its exit endpoint.
    pub fn exit(&self, owner: usize, process: Handle, code: u32) {
        let mut s = self.lock();
        let Ok(Object::Process(index)) = lookup(&s, owner, process) else { panic!("not a process handle") };
        end(&mut s, index, Cause::Exited, code);
        self.change(&mut s);
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

    /// `pid`'s `console_hold` calls, in order: what each asked, and whether a take was told a
    /// kernel line ended the holder's unfinished line (false for a give-back).
    pub fn console_holds(&self, pid: usize) -> Vec<(Hold, bool)> {
        self.lock()
            .holds
            .iter()
            .filter(|(p, _, _)| *p == pid)
            .map(|(_, hold, wrote)| (*hold, *wrote))
            .collect()
    }

    /// A kernel line ended the line `pid` gave the hold back on unfinished: its next take is told
    /// so, once (kernel/devices.md, "The console's one writer").
    pub fn console_line_ended(&self, pid: usize) { self.lock().console_ended.insert(pid); }

    /// Runs `body` as process `pid` on its own thread; the handle yields its exit code.
    pub fn run<F: FnOnce() -> u32 + Send + 'static>(
        &'static self,
        pid: usize,
        body: F,
    ) -> std::thread::JoinHandle<u32> {
        std::thread::spawn(move || {
            let _counted = Counted::new(self);
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
        let _counted = Counted::new(self);
        let old = CURRENT.with(|c| c.replace(Some(pid)));
        let result = body();
        CURRENT.with(|c| c.set(old));
        result
    }

    /// Lets a wait with no deadline last for ever, for a program run on this fake that waits on a
    /// person rather than a test: a shell whose user is thinking changes nothing for as long as
    /// they think. Tests never call it, so a test that stalls still fails after [`STUCK`].
    pub fn never_stuck(&self) { self.patient.store(true, Ordering::Relaxed); }

    /// Seeds the kernel's `random`, for a program run on this fake for a person, whose keys must
    /// not repeat from run to run. Tests never call it, so their randomness repeats, as a test's
    /// should.
    pub fn seed_random(&self, seed: u64) { self.lock().rng = seed | 1; }

    /// Microseconds since boot: the host's clock, or the test's once it holds it.
    fn now(&self) -> u64 {
        if self.held.load(Ordering::Acquire) {
            self.clock.load(Ordering::Acquire)
        } else {
            self.boot.elapsed().as_micros() as u64
        }
    }

    /// Takes the clock from the host: from here `time_now` stands still and only [`Fake::tick`]
    /// moves it, so a deadline, a server's own or a call's or receive's timeout, is a step the
    /// test takes once the state it waits on is reached, never a race against the host. A test
    /// that parks a call and expires it, or has a caller give up on one, holds the clock; one
    /// that holds it and ticks nothing is stuck ([`STUCK`]) as a `FOREVER` wait is. Held once for
    /// the binary, however many of its tests ask.
    pub fn hold_clock(&self) {
        let _s = self.lock();
        if !self.held.load(Ordering::Acquire) {
            self.clock.store(self.boot.elapsed().as_micros() as u64, Ordering::Release);
            self.held.store(true, Ordering::Release);
        }
    }

    /// This thread is one of the fake's no longer: its `run` or `as_process` is over, or it has
    /// exited (`thread_exit`, whose host thread parks for good and reaches no wait). Once.
    fn uncount(&self) {
        if COUNTED.with(|c| c.replace(false)) {
            self.lock().alive -= 1;
            self.quiet.notify_all();
        }
    }

    /// `dt` microseconds pass on the held clock, as the model's `tick`: every wait sees the time.
    /// A quiescent step: taken only once every other thread of the fake is in a wait, so a tick
    /// always ends the waits the test wrote it to end. A test observes a server's state (a call
    /// parked, a count reached) and not its next `receive`, whose deadline is read at the call;
    /// a tick taken between the two would leave that receive a deadline one timeout past the
    /// tick, which no later tick reaches, and its caller waiting for ever. Microseconds natively,
    /// the gap is wide under Miri. A thread a change has woken is not in a wait, whether or not
    /// the host has run it yet: it is counted again only when it waits again, so a tick never
    /// falls between a change and the reaction of the thread it woke (a server's reply handed over
    /// one tick late each time let a retry fall past a test's last tick, on a loaded host). That
    /// wait is bounded by construction: a woken thread either takes the lock back and waits
    /// again, or ends (its `run` returns, `thread_exit`, `process_exit`, a panic), and an end
    /// counts it out ([`Fake::uncount`]), so no tick waits for a thread that is not coming back.
    /// The rule for a test, then: across a tick, keep no thread of the fake busy outside a wait (no
    /// host sleep, channel or join on it). A thread that reaches no wait in [`STUCK`] fails the
    /// tick, naming the clock's time and the waits outstanding.
    pub fn tick(&self, dt: u64) {
        let mut s = self.lock();
        assert!(self.held.load(Ordering::Acquire), "a tick of a clock the test does not hold");
        let others = |s: &State| s.alive - usize::from(COUNTED.with(Cell::get));
        while s.waiting < others(&s) {
            let (g, waited) = self.quiet.wait_timeout(s, STUCK).unwrap_or_else(|e| e.into_inner());
            s = g;
            if waited.timed_out() && s.waiting < others(&s) {
                let (waiting, others, now) = (s.waiting, others(&s), self.now());
                drop(s);
                panic!(
                    "a tick of the held clock at {now} µs waited {STUCK:?} for the other threads of the fake to \
                     reach a wait and {waiting} of {others} did: the test is stuck"
                );
            }
        }
        self.clock.fetch_add(dt, Ordering::AcqRel);
        self.change(&mut s);
    }

    /// Something a wait may be waiting for changed: every waiter is woken, and none counts as
    /// waiting until it waits again ([`Fake::tick`]).
    fn change(&self, s: &mut State) {
        s.changes += 1;
        s.waiting = 0;
        self.changed.notify_all();
    }

    /// When a wait of `timeout` µs ends, on this kernel's clock; `None` for `FOREVER`.
    fn deadline(&self, timeout: u64) -> Option<u64> {
        (timeout != FOREVER).then(|| self.now().saturating_add(timeout))
    }

    /// Waits for a change or the deadline; false once the deadline has passed. A wait only a
    /// change can end, one with no deadline (`FOREVER`) or one on a held clock, that sees nothing
    /// change anywhere in this kernel for [`STUCK`] panics: the test is stuck (a server that
    /// ended early, a reply never sent, a tick never taken), and fails naming it rather than
    /// hanging the run, naming the clock's time and the waits outstanding. Load slows a test's
    /// progress; it does not stop every change. While it waits, the thread is one a tick of the
    /// held clock need not wait for ([`Fake::tick`]).
    fn wait<'a>(&self, guard: &mut Option<MutexGuard<'a, State>>, deadline: Option<u64>) -> bool {
        let mut g = guard.take().unwrap();
        let now = self.now();
        if deadline.is_some_and(|d| now >= d) {
            *guard = Some(g);
            return false;
        }
        let host =
            deadline.filter(|_| !self.held.load(Ordering::Acquire)).map(|d| Duration::from_micros(d - now));
        debug_assert!(
            COUNTED.with(Cell::get),
            "a wait in the fake kernel from a thread that is none of its own"
        );
        g.waiting += 1;
        let changes = g.changes;
        self.quiet.notify_all();
        let (mut g, stuck) = match host {
            Some(host) => (self.changed.wait_timeout(g, host).unwrap_or_else(|e| e.into_inner()).0, false),
            None if self.patient.load(Ordering::Relaxed) => {
                (self.changed.wait(g).unwrap_or_else(|e| e.into_inner()), false)
            }
            None => {
                let (g, waited) = self.changed.wait_timeout(g, STUCK).unwrap_or_else(|e| e.into_inner());
                (g, waited.timed_out())
            }
        };
        // A change discounted every waiter as it woke them; a wait that ended otherwise is still
        // counted.
        if g.changes == changes {
            g.waiting -= 1;
        }
        if stuck {
            let (waiting, others, now) = (g.waiting, g.alive.saturating_sub(1), self.now());
            let clock = if self.held.load(Ordering::Acquire) { "held" } else { "host's" };
            drop(g);
            panic!(
                "a wait only a change can end saw none in the fake kernel for {STUCK:?}: the test is stuck \
                 (the {clock} clock at {now} µs, {waiting} of the {others} other threads of the fake in a wait)"
            );
        }
        *guard = Some(g);
        true
    }
}

/// A new endpoint owned by the budget `owner` (none: the fake's root), whose receive right goes into
/// `pid`'s table stamped with that budget, as the kernel stamps it.
fn new_endpoint(s: &mut State, pid: usize, owner: Option<usize>) -> Handle {
    s.endpoints.push(EndpointState { owner, ..EndpointState::default() });
    let id = s.endpoints.len() - 1;
    install(s, pid, Object::Endpoint(Endpoint { id, badge: 0, stamp: owner }))
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
    s.freed.remove(&(pid, index));
    Handle::new(index as u32).unwrap()
}

/// The set `labels` names, as the kernel keeps one: sorted, without repeats.
fn set(labels: &[u64]) -> Vec<u64> {
    let mut set = labels.to_vec();
    set.sort_unstable();
    set.dedup();
    set
}

/// Whether budget `b` is `top` or below it in the tree.
fn at_or_below(s: &State, mut b: usize, top: usize) -> bool {
    loop {
        if b == top {
            return true;
        }
        match s.budgets[b].parent {
            Some(parent) => b = parent,
            None => return false,
        }
    }
}

fn lookup(s: &State, pid: usize, h: Handle) -> Result<Object, Error> {
    match s.processes[pid].handles.get(h.index() as usize).copied().flatten() {
        None => Err(Error::BadHandle),
        Some(Object::Dead) => Err(Error::Dead),
        Some(object) => Ok(object),
    }
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

/// The pages of a process's placement area ([`Area`]), for a test that fills it.
pub const AREA_PAGES: usize = Area::PAGES;

/// A process's placement area, which its `MapAnon` searches as the kernel searches its own
/// (kernel/memory.md, "Where `map_anon` puts pages"): first fit of the whole run from the start of
/// the run placed last, then from the area's start, mapping the run's first range at most, so
/// that `MapFixed` can extend the run at the pages after it, as the runtime does
/// (`redoubt_rt::handle::map_anon`). One anonymous host mapping, made at the process's first
/// `MapAnon` and never unmapped (its address is kept in `State::anon`); a page is zeroed as it is
/// mapped. The processes share the host's address space, so a page of one's area is free to every
/// process's `MapFixed` until it is taken.
///
/// Not under Miri, which checks each access to an allocation against every pointer and thread
/// that touched it: one area shared by a test's every run made `rt-miri`'s
/// `heap_over_map_anon` run past five minutes, against under a minute with an allocation a run.
/// There each `MapAnon` run is an allocation of its own, as it was before areas, of the range it
/// maps: `rt-miri` maps no run longer than a range, which `MapFixed` would have to extend, and
/// splits no range call a range at a time, since only a whole run is unmapped or moved there.
#[derive(Default)]
struct Area {
    base: usize,
    /// Whether each page is mapped.
    taken: Vec<bool>,
    /// The first page of the run placed last.
    rover: usize,
}

impl Area {
    /// Pages in an area: enough for every host test's live pages.
    const PAGES: usize = 16384;

    fn base(&mut self, anon: &mut HashMap<usize, AtomicPtr<u8>>) -> usize {
        if self.base == 0 {
            // An anonymous mapping, whose pages the host zeroes as they are first touched:
            // `alloc_zeroed` at a page's alignment writes the whole area at once, 64 MiB a
            // process, which slowed the timed tests beside it.
            // SAFETY (host test code only): a fresh private mapping at an address the host
            // chooses, replacing nothing; never unmapped.
            let bytes = unsafe {
                libc::mmap(
                    std::ptr::null_mut(),
                    Self::PAGES * PAGE_SIZE,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                    -1,
                    0,
                )
            };
            assert!(bytes != libc::MAP_FAILED, "the fake's placement area");
            let bytes = bytes as *mut u8;
            anon.insert(bytes as usize, AtomicPtr::new(bytes));
            self.base = bytes as usize;
            self.taken = vec![false; Self::PAGES];
        }
        self.base
    }

    /// The first page index of `addr..addr + len` if it lies wholly in the area, `None` if wholly
    /// outside it; partly in is `InvalidArgument`.
    fn pages(&self, addr: usize, len: usize) -> Result<Option<usize>, Error> {
        let end = self.base + Self::PAGES * PAGE_SIZE;
        if self.base == 0 || addr + len <= self.base || addr >= end {
            Ok(None)
        } else if addr >= self.base && addr + len <= end {
            Ok(Some((addr - self.base) / PAGE_SIZE))
        } else {
            Err(Error::InvalidArgument)
        }
    }

    /// Marks `n` pages from `first` mapped and zeroes them, if every one is free.
    fn take(&mut self, first: usize, n: usize) -> bool {
        if self.taken[first..first + n].iter().any(|t| *t) {
            return false;
        }
        self.taken[first..first + n].fill(true);
        // SAFETY: the pages lie inside the area's allocation, and no process maps them (checked
        // just above), so nothing holds a reference into them.
        unsafe { std::ptr::write_bytes((self.base + first * PAGE_SIZE) as *mut u8, 0, n * PAGE_SIZE) };
        true
    }

    /// The first free run of `run` pages, first fit from the rover, then from the start, of which
    /// the first `n` are taken; its address.
    fn place(&mut self, run: usize, n: usize) -> Option<usize> {
        let fits = |a: &Area, at: usize| at + run <= Self::PAGES && a.taken[at..at + run].iter().all(|t| !*t);
        let at = (self.rover..Self::PAGES).chain(0..self.rover).find(|&at| fits(self, at))?;
        self.take(at, n);
        self.rover = at;
        Some(self.base + at * PAGE_SIZE)
    }
}

/// The area `addr..addr + len` lies in, and its first page there: `None` outside every area,
/// `InvalidArgument` partly in one.
fn area_of(s: &mut State, addr: usize, len: usize) -> Result<Option<(&mut Area, usize)>, Error> {
    for p in &mut s.processes {
        if let Some(first) = p.area.pages(addr, len)? {
            return Ok(Some((&mut p.area, first)));
        }
    }
    Ok(None)
}

/// Frees pages this kernel mapped, which their process no longer holds: pages of an area go back
/// to it (a lend or transfer of a `MapAnon` run is still its area's); under Miri, a `MapAnon` run's
/// own allocation is freed when `whole`, the run given up as it was mapped ([`Area`]); a part of
/// one is not, and no test there gives up a part.
fn free(s: &mut State, addr: usize, len: usize, whole: bool) {
    if let Ok(Some((area, first))) = area_of(s, addr, len) {
        area.taken[first..first + len / PAGE_SIZE].fill(false);
    } else if cfg!(miri) && whole && s.anon.remove(&addr).is_some() {
        // SAFETY: under Miri `MapAnon` made `addr` with `alloc_zeroed` and exactly this layout, and
        // its caller has just taken it out of its process's mappings, so it is freed once.
        unsafe { dealloc(addr as *mut u8, Layout::from_size_align(len, PAGE_SIZE).unwrap()) };
    }
}

/// How long a wait only a change can end may see nothing change before the test is called stuck:
/// far beyond any step of any test here, however loaded the machine.
const STUCK: Duration = Duration::from_secs(60);

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

/// An error before anything is done, as `call` reports it: a `call`'s outcome with its lend
/// returned and no reply, a turn's with nothing done, or the error.
fn refused(call: &Call, error: Error) -> Result<Return, Error> {
    match *call {
        Call::Call { lend, .. } => Ok(Return::Call(CallOutcome {
            status: Err(error),
            lend: if lend.is_some() { LendDisposition::Returned } else { LendDisposition::None },
            reply_present: false,
        })),
        Call::ReplyReceive { .. } | Call::SendReceive { .. } => {
            Ok(Return::Turn(TurnOutcome { status: Err(error), first: FirstDone::Nothing }))
        }
        _ => Err(error),
    }
}

/// A turn's records: its receive record not given up, and its first half's, read and decoded.
fn read_first(s: &State, pid: usize, rec: usize, first_rec: usize, send: bool) -> Result<FirstHalf, Error> {
    not_given_up(s, pid, rec, RECEIVED_SLOTS * 8)?;
    not_given_up(s, pid, first_rec, FIRST_SLOTS * 8)?;
    // SAFETY: the runtime passes the address of a live, 8-aligned `[u64; FIRST_SLOTS]` record it
    // owns for the duration of the call (redoubt-rt `sys::Record`), in this address space.
    let slots = unsafe { (first_rec as *const [u64; FIRST_SLOTS]).read() };
    FirstHalf::decode(send, &slots)
}

/// A turn's receive handle, checked before anything is done: an IRQ, a badge-0 endpoint, or
/// none.
fn receive_source(s: &State, pid: usize, from: Option<Handle>) -> Result<(), Error> {
    let Some(from) = from else { return Ok(()) };
    match lookup(s, pid, from)? {
        Object::Irq(_) => Ok(()),
        Object::Endpoint(ep) if ep.badge != 0 => Err(Error::NotPermitted),
        Object::Endpoint(_) => Ok(()),
        _ => Err(Error::WrongObject),
    }
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
                    let error = s.refusals.remove(i).3;
                    return refused(call, error);
                }
                s.refusals[i].2 -= 1;
            }
            // A killed process runs nothing on the machine; here its host thread runs on, and
            // every call but an exit fails `Dead`.
            if s.processes[pid].killed && !matches!(call, Call::ThreadExit | Call::ProcessExit { .. }) {
                return refused(call, Error::Dead);
            }
        }
        match *call {
            Call::MapAnon { len, .. } => {
                if len == 0 || len % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgument);
                }
                // A run's first range at most is mapped (kernel/memory.md, R22).
                let (run, len) = (len, len.min(MAX_RANGE_PAGES * PAGE_SIZE));
                let mut s = self.lock();
                if cfg!(miri) {
                    let layout = Layout::from_size_align(len, PAGE_SIZE).map_err(|_| Error::OutOfMemory)?;
                    // SAFETY: the layout has a non-zero size.
                    let pages = unsafe { alloc_zeroed(layout) };
                    if pages.is_null() {
                        return Err(Error::OutOfMemory);
                    }
                    s.anon.insert(pages as usize, AtomicPtr::new(pages));
                    s.processes[pid].map(pages as usize, len);
                    return Ok(Return::Addr(pages as usize));
                }
                let State { processes, anon, .. } = &mut *s;
                let area = &mut processes[pid].area;
                area.base(anon);
                let addr = area.place(run / PAGE_SIZE, len / PAGE_SIZE).ok_or(Error::OutOfMemory)?;
                s.processes[pid].map(addr, len);
                Ok(Return::Addr(addr))
            }
            Call::MapFixed { addr, len, .. } => {
                // The kernel's range checks (kernel/memory.md, `map_fixed`); the overlap check is
                // the host's: the fake processes share this address space, and `mmap` with
                // `MAP_FIXED_NOREPLACE` refuses a range any of it has taken, as the kernel
                // refuses one touching a taken page, mapping nothing.
                if addr == 0 || len == 0 || addr % PAGE_SIZE != 0 || len % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgument);
                }
                let Some(end) = addr.checked_add(len) else { return Err(Error::InvalidArgument) };
                range_cap(len)?;
                let mut s = self.lock();
                // In a placement area, which lies wherever the host put it, as `MapAnon`'s pages
                // do: free pages there, as the kernel maps a free range.
                if let Some((area, first)) = area_of(&mut s, addr, len)? {
                    if !area.take(first, len / PAGE_SIZE) {
                        return Err(Error::InvalidArgument);
                    }
                    s.processes[pid].map(addr, len);
                    return Ok(Return::Nothing);
                }
                if end > redoubt_rt::abi::USER_AREA_END {
                    return Err(Error::InvalidArgument);
                }
                // SAFETY (host test code only): an anonymous private mapping of fresh pages at
                // `addr`, replacing nothing (`MAP_FIXED_NOREPLACE`), so no memory of this process
                // is remapped under a reference; the pages are given back by `Unmap` below, once.
                let got = unsafe {
                    libc::mmap(
                        addr as *mut libc::c_void,
                        len,
                        libc::PROT_READ | libc::PROT_WRITE,
                        libc::MAP_PRIVATE | libc::MAP_ANONYMOUS | libc::MAP_FIXED_NOREPLACE,
                        -1,
                        0,
                    )
                };
                if got == libc::MAP_FAILED {
                    return Err(Error::InvalidArgument);
                }
                if got as usize != addr {
                    // An old kernel without `MAP_FIXED_NOREPLACE` took it as a hint: not placed.
                    // SAFETY (host test code only): `got` is the mapping `mmap` just returned.
                    unsafe { libc::munmap(got, len) };
                    return Err(Error::InvalidArgument);
                }
                s.processes[pid].map(addr, len);
                s.fixed.insert(addr);
                Ok(Return::Nothing)
            }
            Call::Unmap { addr, len } => {
                if len == 0 || addr % PAGE_SIZE != 0 || len % PAGE_SIZE != 0 {
                    return Err(Error::InvalidArgument);
                }
                range_cap(len)?;
                let mut s = self.lock();
                if !s.processes[pid].holds(addr, len) {
                    return Err(Error::InvalidArgument);
                }
                not_given_up(&s, pid, addr, len)?;
                let (at, whole) = s.processes[pid].carve(addr, len).expect("held, checked just above");
                if s.fixed.remove(&at) {
                    // What is left of the host mapping on either side is still `MapFixed`'s.
                    s.fixed.extend((addr > at).then_some(at));
                    s.fixed.extend((addr + len < at + whole).then_some(addr + len));
                    // SAFETY (host test code only): `MapFixed` above mapped `at..at + whole`, of
                    // which its process has just given up `addr..addr + len`, so those pages are
                    // unmapped once.
                    unsafe { libc::munmap(addr as *mut libc::c_void, len) };
                } else {
                    free(&mut s, addr, len, (at, whole) == (addr, len));
                }
                Ok(Return::Nothing)
            }
            Call::EndpointCreate { budget } => {
                let mut s = self.lock();
                let caller = s.processes[pid].runs_in;
                // A budget named owns it: the caller's own or one below it (none, the fake's
                // root, is above every budget), with no label the caller lacks (kernel/objects.md):
                // the kernel exempts a system-class caller from that, but every fake process is
                // user-class, as for `budget_create`.
                let owner = match budget {
                    None => caller,
                    Some(b) => {
                        let Object::Budget(index) = lookup(&s, pid, b)? else {
                            return Err(Error::WrongObject);
                        };
                        if caller.is_some_and(|top| !at_or_below(&s, index, top)) {
                            return Err(Error::NotPermitted);
                        }
                        let mine = s.processes[pid].labels;
                        if !s.budgets[index].labels.as_slice().iter().all(|l| mine.as_slice().contains(l)) {
                            return Err(Error::LabelDenied);
                        }
                        Some(index)
                    }
                };
                Ok(Return::Handle(new_endpoint(&mut s, pid, owner)))
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
                    let _counted = Counted::new(fake());
                    CURRENT.with(|c| c.set(Some(pid)));
                    // The body never returns; `process_exit` unwinds, which ends this thread.
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| entry(arg)));
                });
                Ok(Return::Tid(1))
            }
            Call::Mint { source, badge, budget } => {
                let mut s = self.lock();
                // The endpoint and the default stamp: the receive right's, or the stamp of the
                // handle the call came through (kernel/objects.md, `mint`). A handle with no
                // stamp (made by a process a test gave no budget) is stamped with the process's
                // budget, if a test gave it one since.
                let (ep, default) = match source {
                    MintSource::Handle(h) => {
                        let ep = as_endpoint(&s, pid, h)?;
                        if ep.badge != 0 {
                            return Err(Error::NotPermitted);
                        }
                        (ep.id, ep.stamp)
                    }
                    MintSource::Message(id) => match s.open.get(&id.get()) {
                        Some(&(receiver, ep, stamp)) if receiver == pid => (ep, stamp),
                        _ => return Err(Error::InvalidArgument),
                    },
                };
                let default = default.or(s.processes[pid].runs_in);
                // A budget named stamps the handle, so it goes when that budget is destroyed,
                // and it only narrows: at or below the default stamp, or `NotPermitted`
                // (objects.md, mint). A default of none is the fake's root, above every budget.
                let stamp = match budget {
                    None => default,
                    Some(b) => match lookup(&s, pid, b)? {
                        Object::Budget(index) => {
                            if default.is_some_and(|top| !at_or_below(&s, index, top)) {
                                return Err(Error::NotPermitted);
                            }
                            Some(index)
                        }
                        _ => return Err(Error::WrongObject),
                    },
                };
                Ok(Return::Handle(install(
                    &mut s,
                    pid,
                    Object::Endpoint(Endpoint { id: ep, badge: badge.get(), stamp }),
                )))
            }
            Call::HandleClose { handle } => {
                let mut s = self.lock();
                // A dead entry closes like a live handle (I17).
                match lookup(&s, pid, handle) {
                    Ok(_) | Err(Error::Dead) => {}
                    Err(e) => return Err(e),
                }
                s.processes[pid].handles[handle.index() as usize] = None;
                // A test may wait for this close (`wait_held`).
                self.change(&mut s);
                Ok(Return::Nothing)
            }
            Call::Call { endpoint, body_rec, lend, timeout } => {
                match self.message(pid, endpoint, body_rec, lend, timeout, true) {
                    Ok(result) => Ok(result),
                    Err(error) => refused(call, error),
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
                self.reply(&mut s, pid, msg_id, &body).map(Return::Reply)
            }
            Call::ReplyReceive { from, timeout, received_rec, first_rec, .. } => {
                Ok(Return::Turn(self.reply_receive(pid, from, timeout, received_rec, first_rec)))
            }
            Call::SendReceive { from, timeout, received_rec, first_rec, .. } => {
                Ok(Return::Turn(self.send_receive(pid, from, timeout, received_rec, first_rec)))
            }
            Call::TimeNow => Ok(Return::Time(self.now())),
            Call::Random => {
                let mut s = self.lock();
                s.rng ^= s.rng << 13;
                s.rng ^= s.rng >> 7;
                s.rng ^= s.rng << 17;
                Ok(Return::Random(s.rng))
            }
            // The thread ends: it never runs again. Its host thread waits for good, since nothing
            // may unwind through the runtime's `extern "C"` trampoline; the test's end ends it.
            Call::ThreadExit => {
                self.uncount();
                loop {
                    std::thread::park();
                }
            }
            Call::ProcessExit { code } => {
                self.lock().exits.insert(pid, code);
                std::panic::resume_unwind(Box::new(Exited(code)))
            }
            Call::Serve { msg_id } => {
                let mut s = self.lock();
                match s.open.get(&msg_id.get()) {
                    Some(&(receiver, ..)) if receiver == pid => {
                        s.log.push((pid, "serve", msg_id.get()));
                        Ok(Return::Nothing)
                    }
                    _ => Err(Error::InvalidArgument),
                }
            }
            Call::ProcessCreate { budget, exit_endpoint } => {
                let mut s = self.lock();
                let Object::Budget(budget) = lookup(&s, pid, budget)? else { return Err(Error::WrongObject) };
                if s.budgets[budget].destroyed {
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
                range_cap(len)?;
                let mut s = self.lock();
                let Object::Process(index) = lookup(&s, pid, process)? else {
                    return Err(Error::WrongObject);
                };
                if s.launched[index].start.is_some() {
                    return Err(Error::NotPermitted);
                }
                if len == 0
                    || src % PAGE_SIZE != 0
                    || len % PAGE_SIZE != 0
                    || !s.processes[pid].holds(src, len)
                {
                    return Err(Error::InvalidArgument);
                }
                not_given_up(&s, pid, src, len)?;
                let (at, whole) = s.processes[pid].carve(src, len).expect("held, checked just above");
                // SAFETY: `src..src + len` lies in a mapping this kernel made for `pid` (checked
                // just above) and has just left its table, so nothing else reads or frees it.
                let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) }.to_vec();
                free(&mut s, src, len, (at, whole) == (src, len));
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
            Call::BudgetCreate { parent, spec_rec } => {
                let mut s = self.lock();
                not_given_up(&s, pid, spec_rec, BUDGET_SPEC_SLOTS * 8)?;
                // SAFETY: the runtime passes the address of a live, 8-aligned
                // `[u64; BUDGET_SPEC_SLOTS]` record it owns for the call (`Budget::create_child`).
                let slots = unsafe { (spec_rec as *const [u64; BUDGET_SPEC_SLOTS]).read() };
                let spec = BudgetSpec::decode(&slots)?;
                let Object::Budget(index) = lookup(&s, pid, parent)? else { return Err(Error::WrongObject) };
                let parent = s.budgets[index];
                if parent.destroyed {
                    return Err(Error::BadHandle);
                }
                // The kernel's label rule (kernel/budgets.md, "Labels on budgets"), on the set the
                // spec names: every parent label kept, and none added, since every fake process is
                // user-class. Nothing is charged or carved.
                let labels = set(spec.labels.as_slice());
                if !parent.labels.as_slice().iter().all(|l| labels.contains(l)) {
                    return Err(Error::LabelDenied);
                }
                if labels != parent.labels.as_slice() {
                    return Err(Error::ClassDenied);
                }
                s.budgets.push(Budget { labels: parent.labels, parent: Some(index), destroyed: false });
                let child = s.budgets.len() - 1;
                Ok(Return::Handle(install(&mut s, pid, Object::Budget(child))))
            }
            Call::BudgetDestroy { budget } => {
                let mut s = self.lock();
                let Object::Budget(index) = lookup(&s, pid, budget)? else { return Err(Error::WrongObject) };
                // It and every budget below it.
                let below = |s: &State, mut b: usize| loop {
                    if b == index {
                        return true;
                    }
                    match s.budgets[b].parent {
                        Some(parent) => b = parent,
                        None => return false,
                    }
                };
                let gone: Vec<usize> = (0..s.budgets.len()).filter(|&b| below(&s, b)).collect();
                for &b in &gone {
                    s.budgets[b].destroyed = true;
                }
                let children: Vec<usize> =
                    (0..s.launched.len()).filter(|&i| gone.contains(&s.launched[i].budget)).collect();
                for child in children {
                    end(&mut s, child, Cause::Killed, 0);
                }
                // The processes running there are killed (a test plays them; their host threads
                // run on, every call failing `Dead`), and the endpoints those budgets own die.
                for p in s.processes.iter_mut() {
                    if p.runs_in.is_some_and(|b| gone.contains(&b)) {
                        p.killed = true;
                    }
                }
                let mut dying = HashSet::new();
                for (id, e) in s.endpoints.iter_mut().enumerate() {
                    if e.owner.is_some_and(|b| gone.contains(&b)) {
                        e.dead = true;
                        dying.insert(id);
                    }
                }
                // As the kernel does (kernel/budgets.md R10, objects.md R9, invariants.md I17):
                // every handle to a budget destroyed, to an endpoint it owned, and every handle
                // one stamped, becomes a dead entry in every table, its index held until its
                // holder closes it.
                let State { processes, freed, .. } = &mut *s;
                for (p, process) in processes.iter_mut().enumerate() {
                    for (i, slot) in process.handles.iter_mut().enumerate() {
                        match *slot {
                            Some(Object::Budget(b)) if gone.contains(&b) => {
                                freed.insert((p, i), b);
                                *slot = Some(Object::Dead);
                            }
                            Some(Object::Endpoint(Endpoint { stamp: Some(b), .. })) if gone.contains(&b) => {
                                *slot = Some(Object::Dead);
                            }
                            Some(Object::Endpoint(ep)) if dying.contains(&ep.id) => {
                                *slot = Some(Object::Dead)
                            }
                            _ => {}
                        }
                    }
                }
                self.change(&mut s);
                Ok(Return::Nothing)
            }
            // The fake prints no kernel lines, so a hold has nothing to make wait: the handle is
            // checked as the kernel checks it, a device's registers, and nothing else happens. A
            // take is told a kernel line ended the holder's unfinished line only when a test said
            // so ([`Fake::console_line_ended`]), and once.
            Call::ConsoleHold { device, hold } => {
                let mut s = self.lock();
                let Object::Mmio(_) = lookup(&s, pid, device)? else {
                    return Err(Error::WrongObject);
                };
                let wrote = hold == Hold::Take && s.console_ended.remove(&pid);
                s.holds.push((pid, hold, wrote));
                Ok(Return::Wrote(wrote))
            }
            other => panic!("the fake kernel does not model {:?}", other.number().name()),
        }
    }
}

impl Fake {
    /// The reply to `msg_id`, an open call of `pid`'s: its handles checked, then made.
    fn reply(
        &self,
        s: &mut MutexGuard<'_, State>,
        pid: usize,
        msg_id: NonZeroU64,
        body: &Body,
    ) -> Result<ReplyOutcome, Error> {
        match s.open.get(&msg_id.get()) {
            Some(&(receiver, ..)) if receiver == pid => {}
            _ => return Err(Error::InvalidArgument),
        }
        let handles =
            body.handles.as_slice().iter().map(|h| lookup(s, pid, *h)).collect::<Result<Vec<_>, _>>()?;
        s.open.remove(&msg_id.get());
        // The lend is the caller's again (or, abandoned, the server's to free).
        s.given_up.remove(&msg_id.get());
        s.log.push((pid, "reply", msg_id.get()));
        // An abandoned call's reply is discarded, and frees its lend (R3).
        let abandoned = s.abandoned.remove(&msg_id.get());
        if let Some(Some((addr, len))) = abandoned {
            s.processes[pid].mappings.remove(&addr);
            free(s, addr, len, true);
        }
        let delivered = abandoned.is_none();
        let installed = if delivered { (1 << handles.len()) - 1 } else { 0 };
        if delivered {
            s.replies.insert(msg_id.get(), (body.words, handles));
        }
        self.change(s);
        Ok(ReplyOutcome { delivered, installed })
    }

    /// `reply_receive`: the receive's handle and the reply's checks first, nothing done if one
    /// fails (the fake checks the receive's handle whole first, not stage by stage); then the
    /// reply, then the receive.
    fn reply_receive(
        &self,
        pid: usize,
        from: Option<Handle>,
        timeout: u64,
        rec: usize,
        first_rec: usize,
    ) -> TurnOutcome {
        let nothing = |error| TurnOutcome { status: Err(error), first: FirstDone::Nothing };
        let replied = {
            let mut s = self.lock();
            let first = read_first(&s, pid, rec, first_rec, false).and_then(|first| {
                receive_source(&s, pid, from)?;
                Ok(first)
            });
            let reply = match first {
                Ok(FirstHalf::Reply { msg_id, body }) => self.reply(&mut s, pid, msg_id, &body),
                Ok(FirstHalf::Send { .. }) => unreachable!("a reply's record decodes as a reply"),
                Err(error) => Err(error),
            };
            match reply {
                Ok(outcome) => outcome,
                Err(error) => return nothing(error),
            }
        };
        let status = self.receive(pid, from, timeout, rec).map(|_| ());
        TurnOutcome { status, first: FirstDone::Replied(replied) }
    }

    /// `send_receive`: the receive's handle and the send's checks first, nothing done if one
    /// fails; then the send, which waits for its taker, then the receive.
    fn send_receive(
        &self,
        pid: usize,
        from: Option<Handle>,
        timeout: u64,
        rec: usize,
        first_rec: usize,
    ) -> TurnOutcome {
        let nothing = |error| TurnOutcome { status: Err(error), first: FirstDone::Nothing };
        let s = self.lock();
        let first = read_first(&s, pid, rec, first_rec, true).and_then(|first| {
            receive_source(&s, pid, from)?;
            Ok(first)
        });
        let FirstHalf::Send { endpoint, timeout: send_timeout, transfer, body } = (match first {
            Ok(first) => first,
            Err(error) => return nothing(error),
        }) else {
            unreachable!("a send's record decodes as a send")
        };
        if let Err(error) = self.queue(s, pid, endpoint, body, transfer, send_timeout, None) {
            return nothing(error);
        }
        let status = self.receive(pid, from, timeout, rec).map(|_| ());
        TurnOutcome { status, first: FirstDone::Sent }
    }

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
        let s = self.lock();
        not_given_up(&s, pid, body_rec, BODY_SLOTS * 8)?;
        let body = read_body(body_rec)?;
        self.queue(s, pid, endpoint, body, pages, timeout, is_call.then_some(body_rec))
    }

    /// Queue a message, a call if it has a record for the reply (`reply_rec`), and wait for the
    /// reply or the taking.
    #[allow(clippy::too_many_arguments)]
    fn queue(
        &self,
        mut s: MutexGuard<'_, State>,
        pid: usize,
        endpoint: Handle,
        body: Body,
        pages: Option<Pages>,
        timeout: u64,
        reply_rec: Option<usize>,
    ) -> Result<Return, Error> {
        let is_call = reply_rec.is_some();
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
        let pending = Pending {
            id,
            call: is_call,
            badge: ep.badge,
            stamp: ep.stamp,
            account,
            labels,
            words,
            handles,
            pages,
            from: pid,
        };
        s.endpoints[ep.id].queue.push_back(pending);
        // From here until the message settles, the pages are given up (docs/kernel/ipc.md,
        // Messages).
        if let Some(p) = pages {
            s.given_up.insert(id.get(), (pid, p.addr, p.npages.get() * PAGE_SIZE));
        }
        self.change(&mut s);
        let deadline = self.deadline(timeout);
        let mut guard = Some(s);
        let result = loop {
            let s = guard.as_mut().unwrap();
            if is_call {
                if let Some((words, handles)) = s.replies.remove(&id.get()) {
                    let handles: Vec<Handle> = handles.into_iter().map(|e| install(s, pid, e)).collect();
                    let body = Body { words, handles: Handles::from_slice(&handles).unwrap() };
                    write_body(reply_rec.expect("a call has its record"), &body);
                    break Ok(Return::Call(CallOutcome {
                        status: Ok(()),
                        lend: if pages.is_some() { LendDisposition::Returned } else { LendDisposition::None },
                        reply_present: true,
                    }));
                }
            } else if s.taken.remove(&id.get()) {
                break Ok(Return::Nothing);
            }
            // Its server was killed holding it: `Dead`, the lend back intact (R4b).
            if s.open.get(&id.get()).is_some_and(|&(receiver, ..)| s.processes[receiver].killed) {
                s.open.remove(&id.get());
                break Err(Error::Dead);
            }
            let queued = s.endpoints[ep.id].queue.iter().position(|p| p.id == id);
            if s.endpoints[ep.id].dead {
                if let Some(i) = queued {
                    s.endpoints[ep.id].queue.remove(i);
                    break Err(Error::Dead);
                }
                // A call already taken is abandoned by the endpoint's destruction, and no notice
                // follows: there is no endpoint left to receive one on (R3).
                if let Some(&(receiver, ..)) = s.open.get(&id.get()) {
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
                if let Some(&(receiver, endpoint, _)) = s.open.get(&id.get()) {
                    s.notices.push_back((receiver, endpoint, id.get()));
                    self.change(s);
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
        let deadline = self.deadline(timeout);
        let mut guard = Some(self.lock());
        not_given_up(guard.as_ref().unwrap(), pid, rec, RECEIVED_SLOTS * 8)?;
        let Some(from) = from else {
            while self.wait(&mut guard, deadline) {}
            return Err(Error::Timeout);
        };
        // An IRQ handle: unmask the source (R5), then wait for it to fire.
        if let Object::Irq(index) = lookup(guard.as_ref().unwrap(), pid, from)? {
            guard.as_mut().unwrap().devices[index].masked = false;
            self.change(guard.as_mut().unwrap());
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
            if s.endpoints[ep.id].dead || s.processes[pid].killed {
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
                    s.open.insert(p.id.get(), (pid, ep.id, p.stamp));
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
                self.change(s);
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
