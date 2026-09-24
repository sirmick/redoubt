//! The kernel model: KERNEL-SPEC.md's four kinds of object, its system calls and rules R1-R12,
//! reconciled to the accepted contracts in docs/KERNEL-SPEC.md.
//!
//! Read it next to the spec. Each system call is one method with the spec's name, and each check
//! in it cites the rule or table row it implements. Checks run in one fixed order, which the
//! trace format relies on (README.md, "Order of checks"):
//! 1. **decoding**, as `redoubt-sys` decodes: the call's registers in order, then the record it points to
//!    (message body, budget spec, handle list): a handle value that cannot be an index is `BadHandle`, a list
//!    longer than its array is `TooLarge`, any other malformed encoding (unknown flag bits, W+X, an unknown
//!    tag, badge 0, a 32-bit field over 32 bits, a page range with exactly one of address and count zero) is
//!    `InvalidArgument`;
//! 2. then the kernel's checks, argument by argument from left to right (does the handle exist, is it the
//!    right object, is the range valid);
//! 3. then permission checks (`NotPermitted`, `ClassDenied`, `LabelDenied`);
//! 4. then resources (`OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`).
//!
//! Abstractions (what the model does not represent, on purpose): physical memory is a set of
//! page frames with one abstract word of content each; a thread has no registers; time is
//! logical and advances only in [`Op::Tick`]. System calls are instantaneous and made by whichever
//! runnable thread the trace names; the scheduler decides only how CPU time is shared during
//! ticks (R12).
//!
//! `mutation` switches on one deliberate rule break (mutation.rs); `self.broken(..)` marks each
//! place. With `mutation == None` this is the specified kernel. Where the spec left a choice open,
//! the site says `(README choice N)`; where a site follows an owner's answer, `(QUESTIONS N)`.

use alloc::collections::{BTreeMap, BTreeSet, VecDeque};
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::ghost::{Blame, Flow, Ghost, Key, Owed, Receiving, Sent, Slot};
use crate::mutation::Mutation;
use crate::sched::Scheduler;
use crate::spec::*;
pub use crate::syscall::MsgKind;
use crate::syscall::*;

/// Pages each kind of kernel object costs (KERNEL-SPEC.md, "What objects cost"; R6). A
/// conformance run writes these into the trace and must use the real kernel's values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Costs {
    /// A budget's own page, charged to its parent (QUESTIONS 76).
    pub budget: u64,
    /// A process object, charged to its creator's budget; it holds the exit notice (QUESTIONS 74).
    pub process: u64,
    /// Saved process register contexts, separate from the process/notice object (answer 127).
    pub contexts: u64,
    /// Per-thread IPC state.
    pub thread: u64,
    /// An endpoint.
    pub endpoint: u64,
    /// A handle table costs one page per this many live handles (rounded up).
    pub handles_per_page: u64,
    /// One page-table page (the root is allocated with the process; README choice 20).
    pub page_table: u64,
    /// One open call, charged to the receiving process's budget (QUESTIONS 2).
    pub open_call: u64,
}

impl Default for Costs {
    /// The rv64 cost table: two saved-context pages, one per other object, 128 handles per table page.
    fn default() -> Costs {
        Costs {
            budget: 1,
            process: 1,
            contexts: 2,
            thread: 1,
            endpoint: 1,
            handles_per_page: 128,
            page_table: 1,
            open_call: 1,
        }
    }
}

/// The most any one object may cost in a boot configuration, and the most handles a page may
/// hold: a hostile trace's `costs` line must not make the kernel's sums overflow.
pub const MAX_COST: u64 = 1 << 16;

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
    /// `users`, carved from `root`. What `root` keeps is `init`'s own (README choice 19).
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

impl Boot {
    /// The boot the property tests use: the default, with 8 handles per table page so that
    /// handle-table growth (and its charge) happens in short sequences.
    pub fn testing() -> Boot {
        Boot { costs: Costs { handles_per_page: 8, ..Costs::default() }, ..Boot::default() }
    }
}

/// Refuse a `Boot` the kernel could not start: `system` and `users`, with their own pages, must fit
/// in `root` with room for `init` (its process object, root page table, thread and handle table),
/// and devices must be well formed.
fn check_boot(b: &Boot) -> Result<(), String> {
    use alloc::format;
    let c = b.costs;
    let each = [c.budget, c.process, c.contexts, c.thread, c.endpoint, c.page_table, c.open_call];
    if each.iter().any(|x| *x > MAX_COST) {
        return Err(format!("boot: an object may cost at most {MAX_COST} pages"));
    }
    if c.handles_per_page == 0 || c.handles_per_page > MAX_COST {
        return Err(format!("boot: handles per page must be 1..={MAX_COST}"));
    }
    if b.devices.len() > 64 {
        return Err("boot: at most 64 devices".into());
    }
    let mut irqs = BTreeSet::new();
    for d in &b.devices {
        match *d {
            DeviceSpec::Mmio { base, pages, .. } => {
                if pages == 0 || pages > 1 << 20 || !base.is_multiple_of(PAGE_SIZE) {
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
    let pages = [
        c.budget,
        b.system.pages,
        c.budget,
        b.users.pages,
        c.process,
        c.contexts,
        c.page_table,
        c.thread,
        table,
    ]
    .iter()
    .try_fold(0u64, |acc, x| acc.checked_add(*x));
    if pages.is_none_or(|p| p > b.root.pages) {
        return Err("boot: system, users and init do not fit in root's pages".into());
    }
    let procs = b.system.processes.checked_add(b.users.processes).and_then(|x| x.checked_add(1));
    if procs.is_none_or(|p| p > b.root.processes) {
        return Err("boot: system, users and init do not fit in root's processes".into());
    }
    let weight = b.system.weight.checked_add(b.users.weight);
    if weight.is_none_or(|w| w >= b.root.weight) || b.root.weight > U32_MAX {
        return Err("boot: system and users must leave root (init) some weight".into());
    }
    Ok(())
}

/// Where user virtual addresses end (Sv39's lower half). `process_map`'s `dst` must lie below.
/// User space has no lower bound: page 0 is in it (MEMORY-LAYOUT.md, Decision 2; K5a-addr0).
pub const USER_TOP: u64 = 1 << 38;
/// Where the kernel places the mappings it chooses addresses for (`map_anon`, received buffers):
/// above everything the process has mapped, from here (README choice 20).
pub const KERNEL_CHOSEN_BASE: u64 = 0x10_0000_0000;
/// Physical address of frame 0; `dma_alloc` returns physical addresses from here.
pub const RAM_BASE: u64 = 0x8000_0000;
/// The longest `Op::Tick` the model accepts (one hour): a replay of hostile input must finish.
pub const MAX_TICK: u64 = 3_600_000_000;

/// The pid of `init`, the one process the kernel creates (README choice 27: fixed, not drawn).
pub const INIT_PID: u64 = 1;
/// PIDs are ASIDs: Sv39's 16 bits (`init` has 1; the others are drawn from 2..=MAX_PID).
pub const MAX_PID: u64 = 0xffff;
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
    /// Inherited from the parent (QUESTIONS 73).
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

/// Sv39 page-table pages below the root that map virtual page `vpn`: its middle table (one per
/// GiB) and its leaf table (one per 2 MiB).
fn table_keys(vpn: u64) -> [(u8, u64); 2] { [(1, vpn >> 18), (0, vpn >> 9)] }

#[derive(Clone, Debug)]
pub struct Process {
    pub pid: u64,
    pub budget: u64,
    pub started: bool,
    pub threads: BTreeSet<u64>,
    /// The handle table: index -> handle. Index 0 is never used (QUESTIONS 10).
    pub handles: BTreeMap<u64, Handle>,
    /// Address space: virtual page number -> mapping.
    pub space: BTreeMap<u64, Mapping>,
    /// Page-table pages below the root: (level, key) -> pages of `space` under it.
    pub tables: BTreeMap<(u8, u64), u64>,
    /// The exit endpoint handle named at `process_create` (none for `init`).
    pub exit_endpoint: Option<Handle>,
    /// The budget the process object is charged to: its creator's (QUESTIONS 74). The object, and
    /// the charge, outlive the process until its exit notice is received or dropped.
    pub creator: u64,
    /// The next message id this process's threads will receive (QUESTIONS 88).
    pub next_msg_id: u64,
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
    /// Its open calls (kernel message records), in the order it took them. A `send` is never an
    /// open call (QUESTIONS 31).
    pub serving: Vec<u64>,
    /// Its current call: the open call it is working on, or none (QUESTIONS 82). A fault blames it.
    pub current: Option<u64>,
    pub record: Record,
    /// Present only while executing call; None disposition represents a call without a lend.
    pub call_lend: Option<LendDisposition>,
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

/// A message from the moment it is sent until it is finished with (a `call`'s until its reply, a
/// `send`'s until it is delivered). `id` is the kernel's own name for it; the receiver sees `rid`.
#[derive(Clone, Debug)]
pub struct Msg {
    pub id: u64,
    /// The message id the receiving process sees, unique within it (QUESTIONS 88); 0 while
    /// queued.
    pub rid: u64,
    pub kind: MsgKind,
    pub sender_pid: u64,
    pub sender_tid: u64,
    pub sender_budget: u64,
    pub endpoint: u64,
    /// Attached by the kernel: the badge and stamp of the handle it was sent through, and the
    /// sender budget's account and labels.
    pub badge: u64,
    pub stamp: u64,
    pub account: u64,
    pub labels: Vec<u64>,
    /// The R2 group it is queued under (QUESTIONS 17).
    pub key: Key,
    pub words: [u64; WORDS],
    /// Copies of the handles it carries until it is delivered; one revoked meanwhile is `None`,
    /// and arrives as 0 (R10; QUESTIONS 86).
    pub handles: Vec<Option<Handle>>,
    pub buffer: Option<InFlight>,
    /// `(pid, tid)` of the thread that took it; `None` while queued.
    pub server: Option<(u64, u64)>,
    /// A `call` whose caller still waits for the reply.
    pub caller_waiting: bool,
    /// The budget charged for this open call (QUESTIONS 2), once taken; it pays for the lend too
    /// (R3).
    pub open_payer: Option<u64>,
    /// The call was abandoned (R3); `notice` while its abandoned-call notice waits (QUESTIONS 81).
    pub abandoned: bool,
    pub notice: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExitNotice {
    pub pid: u64,
    pub cause: Cause,
    pub code: u64,
    pub blamed_account: u64,
    /// The labels of the blamed call's sender (QUESTIONS 48).
    pub blamed_labels: Vec<u64>,
    /// The exiting process's budget.
    pub budget: u64,
    /// The budget its process object is charged to (its creator's; QUESTIONS 74).
    pub payer: u64,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub id: u64,
    /// The budget it is charged to (its creator's). R1 compares against it (QUESTIONS 4).
    pub owner: u64,
    /// Blocked senders' messages, grouped as R2 says, oldest first.
    pub queue: BTreeMap<Key, VecDeque<u64>>,
    /// The key served last (R2's round-robin cursor).
    pub cursor: Option<Key>,
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
    /// The state of the generator PIDs are drawn from (a stand-in for the kernel's CSPRNG).
    pid_rng: u64,
    next_tid: u64,
    next_endpoint: u64,
    next_frame: u64,
    next_msg: u64,
    wakes: Vec<Wake>,
    notes: Vec<Note>,
    /// Endpoints where something became deliverable during the step; matched with their
    /// receivers once the step's other effects are done (README choice 26).
    to_pump: BTreeSet<u64>,
}

type R<T> = Result<T, Error>;

fn vpn(addr: u64) -> u64 { addr / PAGE_SIZE }

/// Encoding: a register holding a handle must be an index: not `NO_HANDLE`, at most `u32::MAX`
/// (QUESTIONS 10).
pub fn decode_handle(raw: u64) -> R<u64> {
    if raw == NO_HANDLE || raw > U32_MAX { Err(Error::BadHandle) } else { Ok(raw) }
}

/// Encoding: an optional-handle slot; `NO_HANDLE` (or none) means no handle.
pub fn decode_optional_handle(raw: Option<u64>) -> R<Option<u64>> {
    match raw {
        None => Ok(None),
        Some(NO_HANDLE) => Ok(None),
        Some(x) => decode_handle(x).map(Some),
    }
}

/// Encoding: only the R, W and X bits exist, and W+X is refused (R11; QUESTIONS 15, the
/// decoder refuses it and the kernel's check is this one).
fn decode_flags(flags: u64, allow_wx: bool) -> R<()> {
    let wx = flags & FLAG_W != 0 && flags & FLAG_X != 0;
    if flags & !(FLAG_R | FLAG_W | FLAG_X) != 0 || (wx && !allow_wx) {
        Err(Error::InvalidArgument)
    } else {
        Ok(())
    }
}

/// Encoding: a page range in two registers; (0, 0) is none, and exactly one of them 0 is
/// malformed.
fn decode_range(b: Option<Buffer>) -> R<Option<Buffer>> {
    match b {
        None => Ok(None),
        Some(Buffer { addr: 0, npages: 0 }) => Ok(None),
        Some(Buffer { addr, npages }) if addr == 0 || npages == 0 => Err(Error::InvalidArgument),
        Some(b) => Ok(Some(b)),
    }
}

/// Kernel check of mapping flags: some access, and write only with read (RISC-V has no
/// write-only pages). `allow_write_only` is the R11AllowsWriteOnly mutation.
fn check_flags(flags: u64, allow_write_only: bool) -> R<()> {
    if flags == 0 || (flags & FLAG_W != 0 && flags & FLAG_R == 0 && !allow_write_only) {
        Err(Error::InvalidArgument)
    } else {
        Ok(())
    }
}

/// A page-aligned, non-empty byte range inside user space; returns (first page, page count).
fn user_range(addr: u64, len: u64) -> R<(u64, u64)> {
    if len == 0 || !addr.is_multiple_of(PAGE_SIZE) || !len.is_multiple_of(PAGE_SIZE) {
        return Err(Error::InvalidArgument);
    }
    let end = addr.checked_add(len).ok_or(Error::InvalidArgument)?;
    if end > USER_TOP {
        return Err(Error::InvalidArgument);
    }
    Ok((vpn(addr), len / PAGE_SIZE))
}

/// A lend or transfer range (decoded, so both fields are non-zero): page-aligned, inside user
/// space.
fn buffer_range(b: Buffer) -> R<(u64, u64)> {
    let len = b.npages.checked_mul(PAGE_SIZE).ok_or(Error::InvalidArgument)?;
    user_range(b.addr, len)
}

impl Kernel {
    /// Boot: the kernel creates `root`, `system` and `users` from the argument block and starts
    /// `init` in `root` with handles to all three (slots 1, 2, 3) and to every device (slots 4..).
    ///
    /// A `Boot` that cannot be built (from a hostile trace, say) is an `Err`, never a panic.
    pub fn boot(boot: &Boot, mutation: Option<Mutation>) -> Result<Kernel, String> {
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
            pid_rng: 0x9e37_79b9_7f4a_7c15,
            next_tid: 1,
            next_endpoint: 1,
            next_frame: 0,
            next_msg: 1,
            wakes: Vec::new(),
            notes: Vec::new(),
            to_pump: BTreeSet::new(),
        };
        let c = k.costs;
        let (r, s, u) = (boot.root, boot.system, boot.users);
        let (sys, user) = (Class::System, Class::User);
        // `root` and `system` are class system; all budgets share one stride queue.
        let root = k.new_budget(None, sys, Vec::new(), 0, None, r, sys);
        let system = k.new_budget(Some(root), sys, Vec::new(), 0, None, s, sys);
        let users = k.new_budget(Some(root), user, Vec::new(), 0, None, u, sys);
        if (root, system, users) != (ROOT, SYSTEM, USERS) {
            return Err("boot: budget ids".into());
        }
        // init: its process object is charged to root, as if root had created it (README choice
        // 27), and so are its root page table and thread.
        let pid = INIT_PID;
        k.ghost.slots.insert(pid, Slot { payer: root, endpoint: 0, stamp: 0, badge: 0 });
        k.processes.insert(
            pid,
            Process {
                pid,
                budget: root,
                started: true,
                threads: BTreeSet::new(),
                handles: BTreeMap::new(),
                space: BTreeMap::new(),
                tables: BTreeMap::new(),
                exit_endpoint: None,
                creator: root,
                next_msg_id: 1,
            },
        );
        let root_table = k.page_table_cost();
        let rb = k.budgets.get_mut(&root).unwrap();
        rb.pages_used += c.process + c.contexts + root_table;
        rb.processes_used += 1;
        let mut hs = vec![
            Handle { object: Object::Budget(root), badge: 0, stamp: root, origin: Origin::Boot },
            Handle { object: Object::Budget(system), badge: 0, stamp: root, origin: Origin::Boot },
            Handle { object: Object::Budget(users), badge: 0, stamp: root, origin: Origin::Boot },
        ];
        for (i, spec) in boot.devices.iter().enumerate() {
            let id = i as u64 + 1;
            let kind = match *spec {
                DeviceSpec::Mmio { base, pages, dma } => DeviceKind::Mmio { base, pages, dma },
                // Sources start masked; the first `receive` unmasks them (R5; README choice 18).
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

    fn broken(&self, m: Mutation) -> bool { self.mutation == Some(m) }

    // ---------------------------------------------------------------------------------------
    // Queries used by the generator, the trace and the property checks.

    /// Threads that may make a step: runnable threads of started processes, in tid order.
    pub fn runnable(&self) -> Vec<(u64, u64)> {
        if self.halted.is_some() {
            return Vec::new();
        }
        self.threads.values().filter(|t| t.wait.is_none()).map(|t| (t.pid, t.tid)).collect()
    }

    pub fn budget_of(&self, pid: u64) -> Option<u64> { self.processes.get(&pid).map(|p| p.budget) }

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

    /// What one page-table page costs (R6; QUESTIONS 13).
    pub fn page_table_cost(&self) -> u64 {
        if self.broken(Mutation::R6PageTablesFree) { 0 } else { self.costs.page_table }
    }

    /// Open calls held by process `pid`: calls its threads took and have not replied to.
    pub fn open_calls(&self, pid: u64) -> u64 {
        self.msgs.values().filter(|m| m.kind == MsgKind::Call && m.server.is_some_and(|s| s.0 == pid)).count()
            as u64
    }

    /// R4a: does thread `tid` of process `pid` stand at `MAX_OPEN_CALLS` (so it takes no more
    /// calls)?
    pub fn open_calls_full(&self, pid: u64, tid: u64) -> bool {
        let open = if self.broken(Mutation::R4aOpenCallsPerThread) {
            self.threads.get(&tid).map_or(0, |t| t.serving.len() as u64)
        } else {
            self.open_calls(pid)
        };
        open >= MAX_OPEN_CALLS && !self.broken(Mutation::OpenCallsUnlimited)
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

    /// R2's group for a message from budget `sender` with its `account` and `labels`, sent
    /// through a handle stamped `stamp`: (account, label set), and for account 0 the sender's
    /// budget id as well (QUESTIONS 17, 87).
    fn queue_key(&self, account: u64, labels: &[u64], stamp: u64, sender: u64) -> Key {
        let budget =
            if account == 0 && !self.broken(Mutation::R2SystemCallersShareGroup) { sender } else { 0 };
        if self.broken(Mutation::R2KeyByAccountOnly) {
            (account, Vec::new(), budget)
        } else if self.broken(Mutation::R2KeyByStampLabels) {
            (account, self.labels_of(stamp), budget)
        } else {
            (account, labels.to_vec(), budget)
        }
    }

    /// A PID for a new process, drawn at random from the free ones (QUESTIONS 88). A PID stays in
    /// use while its process object lives: while the process runs, and while its exit notice
    /// waits.
    fn draw_pid(&mut self) -> Option<u64> {
        let in_use: BTreeSet<u64> = self
            .processes
            .keys()
            .copied()
            .chain(self.endpoints.values().flat_map(|e| e.exits.iter().map(|n| n.pid)))
            .collect();
        if in_use.len() as u64 >= MAX_PID {
            return None;
        }
        loop {
            self.pid_rng = self.pid_rng.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.pid_rng;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^= z >> 31;
            let pid = 2 + z % (MAX_PID - 1);
            if !in_use.contains(&pid) {
                return Some(pid);
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // Accounting (R6, R7).

    fn free_pages(&self, b: u64) -> u64 {
        self.budgets.get(&b).map_or(0, |x| x.pages_limit.saturating_sub(x.pages_used))
    }

    /// Charge `n` pages to `b`, failing with `OutOfMemory` over its limit.
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

    /// Create a budget object (after all checks). Its own page is charged to its parent (R6;
    /// QUESTIONS 76; `root`'s is the kernel's) and its limits are carved from the parent (R7).
    /// `creator` is the creating caller's budget class, for the ghost.
    #[allow(clippy::too_many_arguments)]
    fn new_budget(
        &mut self,
        parent: Option<u64>,
        class: Class,
        labels: Vec<u64>,
        account: u64,
        deadline: Option<u64>,
        l: Limits,
        creator: Class,
    ) -> u64 {
        let id = self.next_budget;
        self.next_budget += 1;
        self.ghost.budget_created(id, &labels, creator);
        let depth = parent.and_then(|p| self.budgets.get(&p)).map_or(0, |p| p.depth + 1);
        let own_page = if parent.is_some() { self.costs.budget } else { 0 };
        let to_itself = self.broken(Mutation::R6OwnPageChargedToItself);
        let b = Budget {
            id,
            parent,
            class,
            labels,
            account,
            deadline,
            pages_limit: l.pages,
            pages_used: if to_itself { own_page } else { 0 },
            processes_limit: l.processes,
            processes_used: 0,
            weight: l.weight,
            weight_used: 0,
            depth,
        };
        self.budgets.insert(id, b);
        if let Some(p) = parent {
            let x = self.budgets.get_mut(&p).unwrap();
            x.pages_used += l.pages + if to_itself { 0 } else { own_page };
            x.processes_used += l.processes;
            x.weight_used += l.weight;
        }
        self.sched.add_budget(id, l.weight);
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

    /// Plan lowest free slots and actual newly occupied table pages. Holes never compact.
    fn handle_slots(&self, pid: u64, count: usize) -> R<(Vec<u64>, u64)> {
        let p = self.processes.get(&pid).ok_or(Error::Dead)?;
        let mut slots = Vec::new();
        let mut pages: BTreeSet<u64> =
            p.handles.keys().map(|h| (h - 1) / self.costs.handles_per_page).collect();
        let before = pages.len();
        for h in 1..=MAX_HANDLES {
            if slots.len() == count {
                break;
            }
            if !p.handles.contains_key(&h) {
                slots.push(h);
                pages.insert((h - 1) / self.costs.handles_per_page);
            }
        }
        if slots.len() != count {
            return Err(Error::TooLarge);
        }
        Ok((slots, (pages.len() - before) as u64))
    }

    /// Atomically install copies, charging only newly occupied table pages (answers 111/116).
    fn install(&mut self, pid: u64, hs: &[Handle]) -> R<Vec<u64>> {
        let (slots, growth) = self.handle_slots(pid, hs.len())?;
        let budget = self.processes[&pid].budget;
        self.charge(budget, growth)?;
        let p = self.processes.get_mut(&pid).unwrap();
        for (slot, handle) in slots.iter().zip(hs) {
            p.handles.insert(*slot, *handle);
        }
        Ok(slots)
    }

    fn remove_handle(&mut self, pid: u64, h: u64) {
        let Some(p) = self.processes.get_mut(&pid) else { return };
        if p.handles.remove(&h).is_none() {
            return;
        }
        let budget = p.budget;
        let page = (h - 1) / self.costs.handles_per_page;
        let empty = !p.handles.keys().any(|i| (i - 1) / self.costs.handles_per_page == page);
        if empty {
            self.uncharge(budget, 1);
        }
    }

    /// Close every handle matching `pred`, wherever it is: process tables, processes' exit
    /// endpoint references, and messages not yet received, where a closed handle stays in its
    /// place and arrives as 0 (R10; QUESTIONS 86).
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
        let dropped = self.broken(Mutation::R10SweptHandlesDropped);
        for m in self.msgs.values_mut() {
            for slot in &mut m.handles {
                if slot.as_ref().is_some_and(&pred) {
                    *slot = None;
                }
            }
            if dropped {
                m.handles.retain(|x| x.is_some());
            }
        }
    }

    /// A budget's labels (none if it is gone).
    fn labels_of(&self, b: u64) -> Vec<u64> { self.budgets.get(&b).map_or(Vec::new(), |x| x.labels.clone()) }

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
        f
    }

    /// Free a frame and uncharge its payer. Its contents stay in RAM until it is reused.
    fn free_frame(&mut self, f: u64) {
        if let Some(fr) = self.frames.remove(&f) {
            self.uncharge(fr.payer, 1);
            self.free_frames.insert(f, fr.content);
        }
    }

    /// `n` free virtual pages in `pid` for a mapping whose address the kernel chooses: above
    /// everything already mapped, from `KERNEL_CHOSEN_BASE` (README choice 20). If that space
    /// doesn't fit `n` pages -- a `map_fixed` placed a mapping high in `[KERNEL_CHOSEN_BASE,
    /// USER_TOP)`, which the real kernel's bounded `find_virtual_address` window never sees --
    /// fall back to the first gap of `n` free pages anywhere in that range, so this stays as
    /// permissive as the kernel (P1-1).
    fn alloc_va(&self, pid: u64, n: u64) -> R<u64> {
        let p = self.processes.get(&pid).ok_or(Error::Dead)?;
        let above = p.space.last_key_value().map_or(0, |(v, _)| v + 1);
        let start = above.max(vpn(KERNEL_CHOSEN_BASE));
        if let Some(end) = start.checked_add(n) {
            if end <= vpn(USER_TOP) {
                return Ok(start);
            }
        }
        let (lo, hi) = (vpn(KERNEL_CHOSEN_BASE), vpn(USER_TOP));
        let mut cursor = lo;
        for (&v, _) in p.space.range(lo..hi) {
            // `v - cursor`, not `cursor + n`: a hostile `n` near `u64::MAX` must not overflow.
            if v.saturating_sub(cursor) >= n {
                return Ok(cursor);
            }
            cursor = cursor.max(v + 1);
        }
        if hi.saturating_sub(cursor) >= n { Ok(cursor) } else { Err(Error::OutOfMemory) }
    }

    /// Page-table pages (in pages to charge) that mapping `vpns` into `pid` would allocate.
    fn tables_needed(&self, pid: u64, vpns: impl IntoIterator<Item = u64>) -> u64 {
        let Some(p) = self.processes.get(&pid) else { return 0 };
        let mut new = BTreeSet::new();
        for v in vpns {
            for k in table_keys(v) {
                if !p.tables.contains_key(&k) {
                    new.insert(k);
                }
            }
        }
        new.len() as u64 * self.page_table_cost()
    }

    /// Map `m` at `v` in `pid`. The caller has charged the page tables (`tables_needed`).
    fn map_page(&mut self, pid: u64, v: u64, m: Mapping) {
        let p = self.processes.get_mut(&pid).unwrap();
        if p.space.insert(v, m).is_none() {
            for k in table_keys(v) {
                *p.tables.entry(k).or_insert(0) += 1;
            }
        }
    }

    /// Unmap `v` from `pid`, freeing (and uncharging) page tables left empty (README choice 20).
    fn unmap_page(&mut self, pid: u64, v: u64) -> Option<Mapping> {
        let pt = self.page_table_cost();
        let p = self.processes.get_mut(&pid)?;
        let m = p.space.remove(&v)?;
        let mut freed = 0;
        for k in table_keys(v) {
            let n = p.tables.get_mut(&k).unwrap();
            *n -= 1;
            if *n == 0 {
                p.tables.remove(&k);
                freed += pt;
            }
        }
        let b = p.budget;
        self.uncharge(b, freed);
        Some(m)
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

    /// Every page in `[first, first + n)` is absent from `pid`'s address space (`map_fixed`'s
    /// overlap check): the opposite of `own_range`, wanting nothing there rather than an owned
    /// mapping, and a single `BTreeMap::range` lookup instead of `n` point lookups, so it stays
    /// cheap even for a huge `n` (P1-2).
    fn range_free(&self, pid: u64, first: u64, n: u64) -> bool {
        let Some(p) = self.processes.get(&pid) else { return false };
        p.space.range(first..first + n).next().is_none()
    }

    // ---------------------------------------------------------------------------------------
    // Threads, blocking, waking.

    fn new_thread(&mut self, pid: u64) -> u64 {
        let tid = self.next_tid;
        self.next_tid += 1;
        self.threads.insert(
            tid,
            Thread {
                tid,
                pid,
                wait: None,
                deadline: None,
                serving: Vec::new(),
                current: None,
                record: Record::Owned,
                call_lend: None,
            },
        );
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

    fn record_valid(&self, pid: u64, tid: u64, completion: bool) -> bool {
        match self.threads.get(&tid).map(|t| t.record) {
            Some(Record::Owned) => true,
            Some(Record::CopyFault) => !completion,
            Some(Record::Memory(addr)) => {
                addr.is_multiple_of(8)
                    && self.processes.get(&pid).and_then(|p| p.space.get(&(addr / PAGE_SIZE))).is_some_and(
                        |m| {
                            m.state == MapState::Own
                                && matches!(m.backing, Backing::Frame(_))
                                && m.flags & (FLAG_R | FLAG_W) == FLAG_R | FLAG_W
                        },
                    )
            }
            _ => false,
        }
    }

    fn input_record_valid(&self, pid: u64, tid: u64) -> bool {
        match self.threads.get(&tid).map(|t| t.record) {
            Some(Record::Owned | Record::ReadOnly | Record::CopyFault) => true,
            Some(Record::Memory(addr)) => {
                addr.is_multiple_of(8)
                    && self.processes.get(&pid).and_then(|p| p.space.get(&(addr / PAGE_SIZE))).is_some_and(
                        |m| {
                            m.state == MapState::Own
                                && matches!(m.backing, Backing::Frame(_))
                                && m.flags & FLAG_R != 0
                        },
                    )
            }
            _ => false,
        }
    }

    fn wake(&mut self, tid: u64, mut result: R<Ret>) {
        let Some(t) = self.threads.get_mut(&tid) else { return };
        if let Some(lend) = t.call_lend.take() {
            if !matches!(result, Ok(Ret::Call(_))) {
                result = Ok(Ret::Call(CallCompletion { status: result.map(|_| ()), lend, reply: None }));
            }
        }
        t.wait = None;
        t.deadline = None;
        let pid = t.pid;
        if let Some(b) = self.budget_of(pid) {
            self.sched.wake(b, tid);
        }
        if !matches!(result, Ok(Ret::Message(_))) {
            self.ghost.receiving.remove(&tid);
        }
        if self.broken(Mutation::IpcWrongLend) {
            if let Ok(Ret::Call(c)) = &mut result {
                c.lend = LendDisposition::None;
            }
        }
        self.ghost.flows.push(Flow::Woken { tid, result: result.clone() });
        self.wakes.push(Wake { pid, tid, result });
    }

    /// Something may be deliverable on `e`: match it with receivers at the end of the step.
    fn poke(&mut self, e: u64) { self.to_pump.insert(e); }

    /// Process `pid` may take calls again: poke every endpoint one of its threads receives on.
    fn poke_receivers_of(&mut self, pid: u64) {
        let eps: Vec<u64> = self
            .threads
            .values()
            .filter(|t| t.pid == pid)
            .filter_map(|t| match t.wait {
                Some(Wait::Receive { endpoint, .. }) => Some(endpoint),
                _ => None,
            })
            .collect();
        for e in eps {
            self.poke(e);
        }
    }

    /// Deliver whatever became deliverable (delivery can make more so, through a refused transfer).
    fn settle(&mut self) {
        while let Some(e) = self.to_pump.pop_first() {
            self.pump(e);
        }
    }

    // ---------------------------------------------------------------------------------------
    // Messages.

    /// The open call of thread `tid` that its process knows as `rid` (QUESTIONS 88).
    fn open_call_of(&self, tid: u64, rid: u64) -> Option<u64> {
        let t = self.threads.get(&tid)?;
        t.serving.iter().copied().find(|m| self.msgs.get(m).is_some_and(|x| x.rid == rid))
    }

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
        let (e, key) = (m.endpoint, m.key.clone());
        if let Some(ep) = self.endpoints.get_mut(&e) {
            if let Some(q) = ep.queue.get_mut(&key) {
                q.retain(|x| *x != mid);
                if q.is_empty() {
                    ep.queue.remove(&key);
                }
            }
        }
    }

    /// A queued message is withdrawn: out of its queue, and its buffer back to its sender.
    fn discard(&mut self, mid: u64) -> Option<Msg> {
        self.unqueue(mid);
        self.return_buffer(mid);
        self.msgs.remove(&mid)
    }

    /// A queued message fails: its sender gets `err` and its buffer back.
    fn fail_sender(&mut self, mid: u64, err: Error) {
        if let Some(m) = self.discard(mid) {
            self.wake(m.sender_tid, Err(err));
        }
    }

    /// R3: a taken call is abandoned (its caller died, timed out, or was failed by revocation or
    /// its endpoint's destruction). The caller's charge for the lend ends; the lend stays mapped
    /// in the server, charged only there, until the server replies; the open call's abandoned flag
    /// is set, and the holding thread gets a notice on its endpoint (QUESTIONS 81).
    fn abandon(&mut self, mid: u64) {
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        m.caller_waiting = false;
        if m.server.is_none() {
            return;
        }
        if m.buffer.is_some() {
            if let Some(t) = self.threads.get_mut(&m.sender_tid) {
                t.call_lend = Some(LendDisposition::Consumed);
            }
        }
        if let Some(g) = self.ghost.calls.get_mut(&m.sender_tid) {
            g.2 = true;
        }
        m.abandoned = true;
        m.notice = self.mutation != Some(Mutation::AbandonNoticeMissing);
        let (sender_pid, sender_budget, e) = (m.sender_pid, m.sender_budget, m.endpoint);
        let buf = m.buffer.clone();
        self.poke(e);
        let Some(buf) = buf else { return };
        for i in 0..buf.frames.len() as u64 {
            let v = buf.sender_vpn + i;
            let lent = self.processes.get(&sender_pid).and_then(|p| p.space.get(&v));
            if lent.is_some_and(|x| x.state == MapState::LentOut(mid)) {
                self.unmap_page(sender_pid, v);
            }
        }
        if self.broken(Mutation::R3UnmapAbandonedLend) {
            self.uncharge_lend(mid);
            self.unmap_lend_in_server(mid, true);
            return;
        }
        let server_budget = self.msgs[&mid].open_payer.unwrap_or(sender_budget);
        let n = buf.frames.len() as u64;
        if self.broken(Mutation::R3ChargeStaysWithCaller) {
            // The server's charge ends instead of the caller's.
            self.uncharge_lend(mid);
        } else {
            for f in &buf.frames {
                self.frames.get_mut(f).unwrap().payer = server_budget;
            }
            self.uncharge(sender_budget, n);
        }
    }

    /// Pages the receiver of taken call `mid` pays for its lend while its caller waits (R3).
    fn lend_pages(&self, mid: u64) -> u64 {
        let Some(m) = self.msgs.get(&mid) else { return 0 };
        if m.kind != MsgKind::Call || m.server.is_none() || !m.caller_waiting {
            return 0;
        }
        if self.broken(Mutation::R6LendChargedOnce) {
            return 0;
        }
        m.buffer.as_ref().map_or(0, |b| b.frames.len() as u64)
    }

    /// The receiver's charge for the lend of `mid` ends (its reply, its caller's lend returned).
    fn uncharge_lend(&mut self, mid: u64) {
        let n = self.lend_pages(mid);
        if let Some(b) = self.msgs.get(&mid).and_then(|m| m.open_payer) {
            self.uncharge(b, n);
        }
    }

    /// Unmap a received lend from its server; if `free`, free its frames too, otherwise give the
    /// buffer back to its lender (and end the server's charge for it).
    fn unmap_lend_in_server(&mut self, mid: u64, free: bool) {
        if !free {
            self.uncharge_lend(mid);
        }
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        let Some(buf) = m.buffer.take() else { return };
        if let (Some((spid, _)), Some(rv)) = (m.server, buf.receiver_vpn) {
            for i in 0..buf.frames.len() as u64 {
                let lent = self.processes.get(&spid).and_then(|p| p.space.get(&(rv + i)));
                if lent.is_some_and(|x| x.state == MapState::LentIn(mid)) {
                    self.unmap_page(spid, rv + i);
                }
            }
        }
        if free {
            for f in &buf.frames {
                self.free_frame(*f);
            }
        } else {
            self.msgs.get_mut(&mid).unwrap().buffer = Some(buf);
            self.return_buffer(mid);
        }
    }

    /// A call is finished with (replied to, or its server is gone): release its open-call page,
    /// and let its server's process take calls again.
    fn close_call(&mut self, mid: u64) {
        let Some(m) = self.msgs.get_mut(&mid) else { return };
        let server = m.server.map(|s| s.0);
        if let Some(b) = m.open_payer.take() {
            let cost = if self.broken(Mutation::R6OpenCallsFree) { 0 } else { self.costs.open_call };
            self.uncharge(b, cost);
        }
        self.msgs.remove(&mid);
        if let Some(pid) = server {
            self.poke_receivers_of(pid);
        }
    }

    /// A queued message is refused in its turn: its group counts as served (R2) and the sender
    /// fails with `err`.
    fn refuse(&mut self, e: u64, mid: u64, err: Error) {
        let key = self.msgs[&mid].key.clone();
        self.ghost.took(e, &key, &self.endpoints[&e]);
        self.endpoints.get_mut(&e).unwrap().cursor = Some(key);
        self.fail_sender(mid, err);
    }

    /// R2: the next message to take on `e`: the oldest message of the next group after the last
    /// one served, in group order, wrapping around. A process at `MAX_OPEN_CALLS` takes no calls:
    /// without `calls`, R2's turns skip them, and a group's oldest send is its message (README
    /// choice 28).
    fn next_sender(&self, e: u64, calls: bool) -> Option<u64> {
        use core::ops::Bound::{Excluded, Unbounded};
        let ep = self.endpoints.get(&e)?;
        let takes = |m: &u64| calls || self.msgs.get(m).is_some_and(|x| x.kind == MsgKind::Send);
        let head = |q: &VecDeque<u64>| q.iter().copied().find(takes);
        if self.broken(Mutation::R2FifoAcrossAccounts) {
            return ep.queue.values().filter_map(head).min();
        }
        let after: Vec<&VecDeque<u64>> = match &ep.cursor {
            Some(c) => ep
                .queue
                .range((Excluded(c.clone()), Unbounded))
                .chain(ep.queue.range(..=c.clone()))
                .map(|(_, q)| q)
                .collect(),
            None => ep.queue.values().collect(),
        };
        after.into_iter().find_map(head)
    }

    /// The abandoned-call notice waiting for thread `tid` on endpoint `e`, if any (QUESTIONS 81).
    fn notice_for(&self, tid: u64, e: u64) -> Option<u64> {
        let t = self.threads.get(&tid)?;
        t.serving.iter().copied().find(|m| self.msgs.get(m).is_some_and(|x| x.notice && x.endpoint == e))
    }

    /// Match waiting receivers on `e` with what is pending there, until nothing more can be
    /// delivered. Notices come before messages (KERNEL-SPEC.md, Messages): a waiting thread's
    /// abandoned-call notices, then exit notices in the order queued, to the first receiver
    /// (README choice 8). A message goes to the first receiver that can take it (one at
    /// `MAX_OPEN_CALLS` takes only sends).
    fn pump(&mut self, e: u64) {
        loop {
            let Some(ep) = self.endpoints.get(&e) else { return };
            if ep.receivers.is_empty() {
                return;
            }
            let receivers: Vec<u64> = ep.receivers.iter().copied().collect();
            let owner = ep.owner;

            // Abandoned-call notices go to the thread holding the call.
            if let Some((rtid, mid)) = receivers.iter().find_map(|t| self.notice_for(*t, e).map(|m| (*t, m)))
            {
                self.endpoints.get_mut(&e).unwrap().receivers.retain(|x| *x != rtid);
                let again = self.broken(Mutation::AbandonNoticeRepeated);
                let m = self.msgs.get_mut(&mid).unwrap();
                m.notice = again;
                let rid = m.rid;
                self.ghost.flows.push(Flow::AbandonNotice { tid: rtid, msg: mid });
                self.wake(rtid, Ok(Ret::Abandoned { msg_id: rid }));
                continue;
            }

            let rtid = receivers[0];
            if let Some(n) = self.endpoints.get_mut(&e).unwrap().exits.pop_front() {
                // The label check was made when the notice was queued (R1).
                self.endpoints.get_mut(&e).unwrap().receivers.pop_front();
                let want = self.ghost.owed.remove(&n.pid).map(|o| o.want);
                self.free_process_object(n.pid, n.payer);
                let got =
                    Blame { cause: n.cause, account: n.blamed_account, labels: n.blamed_labels.clone() };
                self.ghost.flows.push(Flow::Exit {
                    pid: n.pid,
                    from: self.ghost.labels(n.budget),
                    to_class: self.budgets[&owner].class,
                    to: self.ghost.labels(owner),
                    got,
                    want,
                });
                let ret = Ret::ExitNotice {
                    pid: n.pid,
                    cause: n.cause,
                    code: n.code,
                    blamed_account: n.blamed_account,
                    blamed_labels: n.blamed_labels,
                };
                self.wake(rtid, Ok(ret));
                continue;
            }

            // The first receiver that can take a message, and the message.
            let pick = receivers.iter().find_map(|t| {
                let full = self.open_calls_full(self.threads[t].pid, *t);
                if full && self.broken(Mutation::R4aFullTakesNothing) {
                    return None;
                }
                self.next_sender(e, !full).map(|m| (*t, m))
            });
            let Some((rtid, mid)) = pick else { return };
            if self.deliver(e, rtid, mid) {
                continue;
            }
        }
    }

    /// Deliver message `mid` on `e` to receiving thread `rtid`, or refuse it (R4): a delivery the
    /// receiving process's budget cannot pay for in full, or a transfer over `max_transfer`, fails
    /// its sender with `Refused`, and the receiver keeps waiting. Returns whether anything
    /// happened (always, in this model).
    fn deliver(&mut self, e: u64, rtid: u64, mid: u64) -> bool {
        let rpid = self.threads[&rtid].pid;
        let Some(rbudget) = self.budget_of(rpid) else { return false };
        let Some(Wait::Receive { max_transfer, .. }) = self.threads[&rtid].wait else { return false };
        let m = self.msgs[&mid].clone();
        let pages = m.buffer.as_ref().map_or(0, |b| b.frames.len() as u64);
        let transfer = if m.kind == MsgKind::Send { pages } else { 0 };
        if transfer > max_transfer && !self.broken(Mutation::R4IgnoreMaxTransfer) {
            self.refuse(e, mid, Error::Refused);
            return true;
        }
        // Everything the message brings: the table pages for its handles, a call's open-call
        // page and lent pages, transferred pages, and the page tables to map the buffer (R4).
        let mut hs: Vec<Handle> = m.handles.iter().flatten().copied().collect();
        if self.broken(Mutation::R9ReceivedHandleRestamped) {
            for h in &mut hs {
                h.stamp = rbudget;
            }
        }
        let growth = match self.handle_slots(rpid, hs.len()) {
            Ok((_, growth)) => growth,
            Err(_) => {
                self.refuse(e, mid, Error::Refused);
                return true;
            }
        };
        let open = if m.kind == MsgKind::Call && !self.broken(Mutation::R6OpenCallsFree) {
            self.costs.open_call
        } else {
            0
        };
        let va = if pages > 0 { self.alloc_va(rpid, pages).ok() } else { None };
        let tables = va.map_or(0, |rv| self.tables_needed(rpid, rv..rv + pages));
        let lent =
            if m.kind == MsgKind::Call && !self.broken(Mutation::R6LendChargedOnce) { pages } else { 0 };
        let moved = if m.kind == MsgKind::Send && m.sender_budget != rbudget { pages } else { 0 };
        let need =
            growth.saturating_add(open).saturating_add(tables).saturating_add(lent).saturating_add(moved);
        let overdraw = self.broken(Mutation::R4OverdrawOnDelivery);
        if (self.free_pages(rbudget) < need && !overdraw) || (pages > 0 && va.is_none()) {
            self.refuse(e, mid, Error::Refused);
            return true;
        }

        // Deliver.
        let key = m.key.clone();
        self.ghost.took(e, &key, &self.endpoints[&e]);
        self.unqueue(mid);
        let ep = self.endpoints.get_mut(&e).unwrap();
        ep.cursor = Some(key);
        ep.receivers.retain(|x| *x != rtid);
        // The message's copies of its handles move into the receiver's table; a revoked one
        // arrives as 0 (R10).
        self.add_usage(rbudget, growth);
        let p = self.processes.get_mut(&rpid).unwrap();
        let mut slots = Vec::new();
        let mut next = 1;
        let mut stamps = hs.iter();
        for h in &m.handles {
            if h.is_none() {
                slots.push(NO_HANDLE);
                continue;
            }
            while p.handles.contains_key(&next) {
                next += 1;
            }
            p.handles.insert(next, *stamps.next().unwrap());
            slots.push(next);
        }
        // Its id, from the receiving process's own counter (QUESTIONS 88).
        let rid = if self.broken(Mutation::MsgIdsGlobal) {
            mid
        } else {
            let p = self.processes.get_mut(&rpid).unwrap();
            p.next_msg_id += 1;
            p.next_msg_id - 1
        };
        self.add_usage(rbudget, open + tables + lent);
        let mut received = None;
        if let (Some(buf), Some(rv)) = (&m.buffer, va) {
            match m.kind {
                MsgKind::Call => {
                    for (i, f) in buf.frames.iter().enumerate() {
                        let map = Mapping {
                            backing: Backing::Frame(*f),
                            flags: FLAG_R | FLAG_W, // README choice 12
                            state: MapState::LentIn(mid),
                        };
                        self.map_page(rpid, rv + i as u64, map);
                    }
                }
                MsgKind::Send => {
                    // Map in the receiver first: its page tables were counted with the sender's
                    // still in place (they may be the same process).
                    for (i, f) in buf.frames.iter().enumerate() {
                        let map = Mapping {
                            backing: Backing::Frame(*f),
                            flags: FLAG_R | FLAG_W,
                            state: MapState::Own,
                        };
                        self.map_page(rpid, rv + i as u64, map);
                    }
                    for i in 0..pages {
                        self.unmap_page(m.sender_pid, buf.sender_vpn + i);
                    }
                    for f in &buf.frames {
                        self.frames.get_mut(f).unwrap().payer = rbudget;
                    }
                    self.uncharge(m.sender_budget, pages);
                    self.add_usage(rbudget, pages);
                }
            }
            let kind = if m.kind == MsgKind::Call { BufferKind::Lend } else { BufferKind::Transfer };
            received = Some(Received { kind, addr: rv * PAGE_SIZE, pages });
        }
        // A call becomes an open call of the thread, and its current call (QUESTIONS 82); a send
        // is done with (QUESTIONS 31).
        match m.kind {
            MsgKind::Call => {
                let never = self.broken(Mutation::CurrentNeverSet);
                let t = self.threads.get_mut(&rtid).unwrap();
                t.serving.push(mid);
                if !never {
                    t.current = Some(mid);
                }
                let mm = self.msgs.get_mut(&mid).unwrap();
                mm.server = Some((rpid, rtid));
                mm.open_payer = Some(rbudget);
                mm.rid = rid;
                mm.handles.clear();
                if let Some(b) = mm.buffer.as_mut() {
                    b.receiver_vpn = va;
                }
            }
            MsgKind::Send => {
                self.msgs.remove(&mid);
            }
        }
        let msg = Message {
            kind: m.kind,
            msg_id: rid,
            badge: if self.broken(Mutation::MsgBadgeZero) { 0 } else { m.badge },
            account: m.account,
            labels: m.labels.clone(),
            words: m.words,
            handles: slots,
            buffer: received,
        };
        if m.kind == MsgKind::Call {
            if let Some(g) = self.ghost.calls.get_mut(&m.sender_tid) {
                g.1 = true;
            }
        }
        self.ghost.delivered(rtid, rpid, mid, &msg);
        self.wake(rtid, Ok(Ret::Message(msg)));
        match m.kind {
            MsgKind::Send => self.wake(m.sender_tid, Ok(Ret::Unit)),
            MsgKind::Call => {
                let t = self.threads.get_mut(&m.sender_tid).unwrap();
                t.wait = Some(Wait::Reply(mid));
            }
        }
        true
    }

    // ---------------------------------------------------------------------------------------
    // Ending things: threads, processes, endpoints, budgets (R10).

    /// A thread ends. What it waited for is withdrawn; what it was serving is finished: a caller
    /// still waiting gets `Dead` and its lend back; an abandoned lend is freed (R4b).
    fn end_thread(&mut self, tid: u64) {
        let Some(t) = self.threads.get(&tid).cloned() else { return };
        match t.wait {
            Some(Wait::Send(mid)) => {
                self.discard(mid);
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
        for mid in t.serving {
            self.finish_served(mid, Error::Dead);
        }
        let budget = self.budget_of(t.pid);
        if let Some(b) = budget {
            self.sched.block(b, tid);
        }
        self.threads.remove(&tid);
        self.ghost.thread_gone(tid);
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
                let r = if self.broken(Mutation::R4bDeadServerFakesReply) {
                    Ok(Ret::Call(CallCompletion {
                        status: Ok(()),
                        lend: self.threads[&caller].call_lend.unwrap_or(LendDisposition::None),
                        reply: Some(ReplyRecord { words: [0; WORDS], handles: Vec::new() }),
                    }))
                } else {
                    Err(err)
                };
                self.wake(caller, r);
            } else {
                self.unmap_lend_in_server(mid, true);
            }
        }
        self.close_call(mid);
    }

    /// Free a process object charged to `payer`, once its exit notice is received or dropped
    /// (QUESTIONS 74).
    fn free_process_object(&mut self, pid: u64, payer: u64) {
        self.sweep(|h| h.object == Object::Process(pid));
        self.ghost.process_freed(pid);
        if !self.broken(Mutation::R6ProcessObjectFree) {
            self.uncharge(payer, self.costs.process);
        }
    }

    /// A process ends: its threads, address space and handle table are freed. References to its
    /// creator-paid object remain live until its exit notice is received or dropped.
    /// Its object, charged to its creator, stays until the notice is received or dropped.
    fn end_process(&mut self, pid: u64, code: u64, blame: Blame) {
        let Some(p) = self.processes.get(&pid) else { return };
        // Ghost: what the notice must report: a kill, unless the process began to die by an exit
        // or a fault (`Ghost::exiting`).
        let want = self.ghost.exit_expect.remove(&pid).unwrap_or_else(Blame::killed);
        let tids: Vec<u64> = p.threads.iter().copied().collect();
        for tid in tids {
            self.end_thread(tid);
        }
        let vpns: Vec<u64> = self.processes[&pid].space.keys().copied().collect();
        for v in vpns {
            let m = self.unmap_page(pid, v).unwrap();
            if let Backing::Frame(f) = m.backing {
                if m.state == MapState::Own
                    || matches!(m.state, MapState::LentOut(x) if !self.msgs.contains_key(&x))
                {
                    self.free_frame(f);
                }
            }
        }
        let p = self.processes.remove(&pid).unwrap();
        let table =
            p.handles.keys().map(|h| (h - 1) / self.costs.handles_per_page).collect::<BTreeSet<_>>().len()
                as u64;
        self.uncharge(p.budget, table + self.page_table_cost() + self.costs.contexts);
        // It stops counting against its budget's process limit now (QUESTIONS 106).
        if let Some(b) = self.budgets.get_mut(&p.budget) {
            b.processes_used = b.processes_used.saturating_sub(1);
        }
        // Ghost: the notice is owed if its object's payer, the exit endpoint and the stamp of the
        // handle naming it are alive, and the label rule allows it, judged from the ghost's records.
        if let Some(s) = self.ghost.slots.get(&pid).copied() {
            let live = self.budgets.contains_key(&s.payer)
                && self.budgets.contains_key(&s.stamp)
                && self.endpoints.contains_key(&s.endpoint);
            if live {
                let owner = self.endpoints[&s.endpoint].owner;
                let rule = self.budgets[&owner].class == Class::System
                    || superset(&self.ghost.labels(owner), &self.ghost.labels(p.budget));
                if rule {
                    self.ghost.owed.insert(pid, Owed { endpoint: s.endpoint, payer: s.payer, want });
                }
            }
        }
        self.queue_exit_notice(&p, code, blame);
    }

    /// The exit notice of process `p`, which has just ended; without one, its object is freed.
    fn queue_exit_notice(&mut self, p: &Process, code: u64, blame: Blame) {
        let payer = p.creator;
        if !self.budgets.contains_key(&payer) {
            self.ghost.owed.remove(&p.pid);
            self.free_process_object(p.pid, payer);
            return;
        }
        let e = match p.exit_endpoint.map(|h| h.object) {
            Some(Object::Endpoint(e)) if self.endpoints.contains_key(&e) => e,
            _ => {
                self.free_process_object(p.pid, payer);
                return;
            }
        };
        // R1: an exit notice is a flow from the exiting budget to the exit endpoint's owner;
        // one that fails the rule is dropped.
        let owner = self.endpoints[&e].owner;
        let (oc, ol) = (self.budgets[&owner].class, self.budgets[&owner].labels.clone());
        let exiting = self.budgets.get(&p.budget);
        let exempt = if self.broken(Mutation::R1ExitExemptBySystemExiting) {
            exiting.is_some_and(|b| b.class == Class::System)
        } else {
            oc == Class::System
        };
        let allowed = exempt
            || superset(&ol, &exiting.map(|b| b.labels.clone()).unwrap_or_default())
            || self.broken(Mutation::R1ExitNoticeIgnoresLabels);
        let dropped =
            self.broken(Mutation::ExitNoticeDroppedIfNoReceiver) && self.endpoints[&e].receivers.is_empty();
        if !allowed || dropped {
            self.free_process_object(p.pid, payer);
            return;
        }
        let n = ExitNotice {
            pid: p.pid,
            cause: blame.cause,
            code,
            blamed_account: blame.account,
            blamed_labels: blame.labels,
            budget: p.budget,
            payer,
        };
        self.endpoints.get_mut(&e).unwrap().exits.push_back(n);
        self.poke(e);
    }

    /// Destroy an endpoint: blocked senders and receivers get `Dead`; taken calls in flight fail
    /// with `Dead` and are abandoned (R3); pending exit notices are dropped.
    fn destroy_endpoint(&mut self, e: u64) {
        let Some(ep) = self.endpoints.get(&e) else { return };
        let queued: Vec<u64> = ep.queue.values().flatten().copied().collect();
        let receivers: Vec<u64> = ep.receivers.iter().copied().collect();
        let exits: Vec<ExitNotice> = ep.exits.iter().cloned().collect();
        let owner = ep.owner;
        if !self.broken(Mutation::R10RevokedMessageDelivered) {
            for mid in queued {
                self.fail_sender(mid, Error::Dead);
            }
        }
        for tid in receivers {
            self.wake(tid, Err(Error::Dead));
        }
        for n in exits {
            self.ghost.owed.remove(&n.pid);
            self.free_process_object(n.pid, n.payer);
        }
        let in_flight: Vec<u64> = self
            .msgs
            .values()
            .filter(|m| m.endpoint == e && m.kind == MsgKind::Call && m.caller_waiting && m.server.is_some())
            .map(|m| m.id)
            .collect();
        if !self.broken(Mutation::R10RevokedCallAnswered) {
            for mid in in_flight {
                let caller = self.msgs[&mid].sender_tid;
                self.abandon(mid);
                self.wake(caller, Err(Error::Dead));
            }
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
                self.end_process(pid, 0, Blame::killed());
            }
        }
        // Process objects charged to the doomed budgets are freed with them, killing the processes
        // that still run elsewhere; their notices go below, with their payer (QUESTIONS 74).
        if !self.broken(Mutation::R10CreatorDeathSparesProcess) {
            let pids: Vec<u64> =
                self.processes.values().filter(|p| doomed.contains(&p.creator)).map(|p| p.pid).collect();
            for pid in pids {
                self.end_process(pid, 0, Blame::killed());
            }
        }
        let eps: Vec<u64> =
            self.endpoints.values().filter(|e| doomed.contains(&e.owner)).map(|e| e.id).collect();
        for e in eps {
            self.destroy_endpoint(e);
        }
        // Process objects the doomed budgets paid for go with them, and their notices (QUESTIONS
        // 74; README choice 21).
        if !self.broken(Mutation::R10ExitNoticesOutlivePayer) {
            let freed: Vec<_> = self
                .endpoints
                .values()
                .flat_map(|ep| ep.exits.iter())
                .filter(|n| doomed.contains(&n.payer))
                .map(|n| (n.pid, n.payer))
                .collect();
            for ep in self.endpoints.values_mut() {
                ep.exits.retain(|n| !doomed.contains(&n.payer));
            }
            for (pid, payer) in freed {
                self.ghost.owed.remove(&pid);
                self.free_process_object(pid, payer);
            }
        }
        // Revocation reaches messages already sent through a doomed stamp (QUESTIONS 30): a queued
        // one fails its sender with `Dead`; a taken call fails its caller with `Dead` at once and
        // is abandoned (R3).
        let revoked: Vec<(u64, bool)> = self
            .msgs
            .values()
            .filter(|m| doomed.contains(&m.stamp))
            .map(|m| (m.id, m.server.is_some()))
            .collect();
        for (mid, taken) in revoked {
            if !taken && !self.broken(Mutation::R10RevokedMessageDelivered) {
                self.fail_sender(mid, Error::Dead);
            } else if taken
                && self.msgs[&mid].caller_waiting
                && !self.broken(Mutation::R10RevokedCallAnswered)
            {
                let caller = self.msgs[&mid].sender_tid;
                self.abandon(mid);
                self.wake(caller, Err(Error::Dead));
            }
        }
        if !self.broken(Mutation::R10KeepForeignHandles) {
            self.sweep(|h| doomed.contains(&h.stamp));
        }
        self.sweep(|h| matches!(h.object, Object::Budget(x) if doomed.contains(&x)));
        // Return the carved limits and the budget's own page to the parent.
        let bb = self.budgets[&b].clone();
        if let Some(p) = bb.parent {
            if !self.broken(Mutation::R10KeepCarvedLimits) {
                let own = if self.broken(Mutation::R6OwnPageChargedToItself) { 0 } else { self.costs.budget };
                let px = self.budgets.get_mut(&p).unwrap();
                px.pages_used = px.pages_used.saturating_sub(bb.pages_limit + own);
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
        let deadlines = !self.broken(Mutation::BudgetDeadlineIgnored);
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
                .filter(|b| deadlines && b.deadline.is_some_and(|d| d <= self.now))
                .min_by_key(|b| (b.deadline, b.id))
                .map(|b| (b.deadline.unwrap(), 1, b.id));
            let next = match (t, b) {
                (Some(x), Some(y)) => x.min(y),
                (Some(x), None) | (None, Some(x)) => x,
                (None, None) => {
                    self.settle();
                    return;
                }
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
                self.discard(mid);
            }
            // R3: the server keeps the lend, charged to it, until its reply.
            Wait::Reply(mid) => self.abandon(mid),
            Wait::Receive { endpoint, .. } => {
                if let Some(ep) = self.endpoints.get_mut(&endpoint) {
                    ep.receivers.retain(|x| *x != tid);
                }
            }
            // With several threads waiting on one IRQ, a second event can stay pending (masked)
            // while this one times out: R5 unmasks only when a receive begins (README choice 18).
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
    /// the next due event; each run is charged at deschedule (R12). With nothing to run, time
    /// jumps to the next event.
    fn tick(&mut self, dt: u64) {
        let end = self.now.saturating_add(dt);
        while self.now < end {
            let pick = self.sched.pick();
            // With one budget runnable and nothing due, every whole slice until the next event
            // goes to it alike: charge them at once (the same passes as slice by slice), so that a
            // long tick costs the model no more than a short one.
            let until = self.next_event().filter(|e| *e > self.now).map_or(end, |e| e.min(end));
            let slices = (until - self.now) / SLICE;
            if let Some((b, _)) = pick.filter(|_| slices > 1 && self.sched.runnable_budgets() == 1) {
                self.now += slices * SLICE;
                self.sched.charge_slices(b, slices);
                self.expire();
                continue;
            }
            // No delivery or deadline can change runnable state before `until`. Keep
            // every scheduler pick and charge, but defer empty expiry scans to that boundary.
            // A stale earliest deadline under mutation still uses the slice-by-slice path.
            if slices > 1
                && self.to_pump.is_empty()
                && self.next_event().is_none_or(|e| e > self.now)
                && pick.is_some()
            {
                for _ in 0..slices {
                    let (b, _) = self.sched.pick().unwrap();
                    self.now += SLICE;
                    self.sched.charge(b, SLICE);
                }
                self.expire();
                continue;
            }
            let mut run = end - self.now;
            if pick.is_some() {
                run = run.min(SLICE);
            }
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
    /// exist or is blocked, a tick is longer than `MAX_TICK`, or the machine is halted); the
    /// state is then unchanged.
    /// Pending question 171: these events would require choosing a late-invalid receive
    /// result. Keep them outside the oracle until the owner settles that contract.
    pub fn unsupported_receive_output(&self, op: &Op) -> bool {
        if let Op::Sys { tid, call: Syscall::Receive { .. }, .. } = op {
            if self.threads.get(tid).is_some_and(|t| t.record == Record::CopyFault) {
                return true;
            }
        }
        for t in self
            .threads
            .values()
            .filter(|t| matches!(t.wait, Some(Wait::Receive { .. } | Wait::Irq { .. } | Wait::Sleep)))
        {
            if let Op::Record { tid, record, .. } = op {
                if *tid == t.tid && *record != t.record {
                    return true;
                }
            }
            let Record::Memory(address) = t.record else { continue };
            let Op::Sys { pid, call, .. } = op else { continue };
            if *pid != t.pid {
                continue;
            }
            let range = match call {
                Syscall::Unmap { addr, len } | Syscall::SetFlags { addr, len, .. } => Some((*addr, *len)),
                Syscall::ProcessMap { src, len, .. } => Some((*src, *len)),
                Syscall::Call { lend: Some(b), .. } | Syscall::Send { transfer: Some(b), .. } => {
                    Some((b.addr, b.npages.saturating_mul(PAGE_SIZE)))
                }
                _ => None,
            };
            if range.is_some_and(|(start, len)| address >= start && address < start.saturating_add(len)) {
                return true;
            }
        }
        false
    }

    pub fn step(&mut self, op: &Op) -> Option<Step> {
        if self.unsupported_receive_output(op) {
            return None;
        }
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
            Op::Record { pid, tid, .. } => {
                if self.threads.get(tid)?.pid != *pid {
                    return None;
                }
                None
            }
            Op::Tick { dt } if *dt > MAX_TICK => return None,
            Op::Irq { .. } | Op::Tick { .. } => None,
        };
        self.wakes.clear();
        self.notes.clear();
        self.ghost.begin_step();
        let outcome = match op {
            Op::Record { tid, record, .. } => {
                self.threads.get_mut(tid).unwrap().record = *record;
                Outcome::Done(Ok(Ret::Unit))
            }
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
        self.settle();
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

    /// Thread `tid` of `pid` faults: the process ends `faulted`.
    fn fault(&mut self, pid: u64, tid: u64) { self.exit_as(pid, tid, 0, true); }

    /// Process `pid` ends through thread `tid`: by a fault if `fault`, else by an exit with
    /// `code`. It is reported `faulted` if it faulted or holds open calls (QUESTIONS 55), blaming
    /// the sender of `tid`'s current call, or nobody if it has none (QUESTIONS 48, 82); otherwise
    /// `exited`.
    fn exit_as(&mut self, pid: u64, tid: u64, code: u64, fault: bool) {
        let Some(p) = self.processes.get(&pid) else { return };
        let threads: Vec<u64> = p.threads.iter().copied().collect();
        self.ghost.exiting(pid, tid, &threads, fault);
        let open = self.open_calls(pid) > 0 && !self.broken(Mutation::ExitWithOpenCallsNotFaulted);
        let t = self.threads.get(&tid);
        let blamed = if self.broken(Mutation::BlameNobody) {
            None
        } else if self.broken(Mutation::BlameNewestCall) {
            t.and_then(|t| t.serving.last().copied())
        } else {
            t.and_then(|t| t.current)
        };
        let blamed = blamed.and_then(|m| self.msgs.get(&m));
        // An exit reported `faulted` keeps its code (README choice 29).
        let blame = if fault || open {
            Blame {
                cause: Cause::Faulted,
                account: blamed.map_or(0, |m| m.account),
                labels: blamed.map_or(Vec::new(), |m| m.labels.clone()),
            }
        } else {
            Blame { cause: Cause::Exited, account: 0, labels: Vec::new() }
        };
        self.end_process(pid, code, blame);
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
            S::ProcessExit { code } => match self.process_exit(pid, tid, *code) {
                Ok(()) => Outcome::Gone,
                Err(e) => done(Err(e)),
            },
            S::ProcessCreate { budget, exit_endpoint } => {
                done(self.process_create(pid, *budget, *exit_endpoint).map(Ret::Handle))
            }
            S::ProcessMap { process, src, dst, len, flags } => {
                done(self.process_map(pid, *process, *src, *dst, *len, *flags).map(|_| Ret::Unit))
            }
            S::ProcessStart { process, entry, sp, arg, handles } => {
                done(self.process_start(pid, *process, *entry, *sp, *arg, handles).map(|_| Ret::Unit))
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
            S::Reply { msg_id, words, handles } => done(self.reply(pid, tid, *msg_id, *words, handles)),
            S::Serve { msg_id } => done(self.serve(tid, *msg_id).map(|_| Ret::Unit)),
            S::HandleClose { h } => done(self.handle_close(pid, *h).map(|_| Ret::Unit)),
            S::BudgetCreate { parent, pages, processes, weight, labels, account, deadline } => done(
                self.budget_create(pid, *parent, *pages, *processes, *weight, labels, *account, *deadline)
                    .map(Ret::Handle),
            ),
            S::BudgetDestroy { h } => match self.budget_destroy(pid, *h) {
                Ok(()) if !self.threads.contains_key(&tid) => Outcome::Gone,
                r => done(r.map(|_| Ret::Unit)),
            },
            S::BudgetUsage { h } => done(self.budget_usage(pid, *h).map(Ret::Usage)),
            S::TimeNow => done(Ok(Ret::Time(self.time_now()))),
            S::Random => done(Ok(Ret::Random)),
            S::SystemReset { h, kind } => done(self.system_reset(pid, *h, *kind).map(|_| Ret::Unit)),
            S::MapFixed { addr, len, flags } => {
                done(self.map_fixed(pid, *addr, *len, *flags).map(|_| Ret::Unit))
            }
        }
    }

    // ---------------------------------------------------------------------------------------
    // The system calls, in KERNEL-SPEC.md's table order.

    /// Map `n` fresh zeroed frames at a kernel-chosen address, charging frames and page tables.
    fn map_fresh(&mut self, pid: u64, n: u64, flags: u64, contiguous: bool) -> R<(u64, u64)> {
        let b = self.budget_of(pid).ok_or(Error::Dead)?;
        let start = self.alloc_va(pid, n)?;
        let tables = self.tables_needed(pid, start..start + n);
        self.charge(b, n.checked_add(tables).ok_or(Error::OutOfMemory)?)?;
        let first = self.next_frame;
        for i in 0..n {
            // Contiguous (dma_alloc): fresh frames from the top of what was ever used.
            let f = if contiguous {
                self.next_frame += 1;
                self.frames.insert(first + i, Frame { payer: b, content: 0 });
                first + i
            } else {
                self.alloc_frame(b)
            };
            self.map_page(
                pid,
                start + i,
                Mapping { backing: Backing::Frame(f), flags, state: MapState::Own },
            );
        }
        Ok((start * PAGE_SIZE, RAM_BASE + first * PAGE_SIZE))
    }

    /// `map_anon(len, flags) -> addr`: pages charged; not W+X; zeroed.
    pub fn map_anon(&mut self, pid: u64, len: u64, flags: u64) -> R<u64> {
        decode_flags(flags, false)?;
        if len == 0 || !len.is_multiple_of(PAGE_SIZE) {
            return Err(Error::InvalidArgument);
        }
        check_flags(flags, false)?;
        self.map_fresh(pid, len / PAGE_SIZE, flags, false).map(|x| x.0)
    }

    /// `unmap(addr, len)`: own mapping; not currently lent.
    pub fn unmap(&mut self, pid: u64, addr: u64, len: u64) -> R<()> {
        let (first, n) = user_range(addr, len)?;
        self.own_range(pid, first, n, |_| true)?;
        for v in first..first + n {
            let m = self.unmap_page(pid, v).unwrap();
            if let Backing::Frame(f) = m.backing {
                self.free_frame(f);
            }
        }
        Ok(())
    }

    /// `set_flags(addr, len, flags)`: own mapping; not W+X; not W without R.
    pub fn set_flags(&mut self, pid: u64, addr: u64, len: u64, flags: u64) -> R<()> {
        decode_flags(flags, self.broken(Mutation::R11SetFlagsAllowsWx))?;
        let (first, n) = user_range(addr, len)?;
        check_flags(flags, self.broken(Mutation::R11AllowsWriteOnly))?;
        self.own_range(pid, first, n, |_| true)?;
        let p = self.processes.get_mut(&pid).unwrap();
        for v in first..first + n {
            p.space.get_mut(&v).unwrap().flags = flags;
        }
        Ok(())
    }

    /// `map_fixed(addr, len, flags)`: as `map_anon`, but at exactly `addr`; never replaces a
    /// mapping (KERNEL-SPEC.md R11, answer 172). Same order as the kernel: decode flags, the
    /// range (`user_range`, page 0 included: K5a-addr0), the whole range's overlap with any of
    /// `pid`'s mappings (`range_free`, before anything is charged), the flags rule, then the
    /// charge -- pages alone first, cheaply (P1-2/N2: a hostile `map_fixed(0, USER_TOP)` must
    /// stay fast, never walking `tables_needed` over pages it was never going to afford).
    pub fn map_fixed(&mut self, pid: u64, addr: u64, len: u64, flags: u64) -> R<()> {
        decode_flags(flags, false)?;
        let (first, n) = user_range(addr, len)?;
        if !self.range_free(pid, first, n) && !self.broken(Mutation::R11MapFixedSkipsOverlap) {
            return Err(Error::InvalidArgument);
        }
        check_flags(flags, false)?;
        let b = self.budget_of(pid).ok_or(Error::Dead)?;
        if n > self.free_pages(b) {
            return Err(Error::OutOfMemory);
        }
        let tables = self.tables_needed(pid, first..first + n);
        self.charge(b, n.checked_add(tables).ok_or(Error::OutOfMemory)?)?;
        for i in 0..n {
            let f = self.alloc_frame(b);
            self.map_page(pid, first + i, Mapping { backing: Backing::Frame(f), flags, state: MapState::Own });
        }
        Ok(())
    }

    /// `map_device(h(MMIO)) -> addr`: MMIO device handle. The range is mapped read-write. Device
    /// pages are not RAM; their page tables are charged.
    pub fn map_device(&mut self, pid: u64, h: u64) -> R<u64> {
        let h = decode_handle(h)?;
        let Object::Device(d) = self.lookup(pid, h)?.object else { return Err(Error::WrongObject) };
        let DeviceKind::Mmio { pages, .. } = self.devices[&d].kind else { return Err(Error::WrongObject) };
        let start = self.alloc_va(pid, pages)?;
        let tables = self.tables_needed(pid, start..start + pages);
        self.charge(self.budget_of(pid).unwrap(), tables)?;
        for i in 0..pages {
            let backing = Backing::Device { device: d, page: i };
            self.map_page(pid, start + i, Mapping { backing, flags: FLAG_R | FLAG_W, state: MapState::Own });
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
        self.map_fresh(pid, npages, FLAG_R | FLAG_W, true)
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

    /// `thread_exit`. The last thread's exit ends the process as `process_exit(0)` would (README
    /// answer 170), `faulted` if it holds open calls.
    pub fn thread_exit(&mut self, pid: u64, tid: u64) {
        if self.processes.get(&pid).is_some_and(|p| p.threads.len() == 1) {
            self.exit_as(pid, tid, 0, false);
        } else {
            self.end_thread(tid);
        }
    }

    /// `process_exit(code)`: exit notice `exited`, or `faulted` while the process holds open calls.
    pub fn process_exit(&mut self, pid: u64, tid: u64, code: u64) -> R<()> {
        if code > U32_MAX {
            return Err(Error::InvalidArgument);
        }
        self.exit_as(pid, tid, code, false);
        Ok(())
    }

    /// `process_create(h(budget), h(exit endpoint)) -> h(process)`: the budget's weight not 0;
    /// the exit endpoint's badge 0 (QUESTIONS 93); the budget's process and page limits (the root
    /// page table); the process object charged to the caller (QUESTIONS 74). The new handle is
    /// stamped with the caller's budget (R9).
    pub fn process_create(&mut self, pid: u64, budget: u64, exit_endpoint: u64) -> R<u64> {
        let budget = decode_handle(budget)?;
        let exit_endpoint = decode_handle(exit_endpoint)?;
        let b = self.lookup_budget(pid, budget)?;
        let (endpoint, exit) = self.lookup_endpoint(pid, exit_endpoint)?;
        let bx = &self.budgets[&b];
        // A budget with weight 0 cannot hold a process (QUESTIONS 12).
        if bx.weight == 0 && !self.broken(Mutation::ProcessInWeightlessBudget) {
            return Err(Error::InvalidArgument);
        }
        // Only a receive right names an exit endpoint: otherwise anyone could spray notices.
        if exit.badge != 0 && !self.broken(Mutation::ExitEndpointBadged) {
            return Err(Error::NotPermitted);
        }
        if bx.processes_used >= bx.processes_limit {
            return Err(Error::OutOfProcesses);
        }
        let tables = self.page_table_cost() + self.costs.contexts;
        self.charge(b, tables)?;
        let caller_budget = self.budget_of(pid).unwrap();
        let object = if self.broken(Mutation::R6ProcessObjectFree) { 0 } else { self.costs.process };
        let payer = if self.broken(Mutation::R6ProcessObjectChargedToBudget) { b } else { caller_budget };
        if let Err(e) = self.charge(payer, object) {
            self.uncharge(b, tables);
            return Err(e);
        }
        let Some(child) = self.draw_pid() else {
            self.uncharge(b, tables);
            self.uncharge(payer, object);
            return Err(Error::OutOfProcesses);
        };
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
                tables: BTreeMap::new(),
                exit_endpoint: Some(exit),
                creator: payer,
                next_msg_id: 1,
            },
        );
        match self.install(pid, &[h]) {
            Ok(v) => {
                self.budgets.get_mut(&b).unwrap().processes_used += 1;
                // Ghost: the object is the caller's, read from its process object.
                let slot = Slot {
                    payer: self.processes[&pid].budget,
                    endpoint,
                    stamp: exit.stamp,
                    badge: exit.badge,
                };
                self.ghost.process_created(child, slot);
                self.notes.push(Note::Process { creator: pid, h: v[0], pid: child });
                Ok(v[0])
            }
            Err(e) => {
                self.processes.remove(&child);
                self.uncharge(b, tables);
                self.uncharge(payer, object);
                Err(e)
            }
        }
    }

    /// `process_map(h(process), src, dst, len, flags)`: process not started; src owned by the
    /// caller; pages move to the child's budget; not W+X.
    pub fn process_map(&mut self, pid: u64, process: u64, src: u64, dst: u64, len: u64, flags: u64) -> R<()> {
        let process = decode_handle(process)?;
        decode_flags(flags, false)?;
        let child = self.lookup_process(pid, process)?;
        let (s, n) = user_range(src, len)?;
        let (d, _) = user_range(dst, len)?;
        self.own_range(pid, s, n, |m| matches!(m.backing, Backing::Frame(_)))?;
        let cp = self.processes.get(&child);
        if cp.is_some_and(|p| (d..d + n).any(|v| p.space.contains_key(&v))) {
            return Err(Error::InvalidArgument);
        }
        check_flags(flags, self.broken(Mutation::R11AllowsWriteOnly))?;
        let cp = cp.ok_or(Error::NotPermitted)?;
        if cp.started {
            return Err(Error::NotPermitted);
        }
        let (from, to) = (self.budget_of(pid).unwrap(), cp.budget);
        let tables = self.tables_needed(child, d..d + n);
        self.charge(to, tables + if from != to { n } else { 0 })?;
        if from != to {
            self.uncharge(from, n);
        }
        for i in 0..n {
            let m = self.unmap_page(pid, s + i).unwrap();
            if let Backing::Frame(f) = m.backing {
                self.frames.get_mut(&f).unwrap().payer = to;
            }
            self.map_page(child, d + i, Mapping { flags, ..m });
        }
        Ok(())
    }

    /// `process_start(h(process), entry, sp, arg, handles)`: not started; handles copied into
    /// slots 1..n (at most `MAX_START_HANDLES`, QUESTIONS 10). The child's table and first thread
    /// are charged to the child's budget. `arg` (the startup page's address, QUESTIONS 40) is not
    /// checked; it reaches the first thread, which the model does not run.
    pub fn process_start(
        &mut self,
        pid: u64,
        process: u64,
        _entry: u64,
        _sp: u64,
        _arg: u64,
        handles: &[u64],
    ) -> R<()> {
        let process = decode_handle(process)?;
        if handles.len() > MAX_START_HANDLES {
            return Err(Error::TooLarge);
        }
        for h in handles {
            decode_handle(*h)?;
        }
        let child = self.lookup_process(pid, process)?;
        let mut hs = Vec::new();
        for h in handles {
            hs.push(self.lookup(pid, *h)?);
        }
        if self.processes.get(&child).is_none_or(|p| p.started) {
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
        // Decoding, in register order: the source (a message id of 0 is malformed), the badge (0
        // is refused, QUESTIONS 15), the optional budget.
        match source {
            MintSource::Handle(h) => {
                decode_handle(h)?;
            }
            MintSource::Message(0) => return Err(Error::InvalidArgument),
            MintSource::Message(_) => {}
        }
        if badge == 0 {
            return Err(Error::InvalidArgument);
        }
        let budget = decode_optional_handle(budget)?;
        // Arguments: the endpoint and the default stamp (a dead message's is `Dead`, one of the
        // spec's two stated exceptions), then the budget.
        let (e, default, source_badge) = match source {
            MintSource::Message(rid) => {
                let open = if self.broken(Mutation::MintFromUnservedMessage) {
                    self.msgs
                        .values()
                        .find(|x| x.rid == rid && x.server.is_some_and(|s| s.0 == pid))
                        .map(|x| x.id)
                } else {
                    self.open_call_of(tid, rid)
                };
                let Some(msg) = open.and_then(|m| self.msgs.get(&m)) else {
                    return Err(Error::InvalidArgument);
                };
                if !self.endpoints.contains_key(&msg.endpoint) || !self.budgets.contains_key(&msg.stamp) {
                    return Err(Error::Dead);
                }
                (msg.endpoint, msg.stamp, 0)
            }
            MintSource::Handle(h) => {
                let (e, hd) = self.lookup_endpoint(pid, h)?;
                (e, hd.stamp, hd.badge)
            }
        };
        let narrow = match budget {
            Some(bh) => Some(self.lookup_budget(pid, bh)?),
            None => None,
        };
        // Permission: only a receive right mints, and a budget handle only narrows (the default
        // stamp or a descendant of it).
        if source_badge != 0 {
            return Err(Error::NotPermitted);
        }
        let mut stamp = default;
        if let Some(b) = narrow {
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
        let out = self.install(pid, &[h])?[0];
        self.ghost.flows.push(Flow::Minted { pid, tid, endpoint: e, via: source, default_stamp: default });
        Ok(out)
    }

    /// Checks shared by `call` and `send` up to queueing: decoding (the endpoint and buffer
    /// registers, then the body), the endpoint, the handles, the buffer, R1, then R2's cap.
    /// Returns the endpoint, the copied handles and the buffer range.
    #[allow(clippy::type_complexity)]
    fn check_message(
        &mut self,
        pid: u64,
        h: u64,
        handles: &[u64],
        buffer: Option<Buffer>,
        lend: bool,
    ) -> R<(u64, Vec<Handle>, Option<(u64, u64)>)> {
        let h = decode_handle(h)?;
        let buffer = decode_range(buffer)?;
        if handles.len() > MAX_MSG_HANDLES {
            return Err(Error::TooLarge);
        }
        for x in handles {
            decode_handle(*x)?;
        }
        let (e, via) = self.lookup_endpoint(pid, h)?;
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
        // R1: between two user budgets, only equal label sets; the receiving side is the
        // endpoint's owner (QUESTIONS 4, 46).
        let sender = &self.budgets[&self.budget_of(pid).unwrap()];
        let owner_id = self.endpoints[&e].owner;
        let mut owner = &self.budgets[&owner_id];
        if self.broken(Mutation::R1ChecksReceiverNotOwner) {
            if let Some(r) = self.endpoints[&e].receivers.front() {
                owner = &self.budgets[&self.budget_of(self.threads[r].pid).unwrap()];
            }
        }
        let sender_class = if self.broken(Mutation::R1SenderClassFromStamp) {
            self.budgets.get(&via.stamp).map_or(sender.class, |b| b.class)
        } else {
            sender.class
        };
        if sender_class == Class::User
            && owner.class == Class::User
            && sender.labels != owner.labels
            && !self.broken(Mutation::R1SkipLabelCheck)
        {
            // Ghost: the refusal, as the sender's and the owner's budget objects describe them.
            let owner = &self.budgets[&owner_id];
            self.ghost.flows.push(Flow::LabelDenied {
                from_class: sender.class,
                from: self.ghost.labels(sender.id),
                to_class: owner.class,
                to: self.ghost.labels(owner_id),
            });
            return Err(Error::LabelDenied);
        }
        let account = if self.broken(Mutation::MsgAccountZero) { 0 } else { sender.account };
        let labels = if self.broken(Mutation::MsgNoLabels) { Vec::new() } else { sender.labels.clone() };
        let key = self.queue_key(account, &labels, via.stamp, sender.id);
        let waiting = self.endpoints[&e].queue.get(&key).map_or(0, |q| q.len() as u64);
        if waiting >= WAIT_CAP && !self.broken(Mutation::R2NoWaitCap) {
            // Ghost: the group as the sender's budget object gives it.
            let ghost_key = crate::ghost::group(sender.account, self.ghost.labels(sender.id), sender.id);
            self.ghost.flows.push(Flow::Busy { endpoint: e, key: ghost_key });
            return Err(Error::Busy);
        }
        Ok((e, hs, range))
    }

    /// Queue a message on its endpoint, block the sender, and try to deliver.
    #[allow(clippy::too_many_arguments)]
    fn enqueue(
        &mut self,
        pid: u64,
        tid: u64,
        kind: MsgKind,
        h: u64,
        e: u64,
        words: [u64; WORDS],
        hs: Vec<Handle>,
        range: Option<(u64, u64)>,
        timeout: u64,
    ) -> Outcome {
        let b = self.budget_of(pid).unwrap();
        let bx = &self.budgets[&b];
        let account = if self.broken(Mutation::MsgAccountZero) { 0 } else { bx.account };
        let labels = if self.broken(Mutation::MsgNoLabels) { Vec::new() } else { bx.labels.clone() };
        let hd = self.processes[&pid].handles[&h];
        let stamp = if self.broken(Mutation::R9MsgStampIsSenderBudget) { b } else { hd.stamp };
        let id = self.next_msg; // ids ascend, so the oldest queued message is the lowest id
        self.next_msg += 1;
        // Ghost: the message as the sender's objects describe it.
        let owner = self.endpoints[&e].owner;
        // I12: message ids are non-zero and never reused.
        if id == 0 || self.ghost.sent.contains_key(&id) {
            self.ghost.violations.push(alloc::format!("I12: message id {id} reused"));
        }
        self.ghost.sent.insert(
            id,
            Sent {
                kind,
                sender_budget: b,
                sender_tid: tid,
                sender_class: bx.class,
                labels: self.ghost.labels(b),
                account: bx.account,
                endpoint: e,
                owner_class: self.budgets[&owner].class,
                owner_labels: self.ghost.labels(owner),
                badge: hd.badge,
                stamp: hd.stamp,
                lent_pages: if kind == MsgKind::Call { range.map_or(0, |r| r.1) } else { 0 },
                handles: hs.clone(),
            },
        );
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
        let key = self.queue_key(account, &labels, hd.stamp, b);
        self.msgs.insert(
            id,
            Msg {
                id,
                rid: 0,
                kind,
                sender_pid: pid,
                sender_tid: tid,
                sender_budget: b,
                endpoint: e,
                badge: hd.badge,
                stamp,
                account,
                labels,
                key: key.clone(),
                words,
                handles: hs.into_iter().map(Some).collect(),
                buffer,
                server: None,
                caller_waiting: kind == MsgKind::Call,
                open_payer: None,
                abandoned: false,
                notice: false,
            },
        );
        self.endpoints.get_mut(&e).unwrap().queue.entry(key).or_default().push_back(id);
        self.block(tid, Wait::Send(id), timeout);
        self.poke(e);
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
        self.ghost.calls.insert(tid, (lend.is_some_and(|b| b.addr != 0 || b.npages != 0), false, false));
        let disposition = if lend.is_some_and(|b| b.addr != 0 || b.npages != 0) {
            LendDisposition::Returned
        } else {
            LendDisposition::None
        };
        let decoded = decode_handle(h).and_then(|_| {
            if lend.is_some_and(|b| (b.addr == 0) != (b.npages == 0)) || !self.record_valid(pid, tid, false) {
                Err(Error::InvalidArgument)
            } else {
                Ok(())
            }
        });
        let result = decoded.and_then(|_| self.check_message(pid, h, handles, lend, true));
        match result {
            Err(e) => Outcome::Done(Ok(Ret::Call(CallCompletion {
                status: Err(e),
                lend: disposition,
                reply: None,
            }))),
            Ok((e, hs, range)) => {
                self.threads.get_mut(&tid).unwrap().call_lend = Some(disposition);
                let outcome = self.enqueue(pid, tid, MsgKind::Call, h, e, words, hs, range, timeout);
                if let Outcome::Done(result) = outcome {
                    self.threads.get_mut(&tid).unwrap().call_lend = None;
                    Outcome::Done(Ok(Ret::Call(CallCompletion {
                        status: result.map(|_| ()),
                        lend: disposition,
                        reply: None,
                    })))
                } else {
                    outcome
                }
            }
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
        if let Err(e) = decode_handle(h) {
            return Outcome::Done(Err(e));
        }
        if transfer.is_some_and(|b| (b.addr == 0) != (b.npages == 0)) || !self.input_record_valid(pid, tid) {
            return Outcome::Done(Err(Error::InvalidArgument));
        }
        match self.check_message(pid, h, handles, transfer, false) {
            Err(e) => Outcome::Done(Err(e)),
            Ok((e, hs, range)) => self.enqueue(pid, tid, MsgKind::Send, h, e, words, hs, range, timeout),
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
        if let Err(e) = decode_optional_handle(h) {
            return Outcome::Done(Err(e));
        }
        if !self.record_valid(pid, tid, false) {
            return Outcome::Done(Err(Error::InvalidArgument));
        }
        // Whatever it returns, the thread has no current call until it takes one (QUESTIONS 82).
        if !self.broken(Mutation::ReceiveKeepsCurrent) {
            self.threads.get_mut(&tid).unwrap().current = None;
        }
        self.ghost.receive_begins(tid);
        let h = match decode_optional_handle(h) {
            Ok(Some(h)) => h,
            Ok(None) => {
                self.block(tid, Wait::Sleep, timeout);
                return Outcome::Blocked;
            }
            Err(e) => return Outcome::Done(Err(e)),
        };
        let hd = match self.lookup(pid, h) {
            Ok(x) => x,
            Err(e) => return Outcome::Done(Err(e)),
        };
        match hd.object {
            Object::Endpoint(e) => {
                // Only a badge-0 handle is a receive right.
                if hd.badge != 0 && !self.broken(Mutation::ReceiveWithBadgedHandle) {
                    return Outcome::Done(Err(Error::NotPermitted));
                }
                // At MAX_OPEN_CALLS the process takes no calls, but still sends and notices (R4a);
                // `pump` sees to it.
                if self.broken(Mutation::ReceiveDropsOpenCalls) {
                    self.threads.get_mut(&tid).unwrap().serving.clear();
                }
                self.ghost.receiving.insert(tid, Receiving { handle: hd, max_transfer });
                self.block(tid, Wait::Receive { endpoint: e, h, max_transfer }, timeout);
                self.endpoints.get_mut(&e).unwrap().receivers.push_back(tid);
                self.poke(e);
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

    /// `reply(msg_id, words, handles)`: `msg_id` is an open call of the caller's thread (a `send`
    /// never is, QUESTIONS 1); returns the lend (an abandoned call's is freed and its reply
    /// discarded, R3).
    pub fn reply(&mut self, pid: u64, tid: u64, msg_id: u64, words: [u64; WORDS], handles: &[u64]) -> R<Ret> {
        // Decoding: the message id register (0 is malformed), then the body.
        if msg_id == 0 {
            return Err(Error::InvalidArgument);
        }
        if !self.input_record_valid(pid, tid) {
            return Err(Error::InvalidArgument);
        }
        if handles.len() > MAX_MSG_HANDLES {
            return Err(Error::TooLarge);
        }
        for x in handles {
            decode_handle(*x)?;
        }
        let Some(msg_id) = self.open_call_of(tid, msg_id) else {
            return Err(Error::InvalidArgument);
        };
        let mut hs = Vec::new();
        for x in handles {
            hs.push(self.lookup(pid, *x)?);
        }
        let t = self.threads.get_mut(&tid).unwrap();
        t.serving.retain(|m| *m != msg_id);
        if t.current == Some(msg_id) {
            t.current = None;
        }
        self.ghost.replied(tid, msg_id);
        let m = self.msgs[&msg_id].clone();
        if !m.caller_waiting {
            self.unmap_lend_in_server(msg_id, true);
            self.close_call(msg_id);
            return Ok(Ret::Replied { delivered: false, installed_mask: 0 });
        }
        // Return the lend before inspecting the output record, which may be inside it.
        self.unmap_lend_in_server(msg_id, false);
        self.close_call(msg_id);
        let before = self.processes[&m.sender_pid].handles.clone();
        let output_valid = self.record_valid(m.sender_pid, m.sender_tid, true);
        let mut installed = Vec::new();
        let mut mask = 0;
        let mut status = Ok(());
        for (i, h) in hs.iter().enumerate() {
            match self.install(m.sender_pid, &[*h]) {
                Ok(slots) => {
                    installed.push(slots[0]);
                    mask |= 1 << i;
                }
                Err(_) => {
                    installed.push(NO_HANDLE);
                    status = Err(Error::OutOfMemory);
                }
            }
        }
        let valid = output_valid || self.broken(Mutation::IpcSkipOutputCheck);
        let reply = if valid {
            if status.is_err() && self.broken(Mutation::IpcDropPartial) {
                None
            } else {
                Some(ReplyRecord { words, handles: installed })
            }
        } else {
            for h in installed {
                if h != NO_HANDLE && !self.broken(Mutation::IpcLeakRollback) {
                    self.remove_handle(m.sender_pid, h);
                }
            }
            status = Err(Error::InvalidArgument);
            mask = 0;
            None
        };
        let lend = self.threads[&m.sender_tid].call_lend.unwrap_or(LendDisposition::None);
        let completion = CallCompletion { status, lend, reply };
        let delivered = valid || self.broken(Mutation::IpcFalseDelivery);
        self.ghost.flows.push(Flow::ReplyCompletion {
            before,
            after: self.processes[&m.sender_pid].handles.clone(),
            supplied: hs,
            output_valid,
            completion: completion.clone(),
            delivered,
            mask,
        });
        self.wake(m.sender_tid, Ok(Ret::Call(completion)));
        Ok(Ret::Replied { delivered, installed_mask: mask })
    }

    /// `serve(msg_id)`: `msg_id` is an open call of the caller's thread; it becomes the thread's
    /// current call, the one a fault blames (QUESTIONS 82).
    pub fn serve(&mut self, tid: u64, msg_id: u64) -> R<()> {
        if msg_id == 0 {
            return Err(Error::InvalidArgument);
        }
        let Some(m) = self.open_call_of(tid, msg_id) else { return Err(Error::InvalidArgument) };
        if !self.broken(Mutation::ServeIgnored) {
            self.threads.get_mut(&tid).unwrap().current = Some(m);
        }
        self.ghost.served_now(tid, m);
        Ok(())
    }

    /// `handle_close(h)`.
    pub fn handle_close(&mut self, pid: u64, h: u64) -> R<()> {
        let h = decode_handle(h)?;
        self.lookup(pid, h)?;
        self.remove_handle(pid, h);
        Ok(())
    }

    /// `budget_create(h(parent), pages, processes, weight, labels, account, deadline) -> h`:
    /// R6-R8; the class is the parent's (QUESTIONS 73); labels as the spec says; depth
    /// < `MAX_DEPTH`.
    #[allow(clippy::too_many_arguments)]
    pub fn budget_create(
        &mut self,
        pid: u64,
        parent: u64,
        pages: u64,
        processes: u64,
        weight: u64,
        labels: &[u64],
        account: u64,
        deadline: u64,
    ) -> R<u64> {
        // Decoding: the parent register, then the BudgetSpec record in slot order (processes and
        // weight are u32; label count is checked first, before deduplication).
        let parent = decode_handle(parent)?;
        if labels.len() > MAX_LABELS {
            return Err(Error::TooLarge);
        }
        if processes > U32_MAX || weight > U32_MAX {
            return Err(Error::InvalidArgument);
        }
        let p = self.lookup_budget(pid, parent)?;
        let px = self.budgets[&p].clone();
        let caller_budget = self.budget_of(pid).unwrap();
        let caller = self.budgets[&caller_budget].class;
        if px.depth + 1 >= MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        // Labels are sorted and deduplicated, a superset of the parent's (README choice 3);
        // adding labels needs the caller's own budget to be class `system`.
        let mut labels = labels.to_vec();
        labels.sort_unstable();
        labels.dedup();
        if !superset(&labels, &px.labels) {
            return Err(Error::LabelDenied);
        }
        let adder = if self.broken(Mutation::LabelsAddedByParentClass) { px.class } else { caller };
        if labels != px.labels && adder != Class::System {
            return Err(Error::ClassDenied);
        }
        // R6/R7: carve from the parent's free limits, with the budget's own page (QUESTIONS 76).
        let carve_check = !self.broken(Mutation::R7NoCarveCheck);
        let own = if self.broken(Mutation::R6OwnPageChargedToItself) { 0 } else { self.costs.budget };
        if pages.saturating_add(own) > self.free_pages(p) && (carve_check || own > self.free_pages(p)) {
            return Err(Error::OutOfMemory);
        }
        if carve_check && processes > px.processes_limit.saturating_sub(px.processes_used) {
            return Err(Error::OutOfProcesses);
        }
        // README choice 2.
        if carve_check && weight > px.weight.saturating_sub(px.weight_used) {
            return Err(Error::InvalidArgument);
        }
        // R8: the parent's account, unless it is 0 (then the argument; README choice 4).
        let account = if px.account != 0 && !self.broken(Mutation::R8AccountFromArgument) {
            px.account
        } else {
            account
        };
        let class = if self.broken(Mutation::ClassNotInherited) { caller } else { px.class };
        let deadline = if deadline == FOREVER { None } else { Some(deadline) }; // README choice 17
        let l = Limits { pages, processes, weight };
        let id = self.new_budget(Some(p), class, labels, account, deadline, l, caller);
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

    /// `budget_usage(h(budget)) -> counters`: R1, a flow from the target to the caller's budget:
    /// the caller's labels ⊇ the target's, unless the caller's budget is class `system`.
    pub fn budget_usage(&mut self, pid: u64, h: u64) -> R<Counters> {
        let h = decode_handle(h)?;
        let b = self.lookup_budget(pid, h)?;
        let caller = &self.budgets[&self.budget_of(pid).unwrap()];
        let target = &self.budgets[&b];
        let exempt = if self.broken(Mutation::R1UsageExemptBySystemTarget) {
            target.class == Class::System
        } else {
            caller.class == Class::System || self.broken(Mutation::R1UsageIgnoresLabels)
        };
        if !exempt && !superset(&caller.labels, &target.labels) {
            self.ghost.flows.push(Flow::UsageDenied {
                from: self.ghost.labels(target.id),
                to_class: caller.class,
                to: self.ghost.labels(caller.id),
            });
            return Err(Error::LabelDenied);
        }
        self.ghost.flows.push(Flow::Usage {
            from: self.ghost.labels(target.id),
            to_class: caller.class,
            to: self.ghost.labels(caller.id),
        });
        Ok(Counters {
            pages_limit: target.pages_limit,
            pages_usage: target.pages_used,
            processes_limit: target.processes_limit,
            processes_usage: target.processes_used,
            weight_limit: target.weight,
            weight_usage: target.weight_used,
        })
    }

    /// `time_now() -> µs`.
    pub fn time_now(&self) -> u64 { self.now }

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
}

#[cfg(test)]
mod lifetime_regressions {
    use super::*;
    use crate::invariants;

    fn checked(k: &mut Kernel, pid: u64, tid: u64, call: Syscall) -> Step {
        let s = k.step(&Op::Sys { pid, tid, call }).unwrap();
        invariants::check(k).unwrap();
        s
    }

    #[test]
    fn pid_reuse_after_notice_drop_retires_only_the_finished_lifetime() {
        for destroy_creator in [false, true] {
            for destroy_before_exit in [false, true] {
                let mut k = Kernel::boot(&Boot::default(), None).unwrap();
                let ep = k.endpoint_create(INIT_PID).unwrap();
                let Object::Endpoint(e) = k.processes[&INIT_PID].handles[&ep].object else { panic!() };
                let creator_budget = k.budget_create(INIT_PID, USERS, 32, 1, 10, &[], 0, FOREVER).unwrap();
                let creator_h = k.process_create(INIT_PID, creator_budget, ep).unwrap();
                let Object::Process(creator) = k.processes[&INIT_PID].handles[&creator_h].object else {
                    panic!()
                };
                k.process_start(INIT_PID, creator_h, 0, 0, 0, &[SYSTEM, ep]).unwrap();
                // Control the allocator's random stream in this private unit test, without a
                // production API. The second allocation must try the exact same candidate.
                let candidate_stream = k.pid_rng;
                let child_h = k.process_create(creator, 1, 2).unwrap();
                let Object::Process(child) = k.processes[&creator].handles[&child_h].object else { panic!() };
                k.process_start(creator, child_h, 0, 0, 0, &[]).unwrap();
                let tid = *k.processes[&child].threads.first().unwrap();
                checked(&mut k, child, tid, Syscall::ThreadExit);
                assert!(k.ghost.owed.contains_key(&child));
                assert!(k.ghost.slots.contains_key(&child));
                // Trying the old candidate while its notice waits must choose another PID.
                k.pid_rng = candidate_stream;
                assert_ne!(k.draw_pid().unwrap(), child);
                if destroy_creator {
                    checked(&mut k, INIT_PID, 1, Syscall::BudgetDestroy { h: creator_budget });
                    assert!(k.endpoints.contains_key(&e)); // The old endpoint deliberately survives.
                } else {
                    k.destroy_endpoint(e);
                    invariants::check(&k).unwrap();
                }
                assert!(!k.ghost.owed.contains_key(&child));
                assert!(!k.ghost.slots.contains_key(&child));
                assert!(!k.endpoints.values().any(|e| e.exits.iter().any(|n| n.pid == child)));
                assert!(
                    !k.processes
                        .values()
                        .any(|p| { p.handles.values().any(|h| h.object == Object::Process(child)) })
                );

                let new_ep = k.endpoint_create(INIT_PID).unwrap();
                let Object::Endpoint(new_e) = k.processes[&INIT_PID].handles[&new_ep].object else {
                    panic!()
                };
                let pages_before = k.budgets[&ROOT].pages_used;
                k.pid_rng = candidate_stream;
                let new_h = k.process_create(INIT_PID, SYSTEM, new_ep).unwrap();
                assert_eq!(k.processes[&INIT_PID].handles[&new_h].object, Object::Process(child));
                k.process_start(INIT_PID, new_h, 0, 0, 0, &[]).unwrap();
                if destroy_before_exit {
                    k.destroy_endpoint(new_e);
                }
                let tid = *k.processes[&child].threads.first().unwrap();
                checked(&mut k, child, tid, Syscall::ThreadExit);
                if !destroy_before_exit {
                    assert!(k.ghost.owed.contains_key(&child));
                    k.destroy_endpoint(new_e);
                    invariants::check(&k).unwrap();
                }
                assert!(!k.ghost.owed.contains_key(&child));
                assert!(!k.ghost.slots.contains_key(&child));
                assert!(!k.processes[&INIT_PID].handles.contains_key(&new_h));
                assert_eq!(k.budgets[&ROOT].pages_used, pages_before - k.costs.endpoint);
            }
        }
    }

    #[test]
    fn ghost_rejects_reuse_and_free_before_notice_retirement() {
        let mut k = Kernel::boot(&Boot::default(), None).unwrap();
        let ep = k.endpoint_create(INIT_PID).unwrap();
        let h = k.process_create(INIT_PID, SYSTEM, ep).unwrap();
        let Object::Process(child) = k.processes[&INIT_PID].handles[&h].object else { panic!() };
        k.process_start(INIT_PID, h, 0, 0, 0, &[]).unwrap();
        let tid = *k.processes[&child].threads.first().unwrap();
        checked(&mut k, child, tid, Syscall::ThreadExit);
        let mut reused = k.ghost.clone();
        reused.process_created(child, reused.slots[&child]);
        assert!(reused.violations.iter().any(|v| v.contains("reused before")));
        let mut freed = k.ghost.clone();
        freed.process_freed(child);
        assert!(freed.violations.iter().any(|v| v.contains("still owed")));
    }
}

#[cfg(test)]
mod tick_equivalence {
    extern crate std;
    use alloc::format;

    use super::*;
    use crate::gen::Gen;
    fn reference_tick(k: &mut Kernel, dt: u64) {
        let end = k.now.saturating_add(dt);
        while k.now < end {
            let pick = k.sched.pick();
            // With one budget runnable and nothing due, every whole slice until the next event
            // goes to it alike: charge them at once (the same passes as slice by slice), so that a
            // long tick costs the model no more than a short one.
            let until = k.next_event().filter(|e| *e > k.now).map_or(end, |e| e.min(end));
            let slices = (until - k.now) / SLICE;
            if let Some((b, _)) = pick.filter(|_| slices > 1 && k.sched.runnable_budgets() == 1) {
                k.now += slices * SLICE;
                k.sched.charge_slices(b, slices);
                k.expire();
                continue;
            }
            let mut run = end - k.now;
            if pick.is_some() {
                run = run.min(SLICE);
            }
            if let Some(e) = k.next_event() {
                if e > k.now {
                    run = run.min(e - k.now);
                }
            }
            k.now += run;
            if let Some((b, _)) = pick {
                k.sched.charge(b, run);
            }
            k.expire();
        }
    }

    fn compare(k: &Kernel, dt: u64) {
        assert!(dt <= MAX_TICK);
        let mut fast = k.clone();
        let mut reference = k.clone();
        fast.tick(dt);
        reference_tick(&mut reference, dt);
        // Compare every field, including scheduler passes/queues, pending delivery, wakes,
        // notes and ghost records. Both paths retain all ordinary property checks elsewhere.
        assert_eq!(format!("{fast:?}"), format!("{reference:?}"), "dt={dt}, mutation={:?}", k.mutation);
    }

    #[test]
    fn event_free_tick_matches_slice_reference() {
        let mut boundary_cases = 0;
        for mutation in core::iter::once(None).chain(Mutation::ALL.into_iter().map(Some)) {
            for kind in 0..4 {
                let mut k = Kernel::boot(&Boot::default(), mutation).unwrap();
                let ep = k.endpoint_create(INIT_PID).unwrap();
                // All but the one-budget control have two runnable budgets.
                if kind != 0 {
                    let child = k.process_create(INIT_PID, SYSTEM, ep).unwrap();
                    k.process_start(INIT_PID, child, 0, 0, 0, &[]).unwrap();
                }
                match kind {
                    0 => {
                        k.thread_create(INIT_PID, 0, 0, 0).unwrap();
                    }
                    1 => {
                        let h = k.mint(INIT_PID, 1, MintSource::Handle(ep), 7, None).unwrap();
                        let receiver = k.thread_create(INIT_PID, 0, 0, 0).unwrap();
                        k.thread_create(INIT_PID, 0, 0, 0).unwrap();
                        k.receive(INIT_PID, receiver, Some(ep), FOREVER, 0);
                        k.call(INIT_PID, 1, h, [0; WORDS], &[], None, FOREVER);
                        assert!(!k.to_pump.is_empty());
                    }
                    2 => {
                        k.budget_create(INIT_PID, USERS, 4, 0, 1, &[], 0, 5000).unwrap();
                        let later = k.thread_create(INIT_PID, 0, 0, 0).unwrap();
                        k.thread_create(INIT_PID, 0, 0, 0).unwrap();
                        k.receive(INIT_PID, 1, None, 5000, 0);
                        k.receive(INIT_PID, later, None, 7000, 0);
                    }
                    3 => k.now = u64::MAX - 35_000,
                    _ => unreachable!(),
                }
                for dt in
                    [0, 1, SLICE - 1, SLICE, SLICE + 1, 2 * SLICE, 2 * SLICE + 1, 5000, 7000, 7500, 35_001]
                {
                    compare(&k, dt);
                    boundary_cases += 1;
                }
                let before = format!("{k:?}");
                assert!(k.step(&Op::Tick { dt: u64::MAX }).is_none());
                assert_eq!(before, format!("{k:?}"));
                boundary_cases += 1;
            }
        }
        let mut history_ops = 0;
        let mut history_ticks = 0;
        for seed in 0..64 {
            let mut k = Kernel::boot(&Boot::testing(), None).unwrap();
            let mut generator = Gen::new(seed);
            let mut checker = crate::invariants::Checker::new(&k);
            for i in 0..150 {
                if k.halted.is_some() {
                    break;
                }
                let op = generator.next_op(&k);
                if let Op::Tick { dt } = op {
                    compare(&k, dt);
                    history_ticks += 1;
                }
                if i % 20 == 19 {
                    // Probe clones so a large tick cannot invalidate the generator's setup.
                    compare(&k, 60_000_123);
                    history_ticks += 1;
                }
                k.step(&op).unwrap();
                checker.check(&k).unwrap();
                history_ops += 1;
            }
        }
        std::eprintln!(
            "tick differential: {boundary_cases} boundary cases, 64 histories, {history_ops} history operations, {history_ticks} history tick comparisons; all {} mutations; no omitted histories",
            Mutation::ALL.len()
        );
    }
}
