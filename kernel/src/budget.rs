// SPDX-License-Identifier: MIT OR Apache-2.0

//! Budgets (kernel/budgets.md; R6-R10) and the per-process accounts that charge them.
//!
//! # Where things live
//! Every budget occupies one RAM frame of its own, allocated to `mem::OBJECT_OWNER`: that frame *is*
//! the page the cost table charges for it, so budgets need no kernel-static table whose size
//! another budget could exhaust (R7: an allocation fails only on the caller's own budget). A
//! budget is named by its frame's index in the page-ownership table ([`BudgetFrame`]); the tree
//! is linked through the frames by `parent` alone, and a subtree is found by scanning the object
//! frames for budgets below its top (as the model does), since a scan costs less than keeping
//! sibling links right.
//!
//! The per-process side ([`Account`]: which budget pays, how many threads and frames, the handle
//! table) is a fixed array indexed by PID, because PIDs are a fixed pool anyway (processes are
//! carved from `root`'s limit like pages, so that pool cannot be exhausted across budgets either).
//!
//! All of this is part of the [`MemoryManager`]: charging happens where frames change hands
//! (`alloc_page`, the ownership table), so the ledger must be reachable there without taking a
//! second kernel lock.
//!
//! # What is charged (the cost table)
//! A budget's own object costs its parent one page (R6). A process object's notice page
//! costs its creator one page; every thread's IPC page, which holds its saved registers too, costs
//! the execution budget one page. The process's header page costs the execution budget its one
//! frame on both widths, in addition to its page tables and mapped RAM (kernel/objects.md).
//! Frame charges follow ownership in `mem.rs`; handle-table pages are charged in `handle.rs`.

use redoubt_layout::Pid;
use redoubt_sys::{BudgetSpec, Error, FOREVER, MAX_DEPTH, MAX_LABELS, MAX_THREADS, Usage};

use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT, TidMask};
use crate::handle::{BudgetRef, Handle, HandleTable, Object};
use crate::kframe;
use crate::mem::MemoryManager;
use crate::ptable::ProcessTable;

/// A budget, named by the index of its frame in the page-ownership table.
pub type BudgetFrame = u32;

/// The cost table (kernel/objects.md, "What objects cost"), in pages.
pub const BUDGET_PAGES: u64 = 1;
pub const PROCESS_PAGES: u64 = 1;
pub const THREAD_PAGES: u64 = 1;

/// The weight `root` starts with. Weights only matter relative to each other (R12), so any
/// value works; this one leaves room to carve the manifest weights (1000 for `init`, the steward
/// and the drivers, 100 for a session; servers/init.md). It is fixed here until `init` builds the
/// tree from the manifest (kernel/budgets.md, "The tree from the boot manifest";
/// plan/m1-separation.md).
const ROOT_WEIGHT: u32 = 1_000_000;
/// What `root` keeps for `init` when carving `system` and `users` (kernel/budgets.md R7: a
/// budget holding a process has free weight).
const INIT_WEIGHT: u32 = 1000;
/// The pages `root` keeps for `init` to work in, beyond what the loader gave it and its first
/// thread: its stack's demand-paged pages, its own `map_anon` and the endpoints it owns
/// (kernel/budgets.md, "The tree from the boot manifest").
const INIT_PAGES: u64 = 1024;
/// `init`, the one process the loader starts (kernel/boot.md).
pub(crate) const INIT_PID: Pid = match Pid::new(2) {
    Some(pid) => pid,
    None => unreachable!(),
};

/// A budget's class (kernel/budgets.md, "Class is trust, not order"): inherited from its parent,
/// so never in the ABI.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    User = 1,
    System = 2,
}

/// A budget as the kernel works with it. It lives in its frame as words (`load`, `store`).
#[derive(Clone, Copy)]
pub struct Budget {
    pub id: u64,
    pub parent: Option<BudgetFrame>,
    /// The first budget carved from this one, and the next budget under the same parent (R10):
    /// a destruction walks the dying subtree through these instead of scanning every object
    /// frame ([Residual risks](#residual-risks)).
    pub first_child: Option<BudgetFrame>,
    pub next_sibling: Option<BudgetFrame>,
    /// The endpoints and devices charged to this budget, in one chain through their frames'
    /// [`OWNED_WORD`]; a destruction ends exactly its own.
    pub first_owned: Option<u32>,
    pub depth: u32,
    pub class: Class,
    /// Set by `budget_destroy` on the whole subtree before it tears anything down (R10).
    pub dying: bool,
    pub labels: [u64; MAX_LABELS],
    pub nlabels: usize,
    pub account: u64,
    /// Absolute µs since boot; `FOREVER` for none. When it passes, the kernel destroys the
    /// budget (`time.rs`).
    pub deadline: u64,
    /// The next budget on the kernel's list of budgets with a deadline (`Objects::deadlines`).
    pub next_deadline: Option<BudgetFrame>,
    pub pages_limit: u64,
    pub pages_used: u64,
    pub processes_limit: u32,
    pub processes_used: u32,
    pub weight_limit: u32,
    /// The children's weight limits (R7).
    pub weight_carved: u32,
    /// Its place in the stride queue (`sched.rs`).
    pub sched: redoubt_stride::State,
    /// The (pid, tid) it ran last, for round-robin among its threads.
    pub cursor: Option<(Pid, usize)>,
}

impl Budget {
    /// The labels it actually carries (R1, I6).
    pub fn labels_of(&self) -> &[u64] { &self.labels[..self.nlabels] }

    fn labels(&self) -> &[u64] { self.labels_of() }

    fn free_pages(&self) -> u64 { self.pages_limit.saturating_sub(self.pages_used) }

    fn free_processes(&self) -> u32 { self.processes_limit.saturating_sub(self.processes_used) }

    fn free_weight(&self) -> u32 { self.weight_limit.saturating_sub(self.weight_carved) }
}

/// First word of every budget frame, so that a frame read as a budget that is not one is caught.
const MAGIC: u64 = u64::from_le_bytes(*b"budget\0\0");
/// Words a budget takes in its frame, one per field (a frame has 512): `load` and `store` below.
/// The tree and owner links sit after the scheduling words; a frame has room to spare.
const W_FIRST_CHILD: usize = W_SCHED + 8;
const W_NEXT_SIBLING: usize = W_SCHED + 9;
const W_FIRST_OWNED: usize = W_SCHED + 10;
const WORDS: usize = W_SCHED + 11;
/// Where the scheduling words start, after the labels.
const W_SCHED: usize = 16 + MAX_LABELS;
/// A scratch word in every object frame: the next frame in `Objects::deferred`. Above every
/// object's own words. `None` while the frame is not deferred.
pub(crate) const DEFER_WORD: usize = 100;
/// The heads of the handle chains (`handle.rs`), above every object's own words too, so that
/// storing an object never touches them: the object chain's in a budget or a process object,
/// the stamp chain's in a budget. 0 for an empty chain; a new frame is zeroed.
pub(crate) const HELD_WORD: usize = 101;
pub(crate) const STAMPED_WORD: usize = 102;
/// The next endpoint or device in its owner's chain (`Budget::first_owned`), at one place in
/// both kinds' frames, so a destruction's owner walk reads it alone. 0 for the last.
pub(crate) const OWNED_WORD: usize = 103;
/// The budget's ready threads, as the scheduler counts them (`sched.rs`), above its own words so
/// storing a budget never touches it. 0 for a new budget; a new frame is zeroed.
const READY_WORD: usize = 104;

/// The frame index a `frame + 1` word names, or `None` for 0.
pub(crate) fn frame_of(word: u64) -> Option<u32> { (word as u32).checked_sub(1) }

/// A `frame + 1` word for a frame, 0 for none.
pub(crate) fn frame_word(frame: Option<u32>) -> u64 { frame.map_or(0, |f| u64::from(f) + 1) }

/// One process's side of the ledger. The kernel (PID 1) has none: it has no budget.
#[derive(Clone, Copy)]
pub struct Account {
    pub budget: Option<BudgetFrame>,
    pub threads: u64,
    /// RAM frames owned by the process and charged to its budget (page tables and mapped pages).
    pub frames: u64,
    pub handles: HandleTable,
    /// The physical address of the process's header page (`arch::process`), 0 until its address
    /// space has one. The header's table names each thread's IPC page (`message.rs`) by frame
    /// index, indexed by TID (`1..=MAX_THREADS`; entry 0 names no thread), 0 for a thread that
    /// has none. That page *is* the one the cost table charges for a thread: everything IPC
    /// needs (what the thread waits for, the message it is sending, the calls it holds open) and
    /// the thread's saved registers live there, so `call` and `send` never allocate.
    pub header: usize,
    /// The TIDs whose IPC page is in the header's table, bit `tid` for thread `tid`: what every
    /// walk of the threads iterates, so its cost follows the threads that exist
    /// ([`MemoryManager::live_tids`]).
    pub live: TidMask,
    /// Open calls this process's threads hold (R4a): at `MAX_OPEN_CALLS` it takes no more.
    pub open_calls: u32,
    /// The last message id its threads handed a sender, 0 before the first: ids are never 0,
    /// never reused within this process, and from no counter anyone else can see (I12).
    pub last_msg_id: u64,
    /// The DMA registry slots it has mapped with `map_device` (`dma.rs`): half of the
    /// set its death must reset. Zero again for a new process in the same PID.
    pub dma_mapped: u16,
    /// No thread of this process has a timeout earlier than this (`message::next_timeout`): only
    /// ever early, so expiry walks just the processes it might be due in.
    pub earliest_timeout: u64,
}

impl Account {
    /// A PID with no process: all zeros, so the table of accounts is `.bss`.
    const NONE: Account = Account {
        budget: None,
        threads: 0,
        frames: 0,
        handles: HandleTable::EMPTY,
        header: 0,
        live: TidMask::EMPTY,
        open_calls: 0,
        last_msg_id: 0,
        earliest_timeout: 0,
        dma_mapped: 0,
    };
}

/// The kernel's objects and the per-PID tables. Its starting value is all zeros, so it is `.bss`
/// and a table sized by a limit costs RAM, never image.
pub struct Objects {
    /// The last object id handed out, 0 before the first; ids are shared by budgets and
    /// endpoints, never 0 and never reused (I12), and a `u64` cannot run out.
    last_id: u64,
    /// The last value of the one order of message arrivals and takes (R2: "the group served least
    /// recently"), 0 before the first. A `u64` cannot run out, and userspace never sees it, so it
    /// is no covert channel.
    last_seq: u64,
    /// The highest frame ever given to a kernel object: where a scan for budgets stops.
    pub high_frame: u32,
    /// The first of the budgets with a deadline, linked through their frames
    /// (`Budget::next_deadline`), so finding the next deadline never scans every frame.
    deadlines: Option<BudgetFrame>,
    accounts: [Account; MAX_PROCESS_COUNT],
    /// The PIDs with an account, bit `account_index`: what every walk of the processes iterates
    /// ([`MemoryManager::live_pids`]), so its cost follows the processes that exist.
    live_pids: PidMask,
    /// Each PID's process object, by `account_index`: kept at `process_create` and `free_object`
    /// (`process.rs`), so finding one is a lookup, never a scan of the object frames (R12).
    pub(crate) processes: [Option<u32>; MAX_PROCESS_COUNT],
    /// The PIDs `processes` names an object for, bit `account_index`: what a walk of the process
    /// objects iterates, so its cost follows the objects that exist.
    pub(crate) process_pids: PidMask,
    /// Each interrupt's IRQ object: kept at `boot_devices` and `free_device` (`device.rs`), so an
    /// interrupt finds its object in one lookup (R12).
    pub(crate) irqs: [Option<u32>; crate::device::MAX_IRQS],
    /// The head of the object frames whose `free_object_frame` a destruction deferred past its
    /// single handle sweep (I1), linked through each frame's [`DEFER_WORD`].
    pub(crate) deferred: Option<u32>,
    /// Whether a destruction is running, so object-frame frees defer and the per-object handle
    /// sweeps fold into its single pass (`destroy_marked`).
    pub(crate) deferring: bool,
}

impl Objects {
    pub const fn new() -> Objects {
        Objects {
            last_id: 0,
            last_seq: 0,
            high_frame: 0,
            deadlines: None,
            accounts: [Account::NONE; MAX_PROCESS_COUNT],
            live_pids: PidMask::EMPTY,
            processes: [None; MAX_PROCESS_COUNT],
            process_pids: PidMask::EMPTY,
            irqs: [None; crate::device::MAX_IRQS],
            deferred: None,
            deferring: false,
        }
    }
}

pub(crate) fn account_index(pid: Pid) -> Option<usize> {
    let index = usize::from(pid.get()) - 1;
    (index < MAX_PROCESS_COUNT).then_some(index)
}

/// Where entry `tid` of a header's IPC-frame table lies: the byte offset of its 64-bit word in the
/// header page, and the entry's shift within that word (RISC-V is little-endian).
fn ipc_entry(tid: usize) -> (usize, u32) {
    let byte = crate::arch::process::IPC_TABLE_OFFSET + tid * 4;
    (byte & !7, (byte & 7) as u32 * 8)
}

/// The PID `n` names, if it is one: never truncated, so a word too wide for a PID is none.
pub(crate) fn pid_from(n: impl core::convert::TryInto<u16>) -> Option<Pid> {
    n.try_into().ok().and_then(Pid::new)
}

/// Every PID, lowest first: what drawing a free one walks. A walk of the processes iterates
/// [`MemoryManager::live_pids`] instead.
pub(crate) fn pids() -> impl Iterator<Item = Pid> { (1..=MAX_PROCESS_COUNT).filter_map(pid_from) }

/// A set of PIDs, bit `account_index` for each.
type PidMask = crate::bits::Bits<{ MAX_PROCESS_COUNT.div_ceil(64) }>;

// A PID is 16 bits, and the scheduler's cursor keeps a TID plus one in the 16 bits below it.
const _: () = assert!(MAX_PROCESS_COUNT <= u16::MAX as usize && MAX_THREADS < u16::MAX as usize);

/// `a ⊇ b`, both sorted.
fn superset(a: &[u64], b: &[u64]) -> bool { b.iter().all(|x| a.binary_search(x).is_ok()) }

impl MemoryManager {
    // --- Frames of kernel objects -----------------------------------------------------------

    pub fn budget(&self, frame: BudgetFrame) -> Budget {
        let phys = self.object_phys(frame);
        let w = |i: usize| kframe::read(phys, i * 8);
        // A budget frame that does not hold a budget means a stale reference survived R10's
        // sweep: a violated invariant (I1), so the kernel stops rather than trust the frame.
        assert!(w(0) == MAGIC, "I1: frame {} holds no budget", frame);
        let class = match w(4) {
            1 => Class::User,
            2 => Class::System,
            _ => panic!("I1: budget frame {} is corrupt", frame),
        };
        let mut labels = [0; MAX_LABELS];
        for (i, label) in labels.iter_mut().enumerate() {
            *label = w(16 + i);
        }
        Budget {
            id: w(1),
            // Word 2 is the parent's frame plus one; 0 for none.
            parent: (w(2) as u32).checked_sub(1),
            first_child: frame_of(w(W_FIRST_CHILD)),
            next_sibling: frame_of(w(W_NEXT_SIBLING)),
            first_owned: frame_of(w(W_FIRST_OWNED)),
            depth: w(3) as u32,
            class,
            dying: w(5) != 0,
            labels,
            nlabels: (w(6) as usize).min(MAX_LABELS),
            account: w(7),
            deadline: w(8),
            // Word 15 is the next deadline budget's frame plus one; 0 for none.
            next_deadline: (w(15) as u32).checked_sub(1),
            sched: redoubt_stride::State {
                pass: u128::from(w(W_SCHED)) | u128::from(w(W_SCHED + 1)) << 64,
                entry: u128::from(w(W_SCHED + 2)) | u128::from(w(W_SCHED + 3)) << 64,
                rem: w(W_SCHED + 4),
                tie: w(W_SCHED + 5) as i64,
                queued: w(W_SCHED + 6) != 0,
            },
            // The cursor: the PID above the TID plus one in the low 16 bits, 0 for none.
            cursor: pid_from(w(W_SCHED + 7) >> 16).map(|pid| (pid, (w(W_SCHED + 7) & 0xffff) as usize - 1)),
            pages_limit: w(9),
            pages_used: w(10),
            processes_limit: w(11) as u32,
            processes_used: w(12) as u32,
            weight_limit: w(13) as u32,
            weight_carved: w(14) as u32,
        }
    }

    pub fn store(&mut self, frame: BudgetFrame, b: &Budget) {
        let phys = self.object_phys(frame);
        let mut words = [0u64; WORDS];
        words[0] = MAGIC;
        words[1] = b.id;
        words[2] = b.parent.map_or(0, |p| u64::from(p) + 1);
        words[3] = u64::from(b.depth);
        words[4] = b.class as u64;
        words[5] = u64::from(b.dying);
        words[6] = b.nlabels as u64;
        words[7] = b.account;
        words[8] = b.deadline;
        words[9] = b.pages_limit;
        words[10] = b.pages_used;
        words[11] = u64::from(b.processes_limit);
        words[12] = u64::from(b.processes_used);
        words[13] = u64::from(b.weight_limit);
        words[14] = u64::from(b.weight_carved);
        words[15] = b.next_deadline.map_or(0, |f| u64::from(f) + 1);
        words[W_SCHED] = b.sched.pass as u64;
        words[W_SCHED + 1] = (b.sched.pass >> 64) as u64;
        words[W_SCHED + 2] = b.sched.entry as u64;
        words[W_SCHED + 3] = (b.sched.entry >> 64) as u64;
        words[W_SCHED + 4] = b.sched.rem;
        words[W_SCHED + 5] = b.sched.tie as u64;
        words[W_SCHED + 6] = u64::from(b.sched.queued);
        words[W_SCHED + 7] = b.cursor.map_or(0, |(p, t)| u64::from(p.get()) << 16 | (t as u64 + 1));
        words[W_FIRST_CHILD] = frame_word(b.first_child);
        words[W_NEXT_SIBLING] = frame_word(b.next_sibling);
        words[W_FIRST_OWNED] = frame_word(b.first_owned);
        words[16..W_SCHED].copy_from_slice(&b.labels);
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, i * 8, *word);
        }
    }

    // --- The scheduler's words, read and written alone (`sched.rs` reads them on every exit) --

    /// `frame`'s place in the stride queue.
    pub fn sched_state(&self, frame: BudgetFrame) -> redoubt_stride::State {
        let phys = self.object_phys(frame);
        let w = |i: usize| kframe::read(phys, i * 8);
        debug_assert!(w(0) == MAGIC, "I1: frame {} holds no budget", frame);
        redoubt_stride::State {
            pass: u128::from(w(W_SCHED)) | u128::from(w(W_SCHED + 1)) << 64,
            entry: u128::from(w(W_SCHED + 2)) | u128::from(w(W_SCHED + 3)) << 64,
            rem: w(W_SCHED + 4),
            tie: w(W_SCHED + 5) as i64,
            queued: w(W_SCHED + 6) != 0,
        }
    }

    pub fn set_sched_state(&mut self, frame: BudgetFrame, s: &redoubt_stride::State) {
        let phys = self.object_phys(frame);
        assert!(kframe::read(phys, 0) == MAGIC, "I1: frame {} holds no budget", frame);
        let words = [
            s.pass as u64,
            (s.pass >> 64) as u64,
            s.entry as u64,
            (s.entry >> 64) as u64,
            s.rem,
            s.tie as u64,
            u64::from(s.queued),
        ];
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, (W_SCHED + i) * 8, *word);
        }
    }

    /// The ready threads the scheduler counts in `frame`.
    pub fn sched_ready(&self, frame: BudgetFrame) -> u32 {
        kframe::read(self.object_phys(frame), READY_WORD * 8) as u32
    }

    pub fn set_sched_ready(&mut self, frame: BudgetFrame, n: u32) {
        kframe::write(self.object_phys(frame), READY_WORD * 8, u64::from(n))
    }

    /// `frame`'s id.
    pub fn budget_id(&self, frame: BudgetFrame) -> u64 { kframe::read(self.object_phys(frame), 8) }

    /// `frame`'s free weight: its stride weight (R12).
    pub fn free_weight_of(&self, frame: BudgetFrame) -> u64 {
        let phys = self.object_phys(frame);
        kframe::read(phys, 13 * 8).saturating_sub(kframe::read(phys, 14 * 8))
    }

    /// The next never-reused object id (budgets, endpoints).
    pub fn next_object_id(&mut self) -> u64 {
        self.objects.last_id = self.objects.last_id.checked_add(1).expect("I12: object ids exhausted");
        self.objects.last_id
    }

    /// The next value of the one order of message arrivals and R2 takes.
    pub fn next_seq(&mut self) -> u64 {
        self.objects.last_seq = self.objects.last_seq.checked_add(1).expect("send order exhausted");
        self.objects.last_seq
    }

    /// The next message id process `pid` hands a sender (I12).
    pub fn next_msg_id(&mut self, pid: Pid) -> u64 {
        let account = self.account_mut(pid).expect("account");
        account.last_msg_id = account.last_msg_id.checked_add(1).expect("I12: message ids exhausted");
        account.last_msg_id
    }

    /// The frame of thread `tid`'s IPC page, if it has one.
    pub fn ipc_frame(&self, pid: Pid, tid: usize) -> Option<u32> {
        let header = self.account(pid).map(|a| a.header).filter(|h| *h != 0)?;
        if !(1..=MAX_THREADS).contains(&tid) {
            return None;
        }
        let (offset, shift) = ipc_entry(tid);
        Some((kframe::read(header, offset) >> shift) as u32).filter(|f| *f != 0)
    }

    /// Record process `pid`'s header page, at `phys`, once its address space has one
    /// (`MemoryMapping::allocate`; the loader's for `init`). Its IPC-frame table is empty.
    pub fn set_header(&mut self, pid: Pid, phys: usize) {
        self.account_mut(pid).expect("account").header = phys;
    }

    /// Write entry `tid` of `pid`'s IPC-frame table.
    fn set_ipc_entry(&mut self, pid: Pid, tid: usize, frame: u32) {
        let header = self.account(pid).map(|a| a.header).filter(|h| *h != 0).expect("a header page");
        let (offset, shift) = ipc_entry(tid);
        let word = kframe::read(header, offset) & !(u64::from(u32::MAX) << shift);
        kframe::write(header, offset, word | u64::from(frame) << shift);
    }

    /// Give thread `tid` its IPC page. Its cost is [`THREAD_PAGES`], charged when the thread was
    /// created, so the frame is already paid for; one missing here would mean the kernel
    /// over-committed RAM, which `boot_budgets` reserves against, so it stops (fail closed).
    fn give_ipc_frame(&mut self, pid: Pid, tid: usize) {
        if self.account(pid).is_none()
            || !(1..=MAX_THREADS).contains(&tid)
            || self.ipc_frame(pid, tid).is_some()
        {
            return;
        }
        let frame = self.alloc_object_frame().expect("R6: a thread's page was charged but has no frame");
        self.set_ipc_entry(pid, tid, frame);
        let account = self.account_mut(pid).expect("account");
        account.live = account.live.with(tid);
    }

    /// Take thread `tid`'s IPC page back. Its contents are dead by now: `message.rs` unwinds
    /// what the thread waited for and the calls it held before the thread goes.
    fn take_ipc_frame(&mut self, pid: Pid, tid: usize) {
        if let Some(frame) = self.ipc_frame(pid, tid) {
            self.set_ipc_entry(pid, tid, 0);
            let account = self.account_mut(pid).expect("account");
            account.live = account.live.without(tid);
            self.free_object_frame(frame);
        }
    }

    /// The TIDs of `pid`'s threads that have an IPC page, lowest first; none for a PID with no
    /// account. The mask is read once, so the walk may end the threads it visits: one it has
    /// not reached yet and that has ended meanwhile reads as a thread with no page.
    pub fn live_tids(&self, pid: Pid) -> impl Iterator<Item = usize> + use<> {
        self.account(pid).map_or(TidMask::EMPTY, |a| a.live).iter()
    }

    /// The PIDs with an account, lowest first: what every walk of the processes visits, so its
    /// cost follows the processes that exist. The set is read once, as [`Self::live_tids`] reads
    /// its mask: a process that ends meanwhile reads as one with no account.
    pub fn live_pids(&self) -> impl Iterator<Item = Pid> + use<> {
        self.objects.live_pids.iter().filter_map(|index| pid_from(index + 1))
    }

    /// R12's index rule for the live set: it holds exactly the PIDs with an account.
    #[cfg(debug_assertions)]
    fn check_live_pids(&self) {
        for (index, account) in self.objects.accounts.iter().enumerate() {
            assert_eq!(
                self.objects.live_pids.contains(index),
                account.budget.is_some(),
                "live PID {}",
                index + 1
            );
        }
    }

    // --- Charging (R6) ------------------------------------------------------------------------

    /// Charge `pages` to budget `frame`; `OutOfMemory` over its limit (and while R3 holds it
    /// over, for every new allocation).
    pub fn charge(&mut self, frame: BudgetFrame, pages: u64) -> Result<(), Error> {
        let mut b = self.budget(frame);
        if pages > b.free_pages() {
            return Err(Error::OutOfMemory);
        }
        b.pages_used += pages;
        self.store(frame, &b);
        Ok(())
    }

    pub fn uncharge(&mut self, frame: BudgetFrame, pages: u64) {
        let mut b = self.budget(frame);
        // Returning more than was charged is a bookkeeping bug (I5); stop rather than wrap.
        b.pages_used = b.pages_used.checked_sub(pages).expect("I5: budget usage underflow");
        self.store(frame, &b);
    }

    pub fn account(&self, pid: Pid) -> Option<&Account> {
        account_index(pid).map(|i| &self.objects.accounts[i]).filter(|a| a.budget.is_some())
    }

    pub fn account_mut(&mut self, pid: Pid) -> Option<&mut Account> {
        let accounts = &mut self.objects.accounts;
        account_index(pid).map(move |i| &mut accounts[i]).filter(|a| a.budget.is_some())
    }

    /// Pages `frame` may still charge (R6).
    pub fn free_pages(&self, frame: BudgetFrame) -> u64 { self.budget(frame).free_pages() }

    /// Whether `r` still names the budget it named. Unlike `budget_at`, a stale reference is an
    /// answer here, not a kernel bug: a message carries handles that R10 may have revoked while
    /// it waited, and an open call remembers a stamp that may be gone (`mint`'s `Dead`).
    pub fn is_live_budget(&self, r: BudgetRef) -> bool {
        self.is_budget_frame(r.frame) && self.budget_id(r.frame) == r.id
    }

    /// Whether `b` is `ancestor` or below it (R9: a budget handle only narrows).
    pub fn is_at_or_below(&self, b: BudgetFrame, ancestor: BudgetFrame) -> bool { self.below(b, ancestor) }

    /// The budget process `pid` lives in; `None` for the kernel.
    pub fn budget_of(&self, pid: Pid) -> Option<BudgetFrame> { self.account(pid).and_then(|a| a.budget) }

    /// A RAM frame became `pid`'s: charge it to `pid`'s budget, if it has one.
    pub fn charge_frame(&mut self, pid: Pid) -> Result<(), Error> {
        if let Some(budget) = self.budget_of(pid) {
            self.charge(budget, 1)?;
            self.account_mut(pid).expect("account").frames += 1;
        }
        Ok(())
    }

    /// A RAM frame stopped being `pid`'s.
    pub fn uncharge_frame(&mut self, pid: Pid) {
        if let Some(budget) = self.budget_of(pid) {
            let account = self.account_mut(pid).expect("account");
            account.frames = account.frames.checked_sub(1).expect("I5: frame count underflow");
            self.uncharge(budget, 1);
        }
    }

    /// Every frame of `pid`'s was just freed at once (`release_owned_frames`).
    pub fn uncharge_all_frames(&mut self, pid: Pid) {
        if let Some(budget) = self.budget_of(pid) {
            let frames = core::mem::take(&mut self.account_mut(pid).expect("account").frames);
            self.uncharge(budget, frames);
        }
    }

    // --- Processes and threads ------------------------------------------------------------------

    /// One more PID held against `budget`'s process limit (R6): a created process's from
    /// `process_create` until its object is freed, a program the loader started while it lives,
    /// in the budget it runs in either way. Every held PID counts once, and limits are carved from
    /// `root`'s, which is every free PID, so a budget under its limit always finds one free.
    /// Nothing changes on an error.
    pub fn count_process(&mut self, budget: BudgetFrame) -> Result<(), Error> {
        let mut b = self.budget(budget);
        if b.free_processes() == 0 {
            return Err(Error::OutOfProcesses);
        }
        b.processes_used += 1;
        self.store(budget, &b);
        Ok(())
    }

    /// A PID [`MemoryManager::count_process`] counted against `budget` is free again.
    pub fn uncount_process(&mut self, budget: BudgetFrame) {
        let mut b = self.budget(budget);
        b.processes_used = b.processes_used.checked_sub(1).expect("I5: process count underflow");
        self.store(budget, &b);
    }

    /// Put new process `pid` in `budget`: an account of its own, with no threads yet. Its PID is
    /// counted there first ([`MemoryManager::count_process`]); its address space is charged
    /// there frame by frame as it is built (`process.rs`; kernel/objects.md); its object page is
    /// the *creator's*, which `process_create` charges separately. Nothing changes on an error.
    pub fn process_created(&mut self, pid: Pid, budget: BudgetFrame) -> Result<(), Error> {
        let index = account_index(pid).ok_or(Error::InvalidArgument)?;
        // A budget with no free weight holds no process (R12: its stride weight is its free
        // weight).
        if self.budget(budget).free_weight() == 0 {
            return Err(Error::InvalidArgument);
        }
        // No thread has a timeout yet.
        self.objects.accounts[index] =
            Account { budget: Some(budget), earliest_timeout: u64::MAX, ..Account::NONE };
        self.objects.live_pids = self.objects.live_pids.with(index);
        #[cfg(debug_assertions)]
        self.check_live_pids();
        Ok(())
    }

    /// Everything the process still has charged goes back to its budget. Its frames were
    /// released just before (`uncharge_all_frames`); its handle table goes here. Its PID stops
    /// counting only if no process object holds it; one that does counts until it is freed
    /// (`process::free_object`).
    pub fn process_ended(&mut self, pid: Pid) {
        let Some(budget) = self.budget_of(pid) else { return };
        self.close_all_handles(pid);
        let account = self.account_mut(pid).expect("account");
        assert!(account.live.is_empty(), "process {} ended with IPC pages", pid);
        let pages = account.threads * THREAD_PAGES + account.frames;
        *account = Account::NONE;
        self.objects.live_pids =
            self.objects.live_pids.without(account_index(pid).expect("an account's PID"));
        #[cfg(debug_assertions)]
        self.check_live_pids();
        self.uncharge(budget, pages);
        if crate::process::object_of(self, pid).is_none() {
            self.uncount_process(budget);
        }
        #[cfg(debug_assertions)]
        self.check_frame_owners();
    }

    /// A process is ending: its threads' IPC pages go back, while its header page, which names
    /// them, still exists (before `release_all_memory_for_process`). Their charge goes back with
    /// the account, in [`MemoryManager::process_ended`].
    pub fn release_ipc_frames(&mut self, pid: Pid) {
        for tid in self.live_tids(pid) {
            self.take_ipc_frame(pid, tid);
        }
    }

    pub fn thread_created(&mut self, pid: Pid, tid: usize) -> Result<(), Error> {
        if let Some(budget) = self.budget_of(pid) {
            self.charge(budget, THREAD_PAGES)?;
            self.account_mut(pid).expect("account").threads += 1;
            self.give_ipc_frame(pid, tid);
        }
        Ok(())
    }

    pub fn thread_ended(&mut self, pid: Pid, tid: usize) {
        if let Some(budget) = self.budget_of(pid) {
            self.take_ipc_frame(pid, tid);
            let account = self.account_mut(pid).expect("account");
            account.threads = account.threads.checked_sub(1).expect("I5: thread count underflow");
            self.uncharge(budget, THREAD_PAGES);
        }
    }

    // --- Boot -----------------------------------------------------------------------------------

    /// Create `root`, `system` and `users` and put `init`, the one process the loader started,
    /// in `root` (kernel/budgets.md, "The tree from the boot manifest").
    ///
    /// The split is fixed here, not read from the argument block. `root` gets every RAM page the
    /// kernel did not keep at boot, every PID but the kernel's, and all the weight. It keeps for
    /// `init` one process, `INIT_WEIGHT`, and `init`'s pages: everything the loader gave it (its
    /// image, stack, page tables, header page and the bundle's frames), its first thread and
    /// `INIT_PAGES` to work in. `system` gets a quarter of the rest of the pages, 127 processes
    /// and a quarter of the weight; `users` what is left. `init` gets handles to the three
    /// budgets in slots 1-3, stamped with `root`, then the devices.
    ///
    /// If `init`'s pages do not fit, the kernel refuses to boot (fail closed).
    pub fn boot_budgets(&mut self, init_header: usize) {
        // Every RAM page the kernel did not keep for itself or for the DMA pool, less `root`'s
        // own page, which no budget pays for (R6): `root`'s limit bounds every charge in the tree,
        // so the charges and `root`'s page together never exceed the free frames. Nothing else is
        // held back: a process's header page and its root page table are charged to the budget
        // it runs in as they are allocated, like any other frame it owns, so every charged page
        // has a real frame behind it without a reservation.
        let kept =
            (self.ram_frames_owned_by(redoubt_layout::KERNEL_PID) + redoubt_layout::DMA_POOL_PAGES) as u64;
        let pages = self.ram_frames() - kept - BUDGET_PAGES;
        // Everything the loader gave `init`, all owned by its PID in the ownership table.
        let init_frames = self.ram_frames_owned_by(INIT_PID) as u64;
        let init_pages = init_frames + THREAD_PAGES + INIT_PAGES;
        let rest =
            pages.checked_sub(init_pages + 2 * BUDGET_PAGES).expect("boot: init's pages do not fit in RAM");
        let processes = (MAX_PROCESS_COUNT - 1) as u32;
        let (sys_pages, sys_processes, sys_weight) = (rest / 4, processes / 4, ROOT_WEIGHT / 4);
        // `users` gets the rest of the weight and the processes but what `root` keeps for `init`.
        let users_weight = ROOT_WEIGHT - sys_weight - INIT_WEIGHT;
        let users_processes = processes - sys_processes - 1;
        // Root pays for the two budgets' own pages, counted out of `rest` above.
        let users_pages = rest - sys_pages;
        // `root` and `system` are class `system`; `users` is class `user`. Nothing runs before
        // anything else: one stride queue, and weight decides (kernel/scheduling.md).
        let boot = |mm: &mut Self, parent, class, pages, processes, weight| {
            let spec = BudgetSpec {
                pages,
                processes,
                weight,
                labels: Default::default(),
                account: 0,
                deadline: FOREVER,
            };
            mm.new_budget(parent, &spec, class, &[]).expect("boot: no frame for a boot budget")
        };
        let root = boot(self, None, Class::System, pages, processes, ROOT_WEIGHT);
        let system = boot(self, Some(root), Class::System, sys_pages, sys_processes, sys_weight);
        let users = boot(self, Some(root), Class::User, users_pages, users_processes, users_weight);
        assert!(
            self.budget(root).pages_limit + BUDGET_PAGES + kept <= self.ram_frames(),
            "R6: root's limit, its own page, the kernel's frames and the DMA pool exceed RAM"
        );
        self.count_process(root).expect("boot: root keeps no process for init");
        self.process_created(INIT_PID, root).expect("boot: root keeps no weight for init");
        self.set_header(INIT_PID, init_header);
        self.charge(root, init_frames).expect("boot: init's pages do not fit in root");
        self.account_mut(INIT_PID).expect("account").frames = init_frames;
        self.thread_created(INIT_PID, INITIAL_TID).expect("boot: no page for init's first thread");
        let stamp = BudgetRef { frame: root, id: self.budget(root).id };
        for budget in [root, system, users] {
            let id = self.budget(budget).id;
            let handle = Handle { object: Object::Budget(BudgetRef { frame: budget, id }), badge: 0, stamp };
            self.install_handle(INIT_PID, handle).expect("boot: no room for init's handles");
        }
        // The machine's devices, charged to `system` and given to `init` (`device.rs`). They come
        // after the three budget handles, so `init`'s table is 1-3 budgets, 4.. devices. The
        // handles are stamped with `root`, like the three budget handles, and not with the budget
        // the objects are charged to: a stamp says which budget's destruction revokes the *handle*
        // (R10), and these are `init`'s to hand on, so they outlive anything below `root`. The
        // objects themselves are charged to, and die with, `system`.
        self.boot_devices(system, Some(INIT_PID), stamp);
        println!(
            "Budgets: root {} pages ({} kept for init), system {}, users {}",
            pages, init_pages, sys_pages, users_pages
        );
    }

    // --- The calls --------------------------------------------------------------------------------

    /// Create the budget object (after every check): its frame, its id, its place in the tree;
    /// its own object and its limits charged to the parent (R6, R7). The class is the caller's to
    /// choose: `budget_create` passes the parent's (kernel/budgets.md), boot its own.
    fn new_budget(
        &mut self,
        parent: Option<BudgetFrame>,
        spec: &BudgetSpec,
        class: Class,
        labels: &[u64],
    ) -> Result<BudgetFrame, Error> {
        let frame = self.alloc_object_frame()?;
        let id = self.next_object_id();
        let mut label_array = [0; MAX_LABELS];
        label_array[..labels.len()].copy_from_slice(labels);
        let parent_budget = parent.map(|p| self.budget(p));
        let b = Budget {
            id,
            parent,
            // It enters its parent's child list at the head (R10's subtree walk).
            first_child: None,
            next_sibling: parent_budget.and_then(|p| p.first_child),
            first_owned: None,
            depth: parent_budget.map_or(0, |p| p.depth + 1),
            class,
            dying: false,
            labels: label_array,
            nlabels: labels.len(),
            account: spec.account,
            deadline: spec.deadline,
            pages_limit: spec.pages,
            pages_used: 0,
            processes_limit: spec.processes,
            processes_used: 0,
            weight_limit: spec.weight,
            weight_carved: 0,
            next_deadline: None,
            sched: redoubt_stride::State::default(),
            cursor: None,
        };
        self.store(frame, &b);
        if spec.deadline != FOREVER {
            self.link_deadline(frame);
        }
        // It enters the queue's virtual time at max(floor, parent's pass).
        crate::sched::create(self, frame, parent);
        if let Some(p) = parent {
            // Read after the scheduler charged it: the frame holds its scheduling words too.
            let mut pb = self.budget(p);
            pb.pages_used += BUDGET_PAGES + spec.pages;
            pb.processes_used += spec.processes;
            pb.first_child = Some(frame);
            self.store(p, &pb);
            // The carve changes the parent's stride weight: what it ran is charged at the old one.
            crate::sched::change_weight(self, p, |mm| {
                let mut pb = mm.budget(p);
                pb.weight_carved += spec.weight;
                mm.store(p, &pb);
            });
        }
        Ok(frame)
    }

    /// `budget_create(h(parent), spec) -> h`, after decoding. The checks follow kernel/abi.md's
    /// row for `budget_create`, in order.
    pub fn budget_create(&mut self, pid: Pid, parent: u32, spec: &BudgetSpec) -> Result<u32, Error> {
        // Only the kernel (PID 1) has no account, and it makes no Redoubt calls; `NotPermitted` is
        // there so that a bug cannot turn into a panic.
        let caller = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let pf = self.budget_handle(pid, parent)?;
        let p = self.budget(pf);
        let caller_class = self.budget(caller).class;
        if p.depth as usize + 1 >= MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        // A child's class is its parent's (kernel/budgets.md); nothing else about scheduling is
        // the caller's to choose (kernel/scheduling.md: one stride queue, ordered by weight).
        let mut labels = [0; MAX_LABELS];
        let given = spec.labels.as_slice();
        labels[..given.len()].copy_from_slice(given);
        let labels = &mut labels[..given.len()];
        labels.sort_unstable();
        let mut n = 0;
        for i in 0..labels.len() {
            if n == 0 || labels[i] != labels[n - 1] {
                labels[n] = labels[i];
                n += 1;
            }
        }
        let labels = &labels[..n];
        if !superset(labels, p.labels()) {
            return Err(Error::LabelDenied);
        }
        if labels != p.labels() && caller_class != Class::System {
            return Err(Error::ClassDenied);
        }
        // R6, R7: the parent pays the child's own page and carves its limits (a revocation
        // scope, with zero limits, is no special case).
        if spec.pages.checked_add(BUDGET_PAGES).is_none_or(|pages| pages > p.free_pages()) {
            return Err(Error::OutOfMemory);
        }
        if spec.processes > p.free_processes() {
            return Err(Error::OutOfProcesses);
        }
        // No error names weight; the spec's stated exception. A carve may not leave a budget
        // that holds a process with no free weight (R12: its stride weight is its free weight).
        if spec.weight > p.free_weight()
            || (spec.weight > 0 && spec.weight == p.free_weight() && self.holds_process(pf))
        {
            return Err(Error::InvalidArgument);
        }
        // R8: the parent's account, unless it is 0; then the creator's choice.
        let account = if p.account != 0 { p.account } else { spec.account };
        let child = self.new_budget(Some(pf), &BudgetSpec { account, ..*spec }, p.class, labels)?;
        let id = self.budget(child).id;
        // R9: stamped with the caller's budget. The table may have to grow, charged to the caller
        // after the carve (the caller's budget may be the parent); if it cannot, undo.
        let stamp = BudgetRef { frame: caller, id: self.budget(caller).id };
        let handle = Handle { object: Object::Budget(BudgetRef { frame: child, id }), badge: 0, stamp };
        let installed = self.install_handle(pid, handle).inspect_err(|_| {
            crate::sched::change_weight(self, pf, |mm| mm.return_carve(child, true));
            self.unlink_child(pf, child);
            self.unlink_deadline(child);
            self.free_object_frame(child);
        })?;
        if spec.deadline != FOREVER {
            // A deadline already past is destroyed at the next kernel entry.
            crate::time::note_budget_deadline(spec.deadline);
        }
        Ok(installed)
    }

    /// `budget_usage(h) -> counters`, after decoding.
    pub fn budget_usage(&self, pid: Pid, h: u32) -> Result<Usage, Error> {
        // As in `budget_create`: only the kernel has no account.
        let caller = self.budget(self.budget_of(pid).ok_or(Error::NotPermitted)?);
        let frame = self.budget_handle(pid, h)?;
        let target = self.budget(frame);
        // A user-class caller reads only budgets whose labels its own contain (R1, I7).
        if caller.class != Class::System && !superset(caller.labels(), target.labels()) {
            return Err(Error::LabelDenied);
        }
        Ok(Usage {
            pages_limit: target.pages_limit,
            pages_usage: target.pages_used,
            processes_limit: target.processes_limit,
            processes_usage: target.processes_used,
            weight_limit: target.weight_limit,
            weight_carved: target.weight_carved,
        })
    }

    // --- Destruction (R10) ------------------------------------------------------------------------

    /// Whether `b` is `top` or below it. A subtree is destroyed whole, so every live budget's
    /// parent chain is live.
    pub(crate) fn below(&self, b: BudgetFrame, top: BudgetFrame) -> bool {
        let mut cur = Some(b);
        while let Some(f) = cur {
            if f == top {
                return true;
            }
            cur = self.budget(f).parent;
        }
        false
    }

    /// Whether `frame` holds a budget: an object frame that starts with the magic (a handle-table
    /// page starts with a handle's kind, which is below `u32::MAX`, so never the magic).
    pub(crate) fn is_budget_frame(&self, frame: BudgetFrame) -> bool {
        self.is_object_frame(frame) && kframe::read(self.object_phys(frame), 0) == MAGIC
    }

    /// First step of `budget_destroy(h)`: check the handle and mark the budget and everything
    /// below it dying. The caller then kills every process in a dying budget
    /// (`runs_in_dying`), and finishes with [`MemoryManager::destroy_marked`].
    pub fn destroy_begin(&mut self, pid: Pid, h: u32) -> Result<BudgetFrame, Error> {
        let top = self.budget_handle(pid, h)?;
        self.mark_dying(top);
        Ok(top)
    }

    /// Mark `top` and everything below it dying (R10's first step, for `budget_destroy` and for
    /// a deadline alike). Before anything else, `top`'s carve comes back to its parent and `top`
    /// leaves its parent's child list, so the subtree the walk starts from is exactly the one
    /// being destroyed. The destruction's own work (often the parent's own `budget_destroy`) is
    /// charged at the weight the parent has once the child is gone, not at the sliver it kept
    /// while the child held the rest (`sched.rs`: a weight change charges what ran before it).
    /// The budgets below the top return theirs as the scheduler lifts them, bottom-up
    /// ([`MemoryManager::lift_dying`]).
    pub fn mark_dying(&mut self, top: BudgetFrame) {
        if let Some(p) = self.budget(top).parent {
            let limit = self.budget(top).weight_limit;
            crate::sched::change_weight(self, p, |mm| {
                let mut pb = mm.budget(p);
                pb.weight_carved = pb.weight_carved.checked_sub(limit).expect("I5: carve underflow");
                mm.store(p, &pb);
            });
            self.unlink_child(p, top);
        }
        // A pre-order walk of the subtree through the child links (R10): each budget below `top`
        // once, never a scan of every object frame.
        let mut cur = Some(top);
        while let Some(frame) = cur {
            let mut b = self.budget(frame);
            b.dying = true;
            self.store(frame, &b);
            cur = self.subtree_next(top, frame);
        }
    }

    /// The next budget in a pre-order walk of the subtree `top` heads, after `cur`; `None` when
    /// the walk is done. Reads only the child links. `mark_dying` and `message::budgets_dying`
    /// share it, so the destruction's walk is written once.
    pub(crate) fn subtree_next(&self, top: BudgetFrame, cur: BudgetFrame) -> Option<BudgetFrame> {
        if let Some(child) = self.budget(cur).first_child {
            return Some(child);
        }
        let mut at = cur;
        loop {
            if at == top {
                return None;
            }
            if let Some(sibling) = self.budget(at).next_sibling {
                return Some(sibling);
            }
            at = self.budget(at).parent.expect("a subtree budget has a parent");
        }
    }

    /// The next object in its owner's list, an endpoint or a device: one word.
    pub(crate) fn owned_next(&self, frame: u32) -> Option<u32> {
        frame_of(kframe::read(self.object_phys(frame), OWNED_WORD * 8))
    }

    /// Set `frame`'s owner-list link.
    fn set_owned_next(&mut self, frame: u32, next: Option<u32>) {
        kframe::write(self.object_phys(frame), OWNED_WORD * 8, frame_word(next));
    }

    /// Link `frame` at the head of `owner`'s object list (`new_endpoint`, `new_device`).
    pub(crate) fn link_owned(&mut self, owner: BudgetFrame, frame: u32) {
        let mut ob = self.budget(owner);
        let next = ob.first_owned;
        ob.first_owned = Some(frame);
        self.store(owner, &ob);
        self.set_owned_next(frame, next);
    }

    /// Move `frame`, which follows `prev` in `owner`'s object list (`None`: it heads it already),
    /// to the head, in a few words: then `unlink_owned` finds it first.
    pub(crate) fn owned_to_head(&mut self, owner: BudgetFrame, prev: Option<u32>, frame: u32) {
        let Some(prev) = prev else { return };
        let next = self.owned_next(frame);
        self.set_owned_next(prev, next);
        let mut ob = self.budget(owner);
        self.set_owned_next(frame, ob.first_owned);
        ob.first_owned = Some(frame);
        self.store(owner, &ob);
    }

    /// Take `frame` out of `owner`'s object list (`free_endpoint`, `free_device`), one chain for
    /// endpoints and devices alike.
    pub(crate) fn unlink_owned(&mut self, owner: BudgetFrame, frame: u32) {
        let mut ob = self.budget(owner);
        if ob.first_owned == Some(frame) {
            ob.first_owned = self.owned_next(frame);
            self.store(owner, &ob);
            return;
        }
        let mut cur = ob.first_owned;
        while let Some(c) = cur {
            if self.owned_next(c) == Some(frame) {
                let next = self.owned_next(frame);
                self.set_owned_next(c, next);
                return;
            }
            cur = self.owned_next(c);
        }
    }

    /// Link `child` out of `parent`'s child list (`new_budget` linked it at the head). Called by
    /// `budget_create`'s rollback and by `mark_dying` for the destroyed top.
    fn unlink_child(&mut self, parent: BudgetFrame, child: BudgetFrame) {
        let mut pb = self.budget(parent);
        if pb.first_child == Some(child) {
            pb.first_child = self.budget(child).next_sibling;
            self.store(parent, &pb);
            return;
        }
        let mut cur = pb.first_child;
        while let Some(c) = cur {
            if self.budget(c).next_sibling == Some(child) {
                let next = self.budget(child).next_sibling;
                let mut cb = self.budget(c);
                cb.next_sibling = next;
                self.store(c, &cb);
                return;
            }
            cur = self.budget(c).next_sibling;
        }
    }

    /// Who pays for `top`'s destruction when its deadline passes (R10, R12): its parent, or else
    /// the nearest ancestor whose free weight is above 0, or `root` if none is. Asked after
    /// `mark_dying`, so the parent's free weight counts the carve `top` gave back. The walk is at
    /// most `MAX_DEPTH` long. `None` only for a top with no parent (`root`, which has no deadline).
    pub fn destruction_payer(&self, top: BudgetFrame) -> Option<BudgetRef> {
        let mut payer = self.budget(top).parent?;
        while self.free_weight_of(payer) == 0 {
            let Some(up) = self.budget(payer).parent else { break };
            payer = up;
        }
        Some(BudgetRef { frame: payer, id: self.budget_id(payer) })
    }

    /// A checked build's proof that a destroyed top with no parent was `root`: every budget is
    /// dying.
    #[cfg(debug_assertions)]
    pub(crate) fn check_all_dying(&self) {
        for frame in 0..=self.objects.high_frame {
            assert!(!self.is_budget_frame(frame) || self.budget(frame).dying, "a budget outlives root");
        }
    }

    /// Whether `pid` runs in a budget that is being destroyed: the processes `destroy_subtree`
    /// kills first.
    pub fn runs_in_dying(&self, pid: Pid) -> bool {
        self.budget_of(pid).is_some_and(|b| self.budget(b).dying)
    }

    /// Whether the destruction under way will kill `pid`: it runs in a dying budget, or its
    /// process object is charged to one, which frees the object and kills the process with it
    /// (`process::budgets_dying`).
    pub fn process_is_doomed(&self, pid: Pid) -> bool {
        self.runs_in_dying(pid)
            || crate::process::object_of(self, pid)
                .is_some_and(|f| self.budget_at(self.process(f).creator).dying)
    }

    /// Last step of `budget_destroy`, once the doomed budgets' processes are gone: close every
    /// handle naming a doomed budget or stamped with one, in every table (R10, I2); give the
    /// parent back what `top` carved from it (I10), then charge it the quarantined DMA pages and
    /// count in it the held PIDs that outlive the subtree (step 8); free the doomed budgets
    /// (marked, with no processes and no handles left; nothing reads a dying frame's tree links
    /// after this).
    pub fn destroy_marked(&mut self, top: BudgetFrame) {
        // The handles held outside the subtree that depend on a dying budget are in its chains;
        // those held inside went with their holders' tables, and those naming a freed process
        // object with its own chain (`handle.rs`).
        let mut cur = Some(top);
        while let Some(frame) = cur {
            self.close_dependents(frame);
            cur = self.subtree_next(top, frame);
        }
        // Every handle naming them is closed: the endpoint, process and device frames are freed
        // now, past that (I1).
        self.free_owned_endpoints(top);
        self.free_deferred_frames();
        // The weight came back as the scheduler lifted each budget (`sched::destroy`).
        self.return_carve(top, false);
        // Then, and only then, quarantined DMA pages charged in the subtree move to the parent,
        // which has just got back at least that much (kernel/devices.md, "Quarantine").
        self.dma_migrate_quarantine(self.budget(top).parent);
        // And so does every PID still held for a process that ran in the subtree (R6).
        self.migrate_held_pids(self.budget(top).parent);
        // Free the dying budgets themselves: children before their parent, so a parent's frame
        // still holds the links the walk reads.
        self.free_dying_budgets(top);
    }

    /// Free the endpoints the dying subtree owns, the last left on its budgets' owner chains
    /// (`message::budgets_dying` destroyed the devices), each in a link read and a free, and give
    /// each budget its endpoints' pages back in one write. Nothing reads them past this.
    fn free_owned_endpoints(&mut self, top: BudgetFrame) {
        let mut cur = Some(top);
        while let Some(frame) = cur {
            let mut owned = self.budget(frame).first_owned;
            let mut pages = 0;
            while let Some(o) = owned {
                owned = self.owned_next(o);
                self.free_object_frame(o);
                pages += crate::endpoint::ENDPOINT_PAGES;
            }
            self.uncharge(frame, pages);
            cur = self.subtree_next(top, frame);
        }
    }

    /// Free the dying subtree `frame` heads: every descendant first, then the budget, unlinking
    /// each from the deadline list before its frame goes (I1: nothing may name a freed frame).
    fn free_dying_budgets(&mut self, frame: BudgetFrame) {
        let mut free = |mm: &mut MemoryManager, f: BudgetFrame| {
            mm.unlink_deadline(f);
            mm.free_object_frame(f);
        };
        self.for_each_descendant_post(frame, &mut free);
    }

    /// Walk `frame`'s subtree children first, then `frame`, calling `action` on every budget in
    /// it: R10's bottom-up order, shared by `free_dying_budgets` and `lift_dying`. The walk is at
    /// most `MAX_DEPTH` deep and reads only the child links.
    fn for_each_descendant_post(
        &mut self,
        frame: BudgetFrame,
        action: &mut impl FnMut(&mut MemoryManager, BudgetFrame),
    ) {
        let mut child = self.budget(frame).first_child;
        while let Some(c) = child {
            let next = self.budget(c).next_sibling;
            self.for_each_descendant_post(c, &mut *action);
            child = next;
        }
        action(self, frame);
    }

    /// One destruction begins: object frames freed while it runs are deferred to
    /// [`MemoryManager::destroy_marked`]'s single handle sweep, and the per-object sweeps fold
    /// into it (I1, I2).
    pub fn begin_destruction(&mut self) { self.objects.deferring = true; }

    /// The destruction is over. The deferred frames are already freed; the flag that folded the
    /// per-object sweeps into the one pass goes.
    pub fn end_destruction(&mut self) { self.objects.deferring = false; }

    // --- Deadlines ---------------------------------------------------------------------------------

    fn link_deadline(&mut self, frame: BudgetFrame) {
        let mut b = self.budget(frame);
        b.next_deadline = self.objects.deadlines;
        self.store(frame, &b);
        self.objects.deadlines = Some(frame);
    }

    /// Take `frame` off the deadline list, if it is on it.
    fn unlink_deadline(&mut self, frame: BudgetFrame) {
        let next = self.budget(frame).next_deadline;
        if self.objects.deadlines == Some(frame) {
            self.objects.deadlines = next;
            return;
        }
        let mut cur = self.objects.deadlines;
        while let Some(c) = cur {
            let mut cb = self.budget(c);
            if cb.next_deadline == Some(frame) {
                cb.next_deadline = next;
                self.store(c, &cb);
                return;
            }
            cur = cb.next_deadline;
        }
    }

    /// Every live budget with a deadline, as (deadline, id, frame): the list, not a scan.
    pub fn deadlines(&self) -> impl Iterator<Item = (u64, u64, BudgetFrame)> + '_ {
        let mut cur = self.objects.deadlines;
        core::iter::from_fn(move || {
            let f = cur?;
            let b = self.budget(f);
            cur = b.next_deadline;
            Some((b.deadline, b.id, f))
        })
    }

    /// Give `b`'s parent back what `b` carved from it, and `b`'s own page (I10); its weight too,
    /// unless the scheduler already returned it.
    fn return_carve(&mut self, b: BudgetFrame, weight: bool) {
        let b = self.budget(b);
        let Some(p) = b.parent else { return };
        let mut pb = self.budget(p);
        let carved = BUDGET_PAGES + b.pages_limit;
        pb.pages_used = pb.pages_used.checked_sub(carved).expect("I5: carve underflow");
        pb.processes_used = pb.processes_used.checked_sub(b.processes_limit).expect("I5: carve underflow");
        if weight {
            pb.weight_carved = pb.weight_carved.checked_sub(b.weight_limit).expect("I5: carve underflow");
        }
        self.store(p, &pb);
    }

    /// Whether a process runs in `frame`.
    fn holds_process(&self, frame: BudgetFrame) -> bool {
        self.live_pids().any(|pid| self.budget_of(pid) == Some(frame))
    }

    /// The dying budgets, deepest first (every one's descendants before it): R10's bottom-up
    /// order for the scheduler's lifts, each returning its weight to its parent as it goes (the
    /// top's went back at mark time). The subtree walk is at most `MAX_DEPTH` deep.
    pub fn lift_dying(&mut self, top: BudgetFrame) {
        let mut lift = |mm: &mut MemoryManager, f: BudgetFrame| crate::sched::destroy(mm, f, f == top);
        self.for_each_descendant_post(top, &mut lift);
    }
}

/// R10 for the subtree marked dying at `top` (`MemoryManager::mark_dying`), for `budget_destroy`
/// and for a deadline alike: every process in it is killed (each exit notice `killed`), the
/// caller last if it is one of them; the process objects charged to it are freed; messages in
/// flight are failed or abandoned and its endpoints and devices destroyed; then its handles are
/// swept and its frames freed. `caller` is the process whose call or whose interrupted run this
/// is, if any. `deadline_since` is the tick a deadline's handling began, whose whole cost is
/// billed at the end to the payer (`destruction_payer`); `None` for `budget_destroy`, whose caller
/// pays for the call as system-call time. Returns whether the caller is gone (it must not be
/// resumed).
pub fn destroy_subtree(
    ss: &mut ProcessTable,
    top: BudgetFrame,
    caller: Option<Pid>,
    deadline_since: Option<u64>,
) -> bool {
    // Named now, while `top` still links to its parent, and after `mark_dying` gave its carve back.
    let payer = deadline_since.and_then(|_| MemoryManager::with(|mm| mm.destruction_payer(top)));
    #[cfg(feature = "sched-trace")]
    let top_id = MemoryManager::with(|mm| mm.budget_id(top));
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::r10(crate::sched::trace::R10_BEGIN, top_id);
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::record(
        crate::sched::trace::R10_FRAMES,
        0,
        MemoryManager::with(|mm| u128::from(mm.objects.high_frame)),
    );
    // From here the destruction defers every object-frame free and folds the per-object sweeps
    // into `destroy_marked`'s single pass (I1, I2).
    MemoryManager::with_mut(|mm| mm.begin_destruction());
    let mut caller_doomed = false;
    for victim in MemoryManager::with(|mm| mm.live_pids()) {
        if !MemoryManager::with(|mm| mm.runs_in_dying(victim)) {
            continue;
        }
        if Some(victim) == caller {
            caller_doomed = true;
        } else {
            // Each gets an exit notice with cause `killed`, unless its process object is
            // charged to a budget in the same doomed subtree (`process.rs`).
            crate::process::killed(ss, victim);
        }
    }
    if let (true, Some(caller)) = (caller_doomed, caller) {
        crate::process::killed(ss, caller);
    }
    // R10 reaches the process objects charged to the subtree: each is freed, with no notice,
    // its process killed first if it still runs.
    crate::process::budgets_dying(ss);
    // The caller may run outside this subtree but have its process object charged to it.
    // R10 killed it through its creator above; never return registers to that dead PID.
    if let Some(caller) = caller {
        caller_doomed |= MemoryManager::with(|mm| mm.budget_of(caller).is_none());
    }
    // R10 reaches messages in flight: the endpoints the subtree owns are destroyed, and every
    // message sent through a handle stamped with it fails its sender with `Dead`.
    MemoryManager::with_mut(|mm| {
        crate::message::budgets_dying(ss, mm, top);
        // Each budget's work since entry moves to its parent, bottom-up, and its carve returns.
        mm.lift_dying(top);
        mm.destroy_marked(top);
        // A deadline's whole cost, the walk that found it included, is its payer's (R10, R12).
        if let (Some(started), Some(payer)) = (deadline_since, payer) {
            crate::sched::bill(mm, payer, crate::sched::now_ticks().saturating_sub(started));
        }
        mm.end_destruction();
    });
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::r10(crate::sched::trace::R10_END, top_id);
    // The audit, off the measured walk: the links and indexes name exactly the live objects. It
    // neither moves the schedule nor counts in a latency target (`sched::audit`).
    #[cfg(debug_assertions)]
    crate::sched::audit(crate::sched::AUDIT_DESTRUCTION, || {
        MemoryManager::with(|mm| mm.check_object_indexes())
    });
    caller_doomed
}
