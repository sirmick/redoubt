// SPDX-License-Identifier: MIT OR Apache-2.0

//! Messages: `call`, `send`, `receive`, `reply`, `serve` and `mint` (kernel/ipc.md, R1-R4b,
//! R10).
//!
//! # Where a message lives
//! There is no message buffer anywhere. A message is queued exactly while its sender is blocked in
//! `send` or `call`, and a thread blocks at most once, so **the queue is the set of blocked
//! senders**, linked through their own pages. Each thread keeps what it is waiting
//! for, the message it is sending and the calls it holds open in a page of its own
//! ([`crate::budget::Account::ipc`]) — the page the cost table already charges for a thread. Two
//! things follow, and they are the reason for this shape:
//! - `call` and `send` allocate no kernel object, so neither can fail for want of one, which their error rows
//!   require (neither returns `OutOfMemory`; a `call` returns it only for a reply's handles, which is the
//!   caller's own table). A lend of pages the sender never touched is still backed as it is checked, charged
//!   to the sender, and that is `unmap`'s `OutOfMemory` rather than this path's (kernel/ipc.md R3);
//! - nothing a sender does makes the kernel allocate on a receiver's behalf.
//!
//! A **taken** call moves out of the sender's page into an open-call page of its own, charged to
//! the receiving process's budget (R4a) and freed by `reply`. That page holds everything a reply
//! needs: whom to wake, the lend to give back, and who pays for it (R3).
//!
//! # Who waits where
//! What waits is found from what it waits on, never by walking the threads (kernel/scheduling.md
//! R12): each endpoint keeps its receivers in the order they began to wait, R2's groups in the
//! order their turns fall due, the calls owing a notice there and the calls taken there whose
//! callers wait; each device keeps the threads waiting for its interrupt; each budget keeps what
//! was sent under its stamp, for R10. The lists are intrusive, their heads in those objects'
//! frames and their links in the threads' and the open calls' own pages, so they cost no
//! allocation; their links and R2's order are `redoubt-ipclist`'s, host-tested, and every rule
//! here. A checked build audits them against the threads at the end of the kernel entry that
//! changed one ([`audit`]).
//!
//! # Locks
//! Every entry point takes the scheduler (`ss`) and the memory manager (`mm`) together, borrowed
//! once by the dispatcher (`redoubt.rs`) in that order; nothing here borrows either again.
//!
//! # Timeouts
//! A blocking call records its deadline (`mark`); a thread that blocks makes sure the kernel's
//! timer comes by then (`settle`), and [`expire_due`] answers every thread whose deadline has
//! passed, earliest first. The timer and when expiry runs are `time.rs`'s.

use core::num::{NonZeroU64, NonZeroUsize};

use redoubt_ipclist::{self as lists, List, Page, Words};
use redoubt_layout::{KERNEL_PID, Pid};
use redoubt_sys::PAGE_SIZE;
use redoubt_sys::{
    Body, CallOutcome, Error, Handle as AbiHandle, Labels, LendDisposition, MAX_LABELS, MAX_LEND_PAGES,
    MAX_MSG_HANDLES, MAX_OPEN_CALLS, MAX_THREADS, Message, MessageKind, MintSource, Pages, RECEIVED_SLOTS,
    Received, ReceivedBody, ReceivedHandles, ReplyOutcome, Return, WAIT_CAP, WORDS, encode_result,
};

use crate::arch::process::TID;
use crate::budget::{BudgetFrame, Class};
use crate::cell::KernelCell;
use crate::endpoint::Group;
use crate::handle::{BudgetRef, DeviceRef, EndpointRef, Handle, Object};
use crate::kframe;
use crate::mem::MemoryManager;
use crate::ptable::ProcessTable;

/// The cost table (kernel/objects.md, "What objects cost"), in pages.
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
/// What the thread is waiting on, as frame + 1 and id: the endpoint while sending or
/// receiving, the open call while waiting for a reply, the device object while in `receive` on
/// an IRQ handle (R5). `Wait` says which, so one pair of words serves all three.
const W_OBJECT: usize = 3;
const W_OBJECT_ID: usize = 4;
const W_MAX_TRANSFER: usize = 5;
/// `receive`'s record, or the record `call` writes its reply back into.
const W_REC: usize = 6;
const W_KIND: usize = 7;
const W_BADGE: usize = 8;
const W_STAMP: usize = 9; // frame + 1
const W_STAMP_ID: usize = 10;
const W_SENDER_BUDGET: usize = 11; // frame + 1
const W_SENDER_BUDGET_ID: usize = 12;
const W_BUF_ADDR: usize = 13;
const W_BUF_PAGES: usize = 14;
const W_WORDS: usize = 15; // WORDS words
const W_NHANDLES: usize = W_WORDS + WORDS;
const W_HANDLES: usize = W_NHANDLES + 1; // MAX_MSG_HANDLES * 4 words
const W_NCALLS: usize = W_HANDLES + MAX_MSG_HANDLES * 4;
const W_CURRENT: usize = W_NCALLS + 1; // open-call frame + 1
const W_CALLS: usize = W_CURRENT + 1; // MAX_OPEN_CALLS frame numbers
/// The lists' words (`redoubt-ipclist`): its links, its arrival, its group's node.
const W_LISTS: usize = W_CALLS + MAX_OPEN_CALLS;
const THREAD_WORDS: usize = W_LISTS + lists::THREAD_WORDS;
// The thread's saved registers take the page's last bytes (`arch::process`).
const _: () = assert!(THREAD_WORDS * 8 <= crate::arch::process::CONTEXT_OFFSET);

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
    /// In `receive` with no handle: asleep until the timeout (kernel/timer.md).
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

/// The message a thread is sending while it waits (kernel/ipc.md, "Messages"). The account and
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
    /// its slot (servers/wire.md names handles by slot).
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
    /// The endpoint it is sending on or receiving from.
    endpoint: Option<EndpointRef>,
    /// While waiting for a reply, the open call's frame.
    open: u32,
    /// While waiting in `receive` on an IRQ handle, the device object it named.
    irq: Option<DeviceRef>,
    max_transfer: usize,
    rec: usize,
    ncalls: usize,
    /// The thread's current call: the one a fault blames (kernel/processes.md R21). Frame + 1; 0
    /// for none.
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
/// (kernel/processes.md R21). Snapshots, not a reference to the sender's budget, so that blame
/// survives that budget being destroyed while the call is open.
const C_ACCOUNT: usize = 17;
const C_NLABELS: usize = 18;
const C_LABELS: usize = 19; // MAX_LABELS words
/// The lists' words (`redoubt-ipclist`): its links on its endpoint's and its stamp's lists.
/// `store_open_call` writes only below them.
const C_LISTS: usize = C_LABELS + MAX_LABELS;
const CALL_WORDS: usize = C_LISTS + lists::CALL_WORDS;
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

fn pid_of(word: u64) -> Pid { crate::budget::pid_from(word).expect("I1: an open call names no process") }

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
    let mut words = [0u64; C_LISTS];
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

// --- The lists --------------------------------------------------------------------------------------

/// A thread as the lists name it (`redoubt-ipclist`): `pid << 8 | tid`.
fn tref(pid: Pid, tid: TID) -> u64 { u64::from(pid.get()) << 8 | tid as u64 }

/// The thread a list names.
fn thread_of(r: u64) -> (Pid, TID) {
    (crate::budget::pid_from(r >> 8).expect("I1: a list names no process"), (r & 0xff) as TID)
}

/// An open call as the lists name it: its frame + 1.
fn cref(frame: u32) -> u64 { frame_word(frame) }

/// The open call a list names.
fn call_of(r: u64) -> u32 { frame_of(r).expect("I1: a list names no open call") }

/// The process object a list names: its frame, + 1 as the list holds it.
fn object_of(r: u64) -> u32 { frame_of(r).expect("I1: a list names no process object") }

/// The lists' words in the kernel's frames: in each kind of page above its own words, which
/// storing the object rewrites; a budget's two chain heads beside its handle chains' heads.
struct Frames<'a>(&'a MemoryManager);

/// Whether a list changed since the last audit ([`audit`]).
#[cfg(debug_assertions)]
static CHANGED: KernelCell<bool> = KernelCell::new(false);

impl Frames<'_> {
    /// The physical page and byte offset of `page`'s list word `word`; `None` for a thread with
    /// no page, which waits on nothing.
    fn at(&self, page: Page, word: usize) -> Option<(usize, usize)> {
        let mm = self.0;
        let (phys, at) = match page {
            Page::Thread(r) => {
                let (pid, tid) = thread_of(r);
                (thread_phys(mm, pid, tid)?, W_LISTS + word)
            }
            Page::Call(r) => (mm.object_phys(call_of(r)), C_LISTS + word),
            Page::Process(r) => (mm.object_phys(object_of(r)), crate::process::LIST_WORD + word),
            Page::Endpoint(frame) => (mm.object_phys(frame), crate::endpoint::LIST_WORD + word),
            Page::Device(frame) => (mm.object_phys(frame), crate::device::LIST_WORD + word),
            Page::Budget(frame) => {
                let at = [crate::budget::QUEUED_WORD, crate::budget::TAKEN_WORD][word];
                (mm.object_phys(frame), at)
            }
            Page::Kernel => unreachable!("the kernel's words are no frame's"),
        };
        Some((phys, at * 8))
    }
}

impl Words for Frames<'_> {
    fn read(&self, page: Page, word: usize) -> u64 {
        self.at(page, word).map_or(0, |(phys, at)| kframe::read(phys, at))
    }

    fn write(&mut self, page: Page, word: usize, value: u64) {
        #[cfg(debug_assertions)]
        CHANGED.with(|changed| *changed = true);
        if let Some((phys, at)) = self.at(page, word) {
            if let Page::Thread(_) = page {
                // A fresh page is stamped the first time it is written (`set_tword`).
                kframe::write(phys, 0, THREAD_MAGIC);
            }
            kframe::write(phys, at, value);
        }
    }
}

/// `(pid, tid)` stops waiting: it leaves the list its wait put it on. Every wait ends through
/// here (`wake`, `end_thread`), but a taken call's, whose caller `deliver` moves on to wait for
/// the reply.
fn unlist(mm: &MemoryManager, pid: Pid, tid: TID) {
    let s = slot(mm, pid, tid);
    let me = tref(pid, tid);
    let list = match (s.wait, s.endpoint, s.irq) {
        (Wait::Receive, Some(e), _) => List::receivers(e.frame),
        (Wait::Irq, _, Some(d)) => List::irq_waiters(d.frame),
        (Wait::Send, Some(e), _) => return unqueue(mm, e, pid, tid),
        _ => return,
    };
    let w = &mut Frames(mm);
    // Unlinking a thread that is not on the list would empty it.
    assert!(list.contains(w, me), "I1: a waiting thread is not on its list");
    list.remove(w, me);
}

/// `(pid, tid)`'s message leaves `e`'s queue, taken or not, and its stamp's chain.
fn unqueue(mm: &MemoryManager, e: EndpointRef, pid: Pid, tid: TID) {
    let me = tref(pid, tid);
    let send = tword(mm, pid, tid, W_KIND) == MsgKind::Send as u64;
    let stamp = frame_of(tword(mm, pid, tid, W_STAMP)).expect("a queued message has a stamp");
    let w = &mut Frames(mm);
    assert!(lists::group_of(w, me) != 0, "I1: a queued message is in no group");
    lists::dequeue(w, e.frame, me, send);
    List::queued(stamp).remove(w, me);
}

/// Open call `frame` leaves the lists its flags put it on: its endpoint's open list and its
/// stamp's chain while its caller waits, its endpoint's notice list while a notice is owed.
fn unlist_call(mm: &MemoryManager, frame: u32, call: &OpenCall) {
    let w = &mut Frames(mm);
    let c = cref(frame);
    if call.flags & F_WAITING != 0 {
        List::open(call.endpoint.frame).remove(w, c);
        List::taken(call.stamp.frame).remove(w, c);
    }
    if call.flags & F_NOTICE != 0 {
        List::notices(call.endpoint.frame).remove(w, c);
    }
}

/// Whether every list word of endpoint or device `frame` is 0: nothing waits on it, and no head or
/// tail is left behind. One frame lookup, whatever the object.
#[cfg(debug_assertions)]
fn lists_empty(mm: &MemoryManager, frame: u32, endpoint: bool) -> bool {
    let phys = mm.object_phys(frame);
    let (at, n) = if endpoint {
        (crate::endpoint::LIST_WORD, lists::ENDPOINT_WORDS)
    } else {
        (crate::device::LIST_WORD, lists::DEVICE_WORDS)
    };
    (at..at + n).all(|i| kframe::read(phys, i * 8) == 0)
}

/// The R2 group of the message `(pid, tid)` sends, or its group node holds.
fn group_of(mm: &MemoryManager, pid: Pid, tid: TID) -> Group {
    Group::of(&mm.budget_at(msg(mm, pid, tid).sender_budget))
}

// --- Waking and blocking -----------------------------------------------------------------------------

/// Whether `(pid, tid)` is the thread making the system call. It was never taken off the ready
/// list, so an answer for it is just its registers: `settle` resumes it.
fn is_running(ss: &ProcessTable, pid: Pid, tid: TID) -> bool {
    ss.current_pid() == pid && crate::arch::process::Process::current().current_tid() == tid
}

/// Hand a thread its result. One that was blocked goes back on the ready list; the thread making
/// the call was never off it.
fn wake(ss: &mut ProcessTable, mm: &MemoryManager, pid: Pid, tid: TID, result: Result<Return, Error>) {
    unlist(mm, pid, tid);
    set_tword(mm, pid, tid, W_WAIT, Wait::None as u64);
    if !is_running(ss, pid, tid) {
        // A waiting thread belongs to a live process, so this cannot fail.
        ss.ready_thread(pid, tid).expect("a waiting thread belongs to a live process");
    }
    ss.set_redoubt_result(pid, tid, &encode_result(&result)).expect("a waiting thread exists");
}

/// Write `slots` into `(pid, tid)`'s record, in its own address space, and wake it with
/// `result`, or with the error the record earned: a record that is no longer the thread's
/// writable memory is `InvalidArgument`, the error of decoding, which every call's row carries.
/// Every delivery checks the record first (`check_receive_record`) and takes nothing if it fails
/// (kernel/ipc.md, "A bad record takes nothing"), so by the time this runs the record was good a
/// moment ago.
fn answer_record<const N: usize>(
    ss: &mut ProcessTable,
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
/// that follows may answer it at once, and `settle` then never takes it off the ready list. Nor
/// does it arm the timer: only a thread that blocks does (`settle`).
fn mark(mm: &mut MemoryManager, pid: Pid, tid: TID, wait: Wait, timeout: u64) {
    // Timeouts are relative microseconds, added with saturation, so `FOREVER` never expires.
    let deadline = crate::time::now_us().saturating_add(timeout);
    set_tword(mm, pid, tid, W_WAIT, wait as u64);
    set_tword(mm, pid, tid, W_DEADLINE, deadline);
}

/// What a blocking call does once delivery has had its chance: resume with the answer it already
/// has, time out without ever blocking, or block. `Ok(None)` tells the trap handler to resume
/// whatever is current now, which is this thread when it was answered (`redoubt.rs`). Every
/// blocking call ends here, and this is the one place a thread blocks: so the timer is armed for a
/// timeout only here, when its thread blocks, and a call answered at once or already past its
/// deadline arms nothing (kernel/timer.md, "R12 (scheduling) for timer work").
fn settle(
    ss: &mut ProcessTable,
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
    if s.deadline != u64::MAX {
        if let Some(a) = mm.account_mut(pid) {
            a.earliest_timeout = a.earliest_timeout.min(s.deadline);
        }
        crate::time::note_timeout(s.deadline);
    }
    // `can_resume: false` is what takes this thread off the ready list (ptable.rs).
    ss.activate_process_thread(tid, KERNEL_PID, 0, false).expect("the kernel can always run");
    Ok(None)
}

/// A blocked thread's wait ends without an answer: what it waited for is unwound and it gets
/// `error`. This is the one meaning of a timeout (I13), of `Refused` (R4), of `Dead` from
/// revocation or an endpoint's destruction (R10), whatever the thread was waiting for.
fn fail_wait(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid, tid: TID, error: Error) {
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
fn unwind(ss: &ProcessTable, mm: &mut MemoryManager, pid: Pid, tid: TID) -> Option<EndpointRef> {
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

/// `mint(source, badge, budget?) -> h` (kernel/objects.md, `mint`; R9, I3, I4).
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
            // none (kernel/objects.md, `mint`: only "a handle source's badge not 0" is
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
    // other (kernel/abi.md).
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
/// call**, or nobody (kernel/processes.md R21). A thread with no current
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
/// timeout)` (kernel/ipc.md): the same checks, in the order of `call`'s row, then the message
/// is queued and the sender blocks.
#[allow(clippy::too_many_arguments)]
pub fn send(
    ss: &mut ProcessTable,
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
    // Stage 4, R2: a group with `WAIT_CAP` messages already queued here gets `Busy`. Its node
    // counts them; finding it walks this endpoint's groups, at most one per sending budget.
    let group = Group::of(&sender);
    let node = lists::find(&Frames(mm), endpoint.frame, |w, node| {
        let (npid, ntid) = thread_of(node);
        group_of(w.0, npid, ntid) == group
    });
    if node.is_some_and(|node| lists::count(&Frames(mm), node) >= WAIT_CAP as u64) {
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
    set_tword(mm, pid, tid, W_OBJECT, frame_word(endpoint.frame));
    set_tword(mm, pid, tid, W_OBJECT_ID, endpoint.id);
    set_tword(mm, pid, tid, W_REC, body_rec as u64);
    // The sender is queued first, so a receiver taking the message right away finds it waiting
    // and simply answers it: one delivery path, whether a receiver was waiting or not. It goes in
    // its group on the endpoint and on its stamp's chain (R10).
    mark(mm, pid, tid, Wait::Send, timeout);
    let seq = mm.next_seq();
    let w = &mut Frames(mm);
    lists::enqueue(w, endpoint.frame, node, tref(pid, tid), kind == MsgKind::Send, seq);
    List::queued(via.stamp.frame).push_front(w, tref(pid, tid));
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
/// a lend writable (kernel/abi.md, `call`'s row; R3). Anything else is `InvalidArgument`.
fn check_buffer(mm: &mut MemoryManager, pid: Pid, pages: Pages, lend: bool) -> Result<(), Error> {
    if pages.addr % PAGE_SIZE != 0 {
        return Err(Error::InvalidArgument);
    }
    let len = pages.npages.get().checked_mul(PAGE_SIZE).ok_or(Error::InvalidArgument)?;
    let end = pages.addr.checked_add(len).ok_or(Error::InvalidArgument)?;
    // Back every demand-paged page before anything is checked or moved (kernel/memory.md: a range
    // is checked whole before any page moves).
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
fn take_buffer(ss: &ProcessTable, pid: Pid, addr: usize, npages: usize) {
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
    ss: &mut ProcessTable,
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
    List::irq_waiters(device.frame).push_back(&mut Frames(mm), tref(pid, tid));
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
/// (clearing it)", and it happens exactly once per waiting thread, and only once its record is
/// known to be writable: a bad record takes nothing, so the device stays fired (and masked) for
/// the next `receive`.
pub fn irq_ready(ss: &mut ProcessTable, mm: &mut MemoryManager, frame: u32) {
    if !mm.device(frame).fired {
        return;
    }
    // The thread that began to wait first.
    let waiting = List::irq_waiters(frame).first(&Frames(mm));
    if waiting == 0 {
        return;
    }
    let (pid, tid) = thread_of(waiting);
    if let Err(error) = check_receive_record(ss, pid, tid, mm) {
        wake(ss, mm, pid, tid, Err(error));
        return;
    }
    let mut d = mm.device(frame);
    d.fired = false;
    mm.store_device(frame, &d);
    answer_record(ss, mm, pid, tid, &Received::Interrupt.encode(), Ok(Return::Nothing));
}

/// R10: a device whose owner budget is dying. Everything waiting on it gets `Dead`, its
/// source is masked so nothing can raise it again, the handles naming it go (I1: the sweep
/// that follows reads every handle's object), and its page goes back to its owner.
pub fn destroy_device(ss: &mut ProcessTable, mm: &mut MemoryManager, frame: u32) {
    let id = mm.device(frame).id;
    let r = DeviceRef { frame, id };
    fail_each(ss, mm, List::irq_waiters(frame));
    let d = mm.device(frame);
    if d.kind == crate::device::Kind::Irq {
        crate::arch::irq::disable_irq(d.irq as usize);
    }
    // Mark it destroyed while it is still an object frame: a `destroy_quarantined_devices` later
    // in the same destruction must not find and free this frame again (I5, I10).
    let mut d = mm.device(frame);
    d.destroyed = true;
    mm.store_device(frame, &d);
    mm.sweep_handles_now(|_, h| matches!(h.object, Object::Device(x) if x == r));
    mm.free_device(frame);
}

/// `receive(h or none, timeout, max_transfer) -> message | notice | interrupt`.
#[allow(clippy::too_many_arguments)]
pub fn receive(
    ss: &mut ProcessTable,
    mm: &mut MemoryManager,
    pid: Pid,
    tid: TID,
    from: Option<u32>,
    timeout: u64,
    max_transfer: usize,
    rec: usize,
) -> Result<Option<Return>, Error> {
    // Whatever it returns, the thread has no current call until it takes one (R21).
    set_tword(mm, pid, tid, W_CURRENT, 0);
    // The record must be the caller's own writable memory before anything else happens.
    crate::redoubt::check_record::<RECEIVED_SLOTS>(mm, rec)?;
    let Some(h) = from else {
        // No handle: sleep until the timeout (kernel/timer.md).
        mark(mm, pid, tid, Wait::Sleep, timeout);
        return settle(ss, mm, pid, tid);
    };
    // `receive` takes a badge-0 endpoint or an IRQ (kernel/abi.md, `receive`'s row:
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
    // Last among the endpoint's receivers, in the one order of arrivals: an abandoned-call notice
    // goes to the holder that began to wait first.
    let seq = mm.next_seq();
    let w = &mut Frames(mm);
    w.write(Page::Thread(tref(pid, tid)), lists::T_SEQ, seq);
    List::receivers(endpoint.frame).push_back(w, tref(pid, tid));
    pump(ss, mm, endpoint);
    settle(ss, mm, pid, tid)
}

// --- Exit notices (kernel/processes.md) ------------------------------------------------------------

/// Process object `frame`, just made, names exit endpoint `e`: it joins `e`'s reporters.
pub fn reporting(mm: &MemoryManager, e: u32, frame: u32) {
    List::reporters(e).push_front(&mut Frames(mm), frame_word(frame))
}

/// Process object `frame`'s notice is owed on `e`: it leaves `e`'s reporters for its exits' tail,
/// so notices are received in the order they came.
pub fn exit_owed(mm: &MemoryManager, e: u32, frame: u32) {
    let w = &mut Frames(mm);
    List::reporters(e).remove(w, frame_word(frame));
    List::exits(e).push_back(w, frame_word(frame));
}

/// Process object `frame` leaves exit endpoint `e`: from its exits if its notice is owed there,
/// else from its reporters.
pub fn unreport(mm: &MemoryManager, e: u32, frame: u32, owed: bool) {
    let list = if owed { List::exits(e) } else { List::reporters(e) };
    let w = &mut Frames(mm);
    assert!(list.contains(w, frame_word(frame)), "I1: a process object is not on its endpoint's list");
    list.remove(w, frame_word(frame));
}

/// The process object whose exit notice came first of those owed on `e`.
pub fn first_exit(mm: &MemoryManager, e: u32) -> Option<u32> { frame_of(List::exits(e).first(&Frames(mm))) }

/// Unlink and return the first process object naming dying endpoint `e`: its exits first, then its
/// reporters (R10).
pub fn pop_naming(mm: &MemoryManager, e: u32) -> Option<u32> {
    let w = &mut Frames(mm);
    let r = match List::exits(e).pop_front(w) {
        0 => List::reporters(e).pop_front(w),
        r => r,
    };
    frame_of(r)
}

// --- Delivery (R2, R4, R4a) --------------------------------------------------------------------

/// Deliver whatever is pending on `e` (`process.rs` calls this when an exit notice appears).
pub fn pump_endpoint(ss: &mut ProcessTable, mm: &mut MemoryManager, e: EndpointRef) { pump(ss, mm, e); }

/// Match waiting receivers on `e` with what is pending there, until nothing more can be
/// delivered. Notices come before messages (kernel/ipc.md, "What `receive` returns"). Each pick
/// reads `e`'s own lists: its owed notices, its receivers from the first until one can take, and
/// two group heads (R2), never another endpoint's or every thread.
fn pump(ss: &mut ProcessTable, mm: &mut MemoryManager, e: EndpointRef) {
    #[cfg(feature = "walk-trace")]
    let _walk = crate::sched::trace::walk(crate::sched::trace::PUMP);
    loop {
        if !mm.is_live_endpoint(e) {
            return;
        }
        // An abandoned-call notice goes to the thread holding the call, on the endpoint the call
        // arrived on (R3), before any message.
        if let Some((pid, tid, frame, rid)) = owed_notice(mm, e) {
            // A bad record takes nothing: the notice stays owed for the next `receive`.
            if let Err(error) = check_receive_record(ss, pid, tid, mm) {
                wake(ss, mm, pid, tid, Err(error));
                continue;
            }
            // I15: reported exactly once.
            let mut c = open_call_at(mm, frame);
            unlist_call(mm, frame, &c);
            c.flags &= !F_NOTICE;
            store_open_call(mm, frame, &c);
            let id = NonZeroU64::new(rid).expect("I12: a message id is never 0");
            answer_record(ss, mm, pid, tid, &Received::Abandoned(id).encode(), Ok(Return::Nothing));
            continue;
        }
        // Then an exit notice (kernel/ipc.md: notices before messages). Unlike an
        // abandoned-call notice it belongs to no particular thread -- it is addressed to the
        // endpoint -- so the receiver that began to wait first takes it. Taking it frees the
        // process object, which is what frees the PID (kernel/processes.md R20).
        let exit = crate::process::pending_notice(mm, e)
            .and_then(|(frame, notice)| next_receiver(mm, e, 0).map(|(pid, tid)| (frame, notice, pid, tid)));
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
        let Some((rpid, rtid, spid, stid)) = pick(mm, e) else { return };
        deliver(ss, mm, e, rpid, rtid, spid, stid);
    }
}

/// The receiver on `e` after list member `after` (0: from the first) that `pump` may feed: one in
/// a process no destruction is about to end. A doomed thread takes nothing, so what it would have
/// taken stays for a live receiver (R4b).
fn next_receiver(mm: &MemoryManager, e: EndpointRef, after: u64) -> Option<(Pid, TID)> {
    let w = &Frames(mm);
    let list = List::receivers(e.frame);
    let mut r = if after == 0 { list.first(w) } else { list.next(w, after) };
    while r != 0 {
        let (pid, tid) = thread_of(r);
        if !mm.process_is_doomed(pid) {
            return Some((pid, tid));
        }
        r = list.next(w, r);
    }
    None
}

/// The abandoned-call notice `pump` delivers next on `e`, with the thread it goes to: of the
/// notices owed on `e` (R3), one held by the receiver there that began to wait first, its call
/// taken first. A walk of `e`'s owed notices only.
fn owed_notice(mm: &MemoryManager, e: EndpointRef) -> Option<(Pid, TID, u32, u64)> {
    let w = &Frames(mm);
    let list = List::notices(e.frame);
    let mut best: Option<(u64, u64, Pid, TID, u32)> = None;
    let mut c = list.first(w);
    while c != 0 {
        let frame = call_of(c);
        let call = open_call_at(mm, frame);
        let (pid, tid) = call.server;
        let s = slot(mm, pid, tid);
        if s.wait == Wait::Receive && s.endpoint == Some(e) && !mm.process_is_doomed(pid) {
            let arrived = w.read(Page::Thread(tref(pid, tid)), lists::T_SEQ);
            if best.is_none_or(|(a, rid, ..)| (arrived, call.rid) < (a, rid)) {
                best = Some((arrived, call.rid, pid, tid, frame));
            }
        }
        c = list.next(w, c);
    }
    best.map(|(_, rid, pid, tid, frame)| (pid, tid, frame, rid))
}

/// R2 and R4a: the first receiver on `e` that can take a message, and the message it takes: the
/// head group's oldest, or, for a receiver in a process at `MAX_OPEN_CALLS`, which takes no
/// calls, the oldest send of the group whose oldest send is due first. Returns (receiver,
/// sender).
fn pick(mm: &MemoryManager, e: EndpointRef) -> Option<(Pid, TID, Pid, TID)> {
    if lists::no_groups(&Frames(mm), e.frame) {
        return None;
    }
    let mut at = next_receiver(mm, e, 0);
    while let Some((rpid, rtid)) = at {
        let calls = (mm.account(rpid).map_or(0, |a| a.open_calls) as usize) < MAX_OPEN_CALLS;
        let sender = lists::pick(&Frames(mm), e.frame, calls);
        if sender != 0 {
            let (spid, stid) = thread_of(sender);
            return Some((rpid, rtid, spid, stid));
        }
        at = next_receiver(mm, e, tref(rpid, rtid));
    }
    None
}

/// Deliver the message of `(spid, stid)` on `e` to the receiving thread `(rpid, rtid)`, or refuse
/// it (R4): a delivery the receiving process's budget cannot pay for in full, or a transfer over
/// `max_transfer`, fails its sender with `Refused` and the receiver keeps waiting.
#[allow(clippy::too_many_arguments)]
fn deliver(
    ss: &mut ProcessTable,
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
    // Delivered or refused, the group has had its turn, so one sender cannot hold up the rest: its
    // turn is due again from now, behind every group already waiting. Only its own node carries
    // that, so a group with nothing queued keeps nothing, and other groups' turns do not move:
    // one write and the group's two moves to the lists' tails. The take is never readable by a
    // process, like `next_seq` it comes from.
    let now = mm.next_seq();
    lists::served(&mut Frames(mm), e.frame, tref(spid, stid), now);
    let kind = msg(mm, spid, stid).kind;
    match prepare(ss, mm, e, rpid, rtid, spid, stid) {
        Err(error) => fail_wait(ss, mm, spid, stid, error),
        Ok(received) => {
            answer_record(ss, mm, rpid, rtid, &Received::Message(received).encode(), Ok(Return::Nothing));
            match kind {
                // A `send` is done with; a `call` now waits for its reply, off the queue.
                MsgKind::Send => wake(ss, mm, spid, stid, Ok(Return::Nothing)),
                MsgKind::Call => {
                    unqueue(mm, e, spid, stid);
                    set_tword(mm, spid, stid, W_WAIT, Wait::Reply as u64);
                }
            }
        }
    }
}

/// Whether `(pid, tid)`'s `receive` record is still where it can be written. `InvalidArgument`
/// is the error a record earns, and every call's row carries it (decoding, stage 1).
fn check_receive_record(ss: &mut ProcessTable, pid: Pid, tid: TID, mm: &MemoryManager) -> Result<(), Error> {
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
    ss: &ProcessTable,
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
    // R4: handles that would take the receiver past `MAX_HANDLES` refuse the message, like any
    // other cost it cannot pay.
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
        // thread's current call (R21).
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
        // While its caller waits, it is on its endpoint's open list and its stamp's chain (R10).
        let w = &mut Frames(mm);
        List::open(e.frame).push_front(w, cref(frame));
        List::taken(m.stamp.frame).push_front(w, cref(frame));
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
    ss: &ProcessTable,
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
fn reserve_buffer(ss: &ProcessTable, mm: &mut MemoryManager, rpid: Pid, at: usize, pages: usize) {
    let space = ss.mapping_of(rpid).expect("the receiving process is alive");
    for i in 0..pages {
        crate::arch::mem::prepare_map(mm, &space, rpid, at + i * PAGE_SIZE)
            .expect("R4: the page tables were counted and charged for just above");
    }
}

/// Map the buffer into the receiver: a lend stays the sender's (whose entry remembers the loan),
/// a transfer changes owner and payer together. Every page was prepared, so nothing fails.
fn move_buffer(ss: &ProcessTable, mm: &mut MemoryManager, m: &Msg, spid: Pid, rpid: Pid, at: usize) {
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
    // Only now, with every page mapped: a self-send's receiving tables are the sender's, and one
    // prepared but not yet filled would look empty.
    if m.kind == MsgKind::Send {
        let end = m.buf_addr + m.buf_pages * PAGE_SIZE;
        crate::arch::mem::free_empty_tables(mm, &sender_space, m.buf_addr, end);
    }
}

/// Put a queued message's buffer back in its sender's address space.
fn give_buffer_back(ss: &ProcessTable, mm: &MemoryManager, pid: Pid, tid: TID) {
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
    ss: &mut ProcessTable,
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
        // A notice owed here waits on a live endpoint: a destruction took its notices with it
        // (`budgets_dying`).
        debug_assert!(
            call.flags & F_NOTICE == 0 || mm.is_live_endpoint(call.endpoint),
            "I15: an abandoned call owes a notice on a destroyed endpoint"
        );
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
    // `OutOfMemory` either way, the reply still delivered.
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
fn return_lend(ss: &ProcessTable, mm: &mut MemoryManager, call: &OpenCall) {
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
    let end = call.lend_server + call.lend_pages * PAGE_SIZE;
    crate::arch::mem::free_empty_tables(mm, &server, call.lend_server, end);
}

/// The lend of an abandoned call: its pages are the server's alone, so replying frees them.
fn free_abandoned_lend(ss: &ProcessTable, mm: &mut MemoryManager, call: &OpenCall) {
    if call.lend_pages == 0 {
        return;
    }
    let space = ss.mapping_of(call.server.0).expect("an abandoned call's server is still alive");
    for i in 0..call.lend_pages {
        let phys = crate::arch::mem::unmap_from(&space, call.lend_server + i * PAGE_SIZE)
            .expect("an abandoned call retains its protected borrower alias");
        mm.free_frame_of(phys, call.server.0).expect("an abandoned lend's frame remains owned by its server");
    }
    let end = call.lend_server + call.lend_pages * PAGE_SIZE;
    crate::arch::mem::free_empty_tables(mm, &space, call.lend_server, end);
}

/// Free an open call's page and the charges it carried (R4a).
fn close_call(mm: &mut MemoryManager, frame: u32, call: &OpenCall) {
    unlist_call(mm, frame, call);
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
/// holding the call is owed a notice (I15).
fn abandon(ss: &ProcessTable, mm: &mut MemoryManager, frame: u32) {
    let mut call = open_call_at(mm, frame);
    if call.flags & F_WAITING == 0 {
        return;
    }
    // No notice is owed on an endpoint that is being destroyed: nobody is left to receive it on
    // (R3), and the destruction drops the ones owed before it began (`budgets_dying`).
    let notice = if mm.budget_at(mm.endpoint_at(call.endpoint).owner).dying { 0 } else { F_NOTICE };
    unlist_call(mm, frame, &call);
    call.flags = (call.flags & !F_WAITING) | F_ABANDONED | notice;
    store_open_call(mm, frame, &call);
    if notice != 0 {
        List::notices(call.endpoint.frame).push_front(&mut Frames(mm), cref(frame));
    }
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
    let end = call.lend_caller + call.lend_pages * PAGE_SIZE;
    crate::arch::mem::free_empty_tables(mm, &space, call.lend_caller, end);
}

// --- Teardown: R4b, R10, and timeouts --------------------------------------------------------------

/// A thread is ending. What it waited for is withdrawn; a caller still waiting on a call it
/// holds gets `Dead` and its lend back, and an abandoned lend is freed (R4b).
pub fn thread_ending(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid, tid: TID) {
    if let Some(e) = end_thread(ss, mm, pid, tid) {
        // A server may be able to take calls again now that this one is gone.
        pump(ss, mm, e);
    }
}

/// A thread's teardown, with no pump: returns the endpoint its wait served, for the caller to
/// pump once it may.
fn end_thread(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid, tid: TID) -> Option<EndpointRef> {
    // Not `fail_wait`: a dying thread gets no answer and must not go back on the ready list.
    let served = unwind(ss, mm, pid, tid);
    unlist(mm, pid, tid);
    set_tword(mm, pid, tid, W_WAIT, Wait::None as u64);
    // R4b: every call it holds open ends, its caller told.
    while slot(mm, pid, tid).ncalls > 0 {
        let frame = nth_call(mm, pid, tid, 0);
        drop_open_call(mm, pid, tid, frame);
        finish_served(ss, mm, frame);
    }
    served
}

/// A served call's server is gone: a waiting caller gets `Dead` and its lend back; an abandoned
/// lend is freed (R4b).
fn finish_served(ss: &mut ProcessTable, mm: &mut MemoryManager, frame: u32) {
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

/// A process is ending: every one of its threads does (R4b). No endpoint is pumped until the last
/// has ended, so none of them can take a call on the way out; then each endpoint their waits
/// served is pumped once.
pub fn process_ending(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid) {
    #[cfg(feature = "sched-trace")]
    let _threads = crate::sched::trace::threads();
    let n = SERVED.with(|served| {
        let mut n = 0;
        for tid in mm.live_tids(pid) {
            match end_thread(ss, mm, pid, tid) {
                Some(e) if !served[..n].contains(&e) => {
                    served[n] = e;
                    n += 1;
                }
                _ => {}
            }
        }
        n
    });
    for i in 0..n {
        let e = SERVED.with(|served| served[i]);
        pump(ss, mm, e);
    }
}

/// `process_ending`'s endpoints to pump, one per thread at most: a static, not 4 KiB of the 32 KiB
/// kernel stack. All zeros, so `.bss`.
static SERVED: KernelCell<[EndpointRef; MAX_THREADS]> =
    KernelCell::new([EndpointRef { frame: 0, id: 0 }; MAX_THREADS]);

/// R10, the part that reaches messages: after `budget_destroy` marked a subtree dying and before
/// its budgets are freed, destroy the devices it owns and fail every message sent through a handle
/// stamped with it or waiting on an endpoint it owns. The subtree is walked through the budgets'
/// child links, and each budget's own devices through its owner lists: never a scan of every
/// object frame ([Residual risks](../kernel/budgets.md#residual-risks)). Its endpoints are freed
/// later, once no handle names them (`MemoryManager::free_owned_endpoints`).
pub fn budgets_dying(ss: &mut ProcessTable, mm: &mut MemoryManager, top: BudgetFrame) {
    // One pre-order walk of the dying subtree, and each budget's one owner chain, endpoint and
    // device alike, reading each object's link and kind words alone: the walk ends exactly what
    // the subtree owns, never a scan of every frame. A destroyed device leaves the chain, so
    // only endpoints are left on it; it is moved to the chain's head first, so leaving it does
    // not walk the endpoints ahead of it. Each endpoint's message reach is its own lists
    // (`endpoint_dying`); one that nothing waits on costs one read, its count of members.
    let mut cur = Some(top);
    while let Some(frame) = cur {
        let (mut owned, mut prev) = (mm.budget(frame).first_owned, None);
        while let Some(o) = owned {
            owned = mm.owned_next(o);
            if mm.is_endpoint_frame(o) {
                prev = Some(o);
                if lists::waiting(&Frames(mm), o) != 0 {
                    endpoint_dying(ss, mm, o);
                }
            } else {
                mm.owned_to_head(frame, prev, o);
                destroy_device(ss, mm, o);
            }
        }
        cur = mm.subtree_next(top, frame);
    }
    // Then what was sent through a handle stamped with a dying budget, on any endpoint: every
    // dying budget's queued messages first, so that no pump the failed callers below make takes
    // one, then the taken calls whose callers wait, each caller failed and its call abandoned.
    let mut cur = Some(top);
    while let Some(frame) = cur {
        fail_each(ss, mm, List::queued(frame));
        cur = mm.subtree_next(top, frame);
    }
    let mut cur = Some(top);
    while let Some(frame) = cur {
        fail_callers(ss, mm, List::taken(frame));
        cur = mm.subtree_next(top, frame);
    }
}

/// A DMA device whose reset did not confirm is destroyed as R10 destroys one (kernel/devices.md,
/// "Quarantine"), so every handle to it goes, copies in unreceived messages arriving as 0. Its
/// registry slot, keyed by base, stays flagged until reboot, and no device object is ever made
/// again.
pub fn destroy_quarantined_devices(ss: &mut ProcessTable, mm: &mut MemoryManager) {
    if !mm.dma_take_doomed() {
        return;
    }
    // At most one object per DMA slot (`dma::MAX_DMA_DEVICES`), collected before any is
    // destroyed. A destroyed device is skipped: its frame is still an object one (deferred), but
    // it is already on `Objects::deferred`, and destroying it again would cycle that list.
    let mut doomed = [0u32; crate::dma::MAX_DMA_DEVICES];
    let mut count = 0;
    for frame in 0..=mm.objects.high_frame {
        if count == doomed.len() {
            break;
        }
        if mm.is_device_frame(frame) {
            let d = mm.device(frame);
            if d.kind == crate::device::Kind::Mmio && !d.destroyed && mm.dma_quarantined(d.base) {
                doomed[count] = frame;
                count += 1;
            }
        }
    }
    for &frame in &doomed[..count] {
        // During a destruction, `destroy_device`'s own sweep is folded into the one pass that
        // keys on the owner, and this device's owner is `system`, not the dying budget: close
        // its handles here instead, before the frame it names is freed (I1, I2). Outside a
        // destruction `destroy_device` closes them itself.
        if mm.objects.deferring {
            let r = DeviceRef { frame, id: mm.device(frame).id };
            mm.sweep_handles(|_, h| matches!(h.object, Object::Device(x) if x == r));
        }
        destroy_device(ss, mm, frame);
    }
}

/// R10 step 4 for one dying endpoint `e`, from its own lists: its receivers and its queued
/// senders fail with `Dead`, the notices owed on it are dropped (there is no endpoint left to
/// receive one on, R3, and no notice may name a frame about to be freed, I1), and the callers
/// waiting for a reply through it fail with `Dead`, their calls abandoned with no notice
/// (`abandon` owes none on an endpoint whose owner is dying). A failed caller's pump of `e` finds
/// no receiver there, and no other endpoint is pumped. Then the exit notices owed on it are
/// dropped and the processes reporting to it lose their ear (`process.rs`).
fn endpoint_dying(ss: &mut ProcessTable, mm: &mut MemoryManager, e: u32) {
    fail_each(ss, mm, List::receivers(e));
    loop {
        let sender = lists::pick(&Frames(mm), e, true);
        if sender == 0 {
            break;
        }
        fail_one(ss, mm, sender);
    }
    loop {
        let c = List::notices(e).first(&Frames(mm));
        if c == 0 {
            break;
        }
        let mut call = open_call_at(mm, call_of(c));
        unlist_call(mm, call_of(c), &call);
        call.flags &= !F_NOTICE;
        store_open_call(mm, call_of(c), &call);
    }
    fail_callers(ss, mm, List::open(e));
    crate::process::endpoint_dying(mm, e);
}

/// Fail every thread on `list` with `Dead`, the first first, until it is empty: each leaves the
/// list as it is answered (`wake`).
fn fail_each(ss: &mut ProcessTable, mm: &mut MemoryManager, list: List) {
    loop {
        let r = list.first(&Frames(mm));
        if r == 0 {
            return;
        }
        fail_one(ss, mm, r);
    }
}

/// Fail the caller of every open call on `list` with `Dead`, the first first, until it is empty:
/// each call is abandoned and leaves the list (`abandon`).
fn fail_callers(ss: &mut ProcessTable, mm: &mut MemoryManager, list: List) {
    loop {
        let c = list.first(&Frames(mm));
        if c == 0 {
            return;
        }
        let (pid, tid) = open_call_at(mm, call_of(c)).caller;
        fail_one(ss, mm, tref(pid, tid));
    }
}

/// Fail the waiting thread `r` names with `Dead` (R10). It leaves every list it waited on, so a
/// loop over a list's first member ends.
fn fail_one(ss: &mut ProcessTable, mm: &mut MemoryManager, r: u64) {
    let (pid, tid) = thread_of(r);
    assert!(slot(mm, pid, tid).wait != Wait::None, "I1: a list names a thread that waits for nothing");
    fail_wait(ss, mm, pid, tid, Error::Dead);
}

/// What a walk for timeouts found.
pub struct Timeouts {
    /// The timeout due first at `now`: the earliest deadline at or before `now` (at an equal
    /// deadline, the first in (pid, tid) order).
    pub due: Option<(u64, Pid, TID)>,
    /// The earliest deadline still to come (`u64::MAX` for none).
    pub next: u64,
    /// The last process whose cached earliest timeout had come with none of its threads due: a
    /// wait that ended before its timeout left the timer early, and this walk is its
    /// (kernel/timer.md, "R12 (scheduling) for timer work").
    pub stale: Option<Pid>,
}

/// I13: walk for timeouts at `now` ([`Timeouts`]). Only the threads of processes whose cached
/// earliest timeout has come are read; each such cache is recomputed on the way.
pub fn next_timeout(mm: &mut MemoryManager, now: u64) -> Timeouts {
    let mut due: Option<(u64, Pid, TID)> = None;
    let mut next = u64::MAX;
    let mut stale = None;
    for pid in mm.live_pids() {
        let Some(earliest) = mm.account(pid).map(|a| a.earliest_timeout) else { continue };
        if earliest > now {
            next = next.min(earliest);
            continue;
        }
        let mut exact = u64::MAX;
        let mut found = false;
        for tid in mm.live_tids(pid) {
            if Wait::from_word(tword(mm, pid, tid, W_WAIT)) == Wait::None {
                continue;
            }
            let deadline = tword(mm, pid, tid, W_DEADLINE);
            if deadline == u64::MAX {
                continue;
            }
            exact = exact.min(deadline);
            if deadline <= now {
                found = true;
                if due.is_none_or(|(d, _, _)| deadline < d) {
                    due = Some((deadline, pid, tid));
                }
            } else {
                next = next.min(deadline);
            }
        }
        if !found {
            stale = Some(pid);
        }
        if let Some(a) = mm.account_mut(pid) {
            a.earliest_timeout = exact;
        }
    }
    Timeouts { due, next, stale }
}

/// The blocking call of `(pid, tid)` reached its timeout: it returns `Timeout` (I13), with what
/// it waited for unwound (a queued message's buffer back, a taken call abandoned).
pub fn time_out(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid, tid: TID) {
    fail_wait(ss, mm, pid, tid, Error::Timeout);
}

/// After `reply` freed an open call, the process may be able to take calls again (R4a) and an
/// abandoned-call notice may be waiting: try every endpoint its threads receive on.
fn poke_receivers(ss: &mut ProcessTable, mm: &mut MemoryManager, pid: Pid) {
    for tid in mm.live_tids(pid) {
        let s = slot(mm, pid, tid);
        if s.wait == Wait::Receive {
            if let Some(e) = s.endpoint {
                pump(ss, mm, e);
            }
        }
    }
}

// --- The checked build's audit -----------------------------------------------------------------------

/// The checked build's audit of every IPC list from the threads ([`check_lists`]), if one changed
/// since the last, at the end of each outermost kernel entry that can change one: a system call
/// (`redoubt::handle`), an expiry (`time::expire_due`) and an interrupt's delivery
/// (`device::irq_fired`). Never inside a destruction or a pump, which nest in those. An entry that
/// ends otherwise (a fault that kills) leaves it to the next. Like every audit, it neither moves the
/// schedule nor counts in a latency target (`sched::audit`). The full audit, from the objects, runs
/// at the full-audit points ([`check_all`]).
pub fn audit(mm: &MemoryManager) {
    #[cfg(debug_assertions)]
    if CHANGED.with(core::mem::take) {
        crate::sched::audit(crate::sched::AUDIT_IPC_LISTS, || {
            check_lists(mm);
        });
    }
    #[cfg(not(debug_assertions))]
    let _ = mm;
}

/// What the lists hold, or the threads say they must.
#[cfg(debug_assertions)]
#[derive(Default, PartialEq, Eq, Debug)]
struct Listed {
    receivers: usize,
    irq: usize,
    queued: usize,
    stamped: usize,
    waiting: usize,
    taken: usize,
    notices: usize,
    exits: usize,
    reporters: usize,
}

/// The checked build's full audit of the lists, at its full-audit points: after each destruction,
/// at each process-object free and before the hart idles. [`check_lists`], and then every list
/// from the objects that head it, with no scan of the frames: the budget tree from its root, each
/// budget's two stamp chains and its owner chain's endpoints and devices. Those must hold what the
/// threads say, so a list none of whose members waits, which a teardown that failed to unlink would
/// leave, is found here. An endpoint whose list words are all 0 is skipped, as a destruction skips
/// it; one with any other has its lists audited whole, so a stray head or tail fails.
#[cfg(debug_assertions)]
pub fn check_all(mm: &MemoryManager) {
    let want = check_lists(mm);
    let all = enumerate_lists(mm);
    assert!(all == want, "I1: the objects' lists hold {:?}, the threads {:?}", all, want);
}

/// The checked build's audit of the lists at an exit that changed one ([`audit`]). One walk of
/// every thread and its open calls, and of every process object, counts what must be listed, and
/// audits each list whole from the member at its head (`redoubt-ipclist`: its links, its order, and
/// every member's own words); each kind of list must then hold exactly what the threads and process
/// objects say. It walks no budget, so its cost follows the threads. Returns what the threads and
/// process objects say.
#[cfg(debug_assertions)]
fn check_lists(mm: &MemoryManager) -> Listed {
    let w = &Frames(mm);
    let mut listed = Listed::default();
    let mut want = Listed::default();
    for pid in mm.live_pids() {
        for tid in mm.live_tids(pid) {
            let Some(phys) = thread_phys(mm, pid, tid) else { continue };
            let me = tref(pid, tid);
            let on = frame_of(kframe::read(phys, W_OBJECT * 8)).unwrap_or(0);
            match Wait::from_word(kframe::read(phys, W_WAIT * 8)) {
                Wait::Receive => {
                    want.receivers += 1;
                    if List::receivers(on).first(w) == me {
                        listed.receivers += audit_receivers(mm, on);
                    }
                }
                Wait::Irq => {
                    want.irq += 1;
                    if List::irq_waiters(on).first(w) == me {
                        listed.irq += audit_irq(mm, on);
                    }
                }
                Wait::Send => {
                    want.queued += 1;
                    want.stamped += 1;
                    if lists::first_group(w, on) == me {
                        listed.queued += audit_queue(mm, on);
                    }
                    let stamp = frame_of(kframe::read(phys, W_STAMP * 8)).unwrap_or(0);
                    if List::queued(stamp).first(w) == me {
                        listed.stamped += audit_stamped(mm, stamp);
                    }
                }
                _ => {}
            }
            let ncalls = (kframe::read(phys, W_NCALLS * 8) as usize).min(MAX_OPEN_CALLS);
            for i in 0..ncalls {
                let frame = kframe::read(phys, (W_CALLS + i) * 8) as u32;
                let call = mm.object_phys(frame);
                let flags = kframe::read(call, C_FLAGS * 8);
                let e = frame_of(kframe::read(call, C_ENDPOINT * 8)).unwrap_or(0);
                let c = cref(frame);
                if flags & F_WAITING != 0 {
                    want.waiting += 1;
                    want.taken += 1;
                    if List::open(e).first(w) == c {
                        listed.waiting += audit_calls(mm, List::open(e), F_WAITING, e, None);
                    }
                    let stamp = frame_of(kframe::read(call, C_STAMP * 8)).unwrap_or(0);
                    if List::taken(stamp).first(w) == c {
                        listed.taken += audit_calls(mm, List::taken(stamp), F_WAITING, 0, Some(stamp));
                    }
                }
                if flags & F_NOTICE != 0 {
                    want.notices += 1;
                    if List::notices(e).first(w) == c {
                        listed.notices += audit_calls(mm, List::notices(e), F_NOTICE, e, None);
                    }
                }
            }
        }
    }
    // And every process object naming an exit endpoint: on its exits or its reporters.
    for frame in mm.process_frames() {
        let p = mm.process(frame);
        let Some(e) = p.endpoint else { continue };
        let (list, held, n) = if p.notice_queued() {
            (List::exits(e.frame), &mut listed.exits, &mut want.exits)
        } else {
            (List::reporters(e.frame), &mut listed.reporters, &mut want.reporters)
        };
        *n += 1;
        if list.first(w) == frame_word(frame) {
            *held += audit_processes(mm, list, e.frame);
        }
    }
    assert!(listed == want, "I1: the IPC lists hold {:?}, the threads and processes {:?}", listed, want);
    want
}

/// Every list, from the objects that head it ([`check_all`]).
#[cfg(debug_assertions)]
fn enumerate_lists(mm: &MemoryManager) -> Listed {
    let mut all = Listed::default();
    // Every budget is below `root`, found from any process's budget.
    let root = mm.live_pids().find_map(|pid| mm.budget_of(pid)).map(|mut b| {
        while let Some(parent) = mm.budget(b).parent {
            b = parent;
        }
        b
    });
    let mut cur = root;
    while let Some(b) = cur {
        all.stamped += audit_stamped(mm, b);
        all.taken += audit_calls(mm, List::taken(b), F_WAITING, 0, Some(b));
        let mut owned = mm.budget(b).first_owned;
        while let Some(o) = owned {
            owned = mm.owned_next(o);
            if !mm.is_endpoint_frame(o) {
                all.irq += audit_irq(mm, o);
                continue;
            }
            // A destruction skips an endpoint whose count is 0, so that must mean every list
            // word is 0; and the count is exactly what its lists hold.
            let count = lists::waiting(&Frames(mm), o) as usize;
            if count == 0 {
                assert!(lists_empty(mm, o, true), "I1: endpoint {} counts no member but has a list", o);
                continue;
            }
            let held = [
                audit_receivers(mm, o),
                audit_queue(mm, o),
                audit_calls(mm, List::notices(o), F_NOTICE, o, None),
                audit_calls(mm, List::open(o), F_WAITING, o, None),
                audit_processes(mm, List::exits(o), o),
                audit_processes(mm, List::reporters(o), o),
            ];
            assert!(held.iter().sum::<usize>() == count, "I1: endpoint {} miscounts its members", o);
            all.receivers += held[0];
            all.queued += held[1];
            all.notices += held[2];
            all.waiting += held[3];
            all.exits += held[4];
            all.reporters += held[5];
        }
        cur = root.and_then(|root| mm.subtree_next(root, b));
    }
    all
}

/// Word `i` of the page of the thread a list names, read from its page alone.
#[cfg(debug_assertions)]
fn member_word(mm: &MemoryManager, r: u64, i: usize) -> u64 {
    let (pid, tid) = thread_of(r);
    thread_phys(mm, pid, tid).map_or(0, |phys| kframe::read(phys, i * 8))
}

/// Whether the thread `r` names waits as `wait` on object frame `on`.
#[cfg(debug_assertions)]
fn waits_on(mm: &MemoryManager, r: u64, wait: Wait, on: u32) -> bool {
    member_word(mm, r, W_WAIT) == wait as u64 && frame_of(member_word(mm, r, W_OBJECT)) == Some(on)
}

/// An audit's verdict on one member.
#[cfg(debug_assertions)]
fn member(good: bool, page: Page) -> Result<(), lists::Fault> {
    if good { Ok(()) } else { Err(lists::Fault::Member(page)) }
}

#[cfg(debug_assertions)]
fn audit_failed(fault: lists::Fault) -> ! { panic!("I1: the IPC lists: {:?}", fault) }

/// Endpoint `e`'s receivers: each in `receive` there, in the order they began to wait.
#[cfg(debug_assertions)]
fn audit_receivers(mm: &MemoryManager, e: u32) -> usize {
    let mut last = 0;
    List::receivers(e)
        .audit(&Frames(mm), |w, r| {
            let arrived = w.read(Page::Thread(r), lists::T_SEQ);
            let after = arrived > last;
            last = arrived;
            member(waits_on(mm, r, Wait::Receive, e), Page::Thread(r))?;
            if after { Ok(()) } else { Err(lists::Fault::Order(Page::Thread(r))) }
        })
        .unwrap_or_else(|f| audit_failed(f))
}

/// Device `d`'s interrupt waiters: each in `receive` on it.
#[cfg(debug_assertions)]
fn audit_irq(mm: &MemoryManager, d: u32) -> usize {
    List::irq_waiters(d)
        .audit(&Frames(mm), |_, r| member(waits_on(mm, r, Wait::Irq, d), Page::Thread(r)))
        .unwrap_or_else(|f| audit_failed(f))
}

/// Endpoint `e`'s queue, R2's groups: each member sending there, as the kind its chain says.
#[cfg(debug_assertions)]
fn audit_queue(mm: &MemoryManager, e: u32) -> usize {
    lists::audit_groups(&Frames(mm), e, |_, r, send| {
        let kind = member_word(mm, r, W_KIND) == MsgKind::Send as u64;
        member(waits_on(mm, r, Wait::Send, e) && kind == send, Page::Thread(r))
    })
    .unwrap_or_else(|f| audit_failed(f))
}

/// Budget `b`'s chain of queued messages: each queued, under its stamp.
#[cfg(debug_assertions)]
fn audit_stamped(mm: &MemoryManager, b: u32) -> usize {
    List::queued(b)
        .audit(&Frames(mm), |_, r| {
            let queued = member_word(mm, r, W_WAIT) == Wait::Send as u64;
            member(queued && frame_of(member_word(mm, r, W_STAMP)) == Some(b), Page::Thread(r))
        })
        .unwrap_or_else(|f| audit_failed(f))
}

/// Endpoint `e`'s exits or reporters: each a process object naming `e`, its notice queued on the
/// exits and not on the reporters; one on the exits is no longer alive (one on the reporters may be
/// ending, its notice not yet settled).
#[cfg(debug_assertions)]
fn audit_processes(mm: &MemoryManager, list: List, e: u32) -> usize {
    let exits = list == List::exits(e);
    list.audit(&Frames(mm), |_, r| {
        let p = mm.process(object_of(r));
        let at = p.endpoint.map(|x| x.frame) == Some(e);
        member(at && p.notice_queued() == exits && !(exits && p.alive()), Page::Process(r))
    })
    .unwrap_or_else(|f| audit_failed(f))
}

/// A list of open calls: each with `flag`, on endpoint `e` (an endpoint's list) or under stamp
/// `stamp` (a budget's chain).
#[cfg(debug_assertions)]
fn audit_calls(mm: &MemoryManager, list: List, flag: u64, e: u32, stamp: Option<u32>) -> usize {
    list.audit(&Frames(mm), |_, c| {
        let call = open_call_at(mm, call_of(c));
        let at = match stamp {
            Some(b) => call.stamp.frame == b,
            None => call.endpoint.frame == e,
        };
        member(call.flags & flag != 0 && at, Page::Call(c))
    })
    .unwrap_or_else(|f| audit_failed(f))
}
