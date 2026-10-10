// SPDX-License-Identifier: MIT OR Apache-2.0

//! Frames in flight (kernel/memory.md, R81): a freed RAM frame is zeroed outside the kernel lock
//! by the hart that freed it when that hart idles, or under the lock by an allocation that finds
//! the bitmap empty, and enters the free-frame bitmap only zero.
//!
//! A frame is retired under the lock (`MemoryManager::retire`): its owner becomes
//! [`crate::mem::IN_FLIGHT`], so no call names it and no allocation takes it. Then, per hart:
//!
//! 1. **Staged**, while another hart may still hold a translation to it: its entry was just cleared and the
//!    call's shootdown is still to come (`unmap`, an abandoned lend). Its index is kept here, never in the
//!    frame, since a stale store on that hart could rewrite the frame. The shootdown links the process's
//!    staged frames ([`shot`]); a full stage shoots early.
//! 2. **Pending**, once no hart can store to it: linked through the frame's word [`LINK`]. Read and written
//!    only under the lock: filled by its hart, and emptied one frame at a time, by its hart when every hart
//!    is idle ([`take_one`]), or by the lock's holder when an allocation finds the bitmap empty
//!    ([`pop_pending`]).
//! 3. **Zeroing**: the one frame an idle hart took, which it alone holds while it zeroes it outside the lock
//!    ([`zero_taken`]).
//! 4. **Done**: zeroed, all but the link word, which links the hart's done list. The lock's next holder, any
//!    hart, takes the list ([`take_done`]), clears the link and gives the frame to the bitmap
//!    (`MemoryManager::commit`).
//!
//! Each hart touches its own stage and zeroing frame only; a done list has one pusher, its hart, and one
//! taker, the lock's holder, which takes it whole, so it needs no more than a swap. So every frame
//! in flight is in exactly one place, and the checked build counts them ([`counted`]).

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use redoubt_layout::{MAX_HARTS, Pid};
use redoubt_sys::PAGE_SIZE;

/// The byte offset of the word that links a frame in flight: the word a destruction links its
/// deferred object frames through (`budget::DEFER_WORD`), in the same form (index plus one, 0 for
/// none), so that chain goes onto the pending list whole ([`pend_chain`]).
pub const LINK: usize = crate::budget::DEFER_WORD * 8;

/// The frames one hart may stage before it shoots their process down early: a bound on what a
/// call frees ahead of its shootdown, not on what it frees.
pub const STAGE_MAX: usize = 64;

/// One hart's frames in flight. A frame is named by its index in RAM's ownership table plus one,
/// 0 for none, here and in the words that link the lists.
struct Lists {
    /// The staged frames, `staged_n` of them, all to be shot down in `staged_pid`'s space.
    staged: [AtomicU32; STAGE_MAX],
    staged_n: AtomicUsize,
    staged_pid: AtomicUsize,
    /// The pending list's first frame. Under the lock.
    pending: AtomicU32,
    /// The frame this hart is zeroing idle, outside the lock ([`zero_taken`]), 0 for none.
    zeroing: AtomicU32,
    /// The done list's first frame.
    done: AtomicU32,
    /// The ticks and frames of zeroing since the hart's last entry, for its bill ([`take_zeroed`]):
    /// a word, which on rv32 holds 429 s of ticks, far beyond one hart's zeroing between entries.
    zeroed_ticks: AtomicUsize,
    zeroed_frames: AtomicU32,
}

impl Lists {
    const fn new() -> Self {
        Lists {
            staged: [const { AtomicU32::new(0) }; STAGE_MAX],
            staged_n: AtomicUsize::new(0),
            staged_pid: AtomicUsize::new(0),
            pending: AtomicU32::new(0),
            zeroing: AtomicU32::new(0),
            done: AtomicU32::new(0),
            zeroed_ticks: AtomicUsize::new(0),
            zeroed_frames: AtomicU32::new(0),
        }
    }
}

static LISTS: [Lists; MAX_HARTS] = [const { Lists::new() }; MAX_HARTS];

/// The boot index plus one of the lock's holder waiting for a done frame ([`wait_done`]), 0 for
/// none: a zeroing hart wakes it when it finishes a frame.
static WANT: AtomicUsize = AtomicUsize::new(0);

/// The boot index plus one of the hart draining its pending list idle ([`take_one`]), 0 for none.
/// One hart at a time: each frame costs its drainer a release and a take of the lock, and two
/// drainers would find it held each time, each take a halt and each release the other's
/// interrupt (`hart::wake_halted`), some 33,000 at rest after a login. Alone, a drainer finds the
/// lock free. Under the lock.
static DRAINER: AtomicUsize = AtomicUsize::new(0);

fn here() -> &'static Lists { &LISTS[crate::arch::hart::index()] }

fn phys(frame: u32) -> usize { crate::arch::process::ram_start() + (frame as usize - 1) * PAGE_SIZE }

/// Frame `index` was retired, and its entry in `pid`'s space was just cleared: kept until `pid`
/// is shot down. Under the lock.
pub fn stage(index: usize, pid: Pid) {
    let lists = here();
    let n = lists.staged_n.load(Ordering::Relaxed);
    let staged_pid = lists.staged_pid.load(Ordering::Relaxed);
    if n == STAGE_MAX || (n > 0 && staged_pid != usize::from(pid.get())) {
        crate::mem::shoot(Pid::new(staged_pid as u16).expect("a staged frame's process"));
    }
    let n = lists.staged_n.load(Ordering::Relaxed);
    lists.staged[n].store(index as u32 + 1, Ordering::Relaxed);
    lists.staged_n.store(n + 1, Ordering::Relaxed);
    lists.staged_pid.store(usize::from(pid.get()), Ordering::Relaxed);
}

/// `pid` was just shot down on every hart that ran it (`hart::shootdown` returned, every hart it
/// asked having acknowledged): its staged frames can hold no store any more, and go on the
/// pending list. Under the lock.
pub fn shot(pid: Pid) {
    let lists = here();
    if lists.staged_pid.load(Ordering::Relaxed) != usize::from(pid.get()) {
        return;
    }
    for i in 0..lists.staged_n.swap(0, Ordering::Relaxed) {
        pend(lists.staged[i].load(Ordering::Relaxed) as usize - 1);
    }
}

/// The section is ending: anything still staged is shot down now. The paths that stage shoot
/// their process before they return, so this finds nothing unless one did not (a rollback whose
/// process never ran). Under the lock, before its release.
pub fn end_section() {
    let lists = here();
    if lists.staged_n.load(Ordering::Relaxed) > 0 {
        let pid = lists.staged_pid.load(Ordering::Relaxed) as u16;
        crate::mem::shoot(Pid::new(pid).expect("a staged frame's process"));
    }
}

/// Frame `index`, retired, can hold no store from any hart: onto this hart's pending list, linked
/// through its word [`LINK`]. Under the lock.
pub fn pend(index: usize) {
    let lists = here();
    let frame = index as u32 + 1;
    crate::kframe::write(phys(frame), LINK, u64::from(lists.pending.load(Ordering::SeqCst)));
    lists.pending.store(frame, Ordering::SeqCst);
    #[cfg(feature = "inflight-trace")]
    crate::sched::trace::record(crate::sched::trace::PENDED, index as u64, 0);
}

/// The frames from `first` to `last`, retired, linked through [`LINK`] already and holding no
/// store from any hart (a destruction's object frames, never mapped): onto this hart's pending
/// list whole, one write for the chain. Under the lock.
pub fn pend_chain(first: usize, last: usize) {
    let lists = here();
    crate::kframe::write(phys(last as u32 + 1), LINK, u64::from(lists.pending.load(Ordering::SeqCst)));
    lists.pending.store(first as u32 + 1, Ordering::SeqCst);
}

/// Take one frame off a pending list, to zero under the lock: an allocation that found the
/// bitmap empty ([`crate::mem::MemoryManager`]'s wait). This hart's own first, then any other
/// hart's. The frame's index, and the hart whose it was.
pub fn pop_pending() -> Option<(usize, usize)> {
    let me = crate::arch::hart::index();
    let started = crate::arch::hart::started();
    for hart in core::iter::once(me).chain((0..started).filter(|h| *h != me)) {
        let lists = &LISTS[hart];
        let frame = lists.pending.load(Ordering::Relaxed);
        if frame != 0 {
            lists.pending.store(crate::kframe::read(phys(frame), LINK) as u32, Ordering::Relaxed);
            return Some((frame as usize - 1, hart));
        }
    }
    None
}

/// This hart is about to idle: if every started hart is idle (`all_idle`) and no other hart is
/// draining, take the first frame off its pending list, to zero once it gives up the lock
/// ([`zero_taken`]). One frame, so an interrupt that comes meanwhile waits that frame's zeroing at
/// most, about 30 µs under `icount`, before the hart goes back through `kmain`'s interrupt-enabled
/// top. The frame (0 for none); whether more are pending, so that the hart comes back for the next
/// ([`DRAINER`]); and the hart to wake once the lock is given up (a mask, 0 for none). Under the
/// lock.
pub fn take_one(all_idle: bool) -> (u32, bool, usize) {
    let (me, drainer) = (crate::arch::hart::index(), DRAINER.load(Ordering::Relaxed));
    let lists = here();
    let free = all_idle && (drainer == 0 || drainer == me + 1);
    let frame = if free { lists.pending.load(Ordering::Relaxed) } else { 0 };
    if frame != 0 {
        lists.pending.store(crate::kframe::read(phys(frame), LINK) as u32, Ordering::Relaxed);
        lists.zeroing.store(frame, Ordering::SeqCst);
    }
    if frame != 0 && lists.pending.load(Ordering::Relaxed) != 0 {
        DRAINER.store(me + 1, Ordering::Relaxed);
        return (frame, true, 0);
    }
    // No hart drains now: this one's list is empty, or a hart woke to work and whoever idles last
    // starts the next drain. With every hart idle, the next hart with frames pending, halted, is
    // woken to drain its own: handed on by the hart whose drain ended, or by one with none of its
    // own, for a hart that halted with frames while another worked. It is woken after the release
    // (`arch::idle`), so it finds the lock free; a wake that comes before its halt ends the halt.
    if drainer == me + 1 {
        DRAINER.store(0, Ordering::Relaxed);
    }
    let pending = |h: &usize| *h != me && LISTS[*h].pending.load(Ordering::Relaxed) != 0;
    let next = (0..crate::arch::hart::started()).find(pending).filter(|_| free);
    (frame, false, next.map_or(0, |h| 1 << h))
}

/// Zero `frame`, idle, the kernel lock given up, and push it onto the done list; an allocation
/// waiting for a frame ([`wait_done`]) is woken by the push. Counted done before it stops being
/// this hart's, so the checked build's count never misses it.
pub fn zero_taken(frame: u32) {
    let lists = here();
    let started = riscv::register::time::read64();
    let at = phys(frame);
    crate::kframe::zero(at);
    // Onto the done list: its link, then the head, which the lock's holder may take at any time (a
    // swap); Release, so the taker sees the zeroes and the link.
    let mut head = lists.done.load(Ordering::Relaxed);
    loop {
        crate::kframe::write(at, LINK, u64::from(head));
        match lists.done.compare_exchange_weak(head, frame, Ordering::SeqCst, Ordering::Relaxed) {
            Ok(_) => break,
            Err(now) => head = now,
        }
    }
    lists.zeroing.store(0, Ordering::SeqCst);
    let want = WANT.load(Ordering::SeqCst);
    if want != 0 {
        crate::arch::hart::wake_halted(1 << (want - 1));
    }
    let ticks = riscv::register::time::read64().saturating_sub(started);
    lists.zeroed_ticks.fetch_add(ticks as usize, Ordering::Relaxed);
    lists.zeroed_frames.fetch_add(1, Ordering::Relaxed);
}

/// The ticks and frames this hart zeroed since it last asked: at its next entry, under the lock,
/// to bill them (`sched::zeroed`).
pub fn take_zeroed() -> (u64, u32) {
    let lists = here();
    (lists.zeroed_ticks.swap(0, Ordering::Relaxed) as u64, lists.zeroed_frames.swap(0, Ordering::Relaxed))
}

/// Hart `hart`'s done list, taken whole: its first frame's index, the rest linked through
/// [`LINK`] (`index + 1`, 0 ending it). Under the lock.
pub fn take_done(hart: usize) -> Option<usize> {
    let frame = LISTS[hart].done.swap(0, Ordering::SeqCst);
    (frame != 0).then(|| frame as usize - 1)
}

/// The frame after `index` on a done list, its link cleared: the frame is now all zero.
pub fn unlink(index: usize) -> Option<usize> {
    let at = phys(index as u32 + 1);
    let next = crate::kframe::read(at, LINK) as u32;
    crate::kframe::write(at, LINK, 0);
    (next != 0).then(|| next as usize - 1)
}

/// The lock's holder needs a frame and found none free, none done and none pending: the frames in
/// flight are the frames idle harts are zeroing outside the lock, one each. Halt until one is pushed
/// done: that needs no lock, so the wait is at most one frame's zeroing. Like the wait for a
/// shootdown's acknowledgement.
pub fn wait_done() {
    WANT.store(crate::arch::hart::index() + 1, Ordering::SeqCst);
    let started = crate::arch::hart::started();
    while LISTS[..started].iter().all(|l| l.done.load(Ordering::SeqCst) == 0) {
        crate::arch::hart::halt_for_lock();
    }
    WANT.store(0, Ordering::SeqCst);
}

/// The checked build's count of the frames in flight on the lists, against the ownership table
/// (`MemoryManager::check_free_frames`): every pending, staged and done frame, and the frame each
/// idle hart is zeroing. A frame may move from zeroing to a done list while this reads, counted
/// done before it stops being zeroed, so it may be counted twice, never missed: the least and the
/// most there can be. Under the lock; `limit` bounds each walk, so a list linked into a cycle stops
/// the boot rather than the count.
#[cfg(debug_assertions)]
pub fn counted(limit: usize) -> (usize, usize) {
    let walk = |mut frame: u32| {
        let mut n = 0;
        while frame != 0 {
            n += 1;
            assert!(n <= limit, "I1: a list of frames in flight is longer than the frames in flight");
            frame = crate::kframe::read(phys(frame), LINK) as u32;
        }
        n
    };
    let started = crate::arch::hart::started();
    let (mut least, mut most) = (0, 0);
    for lists in &LISTS[..started] {
        let zeroing = usize::from(lists.zeroing.load(Ordering::SeqCst) != 0);
        let fixed = walk(lists.pending.load(Ordering::Relaxed))
            + lists.staged_n.load(Ordering::Relaxed)
            + walk(lists.done.load(Ordering::SeqCst));
        least += fixed + usize::from(lists.zeroing.load(Ordering::SeqCst) != 0);
        most += fixed + zeroing;
    }
    (least, most)
}

/// The frames on a list from `frame`, for the checked build's count (`MemoryManager::committing`).
#[cfg(debug_assertions)]
pub fn length(mut frame: Option<usize>, limit: usize) -> usize {
    let mut n = 0;
    while let Some(index) = frame {
        n += 1;
        assert!(n <= limit, "I1: a list of frames in flight is longer than the frames in flight");
        frame = (crate::kframe::read(phys(index as u32 + 1), LINK) as u32).checked_sub(1).map(|f| f as usize);
    }
    n
}
