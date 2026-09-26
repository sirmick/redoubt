// SPDX-License-Identifier: MIT OR Apache-2.0

//! Messages: `call`, `send`, `receive`, `reply`, `serve` and `mint` (KERNEL-SPEC.md, Messages,
//! R1-R4b, R10).
//!
//! # Where a message lives
//! There is no message queue anywhere. A message is queued exactly while its sender is blocked in
//! `send` or `call`, and a thread blocks at most once, so **the queue is the set of blocked
//! senders**, which the kernel finds by walking the threads. Each thread keeps what it is waiting
//! for, the message it is sending and the calls it holds open in a page of its own
//! ([`crate::budget::Account::ipc`]) — the page the cost table already charges for a thread. Two
//! things follow, and they are the reason for this shape:
//! - `call` and `send` allocate no kernel object, so neither can fail for want of one, which their error rows
//!   require (neither returns `OutOfMemory`; a `call` returns it only for a reply's handles, which is the
//!   caller's own table). A lend of pages the sender never touched is still backed as it is checked, charged
//!   to the sender, and that is `unmap`'s `OutOfMemory` rather than this path's (question 140);
//! - nothing a sender does makes the kernel allocate on a receiver's behalf.
//!
//! A **taken** call moves out of the sender's page into an open-call page of its own, charged to
//! the receiving process's budget (R4a) and freed by `reply`. That page holds everything a reply
//! needs: whom to wake, the lend to give back, and who pays for it (R3).
//!
//! Walking every thread to pick the next sender costs more than a queue would. The design asks
//! for the walk anyway: R2 serves groups round-robin, so a receive must consider every waiting
//! group. The walk is bounded by `MAX_PROCESS_COUNT * MAX_THREADS`, a compile-time constant no
//! process can influence (TENETS.md: clarity beats speed).
//!
//! # Locks
//! Every entry point takes the scheduler (`ss`) and the memory manager (`mm`) together, borrowed
//! once by the dispatcher (`redoubt.rs`) in that order; nothing here borrows either again.
//!
//! # Timeouts
//! A blocking call records its deadline (`mark`, which also makes sure the kernel's timer comes by
//! then), and [`expire_due`] answers every thread whose deadline has passed, earliest first. The
//! timer and when expiry runs are `time.rs`'s.

use core::cmp::Ordering;
use core::num::{NonZeroU64, NonZeroUsize};

use redoubt_sys::PAGE_SIZE;
use redoubt_layout::{KERNEL_PID, Pid};

use crate::arch::process::TID;
use redoubt_sys::{
    Body, CallOutcome, Error, Handle as AbiHandle, Labels, LendDisposition, MAX_LABELS, MAX_LEND_PAGES,
    MAX_MSG_HANDLES, MAX_OPEN_CALLS, Message, MessageKind, MintSource, Pages, RECEIVED_SLOTS, Received,
    MAX_THREADS, ReceivedBody, ReceivedHandles, ReplyOutcome, Return, WAIT_CAP, WORDS, encode_result,
};

use crate::arch::process::MAX_PROCESS_COUNT;
use crate::budget::Class;
use crate::endpoint::Group;
use crate::handle::{BudgetRef, DeviceRef, EndpointRef, Handle, Object};
use crate::kframe;
use crate::mem::MemoryManager;
use crate::services::SystemServices;

/// The cost table (KERNEL-SPEC.md, What objects cost), in pages.
pub const OPEN_CALL_PAGES: u64 = 1;

/// First word of a thread's IPC page, and of an open call's page.
const THREAD_MAGIC: u64 = u64::from_le_bytes(*b"thrdipc\0");
const CALL_MAGIC: u64 = u64::from_le_bytes(*b"opencall");

// --- A thread's IPC page ------------------------------------------------------------------------
//
// Word layout. Every field is a plain 64-bit word, so no frame is ever read as a Rust type with
// invalid bit patterns (`kframe.rs`).
const W_WAIT: usize = 1;
const W_DEADLINE: usize = 2;
const W_SEQ: usize = 3;
/// What the thread is waiting on, as frame + 1 and id: the endpoint while sending or
/// receiving, the open call while waiting for a reply, the device object while in `receive` on
/// an IRQ handle (R5). `Wait` says which, so one pair of words serves all three.
const W_OBJECT: usize = 4;
const W_OBJECT_ID: usize = 5;
const W_MAX_TRANSFER: usize = 6;
/// `receive`'s record, or the record `call` writes its reply back into.
const W_REC: usize = 7;
const W_KIND: usize = 8;
const W_BADGE: usize = 9;
const W_STAMP: usize = 10; // frame + 1
const W_STAMP_ID: usize = 11;
const W_SENDER_BUDGET: usize = 12; // frame + 1
const W_SENDER_BUDGET_ID: usize = 13;
const W_BUF_ADDR: usize = 14;
const W_BUF_PAGES: usize = 15;
const W_WORDS: usize = 16; // WORDS words
const W_NHANDLES: usize = W_WORDS + WORDS;
const W_HANDLES: usize = W_NHANDLES + 1; // MAX_MSG_HANDLES * 4 words
const W_NCALLS: usize = W_HANDLES + MAX_MSG_HANDLES * 4;
const W_CURRENT: usize = W_NCALLS + 1; // open-call frame + 1
const W_CALLS: usize = W_CURRENT + 1; // MAX_OPEN_CALLS frame numbers
const THREAD_WORDS: usize = W_CALLS + MAX_OPEN_CALLS;
const _: () = assert!(THREAD_WORDS * 8 <= PAGE_SIZE);

/// What a blocked thread is waiting for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wait {
    /// Not blocked in a Redoubt call.
    None = 0,
    /// In `send` or `call`, until a receiver takes the message in this page.
    Send = 1,
    /// In `call`, after a server took the message, until the reply.
    Reply = 2,
    /// In `receive` on the endpoint this page names.
    Receive = 3,
    /// In `receive` with no handle: asleep until the timeout (KERNEL-SPEC.md, `receive`).
    Sleep = 4,
    /// In `receive` on the IRQ handle this page names, until it fires (R5).
    Irq = 5,
}

impl Wait {
    fn from_word(w: u64) -> Wait {
        match w {
            0 => Wait::None,
            1 => Wait::Send,
            2 => Wait::Reply,
            3 => Wait::Receive,
            4 => Wait::Sleep,
            5 => Wait::Irq,
            // Only the kernel writes these pages.
            _ => panic!("I1: corrupt thread IPC page"),
        }
    }
}

/// How a message was sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgKind {
    Call = 1,
    Send = 2,
}

/// The message a thread is sending while it waits (KERNEL-SPEC.md, Messages). The account and
/// the labels are not copied: they are read from `sender_budget`, whose account and labels never
/// change and which outlives the message, since destroying it kills the sender (R10).
#[derive(Clone, Copy)]
struct Msg {
    kind: MsgKind,
    badge: u64,
    stamp: BudgetRef,
    sender_budget: BudgetRef,
    /// The lend or transfer as it lies in the sender: first page and page count, 0 for none.
    buf_addr: usize,
    buf_pages: usize,
    words: [u64; WORDS],
    /// Copies made when the message was sent. One R10 revoked meanwhile arrives as 0, keeping
    /// its slot (WIRE.md names handles by slot).
    handles: [Option<Handle>; MAX_MSG_HANDLES],
    nhandles: usize,
}

/// The small, fixed part of a thread's IPC page; the open-call list is reached separately, so
/// that reading one field never copies 64 entries.
#[derive(Clone, Copy)]
struct Slot {
    wait: Wait,
    /// Absolute µs since boot; `u64::MAX` never expires.
    deadline: u64,
    seq: u64,
    /// The endpoint it is sending on or receiving from.
    endpoint: Option<EndpointRef>,
    /// While waiting for a reply, the open call's frame.
    open: u32,
    /// While waiting in `receive` on an IRQ handle, the device object it named.
    irq: Option<DeviceRef>,
    max_transfer: usize,
    rec: usize,
    ncalls: usize,
    /// The thread's current call: the one a fault blames (answer 82). Frame + 1; 0 for none.
    current: u32,
}

fn thread_phys(mm: &MemoryManager, pid: Pid, tid: TID) -> Option<usize> {
    let frame = mm.ipc_frame(pid, tid)?;
    let phys = mm.object_phys(frame);
    let magic = kframe::read(phys, 0);
    // A fresh page is all zeroes and is stamped the first time it is written.
    assert!(magic == 0 || magic == THREAD_MAGIC, "I1: frame {} is not a thread's IPC page", frame);
    Some(phys)
}

fn tword(mm: &MemoryManager, pid: Pid, tid: TID, i: usize) -> u64 {
    thread_phys(mm, pid, tid).map_or(0, |phys| kframe::read(phys, i * 8))
}

fn set_tword(mm: &MemoryManager, pid: Pid, tid: TID, i: usize, value: u64) {
    if let Some(phys) = thread_phys(mm, pid, tid) {
        kframe::write(phys, 0, THREAD_MAGIC);
        kframe::write(phys, i * 8, value);
    }
}

fn frame_of(word: u64) -> Option<u32> { (word as u32).checked_sub(1) }

fn frame_word(frame: u32) -> u64 { u64::from(frame) + 1 }

/// The fixed part of `(pid, tid)`'s IPC page. A thread with no page waits for nothing and holds
/// nothing, which is what all-zero words say.
fn slot(mm: &MemoryManager, pid: Pid, tid: TID) -> Slot {
    let w = |i| tword(mm, pid, tid, i);
    let wait = Wait::from_word(w(W_WAIT));
    let reply = wait == Wait::Reply;
    Slot {
        wait,
        deadline: w(W_DEADLINE),
        seq: w(W_SEQ),
        endpoint: match (wait, frame_of(w(W_OBJECT))) {
            (Wait::Reply | Wait::Irq, _) => None,
            (_, frame) => frame.map(|frame| EndpointRef { frame, id: w(W_OBJECT_ID) }),
        },
        open: if reply { frame_of(w(W_OBJECT)).unwrap_or(0) } else { 0 },
        irq: match (wait == Wait::Irq, frame_of(w(W_OBJECT))) {
            (true, Some(frame)) => Some(DeviceRef { frame, id: w(W_OBJECT_ID) }),
            _ => None,
        },
        max_transfer: w(W_MAX_TRANSFER) as usize,
        rec: w(W_REC) as usize,
        ncalls: (w(W_NCALLS) as usize).min(MAX_OPEN_CALLS),
        current: w(W_CURRENT) as u32,
    }
}

/// The message `(pid, tid)` is sending.
fn msg(mm: &MemoryManager, pid: Pid, tid: TID) -> Msg {
    let w = |i| tword(mm, pid, tid, i);
    let mut words = [0; WORDS];
    for (i, word) in words.iter_mut().enumerate() {
        *word = w(W_WORDS + i);
    }
    let nhandles = (w(W_NHANDLES) as usize).min(MAX_MSG_HANDLES);
    let mut handles = [None; MAX_MSG_HANDLES];
    for (i, item) in handles.iter_mut().enumerate().take(nhandles) {
        *item = Handle::from_words([
            w(W_HANDLES + i * 4),
            w(W_HANDLES + i * 4 + 1),
            w(W_HANDLES + i * 4 + 2),
            w(W_HANDLES + i * 4 + 3),
        ]);
    }
    Msg {
        kind: if w(W_KIND) == MsgKind::Call as u64 { MsgKind::Call } else { MsgKind::Send },
        badge: w(W_BADGE),
        stamp: BudgetRef { frame: frame_of(w(W_STAMP)).unwrap_or(0), id: w(W_STAMP_ID) },
        sender_budget: BudgetRef {
            frame: frame_of(w(W_SENDER_BUDGET)).unwrap_or(0),
            id: w(W_SENDER_BUDGET_ID),
        },
        buf_addr: w(W_BUF_ADDR) as usize,
        buf_pages: w(W_BUF_PAGES) as usize,
        words,
        handles,
        nhandles,
    }
}

fn store_msg(mm: &MemoryManager, pid: Pid, tid: TID, m: &Msg) {
    let set = |i, v| set_tword(mm, pid, tid, i, v);
    set(W_KIND, m.kind as u64);
    set(W_BADGE, m.badge);
    set(W_STAMP, frame_word(m.stamp.frame));
    set(W_STAMP_ID, m.stamp.id);
    set(W_SENDER_BUDGET, frame_word(m.sender_budget.frame));
    set(W_SENDER_BUDGET_ID, m.sender_budget.id);
    set(W_BUF_ADDR, m.buf_addr as u64);
    set(W_BUF_PAGES, m.buf_pages as u64);
    for (i, word) in m.words.iter().enumerate() {
        set(W_WORDS + i, *word);
    }
    set(W_NHANDLES, m.nhandles as u64);
    for (i, item) in m.handles.iter().enumerate() {
        let words = item.map_or([0; 4], |h| h.to_words());
        for (k, word) in words.iter().enumerate() {
            set(W_HANDLES + i * 4 + k, *word);
        }
    }
}

// --- An open call's page ------------------------------------------------------------------------
const C_RID: usize = 1;
const C_CALLER_PID: usize = 2;
const C_CALLER_TID: usize = 3;
const C_SERVER_PID: usize = 4;
const C_SERVER_TID: usize = 5;
const C_ENDPOINT: usize = 6; // frame + 1
const C_ENDPOINT_ID: usize = 7;
const C_BADGE: usize = 8;
const C_STAMP: usize = 9; // frame + 1
const C_STAMP_ID: usize = 10;
const C_FLAGS: usize = 11;
const C_LEND_CALLER: usize = 12; // first page in the caller, 0 for none
const C_LEND_SERVER: usize = 13; // first page in the server
const C_LEND_PAGES: usize = 14;
const C_PAYER: usize = 15; // the receiving budget: frame + 1
const C_PAYER_ID: usize = 16;
/// The sender's account and labels as they were when the call was delivered: what a fault blames
/// (answers 37, 55, 82). Snapshots, not a reference to the sender's budget, so that blame
/// survives that budget being destroyed while the call is open.
const C_ACCOUNT: usize = 17;
const C_NLABELS: usize = 18;
const C_LABELS: usize = 19; // MAX_LABELS words
const CALL_WORDS: usize = C_LABELS + MAX_LABELS;
const _: () = assert!(CALL_WORDS * 8 <= PAGE_SIZE);

/// The caller is still waiting for the reply.
const F_WAITING: u64 = 1;
/// R3: the call was abandoned; its lend is the server's alone until it replies.
const F_ABANDONED: u64 = 2;
/// Its abandoned-call notice is still owed to the holding thread (I15).
const F_NOTICE: u64 = 4;

/// A `call` a thread took with `receive` and has not replied to (R4a).
#[derive(Clone, Copy)]
struct OpenCall {
    rid: u64,
    caller: (Pid, TID),
    server: (Pid, TID),
    endpoint: EndpointRef,
    badge: u64,
    stamp: BudgetRef,
    flags: u64,
    lend_caller: usize,
    lend_server: usize,
    lend_pages: usize,
    /// The receiving budget: it pays for the open-call page and, while the caller waits, for the
    /// lend as well (R3).
    payer: BudgetRef,
    /// The sender's account and labels, for blame.
    account: u64,
    labels: [u64; MAX_LABELS],
    nlabels: usize,
}

fn pid_of(word: u64) -> Pid { Pid::new(word as u8).expect("I1: an open call names no process") }

fn open_call_at(mm: &MemoryManager, frame: u32) -> OpenCall {
    let phys = mm.object_phys(frame);
    let w = |i: usize| kframe::read(phys, i * 8);
    assert!(w(0) == CALL_MAGIC, "I1: frame {} holds no open call", frame);
    OpenCall {
        rid: w(C_RID),
        caller: (pid_of(w(C_CALLER_PID)), w(C_CALLER_TID) as TID),
        server: (pid_of(w(C_SERVER_PID)), w(C_SERVER_TID) as TID),
        endpoint: EndpointRef { frame: frame_of(w(C_ENDPOINT)).unwrap_or(0), id: w(C_ENDPOINT_ID) },
        badge: w(C_BADGE),
        stamp: BudgetRef { frame: frame_of(w(C_STAMP)).unwrap_or(0), id: w(C_STAMP_ID) },
        flags: w(C_FLAGS),
        lend_caller: w(C_LEND_CALLER) as usize,
        lend_server: w(C_LEND_SERVER) as usize,
        lend_pages: w(C_LEND_PAGES) as usize,
        payer: BudgetRef { frame: frame_of(w(C_PAYER)).unwrap_or(0), id: w(C_PAYER_ID) },
        account: w(C_ACCOUNT),
        labels: core::array::from_fn(|i| w(C_LABELS + i)),
        nlabels: (w(C_NLABELS) as usize).min(MAX_LABELS),
    }
}

fn store_open_call(mm: &MemoryManager, frame: u32, c: &OpenCall) {
    let phys = mm.object_phys(frame);
    let mut words = [0u64; CALL_WORDS];
    words[0] = CALL_MAGIC;
    words[C_RID] = c.rid;
    words[C_CALLER_PID] = u64::from(c.caller.0.get());
    words[C_CALLER_TID] = c.caller.1 as u64;
    words[C_SERVER_PID] = u64::from(c.server.0.get());
    words[C_SERVER_TID] = c.server.1 as u64;
    words[C_ENDPOINT] = frame_word(c.endpoint.frame);
    words[C_ENDPOINT_ID] = c.endpoint.id;
    words[C_BADGE] = c.badge;
    words[C_STAMP] = frame_word(c.stamp.frame);
    words[C_STAMP_ID] = c.stamp.id;
    words[C_FLAGS] = c.flags;
    words[C_LEND_CALLER] = c.lend_caller as u64;
    words[C_LEND_SERVER] = c.lend_server as u64;
    words[C_LEND_PAGES] = c.lend_pages as u64;
    words[C_PAYER] = frame_word(c.payer.frame);
    words[C_PAYER_ID] = c.payer.id;
    words[C_ACCOUNT] = c.account;
    words[C_NLABELS] = c.nlabels as u64;
    words[C_LABELS..].copy_from_slice(&c.labels);
    for (i, word) in words.iter().enumerate() {
        kframe::write(phys, i * 8, *word);
    }
}

// --- The open-call list ---------------------------------------------------------------------------

fn nth_call(mm: &MemoryManager, pid: Pid, tid: TID, index: usize) -> u32 {
    tword(mm, pid, tid, W_CALLS + index) as u32
}

fn push_open_call(mm: &mut MemoryManager, pid: Pid, tid: TID, frame: u32) {
    let n = slot(mm, pid, tid).ncalls;
    assert!(n < MAX_OPEN_CALLS, "R4a: a thread took a call past MAX_OPEN_CALLS");
    set_tword(mm, pid, tid, W_CALLS + n, u64::from(frame));
    set_tword(mm, pid, tid, W_NCALLS, (n + 1) as u64);
    mm.account_mut(pid).expect("account").open_calls += 1;
}

fn drop_open_call(mm: &mut MemoryManager, pid: Pid, tid: TID, frame: u32) {
    let n = slot(mm, pid, tid).ncalls;
    let Some(at) = (0..n).find(|i| nth_call(mm, pid, tid, *i) == frame) else { return };
    for i in at..n - 1 {
        let next = tword(mm, pid, tid, W_CALLS + i + 1);
        set_tword(mm, pid, tid, W_CALLS + i, next);
    }
    set_tword(mm, pid, tid, W_NCALLS, (n - 1) as u64);
    if slot(mm, pid, tid).current == frame_word(frame) as u32 {
        set_tword(mm, pid, tid, W_CURRENT, 0);
    }
    let account = mm.account_mut(pid).expect("account");
    account.open_calls = account.open_calls.saturating_sub(1);
}

/// The open call of thread `(pid, tid)` that its process knows as `rid`.
fn open_call_of(mm: &MemoryManager, pid: Pid, tid: TID, rid: u64) -> Option<u32> {
    let n = slot(mm, pid, tid).ncalls;
    (0..n).map(|i| nth_call(mm, pid, tid, i)).find(|f| open_call_at(mm, *f).rid == rid)
}

// --- Walking the threads ---------------------------------------------------------------------------

/// Call `f` for every thread that has an IPC page, until it answers `Some`.
fn find_thread<T>(mm: &MemoryManager, mut f: impl FnMut(&MemoryManager, Pid, TID) -> Option<T>) -> Option<T> {
    for index in 1..=MAX_PROCESS_COUNT {
        let Some(pid) = Pid::new(index as u8) else { continue };
        for tid in 1..=MAX_THREADS {
            if mm.ipc_frame(pid, tid).is_none() {
                continue;
            }
            if let Some(found) = f(mm, pid, tid) {
                return Some(found);
            }
        }
    }
    None
}

/// Whether `(pid, tid)` is a sender queued on `e`.
fn queued_on(mm: &MemoryManager, pid: Pid, tid: TID, e: EndpointRef) -> bool {
    let s = slot(mm, pid, tid);
    s.wait == Wait::Send && s.endpoint == Some(e)
}

/// The R2 group of a queued sender.
fn group_of(mm: &MemoryManager, pid: Pid, tid: TID) -> Group {
    Group::of(&mm.budget_at(msg(mm, pid, tid).sender_budget))
}

// --- Waking and blocking -----------------------------------------------------------------------------

/// Whether `(pid, tid)` is the thread making the system call. It was never taken off the ready
/// list, so an answer for it is just its registers: `settle` resumes it.
fn is_running(ss: &SystemServices, pid: Pid, tid: TID) -> bool {
    ss.current_pid() == pid && crate::arch::process::Process::current().current_tid() == tid
}

/// Hand a thread its result. One that was blocked goes back on the ready list; the thread making
/// the call was never off it.
fn wake(ss: &mut SystemServices, mm: &MemoryManager, pid: Pid, tid: TID, result: Result<Return, Error>) {
    set_tword(mm, pid, tid, W_WAIT, Wait::None as u64);
    if !is_running(ss, pid, tid) {
        // A waiting thread belongs to a live process, so this cannot fail.
        ss.ready_thread(pid, tid).expect("a waiting thread belongs to a live process");
    }
    ss.set_redoubt_result(pid, tid, &encode_result(&result)).expect("a waiting thread exists");
}

/// Write `slots` into `(pid, tid)`'s record, in its own address space, and wake it with
/// `result`. A record that is no longer the thread's writable memory is `InvalidArgument`: the
/// record is part of decoding, whose error every call's row carries.
/// Write a blocked thread's record in its own memory and wake it with `result`, or with the
/// error the record earned. By the time this runs the message is consumed -- taken from the
/// queue, its handles installed, its buffer mapped -- so a record that has gone unwritable
/// since is the receiver's own loss, not the sender's; delivery re-checks it beforehand
/// (`check_receive_record`) so that this is a narrow race, not the usual way.
fn answer_record<const N: usize>(
    ss: &mut SystemServices,
    mm: &MemoryManager,
    pid: Pid,
    tid: TID,
    slots: &[u64; N],
    result: Result<Return, Error>,
) {
    let rec = slot(mm, pid, tid).rec;
    let here = crate::arch::process::current_pid();
    let written =
        ss.activate(pid).map_err(|_| Error::Dead).and_then(|()| crate::redoubt::write_record(mm, rec, slots));
    ss.activate(here).expect("the running process can be activated");
    wake(ss, mm, pid, tid, written.and(result));
}

/// Say what a thread is waiting for, and until when. It does not block yet: the delivery attempt
/// that follows may answer it at once, and `settle` then never takes it off the ready list.
fn mark(mm: &mut MemoryManager, pid: Pid, tid: TID, wait: Wait, timeout: u64) {
    // Timeouts are relative microseconds, added with saturation, so `FOREVER` never expires.
    let deadline = crate::time::now_us().saturating_add(timeout);
    set_tword(mm, pid, tid, W_WAIT, wait as u64);
    set_tword(mm, pid, tid, W_DEADLINE, deadline);
    if deadline != u64::MAX {
        if let Some(a) = mm.account_mut(pid) {
            a.earliest_timeout = a.earliest_timeout.min(deadline);
        }
        crate::time::note_timeout(deadline);
    }
}

/// What a blocking call does once delivery has had its chance: resume with the answer it already
/// has, time out without ever blocking, or block. `Ok(None)` tells the trap handler to resume
/// whatever is current now, which is this thread when it was answered (`redoubt.rs`).
fn settle(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
) -> Result<Option<Return>, Error> {
    let s = slot(mm, pid, tid);
    if s.wait == Wait::None {
        // Answered already: its registers hold the result.
        return Ok(None);
    }
    if s.deadline <= crate::time::now_us() {
        // A deadline that has already passed (a `timeout` of 0 is a poll): never block on it.
        fail_wait(ss, mm, pid, tid, Error::Timeout);
        // fail_wait published the full outcome, including a lend consumed after receipt.
        return Ok(None);
    }
    // `can_resume: false` is what takes this thread off the ready list (services.rs).
    ss.activate_process_thread(tid, KERNEL_PID, 0, false).expect("the kernel can always run");
    Ok(None)
}

/// A blocked thread's wait ends without an answer: what it waited for is unwound and it gets
/// `error`. This is the one meaning of a timeout (I13), of `Refused` (R4), of `Dead` from
/// revocation or an endpoint's destruction (R10), whatever the thread was waiting for.
fn fail_wait(ss: &mut SystemServices, mm: &mut MemoryManager, pid: Pid, tid: TID, error: Error) {
    let waiting = slot(mm, pid, tid);
    let result = match waiting.wait {
        Wait::Send if msg(mm, pid, tid).kind == MsgKind::Call => {
            call_result(Err(error), msg(mm, pid, tid).buf_pages, false, false)
        }
        Wait::Reply => call_result(Err(error), open_call_at(mm, waiting.open).lend_pages, true, false),
        _ => Err(error),
    };
    let served = unwind(ss, mm, pid, tid);
    wake(ss, mm, pid, tid, result);
    if let Some(e) = served {
        pump(ss, mm, e);
    }
}

fn call_result(
    status: Result<(), Error>,
    pages: usize,
    consumed: bool,
    present: bool,
) -> Result<Return, Error> {
    let lend = if pages == 0 {
        LendDisposition::None
    } else if consumed {
        LendDisposition::Consumed
    } else {
        LendDisposition::Returned
    };
    Ok(Return::Call(CallOutcome { status, lend, reply_present: present }))
}

/// Undo what `(pid, tid)`'s page says it waits for: a queued message's buffer goes back to it;
/// a taken call is abandoned (R3). Returns the endpoint of an abandoned call, which is owed a
/// pump once the thread is dealt with: its server may be waiting there for the notice.
fn unwind(ss: &SystemServices, mm: &mut MemoryManager, pid: Pid, tid: TID) -> Option<EndpointRef> {
    let s = slot(mm, pid, tid);
    match s.wait {
        Wait::Send => {
            give_buffer_back(ss, mm, pid, tid);
            None
        }
        Wait::Reply => {
            abandon(ss, mm, s.open);
            Some(open_call_at(mm, s.open).endpoint)
        }
        _ => None,
    }
}

// --- `mint` and `serve` --------------------------------------------------------------------------

/// `mint(source, badge, budget?) -> h` (KERNEL-SPEC.md, `mint`; R9, I3, I4).
pub fn mint(
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    source: MintSource,
    badge: u64,
    budget: Option<u32>,
) -> Result<u32, Error> {
    // Argument stage: the endpoint and the default stamp, then the budget handle.
    let (endpoint, default, source_badge) = match source {
        MintSource::Message(id) => {
            // A message id that is not an open call of the caller's own thread names
            // nothing, a `send`'s id included (a send is never open, R4a).
            let frame = open_call_of(mm, pid, tid, id.get()).ok_or(Error::InvalidArgument)?;
            let call = open_call_at(mm, frame);
            // The spec's stated exception: a gone endpoint or stamp is `Dead` here, at the
            // argument stage. An open call holds its endpoint alive, so in practice only the
            // stamp can have gone; both are checked, for the same reason.
            if !mm.is_live_endpoint(call.endpoint) || !mm.is_live_budget(call.stamp) {
                return Err(Error::Dead);
            }
            // The badge that matters is the *handle* source's; a message source carries
            // none (KERNEL-SPEC.md, `mint`: only "a handle source's badge not 0" is
            // `NotPermitted`). A server answers a badged call by minting under its own
            // receive right, which is the whole point of minting from a message.
            (call.endpoint, call.stamp, 0)
        }
        MintSource::Handle(h) => {
            let (e, handle) = mm.endpoint_handle(pid, h.index())?;
            (e, handle.stamp, handle.badge)
        }
    };
    // I3: a minted badge is never 0, the receive right's. `redoubt-sys` refuses it while
    // decoding, so nothing reaches here; it is checked again so that neither check rests on the
    // other (KERNEL-SPEC.md, ABI).
    if badge == 0 {
        return Err(Error::InvalidArgument);
    }
    let narrow = match budget {
        Some(h) => Some(mm.budget_handle(pid, h)?),
        None => None,
    };
    // Permission: only a receive right mints (I4), and a budget handle only narrows (I3).
    if source_badge != 0 {
        return Err(Error::NotPermitted);
    }
    let mut stamp = default;
    if let Some(frame) = narrow {
        if !mm.is_at_or_below(frame, default.frame) {
            return Err(Error::NotPermitted);
        }
        stamp = BudgetRef { frame, id: mm.budget(frame).id };
    }
    mm.install_handle(pid, Handle { object: Object::Endpoint(endpoint), badge, stamp })
}

/// The account and labels a fault in `(pid, tid)` blames: the sender of the thread's **current
/// call**, or nobody (answers 37, 55, 82; KERNEL-SPEC.md, Messages). A thread with no current
/// call blames nobody, even when other threads of its process hold open calls, and a `send` is
/// never blamed because a send is never an open call.
pub fn current_call_blame(mm: &MemoryManager, pid: Pid, tid: TID) -> Option<(u64, Labels)> {
    let frame = frame_of(u64::from(slot(mm, pid, tid).current))?;
    let call = open_call_at(mm, frame);
    let mut labels = Labels::new();
    for label in &call.labels[..call.nlabels] {
        labels.push(*label).expect("MAX_LABELS");
    }
    Some((call.account, labels))
}

/// `serve(msg_id)`: the call becomes the thread's current call, the one a fault blames.
pub fn serve(mm: &mut MemoryManager, pid: Pid, tid: TID, msg_id: u64) -> Result<(), Error> {
    let frame = open_call_of(mm, pid, tid, msg_id).ok_or(Error::InvalidArgument)?;
    set_tword(mm, pid, tid, W_CURRENT, frame_word(frame));
    Ok(())
}

// --- `call` and `send` ----------------------------------------------------------------------------

/// `call(h, words, handles, lend, timeout) -> reply` and `send(h, words, handles, transfer,
/// timeout)` (KERNEL-SPEC.md): the same checks, in the order of `call`'s row, then the message
/// is queued and the sender blocks.
#[allow(clippy::too_many_arguments)]
pub fn send(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    kind: MsgKind,
    h: u32,
    body_rec: usize,
    buffer: Option<Pages>,
    timeout: u64,
) -> Result<Option<Return>, Error> {
    // Stage 1 finished in `redoubt-sys`; the body record is the rest of it.
    let body = Body::decode(&crate::redoubt::read_record(mm, body_rec, kind == MsgKind::Call)?)?;
    // Stage 2, argument by argument.
    let (endpoint, via) = mm.endpoint_handle(pid, h)?;
    let (handles, nhandles) = lookup_handles(mm, pid, &body)?;
    let (buf_addr, buf_pages) = match buffer {
        None => (0, 0),
        Some(pages) => {
            if kind == MsgKind::Call && pages.npages.get() > MAX_LEND_PAGES {
                return Err(Error::TooLarge);
            }
            check_buffer(mm, pid, pages, kind == MsgKind::Call)?;
            (pages.addr, pages.npages.get())
        }
    };
    // Stage 3, R1: between two user budgets the label sets must be equal, and the receiving
    // side is the endpoint's *owner*, whoever ends up taking the message (I7).
    let sender_frame = mm.budget_of(pid).ok_or(Error::NotPermitted)?;
    let sender = mm.budget(sender_frame);
    let owner = mm.budget_at(mm.endpoint_at(endpoint).owner);
    if sender.class == Class::User && owner.class == Class::User && sender.labels_of() != owner.labels_of() {
        return Err(Error::LabelDenied);
    }
    // Stage 4, R2: a group with `WAIT_CAP` messages already queued here gets `Busy`.
    let group = Group::of(&sender);
    let mut waiting = 0;
    find_thread::<()>(mm, |mm, qpid, qtid| {
        if queued_on(mm, qpid, qtid, endpoint) && group_of(mm, qpid, qtid) == group {
            waiting += 1;
        }
        None
    });
    if waiting >= WAIT_CAP {
        return Err(Error::Busy);
    }
    // Everything is checked: take the buffer out of the sender (I9) and record the message.
    let mut words = [0; WORDS];
    for (i, word) in words.iter_mut().enumerate() {
        *word = body.words[i] as u64;
    }
    if buf_pages != 0 {
        take_buffer(ss, pid, buf_addr, buf_pages);
    }
    let m = Msg {
        kind,
        badge: via.badge,
        stamp: via.stamp,
        sender_budget: BudgetRef { frame: sender_frame, id: sender.id },
        buf_addr,
        buf_pages,
        words,
        handles,
        nhandles,
    };
    store_msg(mm, pid, tid, &m);
    let seq = mm.next_seq();
    set_tword(mm, pid, tid, W_SEQ, seq);
    set_tword(mm, pid, tid, W_OBJECT, frame_word(endpoint.frame));
    set_tword(mm, pid, tid, W_OBJECT_ID, endpoint.id);
    set_tword(mm, pid, tid, W_REC, body_rec as u64);
    // The sender is queued first, so a receiver taking the message right away finds it waiting
    // and simply answers it: one delivery path, whether a receiver was waiting or not.
    mark(mm, pid, tid, Wait::Send, timeout);
    pump(ss, mm, endpoint);
    settle(ss, mm, pid, tid)
}

/// The handles a body names, looked up in the caller's table in order (`BadHandle`).
fn lookup_handles(
    mm: &MemoryManager,
    pid: Pid,
    body: &Body,
) -> Result<([Option<Handle>; MAX_MSG_HANDLES], usize), Error> {
    let mut handles = [None; MAX_MSG_HANDLES];
    for (i, item) in body.handles.as_slice().iter().enumerate() {
        handles[i] = Some(mm.handle(pid, item.index())?);
    }
    Ok((handles, body.handles.as_slice().len()))
}

/// A lend or transfer range: page-aligned, non-empty, backed, all the caller's own RAM, and for
/// a lend writable (KERNEL-SPEC.md, `call`'s row). Anything else is `InvalidArgument`.
fn check_buffer(mm: &mut MemoryManager, pid: Pid, pages: Pages, lend: bool) -> Result<(), Error> {
    if pages.addr % PAGE_SIZE != 0 {
        return Err(Error::InvalidArgument);
    }
    let len = pages.npages.get().checked_mul(PAGE_SIZE).ok_or(Error::InvalidArgument)?;
    let end = pages.addr.checked_add(len).ok_or(Error::InvalidArgument)?;
    // Back every demand-paged page before anything is checked or moved (WP-K0's rule: a range is
    // checked whole before any page moves).
    mm.ensure_range_exists(pages.addr, len).map_err(|_| Error::InvalidArgument)?;
    mm.check_owned_range(pid, pages.addr, len).map_err(|_| Error::InvalidArgument)?;
    for page in (pages.addr..end).step_by(PAGE_SIZE) {
        let flags = crate::arch::mem::page_flags(page).ok_or(Error::InvalidArgument)?;
        if lend && !flags.contains(redoubt_sys::MemFlags::WRITE) {
            return Err(Error::InvalidArgument);
        }
    }
    Ok(())
}

/// Take the buffer out of the sender's address space (I9: a lent page is unmapped from its
/// lender until the call ends). Every page was checked by [`check_buffer`], so nothing fails.
fn take_buffer(ss: &SystemServices, pid: Pid, addr: usize, npages: usize) {
    let space = ss.mapping_of(pid).expect("the sending process is alive");
    for i in 0..npages {
        crate::arch::mem::lend_out(&space, addr + i * PAGE_SIZE).expect("a checked range lends");
    }
}

// --- `receive` -------------------------------------------------------------------------------------

/// `receive(h(IRQ), timeout)` (R5). The source is unmasked when the receive begins, and the
/// call returns as soon as `fired` is set, clearing it. There is no acknowledge call: the
/// source stays masked from the moment it fires until the *next* receive, so a driver that is
/// busy or gone cannot be stormed by its own device.
fn receive_irq(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    device: DeviceRef,
    timeout: u64,
    rec: usize,
) -> Result<Option<Return>, Error> {
    set_tword(mm, pid, tid, W_OBJECT, frame_word(device.frame));
    set_tword(mm, pid, tid, W_OBJECT_ID, device.id);
    set_tword(mm, pid, tid, W_REC, rec as u64);
    mark(mm, pid, tid, Wait::Irq, timeout);
    // Unmask first, then look at `fired`, in the order R5 states. Unmasking a source that is
    // already asserted makes it fire again at once, which is what a level-triggered device
    // wants: the kernel masks it again and the next receive is answered immediately.
    let mut d = mm.device(device.frame);
    if d.masked {
        d.masked = false;
        mm.store_device(device.frame, &d);
        crate::arch::irq::enable_irq(d.irq as usize);
    }
    irq_ready(ss, mm, device.frame);
    settle(ss, mm, pid, tid)
}

/// Hand the interrupt to a thread waiting in `receive` on device `frame`, if one is waiting
/// and the device has fired. Clearing `fired` here is R5's "returns when `fired` is set
/// (clearing it)", and it happens exactly once per waiting thread.
pub fn irq_ready(ss: &mut SystemServices, mm: &mut MemoryManager, frame: u32) {
    if !mm.device(frame).fired {
        return;
    }
    let id = mm.device(frame).id;
    let waiting = find_thread(mm, |mm, pid, tid| {
        let s = slot(mm, pid, tid);
        (s.wait == Wait::Irq && s.irq == Some(DeviceRef { frame, id })).then_some((pid, tid))
    });
    let Some((pid, tid)) = waiting else { return };
    let mut d = mm.device(frame);
    d.fired = false;
    mm.store_device(frame, &d);
    answer_record(ss, mm, pid, tid, &Received::Interrupt.encode(), Ok(Return::Nothing));
}

/// R10: a device whose owner budget is dying. Everything waiting on it gets `Dead`, its
/// source is masked so nothing can raise it again, the handles naming it go (I1: the sweep
/// that follows reads every handle's object), and its page goes back to its owner.
pub fn destroy_device(ss: &mut SystemServices, mm: &mut MemoryManager, frame: u32) {
    let id = mm.device(frame).id;
    let r = DeviceRef { frame, id };
    fail_all(ss, mm, Error::Dead, |mm, pid, tid| slot(mm, pid, tid).irq == Some(r));
    let d = mm.device(frame);
    if d.kind == crate::device::Kind::Irq {
        crate::arch::irq::disable_irq(d.irq as usize);
    }
    mm.sweep_handles(|_, h| matches!(h.object, Object::Device(x) if x == r));
    mm.free_device(frame);
}

/// `receive(h or none, timeout, max_transfer) -> message | notice | interrupt`.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    from: Option<u32>,
    timeout: u64,
    max_transfer: usize,
    rec: usize,
) -> Result<Option<Return>, Error> {
    // Whatever it returns, the thread has no current call until it takes one (answer 82).
    set_tword(mm, pid, tid, W_CURRENT, 0);
    // The record must be the caller's own writable memory before anything else happens.
    crate::redoubt::check_record::<RECEIVED_SLOTS>(mm, rec)?;
    let Some(h) = from else {
        // No handle: sleep until the timeout (KERNEL-SPEC.md, `receive`).
        mark(mm, pid, tid, Wait::Sleep, timeout);
        return settle(ss, mm, pid, tid);
    };
    // `receive` takes a badge-0 endpoint or an IRQ (KERNEL-SPEC.md, `receive`'s row:
    // `BadHandle`, then `WrongObject`, then `NotPermitted`). A device handle carries badge 0
    // always, so only the endpoint case can earn `NotPermitted`.
    let handle = mm.handle(pid, h)?;
    let endpoint = match handle.object {
        Object::Endpoint(e) => e,
        Object::Device(d) if mm.device_at(d).kind == crate::device::Kind::Irq => {
            return receive_irq(ss, mm, pid, tid, d, timeout, rec);
        }
        _ => return Err(Error::WrongObject),
    };
    // I4: only a badge-0 handle is a receive right.
    if handle.badge != 0 {
        return Err(Error::NotPermitted);
    }
    set_tword(mm, pid, tid, W_OBJECT, frame_word(endpoint.frame));
    set_tword(mm, pid, tid, W_OBJECT_ID, endpoint.id);
    set_tword(mm, pid, tid, W_MAX_TRANSFER, max_transfer as u64);
    set_tword(mm, pid, tid, W_REC, rec as u64);
    mark(mm, pid, tid, Wait::Receive, timeout);
    pump(ss, mm, endpoint);
    settle(ss, mm, pid, tid)
}

// --- Delivery (R2, R4, R4a) --------------------------------------------------------------------

/// Deliver whatever is pending on `e` (`process.rs` calls this when an exit notice appears).
pub fn pump_endpoint(ss: &mut SystemServices, mm: &mut MemoryManager, e: EndpointRef) { pump(ss, mm, e); }

/// Match waiting receivers on `e` with what is pending there, until nothing more can be
/// delivered. Notices come before messages (KERNEL-SPEC.md, Messages).
fn pump(ss: &mut SystemServices, mm: &mut MemoryManager, e: EndpointRef) {
    loop {
        if !mm.is_live_endpoint(e) {
            return;
        }
        // An abandoned-call notice goes to the thread holding the call, on the endpoint the call
        // arrived on (answer 104), before any message.
        let notice = find_thread(mm, |mm, pid, tid| {
            let s = slot(mm, pid, tid);
            if s.wait != Wait::Receive || s.endpoint != Some(e) {
                return None;
            }
            (0..s.ncalls).map(|i| nth_call(mm, pid, tid, i)).find_map(|f| {
                let c = open_call_at(mm, f);
                (c.flags & F_NOTICE != 0 && c.endpoint == e).then_some((pid, tid, f, c.rid))
            })
        });
        if let Some((pid, tid, frame, rid)) = notice {
            // I15: reported exactly once.
            let mut c = open_call_at(mm, frame);
            c.flags &= !F_NOTICE;
            store_open_call(mm, frame, &c);
            let id = NonZeroU64::new(rid).expect("I12: a message id is never 0");
            answer_record(ss, mm, pid, tid, &Received::Abandoned(id).encode(), Ok(Return::Nothing));
            continue;
        }
        // Then an exit notice (KERNEL-SPEC.md, Messages: notices before messages). Unlike an
        // abandoned-call notice it belongs to no particular thread -- it is addressed to the
        // endpoint -- so whichever thread is receiving here takes it. Taking it frees the
        // process object, which is what frees the PID (answer 106).
        let exit = crate::process::pending_notice(mm, e).and_then(|(frame, notice)| {
            find_thread(mm, |mm, pid, tid| {
                let s = slot(mm, pid, tid);
                (s.wait == Wait::Receive && s.endpoint == Some(e)).then_some((pid, tid))
            })
            .map(|(pid, tid)| (frame, notice, pid, tid))
        });
        if let Some((frame, notice, pid, tid)) = exit {
            // A failed output record does not consume the notice or release its PID.
            if let Err(error) = check_receive_record(ss, pid, tid, mm) {
                wake(ss, mm, pid, tid, Err(error));
                continue;
            }
            crate::process::free_object(mm, frame);
            answer_record(ss, mm, pid, tid, &Received::Exit(notice).encode(), Ok(Return::Nothing));
            continue;
        }
        // The first receiver that can take a message, and the message R2 picks for it.
        let pick = find_thread(mm, |mm, rpid, rtid| {
            let s = slot(mm, rpid, rtid);
            if s.wait != Wait::Receive || s.endpoint != Some(e) {
                return None;
            }
            // R4a: a process at `MAX_OPEN_CALLS` takes no calls; sends still arrive.
            let calls = (mm.account(rpid).map_or(0, |a| a.open_calls) as usize) < MAX_OPEN_CALLS;
            next_sender(mm, e, calls).map(|(spid, stid)| (rpid, rtid, spid, stid))
        });
        let Some((rpid, rtid, spid, stid)) = pick else { return };
        deliver(ss, mm, e, rpid, rtid, spid, stid);
    }
}

/// R2: the next message to take on `e` — the oldest message of the next group after the one
/// served last, in group order, wrapping round once. Without `calls` a process at
/// `MAX_OPEN_CALLS` skips calls, so a group's oldest *send* is its message (R4a).
fn next_sender(mm: &MemoryManager, e: EndpointRef, calls: bool) -> Option<(Pid, TID)> {
    let cursor = mm.endpoint_at(e).cursor;
    // Rank 0: groups after the cursor. Rank 1: those at or before it, taken only once the others
    // are done, which is the wrap-around. Then group order, then age.
    let mut best: Option<(u8, Group, u64, Pid, TID)> = None;
    find_thread::<()>(mm, |mm, pid, tid| {
        if !queued_on(mm, pid, tid, e) || (!calls && msg(mm, pid, tid).kind == MsgKind::Call) {
            return None;
        }
        let group = group_of(mm, pid, tid);
        let rank = u8::from(cursor.is_some_and(|c| group.order(&c) != Ordering::Greater));
        let seq = slot(mm, pid, tid).seq;
        let better = match &best {
            None => true,
            Some((brank, bgroup, bseq, _, _)) => match rank.cmp(brank).then(group.order(bgroup)) {
                Ordering::Less => true,
                Ordering::Greater => false,
                Ordering::Equal => seq < *bseq,
            },
        };
        if better {
            best = Some((rank, group, seq, pid, tid));
        }
        None
    });
    best.map(|(_, _, _, pid, tid)| (pid, tid))
}

/// Deliver the message of `(spid, stid)` on `e` to the receiving thread `(rpid, rtid)`, or refuse
/// it (R4): a delivery the receiving process's budget cannot pay for in full, or a transfer over
/// `max_transfer`, fails its sender with `Refused` and the receiver keeps waiting.
#[allow(clippy::too_many_arguments)]
fn deliver(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    e: EndpointRef,
    rpid: Pid,
    rtid: TID,
    spid: Pid,
    stid: TID,
) {
    // The receiver's record must still be its own writable memory. Another of its threads may
    // have unmapped it while it was blocked, and a receiver that cannot be told what it took
    // must not take anything: this is a re-check after `receive` decoded the address, not
    // decoding, so it happens before anything commits. The receiver leaves with the error its
    // own call earned, and the message stays queued for whoever receives next.
    if let Err(error) = check_receive_record(ss, rpid, rtid, mm) {
        fail_wait(ss, mm, rpid, rtid, error);
        return;
    }
    // Delivered or refused, the group has had its turn, so one sender cannot hold up the rest.
    let mut ep = mm.endpoint_at(e);
    ep.cursor = Some(group_of(mm, spid, stid));
    mm.store_endpoint(e.frame, &ep);
    let kind = msg(mm, spid, stid).kind;
    match prepare(ss, mm, e, rpid, rtid, spid, stid) {
        Err(error) => fail_wait(ss, mm, spid, stid, error),
        Ok(received) => {
            answer_record(ss, mm, rpid, rtid, &Received::Message(received).encode(), Ok(Return::Nothing));
            match kind {
                // A `send` is done with; a `call` now waits for its reply.
                MsgKind::Send => wake(ss, mm, spid, stid, Ok(Return::Nothing)),
                MsgKind::Call => set_tword(mm, spid, stid, W_WAIT, Wait::Reply as u64),
            }
        }
    }
}

/// Whether `(pid, tid)`'s `receive` record is still where it can be written. `InvalidArgument`
/// is the error a record earns, and every call's row carries it (decoding, stage 1).
fn check_receive_record(
    ss: &mut SystemServices,
    pid: Pid,
    tid: TID,
    mm: &MemoryManager,
) -> Result<(), Error> {
    let rec = slot(mm, pid, tid).rec;
    let here = crate::arch::process::current_pid();
    let checked = ss
        .activate(pid)
        .map_err(|_| Error::Dead)
        .and_then(|()| crate::redoubt::check_record::<RECEIVED_SLOTS>(mm, rec));
    ss.activate(here).expect("the running process can be activated");
    checked
}

/// Everything delivery does: first what can fail, then what cannot. On `Err` nothing has changed
/// and the sender is refused (R4).
#[allow(clippy::too_many_arguments)]
fn prepare(
    ss: &SystemServices,
    mm: &mut MemoryManager,
    e: EndpointRef,
    rpid: Pid,
    rtid: TID,
    spid: Pid,
    stid: TID,
) -> Result<Message, Error> {
    let m = msg(mm, spid, stid);
    let max_transfer = slot(mm, rpid, rtid).max_transfer;
    let rbudget = mm.budget_of(rpid).ok_or(Error::Refused)?;
    let pages = m.buf_pages;
    // A transfer only if the `receive` named a `max_transfer` at least its size (R4).
    if m.kind == MsgKind::Send && pages > max_transfer {
        return Err(Error::Refused);
    }
    // Where the buffer would land in the receiver, and how many page-table pages mapping it
    // there still needs. Both are questions, not changes: nothing is allocated or charged until
    // the whole cost is known, so a refused delivery costs the receiver nothing (R4).
    let mut at = 0;
    let mut tables = 0;
    if pages != 0 {
        at = choose_buffer_address(ss, mm, rpid, pages)?;
        let space = ss.mapping_of(rpid).ok_or(Error::Refused)?;
        tables = crate::arch::mem::tables_needed(&space, at, pages) as u64;
    }
    // Everything the message brings that the receiving budget must pay for: the table pages for
    // its handles, a call's open-call page, its lent or transferred pages, and the page tables
    // to map them (R4).
    let live: usize = m.handles[..m.nhandles].iter().flatten().filter(|h| is_live(mm, **h)).count();
    // Answer 116: handles that would take the receiver past `MAX_HANDLES` refuse the message,
    // like any other cost it cannot pay.
    let growth = mm.table_growth(rpid, live).ok_or(Error::Refused)?;
    let open = if m.kind == MsgKind::Call { OPEN_CALL_PAGES } else { 0 };
    // A lend is charged to the receiver as well while the call is open (R3); a transfer is
    // charged there instead of at the sender, and costs nothing when both share a budget.
    let buffer_cost = match m.kind {
        MsgKind::Call => pages as u64,
        MsgKind::Send if mm.budget_of(spid) != Some(rbudget) => pages as u64,
        MsgKind::Send => 0,
    };
    let need = growth.saturating_add(open).saturating_add(buffer_cost).saturating_add(tables);
    if need > mm.free_pages(rbudget) {
        return Err(Error::Refused);
    }
    // From here nothing fails: every page the rest of this function allocates was counted just
    // above, and the address was free when it was chosen, with the kernel lock held throughout.
    if pages != 0 {
        reserve_buffer(ss, mm, rpid, at, pages);
    }
    if m.kind == MsgKind::Call {
        mm.charge(rbudget, OPEN_CALL_PAGES + pages as u64).expect("R4: checked just above");
    }
    let buffer = (pages != 0)
        .then(|| Pages { addr: at, npages: NonZeroUsize::new(pages).expect("a buffer has pages") });
    move_buffer(ss, mm, &m, spid, rpid, at);
    let (slots, dropped) = install_handles(mm, rpid, &m.handles[..m.nhandles]);
    assert!(!dropped, "R4: checked just above");
    let rid = mm.next_msg_id(rpid);
    let sender = mm.budget_at(m.sender_budget);
    let mut labels = Labels::new();
    for label in sender.labels_of() {
        labels.push(*label).expect("MAX_LABELS");
    }
    let kind = match m.kind {
        MsgKind::Call => MessageKind::Call { lend: buffer },
        MsgKind::Send => MessageKind::Send { transfer: buffer },
    };
    if m.kind == MsgKind::Call {
        // R4a: the call opens, charged to the receiving process's budget, and becomes the
        // thread's current call (answer 82).
        let frame = mm.alloc_object_frame().expect("R4: the open-call page was charged above");
        store_open_call(
            mm,
            frame,
            &OpenCall {
                rid,
                caller: (spid, stid),
                server: (rpid, rtid),
                endpoint: e,
                badge: m.badge,
                stamp: m.stamp,
                flags: F_WAITING,
                lend_caller: m.buf_addr,
                lend_server: at,
                lend_pages: pages,
                payer: BudgetRef { frame: rbudget, id: mm.budget(rbudget).id },
                account: sender.account,
                labels: sender.labels,
                nlabels: sender.nlabels,
            },
        );
        push_open_call(mm, rpid, rtid, frame);
        set_tword(mm, rpid, rtid, W_CURRENT, frame_word(frame));
        // The caller now waits for the reply, not for a taker: its page names the open call.
        set_tword(mm, spid, stid, W_OBJECT, frame_word(frame));
    }
    let mut words = [0usize; WORDS];
    for (i, word) in words.iter_mut().enumerate() {
        *word = m.words[i] as usize;
    }
    Ok(Message {
        kind,
        msg_id: NonZeroU64::new(rid).expect("I12: message ids start at 1"),
        badge: m.badge,
        account: sender.account,
        labels,
        body: ReceivedBody { words, handles: slots },
    })
}

/// Whether a handle a message carries still names live objects: R10 may have revoked it while
/// the message waited.
fn is_live(mm: &MemoryManager, h: Handle) -> bool {
    if !mm.is_live_budget(h.stamp) {
        return false;
    }
    match h.object {
        Object::Budget(b) => mm.is_live_budget(b),
        Object::Endpoint(e) => mm.is_live_endpoint(e),
        Object::Device(d) => mm.is_live_device(d),
        Object::Process(p) => mm.is_live_process(p),
    }
}

/// A message's copies of its handles move into `pid`'s table, each keeping its slot: one R10
/// revoked meanwhile, or one `pid` cannot pay for, is 0 there. Returns whether any was dropped
/// for want of room.
fn install_handles(mm: &mut MemoryManager, pid: Pid, handles: &[Option<Handle>]) -> (ReceivedHandles, bool) {
    let mut slots = ReceivedHandles::new();
    let mut dropped = false;
    for item in handles {
        let index = match item.filter(|h| is_live(mm, *h)) {
            Some(h) => match mm.install_handle(pid, h) {
                Ok(index) => AbiHandle::new(index),
                Err(_) => {
                    dropped = true;
                    None
                }
            },
            None => None,
        };
        slots.push(index).expect("MAX_MSG_HANDLES");
    }
    (slots, dropped)
}

/// Where a buffer would land in the receiver: a free run of `pages` pages in its Messages
/// region. It only looks; nothing is mapped or charged until the whole cost is known (R4).
fn choose_buffer_address(
    ss: &SystemServices,
    mm: &mut MemoryManager,
    rpid: Pid,
    pages: usize,
) -> Result<usize, Error> {
    let here = crate::arch::process::current_pid();
    // `find_virtual_address` reads the receiver's own kernel page, so its space must be active.
    ss.activate(rpid).map_err(|_| Error::Refused)?;
    let found = mm
        .find_virtual_address(core::ptr::null_mut(), pages * PAGE_SIZE, crate::mem::MemoryType::Messages)
        .map(|addr| addr as usize)
        .map_err(|_| Error::Refused);
    ss.activate(here).expect("the running process can be activated");
    found
}

/// Allocate the page tables that map the buffer at `at`, charged to the receiver's budget. The
/// R4 decision counted exactly these, and the address was free when it was chosen, so neither
/// step can fail here.
fn reserve_buffer(ss: &SystemServices, mm: &mut MemoryManager, rpid: Pid, at: usize, pages: usize) {
    let space = ss.mapping_of(rpid).expect("the receiving process is alive");
    for i in 0..pages {
        crate::arch::mem::prepare_map(mm, &space, rpid, at + i * PAGE_SIZE)
            .expect("R4: the page tables were counted and charged for just above");
    }
}

/// Map the buffer into the receiver: a lend stays the sender's (whose entry remembers the loan),
/// a transfer changes owner and payer together. Every page was prepared, so nothing fails.
fn move_buffer(ss: &SystemServices, mm: &mut MemoryManager, m: &Msg, spid: Pid, rpid: Pid, at: usize) {
    if m.buf_pages == 0 {
        return;
    }
    let sender_space = ss.mapping_of(spid).expect("the sending process is alive");
    let receiver_space = ss.mapping_of(rpid).expect("the receiving process is alive");
    for i in 0..m.buf_pages {
        let from = m.buf_addr + i * PAGE_SIZE;
        let phys = crate::arch::mem::lent_frame(&sender_space, from).expect("I9: the page is lent out");
        crate::arch::mem::map_into(
            mm,
            rpid,
            &receiver_space,
            phys,
            at + i * PAGE_SIZE,
            m.kind == MsgKind::Call,
        )
        .expect("R4: prepared just above");
        if m.kind == MsgKind::Send {
            // The transfer is the receiver's for good: its entry in the sender goes, and the
            // owner and the payer change together.
            crate::arch::mem::drop_lent(&sender_space, from).expect("I9: the page is lent out");
            mm.move_frame(phys, spid, rpid).expect("R4: the pages were charged above");
        }
    }
}

/// Put a queued message's buffer back in its sender's address space.
fn give_buffer_back(ss: &SystemServices, mm: &MemoryManager, pid: Pid, tid: TID) {
    let m = msg(mm, pid, tid);
    if m.buf_pages == 0 {
        return;
    }
    if let Some(space) = ss.mapping_of(pid) {
        for i in 0..m.buf_pages {
            crate::arch::mem::lend_back(&space, m.buf_addr + i * PAGE_SIZE).ok();
        }
    }
    set_tword(mm, pid, tid, W_BUF_PAGES, 0);
}

// --- `reply` ---------------------------------------------------------------------------------------

/// `reply(msg_id, words, handles)`: `msg_id` is an open call of the caller's thread (a `send`'s
/// never is, R4a); it returns the lend. An abandoned call's lend is freed and its reply
/// discarded (R3).
pub fn reply(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    msg_id: u64,
    body_rec: usize,
) -> Result<ReplyOutcome, Error> {
    let body = Body::decode(&crate::redoubt::read_record(mm, body_rec, false)?)?;
    let frame = open_call_of(mm, pid, tid, msg_id).ok_or(Error::InvalidArgument)?;
    let (handles, nhandles) = lookup_handles(mm, pid, &body)?;
    let call = open_call_at(mm, frame);
    // The call leaves the thread whatever happens next (I15: an abandoned call stays open until
    // exactly this reply).
    drop_open_call(mm, pid, tid, frame);
    if call.flags & F_WAITING == 0 {
        // R3: the reply to an abandoned call reaches nobody, and its lend is freed.
        free_abandoned_lend(ss, mm, &call);
    } else {
        return_lend(ss, mm, &call);
    }
    close_call(mm, frame, &call);
    if call.flags & F_WAITING == 0 {
        poke_receivers(ss, mm, pid);
        return Ok(ReplyOutcome { delivered: false, installed: 0 });
    }
    // R4: a reply is never refused. A handle that does not fit the caller -- its budget cannot
    // pay, or it is at `MAX_HANDLES` -- is dropped, 0 in its slot, and the `call` returns
    // `OutOfMemory` either way, the reply still delivered (answers 107 and 116).
    let (cpid, ctid) = call.caller;
    let (slots, dropped) = install_handles(mm, cpid, &handles[..nhandles]);
    let body = ReceivedBody { words: body.words, handles: slots };
    // The scheduler and memory-manager guards remain held through mapping validation, copy,
    // handle rollback and register publication. No mapping/lifecycle transition can interleave.
    let here = crate::arch::process::current_pid();
    let rec = slot(mm, cpid, ctid).rec;
    let written = ss
        .activate(cpid)
        .map_err(|_| Error::InvalidArgument)
        .and_then(|()| crate::redoubt::write_record(mm, rec, &body.encode()));
    ss.activate(here).expect("the running process can be activated");
    let (status, delivered, installed) = if written.is_ok() {
        let mask = slots
            .as_slice()
            .iter()
            .enumerate()
            .fold(0, |mask, (i, handle)| mask | (u32::from(handle.is_some()) << i));
        (if dropped { Err(Error::OutOfMemory) } else { Ok(()) }, true, mask)
    } else {
        // Only this attempt's newly installed caller copies are removed; handle_close also
        // releases a now-empty handle-table page (R6). The server originals stay untouched.
        for handle in slots.as_slice().iter().flatten() {
            mm.handle_close(cpid, handle.index()).expect("new reply handle remains installed under lock");
        }
        (Err(Error::InvalidArgument), false, 0)
    };
    wake(ss, mm, cpid, ctid, call_result(status, call.lend_pages, false, delivered));
    poke_receivers(ss, mm, pid);
    Ok(ReplyOutcome { delivered, installed })
}

/// Give a lend back to the caller (R3: until the reply, it stayed mapped in the server).
fn return_lend(ss: &SystemServices, mm: &mut MemoryManager, call: &OpenCall) {
    if call.lend_pages == 0 {
        return;
    }
    let server = ss.mapping_of(call.server.0).expect("a waiting call's server is still alive");
    let caller = ss.mapping_of(call.caller.0).expect("a waiting call's caller is still alive");
    for i in 0..call.lend_pages {
        // `return_page_inner` validates the protected borrower alias and the lender's
        // invalid reservation name the same frame before it changes either PTE.
        crate::arch::mem::return_page_inner(
            mm,
            &server,
            (call.lend_server + i * PAGE_SIZE) as *mut u8,
            call.caller.0,
            &caller,
            (call.lend_caller + i * PAGE_SIZE) as *mut u8,
        )
        .expect("an open call retains both aliases of its lend");
    }
}

/// The lend of an abandoned call: its pages are the server's alone, so replying frees them.
fn free_abandoned_lend(ss: &SystemServices, mm: &mut MemoryManager, call: &OpenCall) {
    if call.lend_pages == 0 {
        return;
    }
    let space = ss.mapping_of(call.server.0).expect("an abandoned call's server is still alive");
    for i in 0..call.lend_pages {
        let phys = crate::arch::mem::unmap_from(&space, call.lend_server + i * PAGE_SIZE)
            .expect("an abandoned call retains its protected borrower alias");
        mm.free_frame_of(phys, call.server.0).expect("an abandoned lend's frame remains owned by its server");
    }
}

/// Free an open call's page and the charges it carried (R4a).
fn close_call(mm: &mut MemoryManager, frame: u32, call: &OpenCall) {
    if mm.is_live_budget(call.payer) {
        // An abandoned call's lend stopped being the payer's when it was abandoned (R3).
        let lend = if call.flags & F_ABANDONED == 0 { call.lend_pages as u64 } else { 0 };
        mm.uncharge(call.payer.frame, OPEN_CALL_PAGES + lend);
    }
    mm.free_object_frame(frame);
}

// --- R3: abandoned calls ------------------------------------------------------------------------

/// R3: a taken call is abandoned, its caller having died, timed out, or been failed by
/// revocation or by its endpoint's destruction. The caller's charge for the lend ends; the lend
/// stays mapped in the server, charged only there, until the server replies; and the thread
/// holding the call is owed a notice (answer 104).
fn abandon(ss: &SystemServices, mm: &mut MemoryManager, frame: u32) {
    let mut call = open_call_at(mm, frame);
    if call.flags & F_WAITING == 0 {
        return;
    }
    call.flags = (call.flags & !F_WAITING) | F_ABANDONED | F_NOTICE;
    store_open_call(mm, frame, &call);
    if call.lend_pages == 0 {
        return;
    }
    // The receiver's R3 charge for the lend ends here; the frames become the server's own, so
    // they are charged there once and freed when it replies.
    if mm.is_live_budget(call.payer) {
        mm.uncharge(call.payer.frame, call.lend_pages as u64);
    }
    let Some(space) = ss.mapping_of(call.caller.0) else { return };
    for i in 0..call.lend_pages {
        if let Ok(phys) = crate::arch::mem::drop_lent(&space, call.lend_caller + i * PAGE_SIZE) {
            // The frame is the caller's, and the receiver already paid for it while the call
            // was open (R3), so the budget it moves to has room for it by construction.
            mm.move_frame(phys, call.caller.0, call.server.0)
                .expect("R3: the receiver has paid for this lend since it took the call");
        }
    }
}

// --- Teardown: R4b, R10, and timeouts --------------------------------------------------------------

/// A thread is ending. What it waited for is withdrawn; a caller still waiting on a call it
/// holds gets `Dead` and its lend back, and an abandoned lend is freed (R4b).
pub fn thread_ending(ss: &mut SystemServices, mm: &mut MemoryManager, pid: Pid, tid: TID) {
    // Not `fail_wait`: a dying thread gets no answer and must not go back on the ready list.
    let served = unwind(ss, mm, pid, tid);
    set_tword(mm, pid, tid, W_WAIT, Wait::None as u64);
    // R4b: every call it holds open ends, its caller told.
    while slot(mm, pid, tid).ncalls > 0 {
        let frame = nth_call(mm, pid, tid, 0);
        drop_open_call(mm, pid, tid, frame);
        finish_served(ss, mm, frame);
    }
    if let Some(e) = served {
        // A server may be able to take calls again now that this one is gone.
        pump(ss, mm, e);
    }
}

/// A served call's server is gone: a waiting caller gets `Dead` and its lend back; an abandoned
/// lend is freed (R4b).
fn finish_served(ss: &mut SystemServices, mm: &mut MemoryManager, frame: u32) {
    let call = open_call_at(mm, frame);
    if call.flags & F_WAITING != 0 {
        return_lend(ss, mm, &call);
    } else {
        free_abandoned_lend(ss, mm, &call);
    }
    close_call(mm, frame, &call);
    if call.flags & F_WAITING != 0 {
        wake(
            ss,
            mm,
            call.caller.0,
            call.caller.1,
            call_result(Err(Error::Dead), call.lend_pages, false, false),
        );
    }
}

/// A process is ending: every one of its threads does (R4b).
pub fn process_ending(ss: &mut SystemServices, mm: &mut MemoryManager, pid: Pid) {
    for tid in 1..=MAX_THREADS {
        if mm.ipc_frame(pid, tid).is_some() {
            thread_ending(ss, mm, pid, tid);
        }
    }
}

/// R10, the part that reaches messages: after `budget_destroy` marked a subtree dying and before
/// its budgets are freed, destroy the endpoints it owns and fail every message sent through a
/// handle stamped with it.
pub fn budgets_dying(ss: &mut SystemServices, mm: &mut MemoryManager) {
    // Endpoints owned by a dying budget: everything waiting on them gets `Dead`.
    while let Some(frame) = (0..=mm.objects.high_frame)
        .find(|frame| mm.is_endpoint_frame(*frame) && mm.budget_at(mm.endpoint(*frame).owner).dying)
    {
        destroy_endpoint(ss, mm, frame);
    }
    // Devices likewise: destroying the budget they are charged to destroys them, and the
    // machine's devices are then unreachable for good, there being no way to create one.
    while let Some(frame) = (0..=mm.objects.high_frame)
        .find(|frame| mm.is_device_frame(*frame) && mm.budget_at(mm.device(*frame).owner).dying)
    {
        destroy_device(ss, mm, frame);
    }
    // Revocation reaches messages already sent (R10): a queued one fails its sender with `Dead`;
    // a taken call fails its caller with `Dead` at once and is abandoned (R3).
    fail_all(ss, mm, Error::Dead, |mm, pid, tid| {
        let s = slot(mm, pid, tid);
        match s.wait {
            Wait::Send => mm.budget_at(msg(mm, pid, tid).stamp).dying,
            Wait::Reply => mm.budget_at(open_call_at(mm, s.open).stamp).dying,
            _ => false,
        }
    });
}

/// OD6 (WP-K5b): a DMA device whose reset did not confirm is destroyed as R10 destroys one, so
/// every handle to it goes, copies in unreceived messages arriving as 0. Its registry slot, keyed
/// by base, stays flagged until reboot, and no device object is ever made again.
pub fn destroy_quarantined_devices(ss: &mut SystemServices, mm: &mut MemoryManager) {
    if !mm.dma_take_doomed() {
        return;
    }
    while let Some(frame) = (0..=mm.objects.high_frame).find(|frame| {
        mm.is_device_frame(*frame) && {
            let d = mm.device(*frame);
            d.kind == crate::device::Kind::Mmio && mm.dma_quarantined(d.base)
        }
    }) {
        destroy_device(ss, mm, frame);
    }
}

/// Destroy an endpoint (R10): blocked senders and receivers get `Dead`, then calls in flight
/// that a server took fail with `Dead` and are abandoned (R3), and the page goes back to its
/// owner. Receivers go first, so none is offered an abandoned-call notice on the way out.
fn destroy_endpoint(ss: &mut SystemServices, mm: &mut MemoryManager, frame: u32) {
    let e = EndpointRef { frame, id: mm.endpoint(frame).id };
    fail_all(ss, mm, Error::Dead, |mm, pid, tid| {
        let s = slot(mm, pid, tid);
        matches!(s.wait, Wait::Send | Wait::Receive) && s.endpoint == Some(e)
    });
    fail_all(ss, mm, Error::Dead, |mm, pid, tid| {
        let s = slot(mm, pid, tid);
        s.wait == Wait::Reply && open_call_at(mm, s.open).endpoint == e
    });
    // Every exit notice owed here is dropped, and a process still running loses the ear it was
    // to report to (R10; `process.rs`).
    crate::process::endpoint_dying(mm, e);
    // The handles naming it go first: `budget_destroy`'s later sweep reads every handle's
    // object, and one naming a freed frame would stop the kernel (I1). Unreachable until a
    // destroyable budget owns an endpoint (WP-K4), and cheaper than a liveness test on a path
    // that runs for every handle in the system.
    mm.sweep_handles(|_, h| matches!(h.object, Object::Endpoint(x) if x == e));
    let owner = mm.endpoint(frame).owner;
    mm.free_endpoint(frame, owner.frame);
}

/// Fail every blocked thread `doomed` picks, one at a time, until none is left.
fn fail_all(
    ss: &mut SystemServices,
    mm: &mut MemoryManager,
    error: Error,
    mut doomed: impl FnMut(&MemoryManager, Pid, TID) -> bool,
) {
    while let Some((pid, tid)) = find_thread(mm, |mm, pid, tid| doomed(mm, pid, tid).then_some((pid, tid))) {
        fail_wait(ss, mm, pid, tid, error);
    }
}

/// I13: the timeout due first at `now`: the earliest deadline at or before `now` (at an equal
/// deadline, the first in (pid, tid) order), and the earliest deadline still to come (`u64::MAX`
/// for none). Only the threads of processes whose cached earliest timeout has come are read; each
/// such cache is recomputed on the way.
pub fn next_timeout(mm: &mut MemoryManager, now: u64) -> (Option<(u64, Pid, TID)>, u64) {
    let mut due: Option<(u64, Pid, TID)> = None;
    let mut next = u64::MAX;
    for index in 1..=MAX_PROCESS_COUNT {
        let Some(pid) = Pid::new(index as u8) else { continue };
        let Some(earliest) = mm.account(pid).map(|a| a.earliest_timeout) else { continue };
        if earliest > now {
            next = next.min(earliest);
            continue;
        }
        let mut exact = u64::MAX;
        for tid in 1..=MAX_THREADS {
            if mm.ipc_frame(pid, tid).is_none() {
                continue;
            }
            if Wait::from_word(tword(mm, pid, tid, W_WAIT)) == Wait::None {
                continue;
            }
            let deadline = tword(mm, pid, tid, W_DEADLINE);
            if deadline == u64::MAX {
                continue;
            }
            exact = exact.min(deadline);
            if deadline <= now {
                if due.is_none_or(|(d, _, _)| deadline < d) {
                    due = Some((deadline, pid, tid));
                }
            } else {
                next = next.min(deadline);
            }
        }
        if let Some(a) = mm.account_mut(pid) {
            a.earliest_timeout = exact;
        }
    }
    (due, next)
}

/// The blocking call of `(pid, tid)` reached its timeout: it returns `Timeout` (I13), with what
/// it waited for unwound (a queued message's buffer back, a taken call abandoned).
pub fn time_out(ss: &mut SystemServices, mm: &mut MemoryManager, pid: Pid, tid: TID) {
    fail_wait(ss, mm, pid, tid, Error::Timeout);
}

/// After `reply` freed an open call, the process may be able to take calls again (R4a) and an
/// abandoned-call notice may be waiting: try every endpoint its threads receive on.
fn poke_receivers(ss: &mut SystemServices, mm: &mut MemoryManager, pid: Pid) {
    for tid in 1..=MAX_THREADS {
        let s = slot(mm, pid, tid);
        if s.wait == Wait::Receive {
            if let Some(e) = s.endpoint {
                pump(ss, mm, e);
            }
        }
    }
}
