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
//! A budget's own object 1 page, always to its parent (answer 76); a process 1; a
//! thread 1; each page-table page and each mapped RAM frame 1 (counted as frames change owner,
//! `mem.rs`); each handle-table page 1 (`handle.rs`). The frames holding a process's saved
//! contexts (`ProcessImpl`) are the physical form of the process and thread objects and are not
//! charged again.

use redoubt_sys::{BudgetSpec, Error, FOREVER, MAX_DEPTH, MAX_LABELS, Usage};
use xous_kernel::PID;

use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT, MAX_THREAD};
use crate::handle::{BudgetRef, Handle, HandleTable, Object};
use crate::kframe;
use crate::mem::MemoryManager;

/// A budget, named by the index of its frame in the page-ownership table.
pub type BudgetFrame = u32;

/// The cost table (KERNEL-SPEC.md, What objects cost), in pages.
pub const BUDGET_PAGES: u64 = 1;
pub const PROCESS_PAGES: u64 = 1;
pub const THREAD_PAGES: u64 = 1;

/// The weight `root` starts with. Weights only matter relative to each other (R12), so any
/// value works; this one leaves room to carve.
const ROOT_WEIGHT: u32 = 1000;

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
    /// Absolute µs since boot; `FOREVER` for none. Recorded here; WP-K5 enforces it.
    pub deadline: u64,
    pub pages_limit: u64,
    pub pages_used: u64,
    pub processes_limit: u32,
    pub processes_used: u32,
    pub weight_limit: u32,
    /// The children's weight limits (R7).
    pub weight_carved: u32,
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
const WORDS: usize = 16 + MAX_LABELS;

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
    accounts: [Account; MAX_PROCESS_COUNT],
}

impl Objects {
    pub const fn new() -> Objects {
        Objects { next_id: 1, next_seq: 1, high_frame: 0, accounts: [Account::NONE; MAX_PROCESS_COUNT] }
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
        words[16..].copy_from_slice(&b.labels);
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, i * 8, *word);
        }
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
        self.is_budget_frame(r.frame) && self.budget(r.frame).id == r.id
    }

    /// Whether `b` is `ancestor` or below it (R9: a budget handle only narrows).
    pub fn is_at_or_below(&self, b: BudgetFrame, ancestor: BudgetFrame) -> bool {
        self.below(b, ancestor)
    }

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

    /// Whether `n` frames of `from`'s can become `to`'s: the budget paying for them changes
    /// only if the two live in different budgets.
    pub fn can_take_frames(&self, from: PID, to: PID, n: u64) -> bool {
        match self.budget_of(to) {
            None => true,
            Some(to_budget) if self.budget_of(from) == Some(to_budget) => true,
            Some(to_budget) => n <= self.budget(to_budget).free_pages(),
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

    /// Put new process `pid`, with its first thread, in `budget`: one process from its process
    /// limit, and the process and thread objects from its pages. Nothing changes on an error.
    pub fn process_created(&mut self, pid: PID, budget: BudgetFrame) -> Result<(), Error> {
        let index = account_index(pid).ok_or(Error::InvalidArgument)?;
        let mut b = self.budget(budget);
        // A weight-0 budget holds no process (R12).
        if b.weight_limit == 0 {
            return Err(Error::InvalidArgument);
        }
        if b.free_processes() == 0 {
            return Err(Error::OutOfProcesses);
        }
        if PROCESS_PAGES + THREAD_PAGES > b.free_pages() {
            return Err(Error::OutOfMemory);
        }
        b.processes_used += 1;
        b.pages_used += PROCESS_PAGES + THREAD_PAGES;
        self.store(budget, &b);
        self.objects.accounts[index] = Account { budget: Some(budget), threads: 1, ..Account::NONE };
        // The first thread's page, charged just above, is its IPC page.
        self.give_ipc_frame(pid, INITIAL_TID);
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
        let pages = PROCESS_PAGES + account.threads * THREAD_PAGES + account.frames;
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
    /// INTERIM (until WP-K4 loads only `init`, and WP-R3's `init` builds the tree from the boot
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
        // INTERIM (QUESTIONS.md 127; until WP-K4 creates processes from userspace and charges
        // it): held back from `root`, so that every charged page has a real frame behind it (R7:
        // an allocation fails only on the caller's own budget, never because the kernel ran
        // out). A process's own page pays for one frame, but its saved contexts (`ProcessImpl`)
        // take `PROCESS_IMPL_PAGES`; the thread pages that once covered the difference now each
        // hold a thread's IPC page (`Account::ipc`). The gap is fixed per process, so reserving
        // it for every PID at boot covers every process the kernel can ever hold.
        let per_process = crate::arch::process::PROCESS_IMPL_PAGES as u64 - PROCESS_PAGES;
        let reserved = per_process * MAX_PROCESS_COUNT as u64;
        let pages = self.ram_frames() - self.ram_frames_owned_by(crate::services::KERNEL_PID) as u64
            - reserved;
        let processes = (MAX_PROCESS_COUNT - 1) as u32;
        let (sys_pages, sys_processes, sys_weight) = (pages / 4, processes / 4, ROOT_WEIGHT / 4);
        // Root pays for the two budgets' own pages. Root's own page is charged to no one: it has
        // no parent, and its frame is one of the RAM pages counted in its limit, taken for the tree
        // itself.
        let users_pages = pages - 2 * BUDGET_PAGES - sys_pages;
        // `root` and `system` are class `system`; `users` is class `user`. Nothing runs before
        // anything else: one stride queue, and weight decides (answer 103).
        let boot = |mm: &mut Self, parent, class, pages, processes, weight| {
            let spec = BudgetSpec { pages, processes, weight, labels: Default::default(), account: 0, deadline: FOREVER };
            mm.new_budget(parent, &spec, class, &[]).expect("boot: no frame for a boot budget")
        };
        let root = boot(self, None, Class::System, pages, processes, ROOT_WEIGHT);
        let system = boot(self, Some(root), Class::System, sys_pages, sys_processes, sys_weight);
        let users = boot(self, Some(root), Class::User, users_pages, processes - sys_processes, ROOT_WEIGHT - sys_weight);
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
            let frames = frames - crate::arch::process::PROCESS_IMPL_PAGES as u64;
            self.process_created(pid, system).expect("boot: the loader's processes do not fit in system");
            self.charge(system, frames).expect("boot: the loader's processes do not fit in system");
            self.account_mut(pid).expect("account").frames = frames;
        }
        let stamp = BudgetRef { frame: root, id: self.budget(root).id };
        if let Some(first) = first {
            for budget in [root, system, users] {
                let id = self.budget(budget).id;
                let handle = Handle { object: Object::Budget(BudgetRef { frame: budget, id }), badge: 0, stamp };
                self.install_handle(first, handle).expect("boot: no room for the first program's handles");
            }
        }
        // The machine's devices, charged to `system` and given to the first program as `init`
        // will receive them (INTERIM, `device.rs`). They come after the three budget handles,
        // so the first program's table is 1-3 budgets, 4.. devices.
        self.boot_devices(system, first, stamp);
        self.boot_endpoint(system, &bundle[..nbundle]);
        println!("Budgets: root {} pages, system {} (the loader's processes), users {}", pages, sys_pages, users_pages);
    }

    /// INTERIM (until WP-K4's `process_start` passes handles and WP-R3's `init` hands out
    /// endpoints from the boot manifest): one endpoint for the bundle's programs to talk over,
    /// because nothing else can put a Redoubt handle in a second process yet.
    ///
    /// The **second** program gets the receive right (badge 0, handle 1) and every later one a
    /// handle badged with its own PID, so a server can tell its clients apart. The first
    /// program's table is left exactly as `init`'s will be: `root`, `system` and `users`.
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
        };
        self.store(frame, &b);
        if let (Some(p), Some(mut pb)) = (parent, parent_budget) {
            pb.pages_used += BUDGET_PAGES + spec.pages;
            pb.processes_used += spec.processes;
            pb.weight_carved += spec.weight;
            self.store(p, &pb);
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
        // No error names weight; the spec's stated exception.
        if spec.weight > p.free_weight() {
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
        self.install_handle(pid, handle).inspect_err(|_| {
            self.return_carve(child);
            self.free_object_frame(child);
        })
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
        for frame in 0..=self.objects.high_frame {
            if self.is_budget_frame(frame) && self.below(frame, top) {
                let mut b = self.budget(frame);
                b.dying = true;
                self.store(frame, &b);
            }
        }
        Ok(top)
    }

    /// Whether `pid` lives in a budget that is being destroyed.
    pub fn process_is_doomed(&self, pid: PID) -> bool { self.budget_of(pid).is_some_and(|b| self.budget(b).dying) }

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
            };
            object_dying || mm.budget_at(h.stamp).dying
        });
        self.return_carve(top);
        for frame in 0..=self.objects.high_frame {
            if self.is_budget_frame(frame) && self.budget(frame).dying {
                self.free_object_frame(frame);
            }
        }
    }

    /// Give `b`'s parent back what `b` carved from it, and `b`'s own page (I10).
    fn return_carve(&mut self, b: BudgetFrame) {
        let b = self.budget(b);
        let Some(p) = b.parent else { return };
        let mut pb = self.budget(p);
        let carved = BUDGET_PAGES + b.pages_limit;
        pb.pages_used = pb.pages_used.checked_sub(carved).expect("I5: carve underflow");
        pb.processes_used = pb.processes_used.checked_sub(b.processes_limit).expect("I5: carve underflow");
        pb.weight_carved = pb.weight_carved.checked_sub(b.weight_limit).expect("I5: carve underflow");
        self.store(p, &pb);
    }
}
