// SPDX-License-Identifier: MIT OR Apache-2.0

//! Processes (KERNEL-SPEC.md, Process; R10) and the calls that make and end them:
//! `process_create`, `process_map`, `process_start`, `thread_create`, `thread_exit` and
//! `process_exit`, with the exit notices they produce.
//!
//! # The process object is the exit slot
//! A process object is one RAM frame of its own, allocated to `mem::OBJECT_OWNER` exactly as a
//! budget (`budget.rs`), an endpoint (`endpoint.rs`) or a device (`device.rs`) is: that frame *is*
//! the page the cost table charges. It is charged to **the creator's budget**, not to the budget
//! the process runs in, because the frame holds the process's one exit notice and that notice must
//! outlive the budget it ran in (R10). Since the frame was bought when the process was created,
//! **delivering a notice never allocates**.
//!
//! So the object has two lives. While the process runs it is what a handle names: `process_map`
//! and `process_start` name it. When the process exits, faults or is killed, everything the
//! process had is freed at once -- threads, address space, handle table, open calls -- and it stops
//! counting against its budget's `processes` usage; the frame stays, now holding only the notice,
//! until a `receive` on the exit endpoint takes it or the notice is dropped. Its PID is reserved
//! for exactly that long (answer 106): [`random_free_pid`] skips a PID a live object names, so no
//! PID is reused while a notice still names it.
//!
//! Destroying the creator's budget frees the object (R10), killing the process first if it still
//! runs; then there is no notice at all.
//!
//! # What a process costs (answer 127)
//! The cost table says one page for the process object. A process needs more than one page of
//! kernel storage: its saved thread contexts take `PROCESS_IMPL_PAGES` frames and its root page
//! table one more. Those die with the process, so they are charged **to the budget it runs in**,
//! as ordinary frames of that process, while the object's own page -- the notice -- is the
//! creator's, paid by someone who is still alive when the process is not. WP-K1 and WP-K2 could
//! not charge them, because nothing created a process from userspace, and held back
//! `PROCESS_IMPL_PAGES - 1` per PID from `root` at boot instead; that reservation is gone with
//! this package.
//!
//! # Blame (answers 37, 55, 82)
//! A `faulted` notice blames the sender of the **current call** of the thread that faulted or
//! called `process_exit`: the call `receive` last delivered to it, or the one `serve` named. A
//! thread with no current call blames nobody, even when other threads of the process hold open
//! calls, and a `send` is never blamed because a send is never an open call. The account and the
//! labels are snapshots taken when the call was delivered (`message.rs`), so blame survives the
//! sender's budget being destroyed in between.
//!
//! # Locks
//! `budget.rs`, `handle.rs` and the reading half of this module work on a borrowed
//! [`MemoryManager`]. Ending a process does not: `SystemServices::terminate_process` takes the
//! memory manager itself, so everything that can end a process ([`died`], [`budgets_dying`],
//! [`thread_create`]) takes only the scheduler and borrows the memory manager in phases. The
//! functions that take both are only ever called from a dispatcher that holds both.

use redoubt_abi::{PID, TID};
use redoubt_sys::{
    Cause, Error, ExitNotice, Handle as AbiHandle, Labels, MAX_LABELS, MAX_START_HANDLES, MemFlags,
};

use crate::arch::process::{INITIAL_TID, MAX_PROCESS_COUNT, Process as ArchProcess};
use crate::budget::{Class, PROCESS_PAGES, THREAD_PAGES};
use crate::handle::{BudgetRef, EndpointRef, Handle, Object, ProcessRef};
use crate::kframe;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// First word of every process frame, so that a frame read as a process that is not one is
/// caught. Distinct from every other object's magic.
const MAGIC: u64 = u64::from_le_bytes(*b"process\0");

// Word layout in the frame (a frame holds 512).
const W_ID: usize = 1;
/// The budget the object is charged to: `process_create`'s caller's.
const W_CREATOR: usize = 2; // frame + 1
const W_CREATOR_ID: usize = 3;
const W_PID: usize = 4;
/// The exit endpoint, as `process_create` named it; 0 once it has been destroyed.
const W_ENDPOINT: usize = 5; // frame + 1
const W_ENDPOINT_ID: usize = 6;
const W_FLAGS: usize = 7;
const W_CAUSE: usize = 8;
const W_CODE: usize = 9;
const W_BLAMED_ACCOUNT: usize = 10;
const W_BLAMED_NLABELS: usize = 11;
const W_BLAMED_LABELS: usize = 12; // MAX_LABELS words
const WORDS: usize = W_BLAMED_LABELS + MAX_LABELS;
const _: () = assert!(WORDS * 8 <= redoubt_abi::arch::PAGE_SIZE);

/// `process_start` has run.
const F_STARTED: u64 = 1;
/// The process still exists: it has not exited, faulted or been killed.
const F_ALIVE: u64 = 2;
/// The process is gone and its notice is still owed to the exit endpoint.
const F_NOTICE: u64 = 4;

/// A process as the kernel works with it; it lives in its frame as words.
#[derive(Clone, Copy)]
pub struct Proc {
    pub id: u64,
    /// The budget charged for this object (`process_create`'s caller's).
    pub creator: BudgetRef,
    pub pid: PID,
    /// The exit endpoint, until it is destroyed.
    pub endpoint: Option<EndpointRef>,
    flags: u64,
    cause: u64,
    code: u32,
    blamed_account: u64,
    blamed_labels: [u64; MAX_LABELS],
    blamed_nlabels: usize,
}

impl Proc {
    pub fn started(&self) -> bool { self.flags & F_STARTED != 0 }

    pub fn alive(&self) -> bool { self.flags & F_ALIVE != 0 }

    fn notice_owed(&self) -> bool { self.flags & F_NOTICE != 0 }
}

/// R1's sender side for an exit notice, read before the budget it names can go: the class and
/// labels of the budget the process ran in.
#[derive(Clone, Copy)]
pub struct Flow {
    labels: [u64; MAX_LABELS],
    nlabels: usize,
}

fn frame_of(word: u64) -> Option<u32> { (word as u32).checked_sub(1) }

fn frame_word(frame: u32) -> u64 { u64::from(frame) + 1 }

impl MemoryManager {
    pub fn process(&self, frame: u32) -> Proc {
        let phys = self.object_phys(frame);
        let w = |i: usize| kframe::read(phys, i * 8);
        // As in `budget.rs`: a frame that does not hold a process means a stale reference
        // survived R10's sweep, a violated invariant (I1), so the kernel stops.
        assert!(w(0) == MAGIC, "I1: frame {} holds no process", frame);
        let mut labels = [0; MAX_LABELS];
        for (i, label) in labels.iter_mut().enumerate() {
            *label = w(W_BLAMED_LABELS + i);
        }
        Proc {
            id: w(W_ID),
            creator: BudgetRef { frame: frame_of(w(W_CREATOR)).unwrap_or(0), id: w(W_CREATOR_ID) },
            pid: PID::new(w(W_PID) as u8).expect("I1: a process object names no PID"),
            endpoint: frame_of(w(W_ENDPOINT)).map(|frame| EndpointRef { frame, id: w(W_ENDPOINT_ID) }),
            flags: w(W_FLAGS),
            cause: w(W_CAUSE),
            code: w(W_CODE) as u32,
            blamed_account: w(W_BLAMED_ACCOUNT),
            blamed_labels: labels,
            blamed_nlabels: (w(W_BLAMED_NLABELS) as usize).min(MAX_LABELS),
        }
    }

    pub fn store_process(&mut self, frame: u32, p: &Proc) {
        let phys = self.object_phys(frame);
        let mut words = [0u64; WORDS];
        words[0] = MAGIC;
        words[W_ID] = p.id;
        words[W_CREATOR] = frame_word(p.creator.frame);
        words[W_CREATOR_ID] = p.creator.id;
        words[W_PID] = u64::from(p.pid.get());
        words[W_ENDPOINT] = p.endpoint.map_or(0, |e| frame_word(e.frame));
        words[W_ENDPOINT_ID] = p.endpoint.map_or(0, |e| e.id);
        words[W_FLAGS] = p.flags;
        words[W_CAUSE] = p.cause;
        words[W_CODE] = u64::from(p.code);
        words[W_BLAMED_ACCOUNT] = p.blamed_account;
        words[W_BLAMED_NLABELS] = p.blamed_nlabels as u64;
        words[W_BLAMED_LABELS..].copy_from_slice(&p.blamed_labels);
        for (i, word) in words.iter().enumerate() {
            kframe::write(phys, i * 8, *word);
        }
    }

    /// Whether `frame` holds a process object. Used by the sweeps, which scan the object frames.
    pub fn is_process_frame(&self, frame: u32) -> bool {
        self.is_object_frame(frame) && kframe::read(self.object_phys(frame), 0) == MAGIC
    }

    /// Whether `r` still names the process object it named (a handle in a message R10 may have
    /// revoked meanwhile).
    pub fn is_live_process(&self, r: ProcessRef) -> bool {
        self.is_process_frame(r.frame) && self.process(r.frame).id == r.id
    }

    /// The process object `r` names, which must still be the one it named (I1).
    pub fn process_at(&self, r: ProcessRef) -> Proc {
        let p = self.process(r.frame);
        assert!(p.id == r.id, "I1: a handle names a process that is gone");
        p
    }

    /// The process object `pid`'s handle `index` names: `BadHandle`, then `WrongObject`.
    pub fn process_handle(&self, pid: PID, index: u32) -> Result<ProcessRef, Error> {
        match self.handle(pid, index)?.object {
            Object::Process(p) => Ok(p),
            _ => Err(Error::WrongObject),
        }
    }

    /// The first object frame for which `f` holds.
    fn find_process(&self, f: impl Fn(&MemoryManager, u32) -> bool) -> Option<u32> {
        (0..=self.objects.high_frame).find(|frame| self.is_process_frame(*frame) && f(self, *frame))
    }
}

/// The object frame of the process `pid`, if it has one. The loader's own programs have none:
/// nobody created them and nobody is owed their notice (`budget.rs`, `boot_budgets`).
pub fn object_of(mm: &MemoryManager, pid: PID) -> Option<u32> {
    mm.find_process(|mm, frame| mm.process(frame).pid == pid)
}

/// A PID drawn at random from the free ASIDs (KERNEL-SPEC.md, Process): free in the process
/// table, and named by no process object, so a PID is not reused while a notice still names it
/// (answer 106). Random so that nothing can predict which ASID a process will get.
fn random_free_pid(ss: &SystemServices, mm: &MemoryManager) -> Option<PID> {
    let free = |pid: PID| ss.get_process(pid).is_err() && object_of(mm, pid).is_none();
    let count = (2..=MAX_PROCESS_COUNT).filter(|i| PID::new(*i as u8).is_some_and(free)).count();
    if count == 0 {
        return None;
    }
    let mut bytes = [0u8; 8];
    crate::platform::rand::fill(&mut bytes);
    let nth = (u64::from_le_bytes(bytes) % count as u64) as usize;
    (2..=MAX_PROCESS_COUNT).filter_map(|i| PID::new(i as u8)).filter(|pid| free(*pid)).nth(nth)
}

// --- `process_create` ---------------------------------------------------------------------------

/// `process_create(h(budget), h(exit endpoint)) -> h(process)` (KERNEL-SPEC.md), after decoding.
/// The checks follow the spec's row for `process_create`, in order.
pub fn process_create(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: PID,
    budget_h: u32,
    endpoint_h: u32,
) -> Result<u32, Error> {
    // Only the kernel (PID 1) has no account, and it makes no Redoubt calls (as `budget_create`).
    let caller_budget = mm.budget_of(pid).ok_or(Error::NotPermitted)?;
    // Stage 2, argument by argument: the budget, then the exit endpoint.
    let target = mm.budget_handle(pid, budget_h)?;
    let (endpoint, exit_handle) = mm.endpoint_handle(pid, endpoint_h)?;
    // A weight-0 budget holds no process (R12); the spec's stated exception, `InvalidArgument`.
    if mm.budget(target).weight_limit == 0 {
        return Err(Error::InvalidArgument);
    }
    // Stage 3: an exit endpoint is named by its receive right, so a notice cannot be steered at
    // a badged handle somebody minted (I4).
    if exit_handle.badge != 0 {
        return Err(Error::NotPermitted);
    }
    // Stage 4, resources: the target budget's process limit, then its pages (the address space),
    // then the caller's (the object itself).
    //
    // A PID is a global resource: one is held by every live process and by every process object
    // whose notice nobody has taken yet. Running out is `OutOfProcesses` too -- the caller asked
    // for a process and there is none to be had.
    let child = random_free_pid(ss, mm).ok_or(Error::OutOfProcesses)?;
    // From here on every step is undone on failure, so a refused `process_create` costs nothing.
    mm.process_created(child, target)?;
    let made = new_address_space(ss, mm, child).and_then(|()| {
        mm.charge(caller_budget, PROCESS_PAGES)?;
        let frame = mm.alloc_object_frame().inspect_err(|_| mm.uncharge(caller_budget, PROCESS_PAGES))?;
        let id = mm.next_object_id();
        let creator = BudgetRef { frame: caller_budget, id: mm.budget(caller_budget).id };
        mm.store_process(
            frame,
            &Proc {
                id,
                creator,
                pid: child,
                endpoint: Some(endpoint),
                flags: F_ALIVE,
                cause: 0,
                code: 0,
                blamed_account: 0,
                blamed_labels: [0; MAX_LABELS],
                blamed_nlabels: 0,
            },
        );
        // R9: the new handle is stamped with the caller's budget.
        let object = Object::Process(ProcessRef { frame, id });
        mm.install_handle(pid, Handle { object, badge: 0, stamp: creator }).inspect_err(|_| {
            mm.free_object_frame(frame);
            mm.uncharge(caller_budget, PROCESS_PAGES);
        })
    });
    made.inspect_err(|_| drop_unstarted(ss, mm, child))
}

/// Give `child` an address space and a slot in the process table, but no thread: it cannot run
/// until `process_start`. Every frame it takes is charged to the budget it runs in (answer 127).
fn new_address_space(ss: &mut SystemServices, mm: &mut MemoryManager, child: PID) -> Result<(), Error> {
    let here = crate::arch::process::current_pid();
    ss.allocate_process_slot(mm, child).map_err(|_| Error::OutOfMemory)?;
    // `setup_empty_process` writes the new space's own `ProcessImpl`, so that space must be the
    // active one and the hardware PID must match.
    let prepared = ss
        .get_process(child)
        .and_then(|p| p.activate())
        .map(|()| ArchProcess::setup_empty_process(child))
        .map_err(|_| Error::OutOfMemory);
    ss.activate(here).expect("the running process can be activated");
    prepared
}

/// Undo a `process_create` that failed after the address space was made. The process has never
/// run, so nothing waits on it and nothing holds its handles; this is `Process::terminate` minus
/// everything that needs the memory manager it does not already hold.
fn drop_unstarted(ss: &mut SystemServices, mm: &mut MemoryManager, child: PID) {
    // Not `release_all_memory_for_process`: that one first walks the page tables for frames the
    // process lent out, and a process that has never run has lent nothing. What is left is the
    // frames it owns, which is what this frees -- and it needs no address space, so a
    // `process_create` that ran out of pages halfway through building one is the same case.
    mm.release_owned_frames(child);
    mm.process_ended(child);
    ss.free_process_slot(child);
}

// --- `process_map` -------------------------------------------------------------------------------

/// `process_map(h(process), src, dst, len, flags)` (KERNEL-SPEC.md): pages of the caller's own RAM
/// move into a process that has not started, at an address the caller chooses, and the budget
/// paying for them moves with them (R6). The image and the startup block travel this way
/// (PACKAGES.md, launching; INIT.md, Startup block).
#[allow(clippy::too_many_arguments)]
pub fn process_map(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: PID,
    process_h: u32,
    src: usize,
    dst: usize,
    len: usize,
    flags: MemFlags,
) -> Result<(), Error> {
    let page_size = redoubt_abi::arch::PAGE_SIZE;
    let r = mm.process_handle(pid, process_h)?;
    let p = mm.process_at(r);
    // Stage 2: the ranges, then the source, which must be the caller's own backed RAM, mapped
    // and not lent out. Checked whole before any page moves (WP-K0's rule).
    let pages = whole_pages(src, dst, len)?;
    mm.ensure_range_exists(src, len).map_err(|_| Error::InvalidArgument)?;
    for i in 0..pages {
        let phys = mm.owned_mapping(pid, src + i * page_size)?;
        if !mm.is_main_memory(phys as *mut u8) {
            return Err(Error::InvalidArgument);
        }
    }
    // R11, checked here as well as while decoding and in the page tables, so neither check
    // rests on the other (KERNEL-SPEC.md, ABI). It must refuse before any page moves,
    // because a later failure would not put the source back. Not W+X, and not writable
    // without readable.
    let flags = crate::mem::redoubt_flags(flags);
    crate::mem::check_map_flags(flags)?;
    let child = p.pid;
    let space = ss.mapping_of(child).ok_or(Error::NotPermitted)?;
    for i in 0..pages {
        if !crate::arch::mem::address_available_in(&space, dst + i * page_size) {
            return Err(Error::InvalidArgument);
        }
    }
    // Stage 3: a started process is closed to its parent. Everything it is given, it is given
    // before it runs, so nothing can be slipped into a process that is already working.
    if p.started() || !p.alive() {
        return Err(Error::NotPermitted);
    }
    // Stage 4: the child's budget pays for the page tables that map the range, and for the
    // frames themselves unless parent and child already share a budget.
    let budget = mm.budget_of(child).ok_or(Error::NotPermitted)?;
    let tables = crate::arch::mem::tables_needed(&space, dst, pages) as u64;
    let moved = if mm.budget_of(pid) == Some(budget) { 0 } else { pages as u64 };
    if tables + moved > mm.free_pages(budget) {
        return Err(Error::OutOfMemory);
    }
    // From here nothing fails: every page was counted just above and every address was free.
    for i in 0..pages {
        crate::arch::mem::prepare_map(mm, &space, child, dst + i * page_size)
            .expect("process_map: the page tables were counted and charged for just above");
    }
    for i in 0..pages {
        let phys = crate::arch::mem::unmap_page_inner(mm, src + i * page_size)
            .expect("process_map: a checked range unmaps");
        crate::arch::mem::map_into_with(mm, child, &space, phys, dst + i * page_size, flags)
            .expect("process_map: prepared just above");
        mm.move_frame(phys, pid, child).expect("process_map: the pages were charged above");
    }
    Ok(())
}

/// A source and destination range: both page-aligned, the same non-empty whole number of pages,
/// and inside user space. Anything else is `InvalidArgument`.
fn whole_pages(src: usize, dst: usize, len: usize) -> Result<usize, Error> {
    let page = redoubt_abi::arch::PAGE_SIZE;
    if len == 0 || len % page != 0 || src % page != 0 || dst % page != 0 {
        return Err(Error::InvalidArgument);
    }
    let src_end = src.checked_add(len).ok_or(Error::InvalidArgument)?;
    let dst_end = dst.checked_add(len).ok_or(Error::InvalidArgument)?;
    if src_end > redoubt_abi::arch::USER_AREA_END || dst_end > redoubt_abi::arch::USER_AREA_END {
        return Err(Error::InvalidArgument);
    }
    Ok(len / page)
}

// --- `process_start` ------------------------------------------------------------------------------

/// `process_start(h(process), entry, sp, arg, handles)` (KERNEL-SPEC.md): the handles are copied
/// into the child's empty table, so they land in slots 1..n, and its first thread starts at
/// `entry` with `sp` and `arg`. `arg` is the startup page's address (INIT.md, answer 40), which
/// the kernel passes on unchanged and never looks at.
#[allow(clippy::too_many_arguments)]
pub fn process_start(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: PID,
    process_h: u32,
    entry: usize,
    sp: usize,
    arg: usize,
    handles_rec: usize,
    count: u32,
) -> Result<(), Error> {
    // Stage 1's rest: the handle list is a record of the caller's, `count` slots long. The count
    // was checked against `MAX_START_HANDLES` while decoding; it is checked again here, so that
    // neither check rests on the other.
    let count = count as usize;
    if count > MAX_START_HANDLES {
        return Err(Error::TooLarge);
    }
    let slots = crate::redoubt::read_slots::<MAX_START_HANDLES>(mm, handles_rec, count)?;
    let mut decoded = [None; MAX_START_HANDLES];
    for (i, slot) in slots.iter().enumerate().take(count) {
        decoded[i] = Some(AbiHandle::from_raw(*slot)?);
    }
    let r = mm.process_handle(pid, process_h)?;
    let p = mm.process_at(r);
    let mut handles = [None; MAX_START_HANDLES];
    for (i, handle) in decoded.iter().enumerate().take(count) {
        handles[i] = Some(mm.handle(pid, handle.expect("decoded above").index())?);
    }
    if p.started() || !p.alive() {
        return Err(Error::NotPermitted);
    }
    let child = p.pid;
    let budget = mm.budget_of(child).ok_or(Error::NotPermitted)?;
    // Stage 4: the child's budget pays for its first thread, then for the table pages its
    // handles need (the spec's row lists them in that order).
    let table = mm.table_growth(child, count).ok_or(Error::TooLarge)?;
    if THREAD_PAGES + table > mm.free_pages(budget) {
        return Err(Error::OutOfMemory);
    }
    // From here nothing fails. The handles go in first, so the child never runs with a table
    // that is not the one `process_start` promised.
    for handle in handles.iter().flatten() {
        mm.install_handle(child, *handle).expect("process_start: the table pages were counted above");
    }
    mm.thread_created(child, INITIAL_TID).expect("process_start: the thread page was counted above");
    let here = crate::arch::process::current_pid();
    ss.get_process(child).and_then(|p| p.activate()).expect("a created process can be activated");
    ArchProcess::setup_first_thread(child, entry, sp, arg);
    ss.activate(here).expect("the running process can be activated");
    ss.start_process(child).expect("a created process can be started");
    let mut p = mm.process(r.frame);
    p.flags |= F_STARTED;
    mm.store_process(r.frame, &p);
    Ok(())
}

// --- Threads ---------------------------------------------------------------------------------------

/// `thread_create(entry, sp, arg) -> tid` (KERNEL-SPEC.md): `TooManyThreads` past `MAX_THREADS`,
/// `OutOfMemory` when the process's budget cannot pay for the thread's page.
pub fn thread_create(
    ss: &mut SystemServices,
    pid: PID,
    entry: usize,
    sp: usize,
    arg: usize,
) -> Result<u32, Error> {
    ss.create_redoubt_thread(pid, entry, sp, arg).map(|tid| tid as u32)
}

/// `thread_exit()` (KERNEL-SPEC.md): the calling thread ends.
///
/// The final thread performs `process_exit(0)` (answer 170). Classify open calls and snapshot
/// current-call blame before any thread cleanup destroys the evidence.
pub fn thread_exit(ss: &mut SystemServices, pid: PID, tid: TID) {
    let last = MemoryManager::with(|mm| mm.account(pid).is_none_or(|a| a.threads <= 1));
    if last {
        process_exit(ss, pid, tid, 0);
        return;
    }
    ss.destroy_thread(pid, tid).expect("the running thread can be destroyed");
    crate::syscall::reset_switchto_caller();
}

// --- Exit, fault and kill ---------------------------------------------------------------------------

/// `process_exit(code)` (KERNEL-SPEC.md): `exited`, or `faulted` while the process holds open
/// calls -- which is where a Rust panic lands (answer 55). A server that means to exit replies to
/// every open call first (R4b).
pub fn process_exit(ss: &mut SystemServices, pid: PID, tid: TID, code: u32) {
    let open = MemoryManager::with(|mm| mm.account(pid).map_or(0, |a| a.open_calls));
    let cause = if open == 0 { Cause::Exited } else { Cause::Faulted };
    died(ss, pid, tid, cause, code);
}

/// A process faulted: the trap handler could not make sense of the trap and the process cannot go
/// on (`arch::irq`). `code` is the RISC-V exception cause.
pub fn faulted(pid: PID, code: u32) {
    let tid = ArchProcess::with_current(|p| p.current_tid());
    SystemServices::with_mut(|ss| died(ss, pid, tid, Cause::Faulted, code));
}

/// R10: destroying a budget kills the processes running in it. The notice has cause `killed`
/// (`died` drops it if the object is going too, which [`budgets_dying`] sees to afterwards).
pub fn killed(ss: &mut SystemServices, victim: PID) { died(ss, victim, INITIAL_TID, Cause::Killed, 0); }

/// The one path out of a process, whatever ended it: record the notice, tear the process down,
/// then deliver the notice or drop it.
///
/// The order matters. Blame is read first, because the teardown frees the open call it names.
/// The budget the process ran in is read first too, because R1 compares *its* labels with the
/// exit endpoint's owner, and the teardown may be the last thing keeping it alive.
fn died(ss: &mut SystemServices, pid: PID, tid: TID, cause: Cause, code: u32) {
    let recorded = MemoryManager::with_mut(|mm| record(mm, pid, tid, cause, code));
    let Some((object, flow)) = recorded else { return };
    end_process(ss, pid);
    if let Some(frame) = object {
        MemoryManager::with_mut(|mm| settle_notice(ss, mm, frame, flow));
    }
}

/// Write the notice into the process object and read what delivering it will need. `None` if the
/// process is already gone: a fault while a `process_exit` is being carried out, or a kill of
/// something already dead, changes nothing.
fn record(
    mm: &mut MemoryManager,
    pid: PID,
    tid: TID,
    cause: Cause,
    code: u32,
) -> Option<(Option<u32>, Option<Flow>)> {
    let object = object_of(mm, pid);
    if let Some(frame) = object {
        let mut p = mm.process(frame);
        if !p.alive() {
            return None;
        }
        p.flags = (p.flags & !F_ALIVE) | F_NOTICE;
        p.cause = cause as u64;
        p.code = code;
        // Blame only for `faulted` (KERNEL-SPEC.md, Messages): `exited` and `killed` blame
        // nobody, and neither does a faulting thread with no current call.
        if cause == Cause::Faulted {
            if let Some((account, labels)) = crate::message::current_call_blame(mm, pid, tid) {
                p.blamed_account = account;
                let labels = labels.as_slice();
                p.blamed_nlabels = labels.len();
                p.blamed_labels[..labels.len()].copy_from_slice(labels);
            }
        }
        mm.store_process(frame, &p);
    }
    let flow = mm.budget_of(pid).map(|f| {
        let b = mm.budget(f);
        Flow { labels: b.labels, nlabels: b.nlabels }
    });
    Some((object, flow))
}

/// Tear the process down: R4b for its threads' open calls, then its memory, handle table and
/// process-table slot. The running process is dealt with as `budget_destroy` deals with a caller
/// that destroyed its own budget.
fn end_process(ss: &mut SystemServices, pid: PID) {
    if ss.get_process(pid).is_err() {
        return;
    }
    if ss.current_pid() == pid {
        let tid = ArchProcess::with_current(|p| p.current_tid());
        ss.unschedule_thread(pid, tid).ok();
        ss.terminate_process(pid).expect("the running process exists");
        crate::syscall::reset_switchto_caller();
    } else {
        ss.kill_process(pid).expect("a process that is ending exists");
    }
}

/// Deliver the notice if R1 allows and there is somewhere to deliver it, or drop it. A dropped
/// notice frees the object at once, so nothing waits for a notice that can never arrive.
fn settle_notice(ss: &mut SystemServices, mm: &mut MemoryManager, frame: u32, flow: Option<Flow>) {
    let p = mm.process(frame);
    // R10 drops objects charged to a dying creator without emitting their notices, even
    // when an external receiver is already blocked on a surviving exit endpoint.
    if mm.budget_at(p.creator).dying {
        free_object(mm, frame);
        return;
    }
    match p.endpoint.filter(|e| mm.is_live_endpoint(*e) && allowed(mm, *e, flow)) {
        Some(e) => crate::message::pump_endpoint(ss, mm, e),
        None => free_object(mm, frame),
    }
}

/// R1 for an exit notice: a flow from the budget the process ran in to the exit endpoint's owner.
fn allowed(mm: &MemoryManager, e: EndpointRef, flow: Option<Flow>) -> bool {
    let owner = mm.budget_at(mm.endpoint_at(e).owner);
    if owner.dying {
        return false;
    }
    // Notices carry a one-way flow. Only a system-class destination bypasses its label check.
    if owner.class == Class::System {
        return true;
    }
    flow.is_some_and(|f| f.labels[..f.nlabels].iter().all(|label| owner.labels_of().contains(label)))
}

/// Free a process object: its handles go (I1: nothing may name a freed frame), its page goes back
/// to the creator's budget, and its PID becomes free again. The process itself is already gone.
pub fn free_object(mm: &mut MemoryManager, frame: u32) {
    let p = mm.process(frame);
    assert!(!p.alive(), "a live process's object was freed");
    let r = ProcessRef { frame, id: p.id };
    mm.sweep_handles(|_, h| matches!(h.object, Object::Process(x) if x == r));
    if mm.is_live_budget(p.creator) {
        mm.uncharge(p.creator.frame, PROCESS_PAGES);
    }
    mm.free_object_frame(frame);
}

/// The notice a process object owes on `e`, with its frame (`message.rs` calls this while it is
/// matching receivers with what is pending on an endpoint).
pub fn pending_notice(mm: &MemoryManager, e: EndpointRef) -> Option<(u32, ExitNotice)> {
    let frame = mm.find_process(|mm, frame| {
        let p = mm.process(frame);
        p.notice_owed() && p.endpoint == Some(e)
    })?;
    let p = mm.process(frame);
    let mut labels = Labels::new();
    for label in &p.blamed_labels[..p.blamed_nlabels] {
        labels.push(*label).expect("MAX_LABELS");
    }
    let cause = match p.cause {
        x if x == Cause::Exited as u64 => Cause::Exited,
        x if x == Cause::Faulted as u64 => Cause::Faulted,
        // Only the kernel writes these frames.
        _ => Cause::Killed,
    };
    let notice = ExitNotice {
        pid: u32::from(p.pid.get()),
        cause,
        code: p.code,
        blamed_account: p.blamed_account,
        blamed_labels: labels,
    };
    Some((frame, notice))
}

// --- Teardown that reaches process objects (R10) -------------------------------------------------

/// R10: every process object charged to a dying budget is freed, its process killed first if it
/// still runs, and then there is no notice. Runs after `budget_destroy` has killed the processes
/// *in* the dying budgets, so what is usually left here is objects whose process already died in
/// another budget.
pub fn budgets_dying(ss: &mut SystemServices) {
    loop {
        let next = MemoryManager::with(|mm| {
            mm.find_process(|mm, frame| mm.budget_at(mm.process(frame).creator).dying)
                .map(|frame| (frame, mm.process(frame).alive(), mm.process(frame).pid))
        });
        let Some((frame, alive, pid)) = next else { return };
        if alive {
            // Not through `died`: the object is going, so nothing is recorded and nobody is told.
            MemoryManager::with_mut(|mm| {
                let mut p = mm.process(frame);
                p.flags &= !F_ALIVE;
                mm.store_process(frame, &p);
            });
            end_process(ss, pid);
        }
        MemoryManager::with_mut(|mm| free_object(mm, frame));
    }
}

/// An exit endpoint is being destroyed (R10): every notice owed to it is dropped and its object
/// freed. A process still running keeps going -- it has lost the ear it was to report to, and its
/// object goes when its creator's budget does.
pub fn endpoint_dying(mm: &mut MemoryManager, e: EndpointRef) {
    while let Some(frame) = mm.find_process(|mm, frame| mm.process(frame).endpoint == Some(e)) {
        let mut p = mm.process(frame);
        if p.alive() {
            p.endpoint = None;
            mm.store_process(frame, &p);
        } else {
            free_object(mm, frame);
        }
    }
}
