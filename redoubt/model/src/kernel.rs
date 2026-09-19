//! The kernel model: KERNEL-SPEC.md's five objects, its system calls and rules R1-R12.
//!
//! Read it next to the spec. Each system call is one method with the spec's name, and each check
//! in it cites the rule or table row it implements. Checks run in one fixed order, which the
//! trace format relies on (README.md, "Order of checks"):
//! 1. **decoding**, argument by argument, as `redoubt-sys` decodes registers and buffers: a handle
//!    value that cannot be an index is `BadHandle`, a list longer than its array is `TooLarge`,
//!    any other malformed encoding (unknown flag bits or tags, a 32-bit field over 32 bits) is
//!    `InvalidArgument`;
//! 2. then the kernel's checks, argument by argument from left to right (does the handle exist,
//!    is it the right object, is the range valid);
//! 3. then permission checks (`NotPermitted`, `ClassDenied`, `LabelDenied`);
//! 4. then resources (`OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`).
//!
//! Abstractions (what the model does not represent, on purpose): physical memory is a set of
//! page frames with one abstract word of content each; page-table pages are not charged
//! separately (they would depend on the address layout; see README.md); a thread has no
//! registers; time is logical and advances only in [`Op::Tick`]. System calls are instantaneous
//! and made by whichever runnable thread the trace names; the scheduler decides only how CPU
//! time is shared during ticks (R12).
//!
//! `mutation` switches on one deliberate rule break (mutation.rs); `self.broken(..)` marks each
//! place. With `mutation == None` this is the specified kernel.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::vec;
use alloc::vec::Vec;

use crate::ghost::{Flow, Ghost};
use crate::mutation::Mutation;
use crate::sched::Scheduler;
use crate::spec::*;
use crate::syscall::*;

/// Pages each kind of kernel object costs its budget (R6). KERNEL-SPEC.md says every object is
/// charged in pages but not how many; these are model parameters, and a conformance run must use
/// the real kernel's values (README.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Costs {
    /// A budget's own object.
    pub budget: u64,
    /// A process object (with its root page table), not counting threads or its handle table.
    pub process: u64,
    /// A thread context.
    pub thread: u64,
    /// An endpoint.
    pub endpoint: u64,
    /// A handle table costs one page per this many live handles (rounded up).
    pub handles_per_page: u64,
}

impl Default for Costs {
    fn default() -> Costs {
        Costs { budget: 1, process: 1, thread: 1, endpoint: 1, handles_per_page: 8 }
    }
}

/// A device object the loader creates from the device tree (KERNEL-SPEC.md, Device).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceSpec {
    Mmio { base: u64, pages: u64, dma: bool },
    Irq { n: u64 },
    Reset,
}

/// A budget's three carved limits (R7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub pages: u64,
    pub processes: u64,
    pub weight: u64,
}

/// What the kernel is booted with: the argument block's budget sizes and the device objects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Boot {
    /// `root`: all of RAM, every PID, all the weight.
    pub root: Limits,
    /// `system`'s reserved share, carved from `root`.
    pub system: Limits,
    /// `users`, carved from `root`. What `root` keeps is `init`'s own (README choice 20).
    pub users: Limits,
    /// In `init`'s handle order after the three budgets.
    pub devices: Vec<DeviceSpec>,
    pub costs: Costs,
}

impl Default for Boot {
    fn default() -> Boot {
        Boot {
            root: Limits { pages: 1024, processes: 24, weight: 1000 },
            system: Limits { pages: 256, processes: 8, weight: 250 },
            users: Limits { pages: 512, processes: 12, weight: 500 },
            devices: vec![
                DeviceSpec::Mmio { base: 0x1000_0000, pages: 1, dma: false },
                DeviceSpec::Mmio { base: 0x1000_1000, pages: 1, dma: true },
                DeviceSpec::Irq { n: 10 },
                DeviceSpec::Irq { n: 11 },
                DeviceSpec::Reset,
            ],
            costs: Costs::default(),
        }
    }
}

/// Refuse a `Boot` the kernel could not start: `system` and `users` must fit in `root` with room
/// for `init` (its process, thread and handle table), each must hold its own budget object, and
/// devices must be well formed.
fn check_boot(b: &Boot) -> Result<(), alloc::string::String> {
    use alloc::format;
    let c = b.costs;
    if c.handles_per_page == 0 {
        return Err("boot: handles_per_page must be at least 1".into());
    }
    if b.devices.len() > 64 {
        return Err("boot: at most 64 devices".into());
    }
    let mut irqs = BTreeSet::new();
    for d in &b.devices {
        match *d {
            DeviceSpec::Mmio { base, pages, .. } => {
                if pages == 0 || pages > 1 << 20 || base % PAGE_SIZE != 0 {
                    return Err(format!("boot: bad MMIO device at {base:#x}"));
                }
            }
            DeviceSpec::Irq { n } => {
                if !irqs.insert(n) {
                    return Err(format!("boot: IRQ {n} listed twice"));
                }
            }
            DeviceSpec::Reset => {}
        }
    }
    let table = (3 + b.devices.len() as u64).div_ceil(c.handles_per_page);
    let init = c.process.checked_add(c.thread).and_then(|x| x.checked_add(table));
    let pages = [c.budget, b.system.pages, b.users.pages, init.unwrap_or(u64::MAX)]
        .iter()
        .try_fold(0u64, |acc, x| acc.checked_add(*x));
    if pages.is_none_or(|p| p > b.root.pages) {
        return Err("boot: system, users and init do not fit in root's pages".into());
    }
    if b.system.pages < c.budget || b.users.pages < c.budget {
        return Err("boot: system and users must each hold their own budget object".into());
    }
    let procs = b.system.processes.checked_add(b.users.processes).and_then(|x| x.checked_add(1));
    if procs.is_none_or(|p| p > b.root.processes) {
        return Err("boot: system, users and init do not fit in root's processes".into());
    }
    let weight = b.system.weight.checked_add(b.users.weight);
    if weight.is_none_or(|w| w > b.root.weight) || b.root.weight > U32_MAX {
        return Err("boot: system and users weigh more than root".into());
    }
    Ok(())
}

/// Where user virtual addresses end (Sv39's lower half). `process_map`'s `dst` must lie below.
pub const USER_TOP: u64 = 1 << 38;
/// The lowest mappable user address (page 0 is never mapped).
pub const USER_BASE: u64 = PAGE_SIZE;
/// Where the kernel places the mappings it chooses addresses for (`map_anon`, received buffers).
pub const KERNEL_CHOSEN_BASE: u64 = 0x10_0000_0000;
/// Physical address of frame 0; `dma_alloc` returns physical addresses from here.
pub const RAM_BASE: u64 = 0x8000_0000;

/// The pid of `init`, the one process the kernel creates.
pub const INIT_PID: u64 = 1;
/// The ids of the three budgets the kernel creates at boot, in creation order.
pub const ROOT: u64 = 1;
pub const SYSTEM: u64 = 2;
pub const USERS: u64 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Object {
    Budget(u64),
    Process(u64),
    Endpoint(u64),
    Device(u64),
}

/// Ghost: how a handle came to be, so that I3, I4 and R9 can be checked independently of the code
/// that stamps handles. Copies (messages, `process_start`) keep it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Given to `init` by the kernel.
    Boot,
    /// Returned by `endpoint_create`, `budget_create` or `process_create` to a caller in `by`.
    Created { by: u64 },
    /// Returned by `mint`, whose default stamp was `default_stamp`.
    Minted { default_stamp: u64 },
}

/// KERNEL-SPEC.md, Handle = (object, badge, stamp). `origin` is ghost state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle {
    pub object: Object,
    pub badge: u64,
    pub stamp: u64,
    pub origin: Origin,
}

#[derive(Clone, Debug)]
pub struct Budget {
    pub id: u64,
    pub parent: Option<u64>,
    pub class: Class,
    pub labels: Vec<u64>,
    pub account: u64,
    pub deadline: Option<u64>,
    pub pages_limit: u64,
    pub pages_used: u64,
    pub processes_limit: u64,
    pub processes_used: u64,
    pub weight: u64,
    /// Weight carved out to children (R7).
    pub weight_used: u64,
    pub depth: u64,
}

impl Budget {
    /// A revocation scope: zero limits (R6).
    pub fn is_scope(&self) -> bool {
        self.pages_limit == 0 && self.processes_limit == 0 && self.weight == 0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backing {
    Frame(u64),
    /// Page `page` of MMIO device `device`.
    Device {
        device: u64,
        page: u64,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapState {
    /// Mapped and accessible.
    Own,
    /// Part of a lend or transfer in flight: the address range stays reserved but the page is
    /// unmapped from this process (I9).
    LentOut(u64),
    /// A lend received in message `msg`: accessible to this (server) process until its reply.
    LentIn(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mapping {
    pub backing: Backing,
    pub flags: u64,
    pub state: MapState,
}

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u64,
    pub budget: u64,
    pub started: bool,
    pub threads: BTreeSet<u64>,
    /// The handle table: index -> handle. Index 0 is never used.
    pub handles: BTreeMap<u64, Handle>,
    /// Address space: virtual page number -> mapping.
    pub space: BTreeMap<u64, Mapping>,
    /// The exit endpoint handle named at `process_create` (none for `init`).
    pub exit_endpoint: Option<Handle>,
}

/// What a blocked thread waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wait {
    /// In `send` or `call`, until a receiver takes message `msg`.
    Send(u64),
    /// In `call`, after the server took message `msg`, until the reply.
    Reply(u64),
    /// In `receive` on endpoint `endpoint` through handle `h`.
    Receive { endpoint: u64, h: u64, max_transfer: u64 },
    /// In `receive` on IRQ device `device` through handle `h`.
    Irq { device: u64, h: u64 },
    /// In `receive` with no handle.
    Sleep,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub tid: u64,
    pub pid: u64,
    pub wait: Option<Wait>,
    /// When the blocking call times out; `None` for `FOREVER`.
    pub deadline: Option<u64>,
    /// The message this thread is serving (for `reply` and `mint`).
    pub serving: Option<u64>,
    /// "The account of the message it is serving (0 when none): set when `receive` delivers a
    /// message, cleared by `reply`" (KERNEL-SPEC.md, Process).
    pub account: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MsgKind {
    Call,
    Send,
}

/// A lend or transfer in flight.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InFlight {
    /// First page of the range in the sender.
    pub sender_vpn: u64,
    pub frames: Vec<u64>,
    /// First page of the range in the receiver, once delivered.
    pub receiver_vpn: Option<u64>,
}

/// A message from the moment it is sent until it is finished with (replied to, or for a `send`,
/// until its receiving thread takes another message or replies).
#[derive(Clone, Debug)]
pub struct Msg {
    pub id: u64,
    pub kind: MsgKind,
    pub sender_pid: u64,
    pub sender_tid: u64,
    pub sender_budget: u64,
    pub endpoint: u64,
    /// Attached by the kernel: the badge and stamp of the handle it was sent through, and the
    /// sender budget's account, labels and class.
    pub badge: u64,
    pub stamp: u64,
    pub account: u64,
    pub labels: Vec<u64>,
    pub class: Class,
    pub words: [u64; WORDS],
    pub handles: Vec<Handle>,
    pub buffer: Option<InFlight>,
    /// Pages lent by a `call` (0 for none); never changes, so a lost lend can be noticed.
    pub lent_pages: u64,
    /// `(pid, tid)` of the thread that took it; `None` while queued.
    pub server: Option<(u64, u64)>,
    /// A `call` whose caller still waits for the reply.
    pub caller_waiting: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitNotice {
    pub pid: u64,
    pub cause: Cause,
    pub code: u64,
    pub blamed_account: u64,
    /// The exiting budget's labels (R1: delivered only to receivers whose labels ⊇ these).
    pub labels: Vec<u64>,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub id: u64,
    /// The budget it is charged to (its creator's).
    pub owner: u64,
    /// Blocked senders' messages, grouped by account, oldest first (R2).
    pub queue: BTreeMap<u64, VecDeque<u64>>,
    /// The account served last (R2's round-robin cursor).
    pub cursor: Option<u64>,
    /// Threads blocked in `receive` on it, first come first served.
    pub receivers: VecDeque<u64>,
    /// Exit notices not yet received.
    pub exits: VecDeque<ExitNotice>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    Mmio { base: u64, pages: u64, dma: bool },
    Irq { n: u64, fired: bool, masked: bool, pending: bool },
    Reset,
}

#[derive(Clone, Debug)]
pub struct Device {
    pub id: u64,
    pub kind: DeviceKind,
    /// Threads blocked in `receive` on this IRQ.
    pub waiters: VecDeque<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame {
    /// The budget it is charged to (R6).
    pub payer: u64,
    /// Abstract contents: the last word written anywhere in the page (0 = zeroed).
    pub content: u64,
}

/// Facts a step reveals that are not system-call results, for the trace (README.md, `note`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// `process_create` in `creator` returned handle `h` naming process `pid`.
    Process { creator: u64, h: u64, pid: u64 },
    /// `process_start` started process `pid` with first thread `tid`.
    Thread { pid: u64, tid: u64 },
}

/// Everything a step produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    /// For the thread the op named (`Done(Ok(Unit))` for `Irq` and `Tick`).
    pub outcome: Outcome,
    /// Results delivered to other blocked threads, in order.
    pub wakes: Vec<Wake>,
    pub notes: Vec<Note>,
}

#[derive(Clone, Debug)]
pub struct Kernel {
    pub mutation: Option<Mutation>,
    pub costs: Costs,
    pub now: u64,
    pub budgets: BTreeMap<u64, Budget>,
    pub processes: BTreeMap<u64, Process>,
    pub threads: BTreeMap<u64, Thread>,
    pub endpoints: BTreeMap<u64, Endpoint>,
    pub devices: BTreeMap<u64, Device>,
    pub frames: BTreeMap<u64, Frame>,
    /// Freed frames and the contents they still hold, reused lowest first (R11 zeroes them then).
    pub free_frames: BTreeMap<u64, u64>,
    pub msgs: BTreeMap<u64, Msg>,
    pub halted: Option<u64>,
    pub sched: Scheduler,
    pub ghost: Ghost,
    next_budget: u64,
    next_pid: u64,
    next_tid: u64,
    next_endpoint: u64,
    next_frame: u64,
    next_msg: u64,
    wakes: Vec<Wake>,
    notes: Vec<Note>,
}

type R<T> = Result<T, Error>;

fn vpn(addr: u64) -> u64 {
    addr / PAGE_SIZE
}

/// Encoding: a register holding a handle must be an index (below `u32::MAX`).
fn decode_handle(raw: u64) -> R<u64> {
    if raw < U32_MAX { Ok(raw) } else { Err(Error::BadHandle) }
}

/// Encoding: only the R, W and X bits exist.
fn decode_flags(flags: u64) -> R<()> {
    if flags & !(FLAG_R | FLAG_W | FLAG_X) != 0 { Err(Error::InvalidArgument) } else { Ok(()) }
}

/// Kernel check of mapping flags: some access, and write only with read (RISC-V has no
/// write-only pages). W+X is a separate check (R11) so its error is `NotPermitted`.
fn check_flags(flags: u64) -> R<()> {
    if flags == 0 || (flags & FLAG_W != 0 && flags & FLAG_R == 0) {
        Err(Error::InvalidArgument)
    } else {
        Ok(())
    }
}

fn is_wx(flags: u64) -> bool {
    flags & FLAG_W != 0 && flags & FLAG_X != 0
}

/// A page-aligned, non-empty byte range inside user space; returns (first page, page count).
fn user_range(addr: u64, len: u64) -> R<(u64, u64)> {
    if len == 0 || !addr.is_multiple_of(PAGE_SIZE) || !len.is_multiple_of(PAGE_SIZE) {
        return Err(Error::InvalidArgument);
    }
    let end = addr.checked_add(len).ok_or(Error::InvalidArgument)?;
    if addr < USER_BASE || end > USER_TOP {
        return Err(Error::InvalidArgument);
    }
    Ok((vpn(addr), len / PAGE_SIZE))
}

/// A lend or transfer range: page-aligned address, at least one page, inside user space.
fn buffer_range(b: Buffer) -> R<(u64, u64)> {
    if b.npages == 0 {
        return Err(Error::InvalidArgument);
    }
    let len = b.npages.checked_mul(PAGE_SIZE).ok_or(Error::InvalidArgument)?;
    user_range(b.addr, len)
}

impl Kernel {
    /// Boot: the kernel creates `root`, `system` and `users` from the argument block and starts
    /// `init` in `root` with handles to all three (slots 1, 2, 3) and to every device (slots 4..).
    ///
    /// A `Boot` that cannot be built (from a hostile trace, say) is an `Err`, never a panic.
    pub fn boot(boot: &Boot, mutation: Option<Mutation>) -> Result<Kernel, alloc::string::String> {
        check_boot(boot)?;
        let mut k = Kernel {
            mutation,
            costs: boot.costs,
            now: 0,
            budgets: BTreeMap::new(),
            processes: BTreeMap::new(),
            threads: BTreeMap::new(),
            endpoints: BTreeMap::new(),
            devices: BTreeMap::new(),
            frames: BTreeMap::new(),
            free_frames: BTreeMap::new(),
            msgs: BTreeMap::new(),
            halted: None,
            sched: Scheduler { mutation, ..Scheduler::default() },
            ghost: Ghost::default(),
            next_budget: ROOT,
            next_pid: INIT_PID,
            next_tid: 1,
            next_endpoint: 1,
            next_frame: 0,
            next_msg: 1,
            wakes: Vec::new(),
            notes: Vec::new(),
        };
        let c = k.costs;
        let root = k.new_budget(
            None,
            Class::System,
            Vec::new(),
            0,
            None,
            boot.root.pages,
            boot.root.processes,
            boot.root.weight,
            Class::System,
        );
        let system = k.new_budget(
            Some(root),
            Class::System,
            Vec::new(),
            0,
            None,
            boot.system.pages,
            boot.system.processes,
            boot.system.weight,
            Class::System,
        );
        let users = k.new_budget(
            Some(root),
            Class::User,
            Vec::new(),
            0,
            None,
            boot.users.pages,
            boot.users.processes,
            boot.users.weight,
            Class::System,
        );
        assert_eq!((root, system, users), (ROOT, SYSTEM, USERS));
        // init: charged to root like any process.
        let pid = k.next_pid;
        k.next_pid += 1;
        k.processes.insert(
            pid,
            Process {
                pid,
                budget: root,
                started: true,
                threads: BTreeSet::new(),
                handles: BTreeMap::new(),
                space: BTreeMap::new(),
                exit_endpoint: None,
            },
        );
        let r = k.budgets.get_mut(&root).unwrap();
        r.pages_used += c.process;
        r.processes_used += 1;
        let mut hs = vec![
            Handle { object: Object::Budget(root), badge: 0, stamp: root, origin: Origin::Boot },
            Handle { object: Object::Budget(system), badge: 0, stamp: root, origin: Origin::Boot },
            Handle { object: Object::Budget(users), badge: 0, stamp: root, origin: Origin::Boot },
        ];
        for (i, spec) in boot.devices.iter().enumerate() {
            let id = i as u64 + 1;
            let kind = match *spec {
                DeviceSpec::Mmio { base, pages, dma } => DeviceKind::Mmio { base, pages, dma },
                // Sources start masked; the first `receive` unmasks them (R5).
                DeviceSpec::Irq { n } => DeviceKind::Irq { n, fired: false, masked: true, pending: false },
                DeviceSpec::Reset => DeviceKind::Reset,
            };
            k.devices.insert(id, Device { id, kind, waiters: VecDeque::new() });
            hs.push(Handle { object: Object::Device(id), badge: 0, stamp: root, origin: Origin::Boot });
        }
        // check_boot made room for these.
        k.install(pid, &hs).map_err(|e| alloc::format!("boot: init's handles: {e:?}"))?;
        k.charge(root, c.thread).map_err(|e| alloc::format!("boot: init's thread: {e:?}"))?;
        k.new_thread(pid);
        Ok(k)
    }

    fn broken(&self, m: Mutation) -> bool {
        self.mutation == Some(m)
    }

    // ---------------------------------------------------------------------------------------
    // Queries used by the generator, the trace and the property checks.

    /// Threads that may make a step: runnable threads of started processes, in tid order.
    pub fn runnable(&self) -> Vec<(u64, u64)> {
        if self.halted.is_some() {
            return Vec::new();
        }
        self.threads.values().filter(|t| t.wait.is_none()).map(|t| (t.pid, t.tid)).collect()
    }

    pub fn budget_of(&self, pid: u64) -> Option<u64> {
        self.processes.get(&pid).map(|p| p.budget)
    }

    /// The budgets whose parent is `b`, in id order.
    pub fn children(&self, b: u64) -> Vec<u64> {
        self.budgets.values().filter(|x| x.parent == Some(b)).map(|x| x.id).collect()
    }

    /// Is `b` equal to `ancestor` or below it?
    pub fn is_descendant_or_self(&self, b: u64, ancestor: u64) -> bool {
        let mut cur = Some(b);
        while let Some(c) = cur {
            if c == ancestor {
                return true;
            }
            cur = self.budgets.get(&c).and_then(|x| x.parent);
        }
        false
    }

    /// Pages a handle table of `n` handles costs.
    pub fn table_pages(&self, n: u64) -> u64 {
        n.div_ceil(self.costs.handles_per_page.max(1))
    }

    /// The earliest time something is due (a timeout or a budget deadline).
    pub fn next_event(&self) -> Option<u64> {
        let t = self.threads.values().filter(|t| t.wait.is_some()).filter_map(|t| t.deadline).min();
        let b = self.budgets.values().filter_map(|b| b.deadline).min();
        match (t, b) {
            (Some(x), Some(y)) => Some(x.min(y)),
            (x, y) => x.or(y),
        }
    }

    /// "The timer is always armed (slice end or the next deadline)" (R12).
    pub fn timer(&self) -> u64 {
        let slice_end = self.now.saturating_add(SLICE);
        self.next_event().map_or(slice_end, |e| e.min(slice_end))
    }

    // ---------------------------------------------------------------------------------------
    // Accounting (R6, R7).

    fn free_pages(&self, b: u64) -> u64 {
        self.budgets.get(&b).map_or(0, |x| x.pages_limit.saturating_sub(x.pages_used))
    }

    /// Charge `n` pages to `b`, failing with `OutOfMemory` over its limit. While a budget is over
    /// its limit through R3, this fails for every new allocation.
    fn charge(&mut self, b: u64, n: u64) -> R<()> {
        if self.free_pages(b) < n || !self.budgets.contains_key(&b) {
            return Err(Error::OutOfMemory);
        }
        self.add_usage(b, n);
        Ok(())
    }

    /// The budgets a charge to `b` lands on: `b` alone (R6), or with the mutation its ancestors too.
    fn charged(&self, b: u64) -> Vec<u64> {
        let mut out = vec![b];
        if self.broken(Mutation::R6ChargeAncestors) {
            let mut cur = self.budgets.get(&b).and_then(|x| x.parent);
            while let Some(p) = cur {
                out.push(p);
                cur = self.budgets[&p].parent;
            }
        }
        out
    }

    fn add_usage(&mut self, b: u64, n: u64) {
        for x in self.charged(b) {
            if let Some(x) = self.budgets.get_mut(&x) {
                x.pages_used += n;
            }
        }
    }

    fn uncharge(&mut self, b: u64, n: u64) {
        for x in self.charged(b) {
            if let Some(x) = self.budgets.get_mut(&x) {
                x.pages_used = x.pages_used.saturating_sub(n);
            }
        }
    }

    /// Create a budget object (after all checks). Charges: its own object to itself, or to its
    /// parent for a revocation scope (R6); its limits carved from the parent (R7).
    #[allow(clippy::too_many_arguments)]
    fn new_budget(
        &mut self,
        parent: Option<u64>,
        class: Class,
        labels: Vec<u64>,
        account: u64,
        deadline: Option<u64>,
        pages: u64,
        processes: u64,
        weight: u64,
        creator_class: Class,
    ) -> u64 {
        let id = self.next_budget;
        self.next_budget += 1;
        self.ghost.budget_created(id, &labels, creator_class);
        let depth = parent.and_then(|p| self.budgets.get(&p)).map_or(0, |p| p.depth + 1);
        let mut b = Budget {
            id,
            parent,
            class,
            labels,
            account,
            deadline,
            pages_limit: pages,
            pages_used: 0,
            processes_limit: processes,
            processes_used: 0,
            weight,
            weight_used: 0,
            depth,
        };
        let scope = b.is_scope();
        if !scope || self.broken(Mutation::R6ScopeChargedToItself) {
            b.pages_used = self.costs.budget;
        }
        self.budgets.insert(id, b);
        if let Some(p) = parent {
            let own_cost =
                if scope && !self.broken(Mutation::R6ScopeChargedToItself) { self.costs.budget } else { 0 };
            let x = self.budgets.get_mut(&p).unwrap();
            x.pages_used += if scope { own_cost } else { pages };
            x.processes_used += processes;
            x.weight_used += weight;
        }
        self.sched.add_budget(id, class, weight);
        id
    }

    // ---------------------------------------------------------------------------------------
    // Handle tables.

    fn lookup(&self, pid: u64, h: u64) -> R<Handle> {
        self.processes.get(&pid).and_then(|p| p.handles.get(&h)).copied().ok_or(Error::BadHandle)
    }

    fn lookup_budget(&self, pid: u64, h: u64) -> R<u64> {
        match self.lookup(pid, h)?.object {
            Object::Budget(b) => Ok(b),
            _ => Err(Error::WrongObject),
        }
    }

    fn lookup_process(&self, pid: u64, h: u64) -> R<u64> {
        match self.lookup(pid, h)?.object {
            Object::Process(p) => Ok(p),
            _ => Err(Error::WrongObject),
        }
    }

    fn lookup_endpoint(&self, pid: u64, h: u64) -> R<(u64, Handle)> {
        let hd = self.lookup(pid, h)?;
        match hd.object {
            Object::Endpoint(e) => Ok((e, hd)),
            _ => Err(Error::WrongObject),
        }
    }

    /// Install copies of `hs` in `pid`'s table at the lowest free indices from 1, charging any
    /// growth of the table to the process's budget (R6). All or nothing.
    fn install(&mut self, pid: u64, hs: &[Handle]) -> R<Vec<u64>> {
        let (budget, n0) = match self.processes.get(&pid) {
            Some(p) => (p.budget, p.handles.len() as u64),
            None => return Err(Error::Dead),
        };
        let growth = self.table_pages(n0 + hs.len() as u64) - self.table_pages(n0);
        self.charge(budget, growth)?;
        let p = self.processes.get_mut(&pid).unwrap();
        let mut out = Vec::new();
        let mut next = 1;
        for h in hs {
            while p.handles.contains_key(&next) {
                next += 1;
            }
            p.handles.insert(next, *h);
            out.push(next);
        }
        Ok(out)
    }

    fn remove_handle(&mut self, pid: u64, h: u64) {
        let Some(p) = self.processes.get_mut(&pid) else { return };
        let n0 = p.handles.len() as u64;
        if p.handles.remove(&h).is_none() {
            return;
        }
        let budget = p.budget;
        let shrink = self.table_pages(n0) - self.table_pages(n0 - 1);
        self.uncharge(budget, shrink);
    }

    /// Close every handle matching `pred`, wherever it is: process tables, messages in flight,
    /// and processes' exit endpoint references.
    fn sweep(&mut self, pred: impl Fn(&Handle) -> bool) {
        let pids: Vec<u64> = self.processes.keys().copied().collect();
        for pid in pids {
            let doomed: Vec<u64> =
                self.processes[&pid].handles.iter().filter(|(_, h)| pred(h)).map(|(i, _)| *i).collect();
            for i in doomed {
                self.remove_handle(pid, i);
            }
            let p = self.processes.get_mut(&pid).unwrap();
            if p.exit_endpoint.as_ref().is_some_and(&pred) {
                p.exit_endpoint = None;
            }
        }
        for m in self.msgs.values_mut() {
            m.handles.retain(|h| !pred(h));
        }
    }

    // ---------------------------------------------------------------------------------------
    // Memory.

    /// A frame for a new mapping: a freed one if there is one, else a fresh one. R11: zeroed
    /// before any process sees it.
    fn alloc_frame(&mut self, payer: u64) -> u64 {
        let (f, stale) = match self.free_frames.pop_first() {
            Some(x) => x,
            None => {
                self.next_frame += 1;
                (self.next_frame - 1, 0)
            }
        };
        let content = if self.broken(Mutation::R11NoZeroing) { stale } else { 0 };
        self.frames.insert(f, Frame { payer, content });
        self.ghost.fresh.insert(f);
        f
    }

    /// Free a frame and uncharge its payer. Its contents stay in RAM until it is reused.
    fn free_frame(&mut self, f: u64) {
        if let Some(fr) = self.frames.remove(&f) {
            self.uncharge(fr.payer, 1);
            self.ghost.fresh.remove(&f);
            self.free_frames.insert(f, fr.content);
        }
    }

    /// `n` free virtual pages in `pid` for a mapping whose address the kernel chooses: above
    /// everything already mapped, from `KERNEL_CHOSEN_BASE`.
    fn alloc_va(&self, pid: u64, n: u64) -> R<u64> {
        let p = self.processes.get(&pid).ok_or(Error::Dead)?;
        let above = p.space.last_key_value().map_or(0, |(v, _)| v + 1);
        let start = above.max(vpn(KERNEL_CHOSEN_BASE));
        match start.checked_add(n) {
            Some(end) if end <= vpn(USER_TOP) => Ok(start),
            _ => Err(Error::OutOfMemory),
        }
    }

    /// Every page in `[first, first + n)` is an accessible mapping of `pid` that it owns
    /// (not lent out, not lent in).
    fn own_range(&self, pid: u64, first: u64, n: u64, want: impl Fn(&Mapping) -> bool) -> R<()> {
        let p = self.processes.get(&pid).ok_or(Error::Dead)?;
        for v in first..first + n {
            match p.space.get(&v) {
                Some(m) if m.state == MapState::Own && want(m) => {}
                _ => return Err(Error::InvalidArgument),
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------------------------------
    // Threads, blocking, waking.

    fn new_thread(&mut self, pid: u64) -> u64 {
        let tid = self.next_tid;
        self.next_tid += 1;
        self.threads.insert(tid, Thread { tid, pid, wait: None, deadline: None, serving: None, account: 0 });
        let p = self.processes.get_mut(&pid).unwrap();
        p.threads.insert(tid);
        let b = p.budget;
        self.sched.wake(b, tid);
        tid
    }

    fn block(&mut self, tid: u64, wait: Wait, timeout: u64) {
        let deadline = if timeout == FOREVER { None } else { Some(self.now.saturating_add(timeout)) };
        let t = self.threads.get_mut(&tid).unwrap();
        t.wait = Some(wait);
        t.deadline = deadline;
        let pid = t.pid;
        if let Some(b) = self.budget_of(pid) {
            self.sched.block(b, tid);
        }
    }

    fn wake(&mut self, tid: u64, result: R<Ret>) {
        let Some(t) = self.threads.get_mut(&tid) else { return };
        t.wait = None;
        t.deadline = None;
        let pid = t.pid;
        if let Some(b) = self.budget_of(pid) {
            self.sched.wake(b, tid);
        }
        self.wakes.push(Wake { pid, tid, result });
    }

    // ---------------------------------------------------------------------------------------
    // Messages.

    /// Put a lend or transfer back in its sender's space (the message did not go through).
    fn return_buffer(&mut self, mid: u64) {
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        let Some(buf) = m.buffer.take() else { return };
        if let Some(p) = self.processes.get_mut(&m.sender_pid) {
            for (i, _) in buf.frames.iter().enumerate() {
                if let Some(map) = p.space.get_mut(&(buf.sender_vpn + i as u64)) {
                    if map.state == MapState::LentOut(mid) {
                        map.state = MapState::Own;
                    }
                }
            }
        }
    }

    /// Remove a queued message from its endpoint's queue.
    fn unqueue(&mut self, mid: u64) {
        let Some(m) = self.msgs.get(&mid) else { return };
        let (e, account) = (m.endpoint, m.account);
        if let Some(ep) = self.endpoints.get_mut(&e) {
            if let Some(q) = ep.queue.get_mut(&account) {
                q.retain(|x| *x != mid);
                if q.is_empty() {
                    ep.queue.remove(&account);
                }
            }
        }
    }

    /// A queued message fails: its sender gets `err` and its buffer back.
    fn fail_sender(&mut self, mid: u64, err: Error) {
        self.unqueue(mid);
        self.return_buffer(mid);
        if let Some(m) = self.msgs.remove(&mid) {
            self.wake(m.sender_tid, Err(err));
        }
    }

    /// R3: the caller of a taken `call` is gone (dead, timed out, or its endpoint destroyed). The
    /// lent pages stay mapped in the server, charged to the server's budget until its reply.
    fn abandon(&mut self, mid: u64) {
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        m.caller_waiting = false;
        let (sender_pid, sender_budget) = (m.sender_pid, m.sender_budget);
        let Some((server_pid, _)) = m.server else { return };
        let Some(buf) = m.buffer.clone() else { return };
        if let Some(p) = self.processes.get_mut(&sender_pid) {
            for i in 0..buf.frames.len() as u64 {
                let v = buf.sender_vpn + i;
                if p.space.get(&v).is_some_and(|x| x.state == MapState::LentOut(mid)) {
                    p.space.remove(&v);
                }
            }
        }
        if self.broken(Mutation::R3UnmapAbandonedLend) {
            self.unmap_lend_in_server(mid, true);
            return;
        }
        let server_budget = self.budget_of(server_pid).unwrap_or(sender_budget);
        if !self.broken(Mutation::R3ChargeStaysWithCaller) {
            for f in &buf.frames {
                self.frames.get_mut(f).unwrap().payer = server_budget;
            }
            self.uncharge(sender_budget, buf.frames.len() as u64);
            self.add_usage(server_budget, buf.frames.len() as u64); // over the limit if need be (R3)
        }
    }

    /// Unmap a received lend from its server; if `free`, free its frames too.
    fn unmap_lend_in_server(&mut self, mid: u64, free: bool) {
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        let Some(buf) = m.buffer.take() else { return };
        if let (Some((spid, _)), Some(rv)) = (m.server, buf.receiver_vpn) {
            if let Some(p) = self.processes.get_mut(&spid) {
                for i in 0..buf.frames.len() as u64 {
                    if p.space.get(&(rv + i)).is_some_and(|x| x.state == MapState::LentIn(mid)) {
                        p.space.remove(&(rv + i));
                    }
                }
            }
        }
        if free {
            for f in &buf.frames {
                self.free_frame(*f);
            }
        } else {
            // Give the buffer back to its lender.
            self.msgs.get_mut(&mid).unwrap().buffer = Some(buf);
            self.return_buffer(mid);
        }
    }

    /// A queued message is refused in its turn: its account counts as served (R2) and the
    /// sender fails with `err`.
    fn refuse(&mut self, e: u64, mid: u64, err: Error) {
        let account = self.msgs[&mid].account;
        self.ghost.took(e, account, &self.endpoints[&e]);
        self.endpoints.get_mut(&e).unwrap().cursor = Some(account);
        self.fail_sender(mid, err);
    }

    /// R2: the next message to take on `e`: the oldest message of the next account after the
    /// last one served, in account order, wrapping around.
    fn next_sender(&self, e: u64) -> Option<u64> {
        let ep = self.endpoints.get(&e)?;
        if self.broken(Mutation::R2FifoAcrossAccounts) {
            return ep.queue.values().filter_map(|q| q.front()).min().copied();
        }
        let next = match ep.cursor {
            Some(c) => ep.queue.range(c + 1..).next().or_else(|| ep.queue.iter().next()),
            None => ep.queue.iter().next(),
        };
        next.and_then(|(_, q)| q.front().copied())
    }

    /// Match waiting receivers on `e` with what is pending there, until one side runs out.
    /// Exit notices come before messages (the spec does not order them; README.md).
    fn pump(&mut self, e: u64) {
        loop {
            let Some(ep) = self.endpoints.get(&e) else { return };
            let Some(&rtid) = ep.receivers.front() else { return };
            let rpid = self.threads[&rtid].pid;
            let Some(rbudget) = self.budget_of(rpid) else { return };
            let (rclass, rlabels) = {
                let b = &self.budgets[&rbudget];
                (b.class, b.labels.clone())
            };
            let Some(Wait::Receive { max_transfer, .. }) = self.threads[&rtid].wait else { return };

            if let Some(n) = self.endpoints.get_mut(&e).unwrap().exits.pop_front() {
                // R1: an exit notice goes only to a receiver whose labels ⊇ the exiting budget's;
                // otherwise it is dropped.
                if superset(&rlabels, &n.labels) || self.broken(Mutation::R1ExitNoticeIgnoresLabels) {
                    self.endpoints.get_mut(&e).unwrap().receivers.pop_front();
                    self.ghost.flows.push(Flow::Exit { from: n.labels.clone(), to: rlabels.clone() });
                    let ret = Ret::ExitNotice {
                        pid: n.pid,
                        cause: n.cause,
                        code: n.code,
                        blamed_account: n.blamed_account,
                    };
                    self.wake(rtid, Ok(ret));
                }
                continue;
            }

            let Some(mid) = self.next_sender(e) else { return };
            let m = self.msgs[&mid].clone();

            // R1: between two user budgets, only equal label sets.
            if m.class == Class::User
                && rclass == Class::User
                && m.labels != rlabels
                && !self.broken(Mutation::R1SkipLabelCheck)
            {
                self.refuse(e, mid, Error::LabelDenied);
                continue;
            }
            let transfer = match (&m.kind, &m.buffer) {
                (MsgKind::Send, Some(b)) => b.frames.len() as u64,
                _ => 0,
            };
            // R4: a transfer larger than the receiver opted into is refused; move on.
            if transfer > max_transfer && !self.broken(Mutation::R4IgnoreMaxTransfer) {
                self.refuse(e, mid, Error::Refused);
                continue;
            }
            // Transferred pages become the receiver's; if its budget cannot hold them, the
            // transfer is refused like an unrequested one (README.md).
            if transfer > 0 && m.sender_budget != rbudget && self.free_pages(rbudget) < transfer {
                self.refuse(e, mid, Error::Refused);
                continue;
            }
            // The receiver pays for the handles it receives. If it cannot, its receive fails and
            // the message stays queued.
            let mut hs = m.handles.clone();
            if self.broken(Mutation::R9ReceivedHandleRestamped) {
                for h in &mut hs {
                    h.stamp = rbudget;
                }
            }
            let n0 = self.processes[&rpid].handles.len() as u64;
            let growth = self.table_pages(n0 + hs.len() as u64) - self.table_pages(n0);
            let need = growth + if m.sender_budget != rbudget { transfer } else { 0 };
            let va = match &m.buffer {
                Some(b) => self.alloc_va(rpid, b.frames.len() as u64).map(Some),
                None => Ok(None),
            };
            if self.free_pages(rbudget) < need || va.is_err() {
                self.endpoints.get_mut(&e).unwrap().receivers.pop_front();
                self.wake(rtid, Err(Error::OutOfMemory));
                continue;
            }
            let va = va.unwrap();

            // Deliver.
            self.ghost.took(e, m.account, &self.endpoints[&e]);
            self.unqueue(mid);
            let ep = self.endpoints.get_mut(&e).unwrap();
            ep.cursor = Some(m.account);
            ep.receivers.pop_front();
            let installed = self.install(rpid, &hs).expect("checked above");
            let mut received = None;
            if let (Some(buf), Some(rv)) = (&m.buffer, va) {
                let n = buf.frames.len() as u64;
                match m.kind {
                    MsgKind::Call => {
                        let p = self.processes.get_mut(&rpid).unwrap();
                        for (i, f) in buf.frames.iter().enumerate() {
                            p.space.insert(
                                rv + i as u64,
                                Mapping {
                                    backing: Backing::Frame(*f),
                                    flags: FLAG_R | FLAG_W,
                                    state: MapState::LentIn(mid),
                                },
                            );
                        }
                    }
                    MsgKind::Send => {
                        if let Some(sp) = self.processes.get_mut(&m.sender_pid) {
                            for i in 0..n {
                                sp.space.remove(&(buf.sender_vpn + i));
                            }
                        }
                        let p = self.processes.get_mut(&rpid).unwrap();
                        for (i, f) in buf.frames.iter().enumerate() {
                            p.space.insert(
                                rv + i as u64,
                                Mapping {
                                    backing: Backing::Frame(*f),
                                    flags: FLAG_R | FLAG_W,
                                    state: MapState::Own,
                                },
                            );
                        }
                        for f in &buf.frames {
                            self.frames.get_mut(f).unwrap().payer = rbudget;
                        }
                        self.uncharge(m.sender_budget, n);
                        self.add_usage(rbudget, n);
                    }
                }
                let kind = if m.kind == MsgKind::Call { BufferKind::Lend } else { BufferKind::Transfer };
                received = Some(Received { kind, addr: rv * PAGE_SIZE, pages: n });
            }
            // The receiving thread now serves this message. A `send` it served before is done.
            if let Some(old) = self.threads[&rtid].serving {
                if self.msgs.get(&old).is_some_and(|o| o.kind == MsgKind::Send) {
                    self.msgs.remove(&old);
                }
            }
            let t = self.threads.get_mut(&rtid).unwrap();
            t.serving = Some(mid);
            t.account = m.account;
            let mm = self.msgs.get_mut(&mid).unwrap();
            mm.server = Some((rpid, rtid));
            if let Some(b) = mm.buffer.as_mut() {
                b.receiver_vpn = va;
                if m.kind == MsgKind::Send {
                    mm.buffer = None; // transferred: no longer in flight
                }
            }
            self.ghost.flows.push(Flow::Message {
                from_class: m.class,
                from: m.labels.clone(),
                to_class: rclass,
                to: rlabels.clone(),
                transfer,
                max_transfer,
            });
            let msg = Message {
                msg_id: mid,
                badge: m.badge,
                account: m.account,
                labels: m.labels.clone(),
                words: m.words,
                handles: installed,
                buffer: received,
            };
            self.wake(rtid, Ok(Ret::Message(msg)));
            match m.kind {
                MsgKind::Send => self.wake(m.sender_tid, Ok(Ret::Unit)),
                MsgKind::Call => {
                    let t = self.threads.get_mut(&m.sender_tid).unwrap();
                    t.wait = Some(Wait::Reply(mid));
                }
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Ending things: threads, processes, endpoints, budgets (R10).

    /// A thread ends. What it waited for is withdrawn; what it was serving is finished: a caller
    /// still waiting gets `Dead` and its lend back; an abandoned lend is freed.
    fn end_thread(&mut self, tid: u64) {
        let Some(t) = self.threads.get(&tid).cloned() else { return };
        match t.wait {
            Some(Wait::Send(mid)) => {
                self.unqueue(mid);
                self.return_buffer(mid);
                self.msgs.remove(&mid);
            }
            Some(Wait::Reply(mid)) => self.abandon(mid),
            Some(Wait::Receive { endpoint, .. }) => {
                if let Some(ep) = self.endpoints.get_mut(&endpoint) {
                    ep.receivers.retain(|x| *x != tid);
                }
            }
            Some(Wait::Irq { device, .. }) => {
                if let Some(d) = self.devices.get_mut(&device) {
                    d.waiters.retain(|x| *x != tid);
                }
            }
            Some(Wait::Sleep) | None => {}
        }
        if let Some(mid) = t.serving {
            self.finish_served(mid, Error::Dead);
        }
        let budget = self.budget_of(t.pid);
        if let Some(b) = budget {
            self.sched.block(b, tid);
        }
        self.threads.remove(&tid);
        if let Some(p) = self.processes.get_mut(&t.pid) {
            p.threads.remove(&tid);
        }
        if let Some(b) = budget {
            self.uncharge(b, self.costs.thread);
        }
    }

    /// A served message's server is gone: a waiting caller gets `err` and its lend back; an
    /// abandoned lend is freed.
    fn finish_served(&mut self, mid: u64, err: Error) {
        let Some(m) = self.msgs.get(&mid) else { return };
        if m.kind == MsgKind::Call {
            if m.caller_waiting {
                let caller = m.sender_tid;
                self.unmap_lend_in_server(mid, false);
                self.wake(caller, Err(err));
            } else {
                self.unmap_lend_in_server(mid, true);
            }
        }
        self.msgs.remove(&mid);
    }

    /// A process ends: its threads, address space and handle table are freed, every handle naming
    /// it is closed, and its exit notice goes to its exit endpoint.
    fn end_process(&mut self, pid: u64, cause: Cause, code: u64, blamed_account: u64) {
        let Some(p) = self.processes.get(&pid) else { return };
        let tids: Vec<u64> = p.threads.iter().copied().collect();
        for tid in tids {
            self.end_thread(tid);
        }
        let p = self.processes.remove(&pid).unwrap();
        for (_, m) in p.space.iter() {
            if let Backing::Frame(f) = m.backing {
                if m.state == MapState::Own
                    || matches!(m.state, MapState::LentOut(x) if !self.msgs.contains_key(&x))
                {
                    self.free_frame(f);
                }
            }
        }
        let table = self.table_pages(p.handles.len() as u64);
        self.uncharge(p.budget, table + self.costs.process);
        if let Some(b) = self.budgets.get_mut(&p.budget) {
            b.processes_used = b.processes_used.saturating_sub(1);
        }
        self.sweep(|h| h.object == Object::Process(pid));
        let labels = self.budgets.get(&p.budget).map(|b| b.labels.clone()).unwrap_or_default();
        if let Some(h) = p.exit_endpoint {
            if let Object::Endpoint(e) = h.object {
                if let Some(ep) = self.endpoints.get_mut(&e) {
                    ep.exits.push_back(ExitNotice { pid, cause, code, blamed_account, labels });
                    self.pump(e);
                }
            }
        }
    }

    /// Destroy an endpoint: blocked senders and receivers get `Dead`; taken calls in flight fail
    /// with `Dead` (their lends stay with the server as in R3); pending exit notices are dropped.
    fn destroy_endpoint(&mut self, e: u64) {
        let Some(ep) = self.endpoints.get(&e) else { return };
        let queued: Vec<u64> = ep.queue.values().flatten().copied().collect();
        let receivers: Vec<u64> = ep.receivers.iter().copied().collect();
        let owner = ep.owner;
        for mid in queued {
            self.fail_sender(mid, Error::Dead);
        }
        for tid in receivers {
            self.wake(tid, Err(Error::Dead));
        }
        let in_flight: Vec<u64> = self
            .msgs
            .values()
            .filter(|m| m.endpoint == e && m.kind == MsgKind::Call && m.caller_waiting && m.server.is_some())
            .map(|m| m.id)
            .collect();
        for mid in in_flight {
            let caller = self.msgs[&mid].sender_tid;
            self.abandon(mid);
            self.wake(caller, Err(Error::Dead));
        }
        self.endpoints.remove(&e);
        if !self.broken(Mutation::R6EndpointsFree) {
            self.uncharge(owner, self.costs.endpoint);
        }
        self.sweep(|h| h.object == Object::Endpoint(e));
    }

    /// R10: destroy budget `b` and everything below it.
    fn destroy_budget(&mut self, b: u64) {
        if !self.budgets.contains_key(&b) {
            return;
        }
        // Descendants first: post-order.
        let mut order = Vec::new();
        let mut stack = vec![(b, false)];
        while let Some((x, done)) = stack.pop() {
            if done {
                order.push(x);
                continue;
            }
            stack.push((x, true));
            for c in self.children(x).into_iter().rev() {
                stack.push((c, false));
            }
        }
        let doomed: BTreeSet<u64> = order.iter().copied().collect();
        for x in &order {
            if *x != b && self.broken(Mutation::R10SpareDescendantProcesses) {
                continue;
            }
            let pids: Vec<u64> = self.processes.values().filter(|p| p.budget == *x).map(|p| p.pid).collect();
            for pid in pids {
                self.end_process(pid, Cause::Killed, 0, 0);
            }
        }
        let eps: Vec<u64> =
            self.endpoints.values().filter(|e| doomed.contains(&e.owner)).map(|e| e.id).collect();
        for e in eps {
            self.destroy_endpoint(e);
        }
        if !self.broken(Mutation::R10KeepForeignHandles) {
            self.sweep(|h| doomed.contains(&h.stamp));
        }
        self.sweep(|h| matches!(h.object, Object::Budget(x) if doomed.contains(&x)));
        // Return the carved limits (or a scope's own object) to the parent.
        let bb = self.budgets[&b].clone();
        if let Some(p) = bb.parent {
            if !self.broken(Mutation::R10KeepCarvedLimits) {
                let scope_cost =
                    if self.broken(Mutation::R6ScopeChargedToItself) { 0 } else { self.costs.budget };
                let px = self.budgets.get_mut(&p).unwrap();
                px.pages_used =
                    px.pages_used.saturating_sub(if bb.is_scope() { scope_cost } else { bb.pages_limit });
                px.processes_used = px.processes_used.saturating_sub(bb.processes_limit);
                px.weight_used = px.weight_used.saturating_sub(bb.weight);
            }
        }
        for x in order {
            self.budgets.remove(&x);
            self.sched.remove_budget(x);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Time.

    /// Everything due at or before `now`: timeouts (I13) and budget deadlines, earliest first.
    fn expire(&mut self) {
        loop {
            let t = self
                .threads
                .values()
                .filter(|t| t.wait.is_some() && t.deadline.is_some_and(|d| d <= self.now))
                .min_by_key(|t| (t.deadline, t.tid))
                .map(|t| (t.deadline.unwrap(), 0, t.tid));
            let b = self
                .budgets
                .values()
                .filter(|b| b.deadline.is_some_and(|d| d <= self.now))
                .min_by_key(|b| (b.deadline, b.id))
                .map(|b| (b.deadline.unwrap(), 1, b.id));
            let next = match (t, b) {
                (Some(x), Some(y)) => x.min(y),
                (Some(x), None) | (None, Some(x)) => x,
                (None, None) => return,
            };
            match next {
                (_, 0, tid) => self.time_out(tid),
                (_, _, id) => self.destroy_budget(id),
            }
        }
    }

    /// A blocking call reached its timeout.
    fn time_out(&mut self, tid: u64) {
        let Some(wait) = self.threads.get(&tid).and_then(|t| t.wait) else { return };
        match wait {
            Wait::Send(mid) => {
                self.unqueue(mid);
                self.return_buffer(mid);
                self.msgs.remove(&mid);
            }
            // R3: the server keeps the lend, charged to it, until its reply.
            Wait::Reply(mid) => self.abandon(mid),
            Wait::Receive { endpoint, .. } => {
                if let Some(ep) = self.endpoints.get_mut(&endpoint) {
                    ep.receivers.retain(|x| *x != tid);
                }
            }
            // With several threads waiting on one IRQ, a second event can stay pending (masked)
            // while this one times out: R5 unmasks only when a receive begins.
            Wait::Irq { device, .. } => {
                if let Some(d) = self.devices.get_mut(&device) {
                    d.waiters.retain(|x| *x != tid);
                }
            }
            Wait::Sleep => {}
        }
        self.wake(tid, Err(Error::Timeout));
    }

    /// `dt` microseconds pass. The scheduler runs its pick for at most a slice at a time, up to
    /// the next due event; each run is charged at deschedule (R12).
    fn tick(&mut self, dt: u64) {
        let end = self.now.saturating_add(dt);
        while self.now < end {
            let pick = self.sched.pick();
            let mut run = (end - self.now).min(SLICE);
            if let Some(e) = self.next_event() {
                if e > self.now {
                    run = run.min(e - self.now);
                }
            }
            self.now += run;
            if let Some((b, _)) = pick {
                self.sched.charge(b, run);
            }
            self.expire();
        }
    }

    /// Interrupt line `n` is raised. R5: if the source is unmasked it fires: the kernel masks it
    /// and sets `fired`, and a waiting receiver gets it.
    fn irq(&mut self, n: u64) {
        let Some(id) = self
            .devices
            .values()
            .find(|d| matches!(d.kind, DeviceKind::Irq { n: m, .. } if m == n))
            .map(|d| d.id)
        else {
            return;
        };
        self.ghost.irq_raised(id);
        if let DeviceKind::Irq { pending, .. } = &mut self.devices.get_mut(&id).unwrap().kind {
            *pending = true;
        }
        self.fire_if_unmasked(id);
    }

    fn fire_if_unmasked(&mut self, id: u64) {
        let no_mask = self.broken(Mutation::R5NoMaskOnFire);
        let Some(d) = self.devices.get_mut(&id) else { return };
        let DeviceKind::Irq { fired, masked, pending, .. } = &mut d.kind else { return };
        if *masked || !*pending {
            return;
        }
        *pending = false;
        *fired = true;
        if !no_mask {
            *masked = true;
        }
        // A waiting receiver takes it at once, clearing `fired`.
        let waiter = d.waiters.pop_front();
        if waiter.is_some() {
            if let DeviceKind::Irq { fired, .. } = &mut d.kind {
                *fired = false;
            }
        }
        self.ghost.irq_fired(id);
        if let Some(tid) = waiter {
            if let Some(Wait::Irq { h, .. }) = self.threads.get(&tid).and_then(|t| t.wait) {
                self.ghost.irq_delivered(id);
                self.wake(tid, Ok(Ret::Interrupt { h }));
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // The step function.

    /// Apply one op. `None` if the op is not a legal event (it names a thread that does not
    /// exist or is blocked, or the machine is halted); the state is then unchanged.
    pub fn step(&mut self, op: &Op) -> Option<Step> {
        if self.halted.is_some() {
            return None;
        }
        let actor = match op {
            Op::Sys { pid, tid, .. }
            | Op::Write { pid, tid, .. }
            | Op::Read { pid, tid, .. }
            | Op::Exec { pid, tid, .. }
            | Op::Fault { pid, tid } => {
                let t = self.threads.get(tid)?;
                if t.pid != *pid || t.wait.is_some() {
                    return None;
                }
                Some(*tid)
            }
            Op::Irq { .. } | Op::Tick { .. } => None,
        };
        self.wakes.clear();
        self.notes.clear();
        self.ghost.begin_step();
        let outcome = match op {
            Op::Sys { pid, tid, call } => self.syscall(*pid, *tid, call),
            Op::Write { pid, tid, addr, value } => self.access(*pid, *tid, *addr, FLAG_W, Some(*value)),
            Op::Read { pid, tid, addr } => self.access(*pid, *tid, *addr, FLAG_R, None),
            Op::Exec { pid, tid, addr } => self.access(*pid, *tid, *addr, FLAG_X, None),
            Op::Fault { pid, tid } => {
                self.fault(*pid, *tid);
                Outcome::Gone
            }
            Op::Irq { n } => {
                self.irq(*n);
                Outcome::Done(Ok(Ret::Unit))
            }
            Op::Tick { dt } => {
                self.tick(*dt);
                Outcome::Done(Ok(Ret::Unit))
            }
        };
        self.expire();
        // A call that blocked and was answered within this same step returns its answer
        // directly (for example `receive` with a message waiting, or a timeout of 0).
        let mut outcome = outcome;
        if let Some(tid) = actor {
            if outcome == Outcome::Blocked {
                if let Some(i) = self.wakes.iter().position(|w| w.tid == tid) {
                    outcome = Outcome::Done(self.wakes.remove(i).result);
                }
            }
            if !self.threads.contains_key(&tid) {
                outcome = Outcome::Gone;
                self.wakes.retain(|w| w.tid != tid);
            }
        }
        // A thread that died later in the step does not receive its earlier wake.
        let threads = &self.threads;
        self.wakes.retain(|w| threads.contains_key(&w.tid));
        Some(Step {
            outcome,
            wakes: core::mem::take(&mut self.wakes),
            notes: core::mem::take(&mut self.notes),
        })
    }

    fn fault(&mut self, pid: u64, tid: u64) {
        // Blame: the account the faulting thread was serving.
        let blamed = self.threads.get(&tid).map_or(0, |t| t.account);
        self.end_process(pid, Cause::Faulted, 0, blamed);
    }

    fn access(&mut self, pid: u64, tid: u64, addr: u64, need: u64, value: Option<u64>) -> Outcome {
        let v = vpn(addr);
        let m = self.processes.get(&pid).and_then(|p| p.space.get(&v)).copied();
        let ok = m.filter(|m| matches!(m.state, MapState::Own | MapState::LentIn(_)) && m.flags & need != 0);
        let Some(m) = ok else {
            self.fault(pid, tid);
            return Outcome::Gone;
        };
        match (m.backing, value) {
            (Backing::Frame(f), Some(x)) => {
                self.frames.get_mut(&f).unwrap().content = x;
                self.ghost.fresh.remove(&f);
                Outcome::Done(Ok(Ret::Unit))
            }
            (Backing::Frame(f), None) => Outcome::Done(Ok(Ret::Word(self.frames[&f].content))),
            (Backing::Device { .. }, Some(_)) => Outcome::Done(Ok(Ret::Unit)),
            (Backing::Device { .. }, None) => Outcome::Done(Ok(Ret::Word(0))),
        }
    }

    fn syscall(&mut self, pid: u64, tid: u64, call: &Syscall) -> Outcome {
        use Syscall as S;
        let done = |r: R<Ret>| Outcome::Done(r);
        match call {
            S::MapAnon { len, flags } => done(self.map_anon(pid, *len, *flags).map(Ret::Addr)),
            S::Unmap { addr, len } => done(self.unmap(pid, *addr, *len).map(|_| Ret::Unit)),
            S::SetFlags { addr, len, flags } => {
                done(self.set_flags(pid, *addr, *len, *flags).map(|_| Ret::Unit))
            }
            S::MapDevice { h } => done(self.map_device(pid, *h).map(Ret::Addr)),
            S::DmaAlloc { h, npages } => {
                done(self.dma_alloc(pid, *h, *npages).map(|(addr, phys)| Ret::AddrPhys { addr, phys }))
            }
            S::ThreadCreate { entry, sp, arg } => {
                done(self.thread_create(pid, *entry, *sp, *arg).map(Ret::Tid))
            }
            S::ThreadExit => {
                self.thread_exit(pid, tid);
                Outcome::Gone
            }
            S::ProcessExit { code } => match self.process_exit(pid, *code) {
                Ok(()) => Outcome::Gone,
                Err(e) => done(Err(e)),
            },
            S::ProcessCreate { budget, exit_endpoint } => {
                done(self.process_create(pid, *budget, *exit_endpoint).map(Ret::Handle))
            }
            S::ProcessMap { process, src, dst, len, flags } => {
                done(self.process_map(pid, *process, *src, *dst, *len, *flags).map(|_| Ret::Unit))
            }
            S::ProcessStart { process, entry, sp, handles } => {
                done(self.process_start(pid, *process, *entry, *sp, handles).map(|_| Ret::Unit))
            }
            S::EndpointCreate => done(self.endpoint_create(pid).map(Ret::Handle)),
            S::Mint { source, badge, budget } => {
                done(self.mint(pid, tid, *source, *badge, *budget).map(Ret::Handle))
            }
            S::Call { h, words, handles, lend, timeout } => {
                self.call(pid, tid, *h, *words, handles, *lend, *timeout)
            }
            S::Send { h, words, handles, transfer, timeout } => {
                self.send(pid, tid, *h, *words, handles, *transfer, *timeout)
            }
            S::Receive { h, timeout, max_transfer } => self.receive(pid, tid, *h, *timeout, *max_transfer),
            S::Reply { msg_id, words, handles } => {
                done(self.reply(pid, tid, *msg_id, *words, handles).map(|_| Ret::Unit))
            }
            S::HandleClose { h } => done(self.handle_close(pid, *h).map(|_| Ret::Unit)),
            S::BudgetCreate { parent, pages, processes, weight, class, labels, account, deadline } => done(
                self.budget_create(
                    pid, *parent, *pages, *processes, *weight, *class, labels, *account, *deadline,
                )
                .map(Ret::Handle),
            ),
            S::BudgetDestroy { h } => match self.budget_destroy(pid, *h) {
                Ok(()) if !self.threads.contains_key(&tid) => Outcome::Gone,
                r => done(r.map(|_| Ret::Unit)),
            },
            S::BudgetUsage { h } => done(self.budget_usage(pid, *h).map(Ret::Usage)),
            S::TimeNow => done(Ok(Ret::Time(self.time_now()))),
            S::Random { len } => done(self.random(*len).map(|len| Ret::Random { len })),
            S::SystemReset { h, kind } => done(self.system_reset(pid, *h, *kind).map(|_| Ret::Unit)),
        }
    }

    // ---------------------------------------------------------------------------------------
    // The system calls, in KERNEL-SPEC.md's table order.

    /// `map_anon(len, flags) -> addr`: pages charged; not W+X; zeroed.
    pub fn map_anon(&mut self, pid: u64, len: u64, flags: u64) -> R<u64> {
        decode_flags(flags)?;
        if len == 0 || !len.is_multiple_of(PAGE_SIZE) {
            return Err(Error::InvalidArgument);
        }
        check_flags(flags)?;
        if is_wx(flags) {
            return Err(Error::NotPermitted);
        }
        let n = len / PAGE_SIZE;
        let b = self.budget_of(pid).ok_or(Error::Dead)?;
        self.charge(b, n)?;
        let start = match self.alloc_va(pid, n) {
            Ok(s) => s,
            Err(e) => {
                self.uncharge(b, n);
                return Err(e);
            }
        };
        for i in 0..n {
            let f = self.alloc_frame(b);
            self.processes
                .get_mut(&pid)
                .unwrap()
                .space
                .insert(start + i, Mapping { backing: Backing::Frame(f), flags, state: MapState::Own });
        }
        Ok(start * PAGE_SIZE)
    }

    /// `unmap(addr, len)`: own mapping; not currently lent.
    pub fn unmap(&mut self, pid: u64, addr: u64, len: u64) -> R<()> {
        let (first, n) = user_range(addr, len)?;
        self.own_range(pid, first, n, |_| true)?;
        for v in first..first + n {
            let m = self.processes.get_mut(&pid).unwrap().space.remove(&v).unwrap();
            if let Backing::Frame(f) = m.backing {
                self.free_frame(f);
            }
        }
        Ok(())
    }

    /// `set_flags(addr, len, flags)`: own mapping; not W+X.
    pub fn set_flags(&mut self, pid: u64, addr: u64, len: u64, flags: u64) -> R<()> {
        decode_flags(flags)?;
        let (first, n) = user_range(addr, len)?;
        check_flags(flags)?;
        if is_wx(flags) && !self.broken(Mutation::R11SetFlagsAllowsWx) {
            return Err(Error::NotPermitted);
        }
        self.own_range(pid, first, n, |_| true)?;
        let p = self.processes.get_mut(&pid).unwrap();
        for v in first..first + n {
            p.space.get_mut(&v).unwrap().flags = flags;
        }
        Ok(())
    }

    /// `map_device(h(MMIO)) -> addr`: MMIO device handle. The range is mapped read-write. Device
    /// pages are not RAM and cost nothing.
    pub fn map_device(&mut self, pid: u64, h: u64) -> R<u64> {
        let h = decode_handle(h)?;
        let Object::Device(d) = self.lookup(pid, h)?.object else { return Err(Error::WrongObject) };
        let DeviceKind::Mmio { pages, .. } = self.devices[&d].kind else { return Err(Error::WrongObject) };
        let start = self.alloc_va(pid, pages)?;
        let p = self.processes.get_mut(&pid).unwrap();
        for i in 0..pages {
            p.space.insert(
                start + i,
                Mapping {
                    backing: Backing::Device { device: d, page: i },
                    flags: FLAG_R | FLAG_W,
                    state: MapState::Own,
                },
            );
        }
        Ok(start * PAGE_SIZE)
    }

    /// `dma_alloc(h(MMIO), npages) -> addr, phys`: DMA flag; pages charged; contiguous; zeroed.
    pub fn dma_alloc(&mut self, pid: u64, h: u64, npages: u64) -> R<(u64, u64)> {
        let h = decode_handle(h)?;
        let Object::Device(d) = self.lookup(pid, h)?.object else { return Err(Error::WrongObject) };
        let DeviceKind::Mmio { dma, .. } = self.devices[&d].kind else { return Err(Error::WrongObject) };
        if npages == 0 {
            return Err(Error::InvalidArgument);
        }
        if !dma {
            return Err(Error::NotPermitted);
        }
        let b = self.budget_of(pid).ok_or(Error::Dead)?;
        self.charge(b, npages)?;
        let start = match self.alloc_va(pid, npages) {
            Ok(s) => s,
            Err(e) => {
                self.uncharge(b, npages);
                return Err(e);
            }
        };
        // Contiguous: fresh frames from the top of what was ever used, never the free list.
        let first = self.next_frame;
        self.next_frame += npages;
        for i in 0..npages {
            let f = first + i;
            self.frames.insert(f, Frame { payer: b, content: 0 });
            self.ghost.fresh.insert(f);
            self.processes.get_mut(&pid).unwrap().space.insert(
                start + i,
                Mapping { backing: Backing::Frame(f), flags: FLAG_R | FLAG_W, state: MapState::Own },
            );
        }
        Ok((start * PAGE_SIZE, RAM_BASE + first * PAGE_SIZE))
    }

    /// `thread_create(entry, sp, arg) -> tid`: pages charged; fewer than `MAX_THREADS`.
    pub fn thread_create(&mut self, pid: u64, _entry: u64, _sp: u64, _arg: u64) -> R<u64> {
        let p = self.processes.get(&pid).ok_or(Error::Dead)?;
        if p.threads.len() as u64 >= MAX_THREADS {
            return Err(Error::TooManyThreads);
        }
        let b = p.budget;
        self.charge(b, self.costs.thread)?;
        Ok(self.new_thread(pid))
    }

    /// `thread_exit`. The last thread's exit ends the process as `process_exit(0)` would.
    pub fn thread_exit(&mut self, pid: u64, tid: u64) {
        self.end_thread(tid);
        if self.processes.get(&pid).is_some_and(|p| p.threads.is_empty()) {
            self.end_process(pid, Cause::Exited, 0, 0);
        }
    }

    /// `process_exit(code)`: exit notice `exited`.
    pub fn process_exit(&mut self, pid: u64, code: u64) -> R<()> {
        if code > U32_MAX {
            return Err(Error::InvalidArgument);
        }
        self.end_process(pid, Cause::Exited, code, 0);
        Ok(())
    }

    /// `process_create(h(budget), h(exit endpoint)) -> h(process)`: budget's process and page
    /// limits. The new handle is stamped with the caller's budget (R9).
    pub fn process_create(&mut self, pid: u64, budget: u64, exit_endpoint: u64) -> R<u64> {
        let budget = decode_handle(budget)?;
        let exit_endpoint = decode_handle(exit_endpoint)?;
        let b = self.lookup_budget(pid, budget)?;
        let (_, exit) = self.lookup_endpoint(pid, exit_endpoint)?;
        let bx = &self.budgets[&b];
        if bx.processes_used >= bx.processes_limit {
            return Err(Error::OutOfProcesses);
        }
        self.charge(b, self.costs.process)?;
        let caller_budget = self.budget_of(pid).unwrap();
        let child = self.next_pid;
        let h = Handle {
            object: Object::Process(child),
            badge: 0,
            stamp: caller_budget,
            origin: Origin::Created { by: caller_budget },
        };
        // Reserve the pid first so the handle names a live object; undone on failure.
        self.processes.insert(
            child,
            Process {
                pid: child,
                budget: b,
                started: false,
                threads: BTreeSet::new(),
                handles: BTreeMap::new(),
                space: BTreeMap::new(),
                exit_endpoint: Some(exit),
            },
        );
        match self.install(pid, &[h]) {
            Ok(v) => {
                self.next_pid += 1;
                self.budgets.get_mut(&b).unwrap().processes_used += 1;
                self.notes.push(Note::Process { creator: pid, h: v[0], pid: child });
                Ok(v[0])
            }
            Err(e) => {
                self.processes.remove(&child);
                self.uncharge(b, self.costs.process);
                Err(e)
            }
        }
    }

    /// `process_map(h(process), src, dst, len, flags)`: process not started; src owned by the
    /// caller; pages move to the child's budget; not W+X.
    pub fn process_map(&mut self, pid: u64, process: u64, src: u64, dst: u64, len: u64, flags: u64) -> R<()> {
        let process = decode_handle(process)?;
        decode_flags(flags)?;
        let child = self.lookup_process(pid, process)?;
        let (s, n) = user_range(src, len)?;
        let (d, _) = user_range(dst, len)?;
        check_flags(flags)?;
        if is_wx(flags) {
            return Err(Error::NotPermitted);
        }
        if self.processes[&child].started {
            return Err(Error::NotPermitted);
        }
        self.own_range(pid, s, n, |m| matches!(m.backing, Backing::Frame(_)))?;
        let cp = &self.processes[&child];
        if (d..d + n).any(|v| cp.space.contains_key(&v)) {
            return Err(Error::InvalidArgument);
        }
        let (from, to) = (self.budget_of(pid).unwrap(), cp.budget);
        if from != to {
            self.charge(to, n)?;
            self.uncharge(from, n);
        }
        for i in 0..n {
            let m = self.processes.get_mut(&pid).unwrap().space.remove(&(s + i)).unwrap();
            if let Backing::Frame(f) = m.backing {
                self.frames.get_mut(&f).unwrap().payer = to;
            }
            self.processes.get_mut(&child).unwrap().space.insert(d + i, Mapping { flags, ..m });
        }
        Ok(())
    }

    /// `process_start(h(process), entry, sp, handles)`: not started; handles copied into slots
    /// 1..n. The child's table and first thread are charged to the child's budget. The list has
    /// no length cap: the child's page limit bounds it (README.md).
    pub fn process_start(&mut self, pid: u64, process: u64, _entry: u64, _sp: u64, handles: &[u64]) -> R<()> {
        let process = decode_handle(process)?;
        for h in handles {
            decode_handle(*h)?;
        }
        let child = self.lookup_process(pid, process)?;
        let mut hs = Vec::new();
        for h in handles {
            hs.push(self.lookup(pid, *h)?);
        }
        if self.processes[&child].started {
            return Err(Error::NotPermitted);
        }
        let b = self.processes[&child].budget;
        self.charge(b, self.costs.thread)?;
        if let Err(e) = self.install(child, &hs) {
            self.uncharge(b, self.costs.thread);
            return Err(e);
        }
        self.processes.get_mut(&child).unwrap().started = true;
        let tid = self.new_thread(child);
        self.notes.push(Note::Thread { pid: child, tid });
        Ok(())
    }

    /// `endpoint_create() -> h (badge 0)`: pages charged. Stamped with the caller's budget (R9).
    pub fn endpoint_create(&mut self, pid: u64) -> R<u64> {
        let b = self.budget_of(pid).ok_or(Error::Dead)?;
        let cost = if self.broken(Mutation::R6EndpointsFree) { 0 } else { self.costs.endpoint };
        self.charge(b, cost)?;
        let id = self.next_endpoint;
        let h =
            Handle { object: Object::Endpoint(id), badge: 0, stamp: b, origin: Origin::Created { by: b } };
        self.endpoints.insert(
            id,
            Endpoint {
                id,
                owner: b,
                queue: BTreeMap::new(),
                cursor: None,
                receivers: VecDeque::new(),
                exits: VecDeque::new(),
            },
        );
        match self.install(pid, &[h]) {
            Ok(v) => {
                self.next_endpoint += 1;
                Ok(v[0])
            }
            Err(e) => {
                self.endpoints.remove(&id);
                self.uncharge(b, cost);
                Err(e)
            }
        }
    }

    /// `mint(source, badge, budget?) -> h`: a handle to an endpoint with `badge != 0`.
    pub fn mint(
        &mut self,
        pid: u64,
        tid: u64,
        source: MintSource,
        badge: u64,
        budget: Option<u64>,
    ) -> R<u64> {
        if let MintSource::Handle(h) = source {
            decode_handle(h)?;
        }
        if let Some(b) = budget {
            decode_handle(b)?;
        }
        // The endpoint and the default stamp.
        let (e, default) = match source {
            MintSource::Message(m) => {
                let Some(msg) = self.msgs.get(&m).filter(|_| self.threads[&tid].serving == Some(m)) else {
                    return Err(Error::InvalidArgument);
                };
                if !self.endpoints.contains_key(&msg.endpoint) || !self.budgets.contains_key(&msg.stamp) {
                    return Err(Error::Dead);
                }
                (msg.endpoint, msg.stamp)
            }
            MintSource::Handle(h) => {
                let (e, hd) = self.lookup_endpoint(pid, h)?;
                if hd.badge != 0 {
                    return Err(Error::NotPermitted);
                }
                (e, hd.stamp)
            }
        };
        if badge == 0 {
            return Err(Error::InvalidArgument);
        }
        let mut stamp = default;
        if let Some(bh) = budget {
            let b = self.lookup_budget(pid, bh)?;
            // A budget handle only narrows: the default stamp or a descendant of it.
            if !self.is_descendant_or_self(b, default) {
                return Err(Error::NotPermitted);
            }
            stamp = b;
        }
        if self.broken(Mutation::R9MintStampsCaller) {
            stamp = self.budget_of(pid).unwrap();
        }
        let h = Handle {
            object: Object::Endpoint(e),
            badge,
            stamp,
            origin: Origin::Minted { default_stamp: default },
        };
        Ok(self.install(pid, &[h])?[0])
    }

    /// Checks shared by `call` and `send` up to queueing: the endpoint, the handles, the buffer,
    /// then R2's cap. Returns the endpoint, the handle used, the copied handles and the buffer
    /// range.
    #[allow(clippy::type_complexity)]
    fn check_message(
        &self,
        pid: u64,
        h: u64,
        handles: &[u64],
        buffer: Option<Buffer>,
        lend: bool,
    ) -> R<(u64, Handle, Vec<Handle>, Option<(u64, u64)>)> {
        let h = decode_handle(h)?;
        if handles.len() > MAX_MSG_HANDLES {
            return Err(Error::TooLarge);
        }
        for x in handles {
            decode_handle(*x)?;
        }
        let (e, hd) = self.lookup_endpoint(pid, h)?;
        let mut hs = Vec::new();
        for x in handles {
            hs.push(self.lookup(pid, *x)?);
        }
        let range = match buffer {
            None => None,
            Some(b) => {
                if lend && b.npages > MAX_LEND_PAGES {
                    return Err(Error::TooLarge);
                }
                let (first, n) = buffer_range(b)?;
                // A lend must be writable; a transfer may be any RAM the caller owns.
                self.own_range(pid, first, n, |m| {
                    matches!(m.backing, Backing::Frame(_)) && (!lend || m.flags & FLAG_W != 0)
                })?;
                Some((first, n))
            }
        };
        let account = self.budgets[&self.budget_of(pid).unwrap()].account;
        let waiting = self.endpoints[&e].queue.get(&account).map_or(0, |q| q.len() as u64);
        if waiting >= WAIT_CAP && !self.broken(Mutation::R2NoWaitCap) {
            return Err(Error::Busy);
        }
        Ok((e, hd, hs, range))
    }

    /// Queue a message on its endpoint, block the sender, and try to deliver.
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        pid: u64,
        tid: u64,
        kind: MsgKind,
        e: u64,
        hd: Handle,
        words: [u64; WORDS],
        hs: Vec<Handle>,
        range: Option<(u64, u64)>,
        timeout: u64,
    ) -> Outcome {
        let b = self.budget_of(pid).unwrap();
        let (account, labels, class) = {
            let bx = &self.budgets[&b];
            (bx.account, bx.labels.clone(), bx.class)
        };
        let id = self.next_msg; // ids ascend, so the oldest queued message is the lowest id
        self.next_msg += 1;
        let buffer = range.map(|(first, n)| {
            let p = self.processes.get_mut(&pid).unwrap();
            let mut frames = Vec::new();
            for v in first..first + n {
                let m = p.space.get_mut(&v).unwrap();
                if let Backing::Frame(f) = m.backing {
                    frames.push(f);
                }
                // I9: a lent page is unmapped from its lender until the call ends.
                if !(kind == MsgKind::Call && self.mutation == Some(Mutation::R11LendStaysMapped)) {
                    m.state = MapState::LentOut(id);
                }
            }
            InFlight { sender_vpn: first, frames, receiver_vpn: None }
        });
        self.msgs.insert(
            id,
            Msg {
                id,
                kind,
                sender_pid: pid,
                sender_tid: tid,
                sender_budget: b,
                endpoint: e,
                badge: hd.badge,
                stamp: hd.stamp,
                account,
                labels,
                class,
                words,
                handles: hs,
                lent_pages: if kind == MsgKind::Call { range.map_or(0, |r| r.1) } else { 0 },
                buffer,
                server: None,
                caller_waiting: kind == MsgKind::Call,
            },
        );
        self.endpoints.get_mut(&e).unwrap().queue.entry(account).or_default().push_back(id);
        self.block(tid, Wait::Send(id), timeout);
        self.pump(e);
        Outcome::Blocked
    }

    /// `call(h, words, handles, lend, timeout) -> reply`: endpoint; R1; R2; lend rules.
    #[allow(clippy::too_many_arguments)]
    pub fn call(
        &mut self,
        pid: u64,
        tid: u64,
        h: u64,
        words: [u64; WORDS],
        handles: &[u64],
        lend: Option<Buffer>,
        timeout: u64,
    ) -> Outcome {
        match self.check_message(pid, h, handles, lend, true) {
            Err(e) => Outcome::Done(Err(e)),
            Ok((e, hd, hs, range)) => self.enqueue(pid, tid, MsgKind::Call, e, hd, words, hs, range, timeout),
        }
    }

    /// `send(h, words, handles, transfer, timeout)`: endpoint; R1; R2; rendezvous.
    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &mut self,
        pid: u64,
        tid: u64,
        h: u64,
        words: [u64; WORDS],
        handles: &[u64],
        transfer: Option<Buffer>,
        timeout: u64,
    ) -> Outcome {
        match self.check_message(pid, h, handles, transfer, false) {
            Err(e) => Outcome::Done(Err(e)),
            Ok((e, hd, hs, range)) => self.enqueue(pid, tid, MsgKind::Send, e, hd, words, hs, range, timeout),
        }
    }

    /// `receive(h or none, timeout, max_transfer)`: badge-0 endpoint, IRQ, or none (sleep).
    pub fn receive(
        &mut self,
        pid: u64,
        tid: u64,
        h: Option<u64>,
        timeout: u64,
        max_transfer: u64,
    ) -> Outcome {
        let Some(h) = h else {
            self.block(tid, Wait::Sleep, timeout);
            return Outcome::Blocked;
        };
        let h = match decode_handle(h) {
            Ok(h) => h,
            Err(e) => return Outcome::Done(Err(e)),
        };
        let hd = match self.lookup(pid, h) {
            Ok(x) => x,
            Err(e) => return Outcome::Done(Err(e)),
        };
        match hd.object {
            Object::Endpoint(e) => {
                // Only a badge-0 handle is a receive right.
                if hd.badge != 0 {
                    return Outcome::Done(Err(Error::NotPermitted));
                }
                // A thread serves one call at a time (README.md).
                let serving_call = self.threads[&tid]
                    .serving
                    .and_then(|m| self.msgs.get(&m))
                    .is_some_and(|m| m.kind == MsgKind::Call);
                if serving_call {
                    return Outcome::Done(Err(Error::Busy));
                }
                self.block(tid, Wait::Receive { endpoint: e, h, max_transfer }, timeout);
                self.endpoints.get_mut(&e).unwrap().receivers.push_back(tid);
                self.pump(e);
                Outcome::Blocked
            }
            Object::Device(d) if matches!(self.devices[&d].kind, DeviceKind::Irq { .. }) => {
                // R5: unmask when the receive begins; return when `fired` is set, clearing it.
                self.ghost.irq_receive_begins(d);
                if !self.broken(Mutation::R5NoUnmaskOnReceive) {
                    if let DeviceKind::Irq { masked, .. } = &mut self.devices.get_mut(&d).unwrap().kind {
                        *masked = false;
                    }
                }
                self.fire_if_unmasked(d);
                if let DeviceKind::Irq { fired, .. } = &mut self.devices.get_mut(&d).unwrap().kind {
                    if *fired {
                        *fired = false;
                        self.ghost.irq_delivered(d);
                        return Outcome::Done(Ok(Ret::Interrupt { h }));
                    }
                }
                self.ghost.irq_not_delivered(d);
                self.block(tid, Wait::Irq { device: d, h }, timeout);
                self.devices.get_mut(&d).unwrap().waiters.push_back(tid);
                Outcome::Blocked
            }
            _ => Outcome::Done(Err(Error::WrongObject)),
        }
    }

    /// `reply(msg_id, words, handles)`: caller is serving `msg_id`; returns the lend.
    pub fn reply(&mut self, pid: u64, tid: u64, msg_id: u64, words: [u64; WORDS], handles: &[u64]) -> R<()> {
        if handles.len() > MAX_MSG_HANDLES {
            return Err(Error::TooLarge);
        }
        for x in handles {
            decode_handle(*x)?;
        }
        if self.threads[&tid].serving != Some(msg_id) || !self.msgs.contains_key(&msg_id) {
            return Err(Error::InvalidArgument);
        }
        let mut hs = Vec::new();
        for x in handles {
            hs.push(self.lookup(pid, *x)?);
        }
        let t = self.threads.get_mut(&tid).unwrap();
        t.serving = None;
        t.account = 0;
        let m = self.msgs[&msg_id].clone();
        if m.kind == MsgKind::Send {
            // Nothing waits for a reply to a `send`; replying only ends serving it.
            self.msgs.remove(&msg_id);
            return Ok(());
        }
        if !m.caller_waiting {
            // R3: a reply to an abandoned call is discarded, and the lend is freed.
            self.unmap_lend_in_server(msg_id, true);
            self.msgs.remove(&msg_id);
            return Ok(());
        }
        self.unmap_lend_in_server(msg_id, false);
        self.msgs.remove(&msg_id);
        // The caller pays for the handles it receives; if it cannot, its call fails with
        // `OutOfMemory` (the server's reply still succeeds: it is not the server's budget).
        let result = match self.install(m.sender_pid, &hs) {
            Ok(installed) => Ok(Ret::Reply { words, handles: installed }),
            Err(e) => Err(e),
        };
        self.wake(m.sender_tid, result);
        Ok(())
    }

    /// `handle_close(h)`.
    pub fn handle_close(&mut self, pid: u64, h: u64) -> R<()> {
        let h = decode_handle(h)?;
        self.lookup(pid, h)?;
        self.remove_handle(pid, h);
        Ok(())
    }

    /// `budget_create(h(parent), pages, processes, weight, class, labels, account, deadline) -> h`:
    /// R6-R8; class and labels rules; depth < `MAX_DEPTH`.
    #[allow(clippy::too_many_arguments)]
    pub fn budget_create(
        &mut self,
        pid: u64,
        parent: u64,
        pages: u64,
        processes: u64,
        weight: u64,
        class: u64,
        labels: &[u64],
        account: u64,
        deadline: u64,
    ) -> R<u64> {
        // Decoding (redoubt-sys BudgetSpec: processes and weight are u32, class is a tag, the
        // label list holds at most MAX_LABELS).
        let parent = decode_handle(parent)?;
        if processes > U32_MAX || weight > U32_MAX {
            return Err(Error::InvalidArgument);
        }
        let class = Class::from_raw(class).ok_or(Error::InvalidArgument)?;
        if labels.len() > MAX_LABELS {
            return Err(Error::TooLarge);
        }
        let p = self.lookup_budget(pid, parent)?;
        let px = self.budgets[&p].clone();
        let caller_budget = self.budget_of(pid).unwrap();
        let caller_class = self.budgets[&caller_budget].class;
        // Class `system` only if the parent is `system`.
        if class > px.class {
            return Err(Error::ClassDenied);
        }
        // Labels are sorted and deduplicated, a superset of the parent's; adding labels needs
        // the caller's own budget to be class `system`.
        let mut labels = labels.to_vec();
        labels.sort_unstable();
        labels.dedup();
        if !superset(&labels, &px.labels) {
            return Err(Error::LabelDenied);
        }
        if labels != px.labels && caller_class != Class::System {
            return Err(Error::ClassDenied);
        }
        if px.depth + 1 >= MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        // R6/R7: carve from the parent's free limits.
        let scope = pages == 0 && processes == 0 && weight == 0;
        let carve_check = !self.broken(Mutation::R7NoCarveCheck);
        if scope {
            if self.free_pages(p) < self.costs.budget {
                return Err(Error::OutOfMemory);
            }
        } else {
            if carve_check && pages > self.free_pages(p) {
                return Err(Error::OutOfMemory);
            }
            if carve_check && processes > px.processes_limit.saturating_sub(px.processes_used) {
                return Err(Error::OutOfProcesses);
            }
            if carve_check && weight > px.weight.saturating_sub(px.weight_used) {
                return Err(Error::InvalidArgument);
            }
            // The budget's own object is charged to itself.
            if pages < self.costs.budget {
                return Err(Error::OutOfMemory);
            }
        }
        // R8: the parent's account, unless it is 0.
        let account = if px.account != 0 && !self.broken(Mutation::R8AccountFromArgument) {
            px.account
        } else {
            account
        };
        let deadline = if deadline == FOREVER { None } else { Some(deadline) };
        let id = self.new_budget(
            Some(p),
            class,
            labels,
            account,
            deadline,
            pages,
            processes,
            weight,
            caller_class,
        );
        let h = Handle {
            object: Object::Budget(id),
            badge: 0,
            stamp: caller_budget,
            origin: Origin::Created { by: caller_budget },
        };
        // The caller's table must have room for the handle (its growth is charged to the caller,
        // with the carve applied in case the caller's budget is the parent); otherwise undo.
        match self.install(pid, &[h]) {
            Ok(v) => Ok(v[0]),
            Err(e) => {
                self.destroy_budget(id);
                Err(e)
            }
        }
    }

    /// `budget_destroy(h(budget))`: always allowed to a holder.
    pub fn budget_destroy(&mut self, pid: u64, h: u64) -> R<()> {
        let h = decode_handle(h)?;
        let b = self.lookup_budget(pid, h)?;
        self.destroy_budget(b);
        Ok(())
    }

    /// `budget_usage(h(budget)) -> counters`: caller's labels ⊇ target's.
    pub fn budget_usage(&mut self, pid: u64, h: u64) -> R<Counters> {
        let h = decode_handle(h)?;
        let b = self.lookup_budget(pid, h)?;
        let caller = &self.budgets[&self.budget_of(pid).unwrap()];
        let target = &self.budgets[&b];
        if !superset(&caller.labels, &target.labels) {
            return Err(Error::LabelDenied);
        }
        self.ghost.flows.push(Flow::Usage { from: target.labels.clone(), to: caller.labels.clone() });
        Ok(Counters {
            pages_limit: target.pages_limit,
            pages_used: target.pages_used,
            processes_limit: target.processes_limit,
            processes_used: target.processes_used,
        })
    }

    /// `time_now() -> µs`.
    pub fn time_now(&self) -> u64 {
        self.now
    }

    /// `system_reset(h(Reset), kind)`: Reset device handle. The machine stops.
    pub fn system_reset(&mut self, pid: u64, h: u64, kind: u64) -> R<()> {
        let h = decode_handle(h)?;
        if kind != RESET_POWER_OFF && kind != RESET_REBOOT {
            return Err(Error::InvalidArgument);
        }
        let Object::Device(d) = self.lookup(pid, h)?.object else { return Err(Error::WrongObject) };
        if self.devices[&d].kind != DeviceKind::Reset {
            return Err(Error::WrongObject);
        }
        self.halted = Some(kind);
        Ok(())
    }

    /// `random(len) -> bytes`: `len` at most 64.
    pub fn random(&self, len: u64) -> R<u64> {
        if len > RANDOM_MAX_LEN { Err(Error::TooLarge) } else { Ok(len) }
    }
}
