// SPDX-License-Identifier: MIT OR Apache-2.0

//! Budgets (KERNEL-SPEC.md, Budget; R6-R10) and the per-process accounts that charge them.
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
//! A budget's own object costs its parent one page (answer 76). A process object's notice page
//! costs its creator one page; every thread's IPC page costs the execution budget one page.
//! Separately allocated saved contexts cost the execution budget their actual physical frames
//! (one on rv32, two on rv64), in addition to its page tables and mapped RAM (answer 127).
//! Frame charges follow ownership in `mem.rs`; handle-table pages are charged in `handle.rs`.

use redoubt_abi::PID;
use redoubt_sys::{BudgetSpec, Error, FOREVER, MAX_DEPTH, MAX_LABELS, Usage};

use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT, MAX_THREAD};
use crate::handle::{BudgetRef, Handle, HandleTable, Object};
use crate::kframe;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// A budget, named by the index of its frame in the page-ownership table.
pub type BudgetFrame = u32;

/// The cost table (KERNEL-SPEC.md, What objects cost), in pages.
pub const BUDGET_PAGES: u64 = 1;
pub const PROCESS_PAGES: u64 = 1;
pub const THREAD_PAGES: u64 = 1;

/// The weight `root` starts with. Weights only matter relative to each other (R12), so any
/// value works; this one leaves room to carve INIT.md's manifest weights (1000 for `init`, the
/// steward and the drivers, 100 for a session). INTERIM, until WP-R3 builds the tree from the
/// manifest.
const ROOT_WEIGHT: u32 = 1_000_000;
/// What `root` keeps for `init` when carving `system` and `users` (KERNEL-SPEC.md, R12: a
/// budget holding a process has free weight).
const INIT_WEIGHT: u32 = 1000;

/// A budget's class (KERNEL-SPEC.md, Budget): inherited from its parent, so never in the ABI.
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
    pub cursor: Option<(u8, u8)>,
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
const WORDS: usize = 16 + MAX_LABELS + 8;
/// Where the scheduling words start, after the labels.
const W_SCHED: usize = 16 + MAX_LABELS;

/// One process's side of the ledger. The kernel (PID 1) has none: it has no budget.
#[derive(Clone, Copy)]
pub struct Account {
    pub budget: Option<BudgetFrame>,
    pub threads: u64,
    /// RAM frames owned by the process and charged to its budget (page tables and mapped pages).
    pub frames: u64,
    pub handles: HandleTable,
    /// Each thread's IPC page (`message.rs`), by frame index; 0 for a thread that has none.
    /// This *is* the page the cost table charges for a thread: the saved registers live in
    /// `ProcessImpl`, and everything IPC needs (what the thread waits for, the message it is
    /// sending, the calls it holds open) lives here, so `call` and `send` never allocate.
    pub ipc: [u32; MAX_THREAD],
    /// Open calls this process's threads hold (R4a): at `MAX_OPEN_CALLS` it takes no more.
    pub open_calls: u32,
    /// The next message id its threads will hand a sender. Never 0, never reused within this
    /// process, and from no counter anyone else can see (I12, CONTAINMENT.md).
    pub next_msg_id: u64,
    /// The DMA registry slots it has mapped with `map_device` (WP-K5b, `dma.rs`): half of the
    /// set its death must reset. Zero again for a new process in the same PID.
    pub dma_mapped: u16,
    /// No thread of this process has a timeout earlier than this (`message::next_timeout`): only
    /// ever early, so expiry walks just the processes it might be due in.
    pub earliest_timeout: u64,
}

impl Account {
    const NONE: Account = Account {
        budget: None,
        threads: 0,
        frames: 0,
        handles: HandleTable::EMPTY,
        ipc: [0; MAX_THREAD],
        open_calls: 0,
        next_msg_id: 1,
        earliest_timeout: u64::MAX,
        dma_mapped: 0,
    };
}

pub struct Objects {
    /// The next object id, shared by budgets and endpoints. Ids are never reused (I12); a
    /// `u64` cannot run out.
    next_id: u64,
    /// The next send order number (R2: "the oldest message of the next group"). A `u64` cannot
    /// run out, and userspace never sees it, so it is no covert channel.
    next_seq: u64,
    /// The highest frame ever given to a kernel object: where a scan for budgets stops.
    pub high_frame: u32,
    /// The first of the budgets with a deadline, linked through their frames
    /// (`Budget::next_deadline`), so finding the next deadline never scans every frame.
    deadlines: Option<BudgetFrame>,
    accounts: [Account; MAX_PROCESS_COUNT],
}

impl Objects {
    pub const fn new() -> Objects {
        Objects {
            next_id: 1,
            next_seq: 1,
            high_frame: 0,
            deadlines: None,
            accounts: [Account::NONE; MAX_PROCESS_COUNT],
        }
    }
}

fn account_index(pid: PID) -> Option<usize> {
    let index = usize::from(pid.get()) - 1;
    (index < MAX_PROCESS_COUNT).then_some(index)
}

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
            // The cursor: (pid, tid) plus one in the low bytes, 0 for none.
            cursor: match w(W_SCHED + 7) {
                0 => None,
                c => Some(((c >> 8) as u8, (c as u8).wrapping_sub(1))),
            },
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
        words[W_SCHED + 7] = b.cursor.map_or(0, |(p, t)| u64::from(p) << 8 | u64::from(t.wrapping_add(1)));
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

    /// `frame`'s id.
    pub fn budget_id(&self, frame: BudgetFrame) -> u64 { kframe::read(self.object_phys(frame), 8) }

    /// `frame`'s free weight: its stride weight (R12).
    pub fn free_weight_of(&self, frame: BudgetFrame) -> u64 {
        let phys = self.object_phys(frame);
        kframe::read(phys, 13 * 8).saturating_sub(kframe::read(phys, 14 * 8))
    }

    /// The next never-reused object id (budgets, endpoints).
    pub fn next_object_id(&mut self) -> u64 {
        let id = self.objects.next_id;
        self.objects.next_id = id.checked_add(1).expect("I12: object ids exhausted");
        id
    }

    /// The next send order number.
    pub fn next_seq(&mut self) -> u64 {
        let seq = self.objects.next_seq;
        self.objects.next_seq = seq.checked_add(1).expect("send order exhausted");
        seq
    }

    /// The next message id process `pid` hands a sender (I12).
    pub fn next_msg_id(&mut self, pid: PID) -> u64 {
        let account = self.account_mut(pid).expect("account");
        let id = account.next_msg_id;
        account.next_msg_id = id.checked_add(1).expect("I12: message ids exhausted");
        id
    }

    /// The frame of thread `tid`'s IPC page, if it has one.
    pub fn ipc_frame(&self, pid: PID, tid: usize) -> Option<u32> {
        self.account(pid).and_then(|a| a.ipc.get(tid).copied()).filter(|f| *f != 0)
    }

    /// Give thread `tid` its IPC page. Its cost is [`THREAD_PAGES`], charged when the thread was
    /// created, so the frame is already paid for; one missing here would mean the kernel
    /// over-committed RAM, which `boot_budgets` reserves against, so it stops (fail closed).
    fn give_ipc_frame(&mut self, pid: PID, tid: usize) {
        if self.account(pid).is_none() || tid >= MAX_THREAD || self.ipc_frame(pid, tid).is_some() {
            return;
        }
        let frame = self.alloc_object_frame().expect("R6: a thread's page was charged but has no frame");
        self.account_mut(pid).expect("account").ipc[tid] = frame;
    }

    /// Take thread `tid`'s IPC page back. Its contents are dead by now: `message.rs` unwinds
    /// what the thread waited for and the calls it held before the thread goes.
    fn take_ipc_frame(&mut self, pid: PID, tid: usize) {
        if let Some(frame) = self.ipc_frame(pid, tid) {
            self.account_mut(pid).expect("account").ipc[tid] = 0;
            self.free_object_frame(frame);
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

    pub fn account(&self, pid: PID) -> Option<&Account> {
        account_index(pid).map(|i| &self.objects.accounts[i]).filter(|a| a.budget.is_some())
    }

    pub fn account_mut(&mut self, pid: PID) -> Option<&mut Account> {
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
    pub fn budget_of(&self, pid: PID) -> Option<BudgetFrame> { self.account(pid).and_then(|a| a.budget) }

    /// A RAM frame became `pid`'s: charge it to `pid`'s budget, if it has one.
    pub fn charge_frame(&mut self, pid: PID) -> Result<(), Error> {
        if let Some(budget) = self.budget_of(pid) {
            self.charge(budget, 1)?;
            self.account_mut(pid).expect("account").frames += 1;
        }
        Ok(())
    }

    /// A RAM frame stopped being `pid`'s.
    pub fn uncharge_frame(&mut self, pid: PID) {
        if let Some(budget) = self.budget_of(pid) {
            let account = self.account_mut(pid).expect("account");
            account.frames = account.frames.checked_sub(1).expect("I5: frame count underflow");
            self.uncharge(budget, 1);
        }
    }

    /// Every frame of `pid`'s was just freed at once (`release_all_memory_for_process`).
    pub fn uncharge_all_frames(&mut self, pid: PID) {
        if let Some(budget) = self.budget_of(pid) {
            let frames = core::mem::take(&mut self.account_mut(pid).expect("account").frames);
            self.uncharge(budget, frames);
        }
    }

    // --- Processes and threads ------------------------------------------------------------------

    /// Put new process `pid` in `budget`: one process from its process limit, and an account of
    /// its own, with no threads yet. Its address space is charged to `budget` frame by frame as
    /// it is built (`process.rs`, and answer 127); its object page is the *creator's*, which
    /// `process_create` charges separately. Nothing changes on an error.
    pub fn process_created(&mut self, pid: PID, budget: BudgetFrame) -> Result<(), Error> {
        let index = account_index(pid).ok_or(Error::InvalidArgument)?;
        let mut b = self.budget(budget);
        // A budget with no free weight holds no process (R12: its stride weight is its free
        // weight).
        if b.free_weight() == 0 {
            return Err(Error::InvalidArgument);
        }
        if b.free_processes() == 0 {
            return Err(Error::OutOfProcesses);
        }
        b.processes_used += 1;
        self.store(budget, &b);
        self.objects.accounts[index] = Account { budget: Some(budget), ..Account::NONE };
        Ok(())
    }

    /// Everything the process still has charged goes back to its budget. Its frames were
    /// released just before (`uncharge_all_frames`); its handle table goes here.
    pub fn process_ended(&mut self, pid: PID) {
        let Some(budget) = self.budget_of(pid) else { return };
        self.close_all_handles(pid);
        for tid in 0..MAX_THREAD {
            self.take_ipc_frame(pid, tid);
        }
        let account = self.account_mut(pid).expect("account");
        let pages = account.threads * THREAD_PAGES + account.frames;
        *account = Account::NONE;
        self.uncharge(budget, pages);
        let mut b = self.budget(budget);
        b.processes_used = b.processes_used.checked_sub(1).expect("I5: process count underflow");
        self.store(budget, &b);
    }

    pub fn thread_created(&mut self, pid: PID, tid: usize) -> Result<(), Error> {
        if let Some(budget) = self.budget_of(pid) {
            self.charge(budget, THREAD_PAGES)?;
            self.account_mut(pid).expect("account").threads += 1;
            self.give_ipc_frame(pid, tid);
        }
        Ok(())
    }

    pub fn thread_ended(&mut self, pid: PID, tid: usize) {
        if let Some(budget) = self.budget_of(pid) {
            self.take_ipc_frame(pid, tid);
            let account = self.account_mut(pid).expect("account");
            account.threads = account.threads.checked_sub(1).expect("I5: thread count underflow");
            self.uncharge(budget, THREAD_PAGES);
        }
    }

    // --- Boot -----------------------------------------------------------------------------------

    /// Create `root`, `system` and `users` and put the loader's processes in `system`.
    ///
    /// INTERIM (until WP-R3 loads only `init` and builds the tree from the boot
    /// manifest): the sizes are computed here rather than read from the argument block. `root`
    /// gets every RAM page the kernel did not keep at boot, every PID but the kernel's, and all
    /// the weight; `system` a quarter of each (RESOURCES.md's default), `users` the rest. Every
    /// loader-started process (every PID owning frames: the loader gave each its pages) is
    /// charged to `system`, and the first one (PID 2) gets handles to the three budgets in slots
    /// 1-3, stamped with `root`, as `init` will.
    ///
    /// A loader bundle whose processes do not fit in `system` cannot run under the rules, so the
    /// kernel refuses to boot (fail closed).
    pub fn boot_budgets(&mut self) {
        // Every RAM page the kernel did not keep for itself. Nothing is held back any more
        // (answer 127): a process's saved contexts and its root page table are charged to
        // the budget it runs in as they are allocated, like any other frame it owns, so every
        // charged page has a real frame behind it without a reservation.
        let pages = self.ram_frames() - self.ram_frames_owned_by(crate::services::KERNEL_PID) as u64;
        let processes = (MAX_PROCESS_COUNT - 1) as u32;
        let (sys_pages, sys_processes, sys_weight) = (pages / 4, processes / 4, ROOT_WEIGHT / 4);
        // `users` gets the rest of the weight but what `root` keeps for `init`.
        let users_weight = ROOT_WEIGHT - sys_weight - INIT_WEIGHT;
        // Root pays for the two budgets' own pages. Root's own page is charged to no one: it has
        // no parent, and its frame is one of the RAM pages counted in its limit, taken for the tree
        // itself.
        let users_pages = pages - 2 * BUDGET_PAGES - sys_pages;
        // `root` and `system` are class `system`; `users` is class `user`. Nothing runs before
        // anything else: one stride queue, and weight decides (answer 103).
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
        let users = boot(self, Some(root), Class::User, users_pages, processes - sys_processes, users_weight);
        let mut first = None;
        let mut bundle = [None; MAX_PROCESS_COUNT];
        let mut nbundle = 0;
        for index in 2..=MAX_PROCESS_COUNT {
            let pid = PID::new(index as u8).expect("PIDs start at 1");
            let frames = self.ram_frames_owned_by(pid) as u64;
            if frames == 0 {
                continue;
            }
            first.get_or_insert(pid);
            bundle[nbundle] = Some(pid);
            nbundle += 1;
            // Everything the loader gave it: its image, its stack, its page tables, its root
            // table and its saved contexts, all owned by the PID in the ownership table.
            self.process_created(pid, system).expect("boot: the loader's processes do not fit in system");
            self.charge(system, frames).expect("boot: the loader's processes do not fit in system");
            self.account_mut(pid).expect("account").frames = frames;
            self.thread_created(pid, INITIAL_TID).expect("boot: no page for a program's first thread");
        }
        let stamp = BudgetRef { frame: root, id: self.budget(root).id };
        if let Some(first) = first {
            for budget in [root, system, users] {
                let id = self.budget(budget).id;
                let handle =
                    Handle { object: Object::Budget(BudgetRef { frame: budget, id }), badge: 0, stamp };
                self.install_handle(first, handle).expect("boot: no room for the first program's handles");
            }
        }
        // The machine's devices, charged to `system` and given to the first program as `init`
        // will receive them (INTERIM, `device.rs`). They come after the three budget handles,
        // so the first program's table is 1-3 budgets, 4.. devices. The handles are stamped
        // with `root`, like the three budget handles, and not with the budget the objects are
        // charged to: a stamp says which budget's destruction revokes the *handle* (R10), and
        // these are `init`'s to hand on, so they outlive anything below `root`. The objects
        // themselves are charged to, and die with, `system`.
        self.boot_devices(system, first, stamp);
        self.boot_endpoint(system, &bundle[..nbundle]);
        self.boot_log_endpoint(system, &bundle[..nbundle]);
        println!(
            "Budgets: root {} pages, system {} (the loader's processes), users {}",
            pages, sys_pages, users_pages
        );
    }

    /// INTERIM (until WP-K4's `process_start` passes handles and WP-R3's `init` hands out
    /// endpoints from the boot manifest): one endpoint for the bundle's programs to talk over,
    /// because nothing else can put a Redoubt handle in a second process yet.
    ///
    /// The **second** program gets the receive right (badge 0, handle 1) and every later one a
    /// handle badged with its own PID, so a server can tell its clients apart. The first
    /// program's table is left as `init`'s will be: `root`, `system` and `users`, then a handle
    /// to every device object the machine has (`device.rs`, `boot_devices`), then only the log
    /// endpoint's receive right (`boot_log_endpoint`).
    fn boot_endpoint(&mut self, system: BudgetFrame, bundle: &[Option<PID>]) {
        let Some(Some(server)) = bundle.get(1).copied() else { return };
        let endpoint = self.new_endpoint(system).expect("boot: system cannot pay for the endpoint");
        let owner = self.endpoint(endpoint.frame).owner;
        for pid in bundle.iter().skip(1).flatten() {
            // The receive right for the server; a badge for each client, its own PID, which
            // `mint` never produces as 0 (I3).
            let badge = if *pid == server { 0 } else { u64::from(pid.get()) };
            let handle = Handle { object: Object::Endpoint(endpoint), badge, stamp: owner };
            self.install_handle(*pid, handle).expect("boot: no room for a program's endpoint handle");
        }
    }

    /// INTERIM (until WP-R3's `init` owns the console): the log endpoint, one in every boot.
    ///
    /// The first program, which owns the console, gets the receive right, installed last, after
    /// the budgets and the devices, so it is the highest index in its table. Every later program
    /// gets a send in slot 2, after the boot endpoint's slot 1, badged with its own PID. The
    /// badge only says whose line it is: the server prints it and grants nothing on it.
    fn boot_log_endpoint(&mut self, system: BudgetFrame, bundle: &[Option<PID>]) {
        let Some(Some(first)) = bundle.first().copied() else { return };
        let endpoint = self.new_endpoint(system).expect("boot: system cannot pay for the log endpoint");
        let owner = self.endpoint(endpoint.frame).owner;
        for pid in bundle.iter().flatten() {
            let badge = if *pid == first { 0 } else { u64::from(pid.get()) };
            let handle = Handle { object: Object::Endpoint(endpoint), badge, stamp: owner };
            self.install_handle(*pid, handle).expect("boot: no room for a program's log handle");
        }
    }

    // --- The calls --------------------------------------------------------------------------------

    /// Create the budget object (after every check): its frame, its id, its place in the tree;
    /// its own object and its limits charged to the parent (R6, R7; answer 76). The class is the
    /// caller's to choose: `budget_create` passes the parent's (answer 73), boot its own.
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

    /// `budget_create(h(parent), spec) -> h`, after decoding. The checks follow KERNEL-SPEC.md's
    /// row for `budget_create`, in order.
    pub fn budget_create(&mut self, pid: PID, parent: u32, spec: &BudgetSpec) -> Result<u32, Error> {
        // Only the kernel (PID 1) has no account, and it makes no Redoubt calls; `NotPermitted` is
        // there so that a bug cannot turn into a panic.
        let caller = self.budget_of(pid).ok_or(Error::NotPermitted)?;
        let pf = self.budget_handle(pid, parent)?;
        let p = self.budget(pf);
        let caller_class = self.budget(caller).class;
        if p.depth as usize + 1 >= MAX_DEPTH {
            return Err(Error::TooLarge);
        }
        // A child's class is its parent's (answer 73); nothing else about scheduling is the
        // caller's to choose (answer 103: one stride queue, ordered by weight).
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
        // R6, R7: the parent pays the child's own page and carves its limits (answer 76: a
        // revocation scope, with zero limits, is no special case).
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
    pub fn budget_usage(&self, pid: PID, h: u32) -> Result<Usage, Error> {
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
    /// (`process_is_doomed`), and finishes with [`MemoryManager::destroy_marked`].
    pub fn destroy_begin(&mut self, pid: PID, h: u32) -> Result<BudgetFrame, Error> {
        let top = self.budget_handle(pid, h)?;
        self.mark_dying(top);
        Ok(top)
    }

    /// Mark `top` and everything below it dying (R10's first step, for `budget_destroy` and for
    /// a deadline alike). Before anything else, `top`'s carve comes back to its parent, so the
    /// destruction's own work (often the parent's own `budget_destroy`) is charged at the weight
    /// the parent has once the child is gone, not at the sliver it kept while the child held the
    /// rest (K5-code-review-4 D1; `sched.rs`: a weight change charges what ran before it). The
    /// budgets below the top return theirs as the scheduler lifts them, bottom-up
    /// ([`MemoryManager::lift_dying`]).
    pub fn mark_dying(&mut self, top: BudgetFrame) {
        if let Some(p) = self.budget(top).parent {
            let limit = self.budget(top).weight_limit;
            crate::sched::change_weight(self, p, |mm| {
                let mut pb = mm.budget(p);
                pb.weight_carved = pb.weight_carved.checked_sub(limit).expect("I5: carve underflow");
                mm.store(p, &pb);
            });
        }
        for frame in 0..=self.objects.high_frame {
            if self.is_budget_frame(frame) && self.below(frame, top) {
                let mut b = self.budget(frame);
                b.dying = true;
                self.store(frame, &b);
            }
        }
    }

    /// Whether `pid` lives in a budget that is being destroyed.
    pub fn process_is_doomed(&self, pid: PID) -> bool {
        self.budget_of(pid).is_some_and(|b| self.budget(b).dying)
    }

    /// Last step of `budget_destroy`, once the doomed budgets' processes are gone: close every
    /// handle naming a doomed budget or stamped with one, in every table (R10, I2); give the
    /// parent back what `top` carved from it (I10); free the doomed budgets (marked, with no
    /// processes and no handles left; nothing reads a dying frame's tree links after this).
    pub fn destroy_marked(&mut self, top: BudgetFrame) {
        self.sweep_handles(|mm, h| {
            let object_dying = match h.object {
                Object::Budget(b) => mm.budget_at(b).dying,
                // An endpoint, and a device, die with their owner, so a handle to one is
                // revoked with it.
                Object::Endpoint(e) => mm.budget_at(mm.endpoint_at(e).owner).dying,
                Object::Device(d) => mm.budget_at(mm.device_at(d).owner).dying,
                // A process object dies with the budget it is charged to, its creator's (R10).
                Object::Process(p) => mm.budget_at(mm.process_at(p).creator).dying,
            };
            object_dying || mm.budget_at(h.stamp).dying
        });
        // The weight came back as the scheduler lifted each budget (`sched::destroy`).
        self.return_carve(top, false);
        // Then, and only then (N1), quarantined DMA pages charged in the subtree move to the
        // parent, which has just got back at least that much (WP-K5b, OD5).
        self.dma_migrate_quarantine(self.budget(top).parent);
        for frame in 0..=self.objects.high_frame {
            if self.is_budget_frame(frame) && self.budget(frame).dying {
                self.unlink_deadline(frame);
                self.free_object_frame(frame);
            }
        }
    }

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
        self.objects.accounts.iter().any(|a| a.budget == Some(frame))
    }

    /// The dying budgets, deepest first (every one's descendants before it): R10's bottom-up
    /// order for the scheduler's lifts, each returning its weight to its parent as it goes (the
    /// top's went back at mark time).
    pub fn lift_dying(&mut self, top: BudgetFrame) {
        let top_depth = self.budget(top).depth;
        for depth in (top_depth..MAX_DEPTH as u32).rev() {
            for frame in 0..=self.objects.high_frame {
                if self.is_budget_frame(frame)
                    && self.budget(frame).dying
                    && self.budget(frame).depth == depth
                {
                    crate::sched::destroy(self, frame, frame == top);
                }
            }
        }
    }
}

/// R10 for the subtree marked dying at `top` (`MemoryManager::mark_dying`), for `budget_destroy`
/// and for a deadline alike: every process in it is killed (each exit notice `killed`), the
/// caller last if it is one of them; the process objects charged to it are freed; messages in
/// flight are failed or abandoned and its endpoints and devices destroyed; then its handles are
/// swept and its frames freed. `caller` is the process whose call or whose interrupted run this
/// is, if any. Returns whether the caller is gone (it must not be resumed).
pub fn destroy_subtree(ss: &mut SystemServices, top: BudgetFrame, caller: Option<PID>, bill: bool) -> bool {
    let started = crate::sched::now_ticks();
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
    let mut caller_doomed = false;
    for index in 1..=MAX_PROCESS_COUNT {
        let Some(victim) = PID::new(index as u8) else { continue };
        if !MemoryManager::with(|mm| mm.process_is_doomed(victim)) {
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
        crate::message::budgets_dying(ss, mm);
        // A deadline's work so far is the dying budget's own, and moves up with its debt.
        if bill {
            let top_ref = BudgetRef { frame: top, id: mm.budget(top).id };
            crate::sched::bill(mm, top_ref, crate::sched::now_ticks().saturating_sub(started));
        }
        // Each budget's work since entry moves to its parent, bottom-up, and its carve returns.
        mm.lift_dying(top);
        mm.destroy_marked(top);
    });
    #[cfg(feature = "sched-trace")]
    crate::sched::trace::r10(crate::sched::trace::R10_END, top_id);
    caller_doomed
}
