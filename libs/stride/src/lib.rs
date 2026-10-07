//! The kernel's stride queue: its arithmetic and its ranks (`kernel/scheduling.md` R12,
//! `kernel/budgets.md` R7). One flat queue over every runnable budget, lowest pass first.
//!
//! This crate holds only the rules, so they can be host-tested and checked against the executable
//! model (`tests/differential.rs`). The kernel keeps each budget's [`State`] in the budget's own
//! frame and supplies it through [`Budgets`]; it decides which threads are ready and which thread
//! of a budget runs, and marks the processes whose ready threads change ([`Marks`]), so that a
//! reconcile visits only the budgets whose runnable state changed.
//!
//! - **Charging** ([`charge`]): `t = rem + runtime·STRIDE; pass += t / w; rem = t % w`, an exact remainder,
//!   with `w` the budget's stride weight (its free weight: limit less carve). Runtime is capped at
//!   [`RUNTIME_CAP`] per charge, so `t < 2^61` fits in 64 bits on both widths; the pass is a `u128` that is
//!   only added to and compared, so it never wraps.
//! - **Floor**: a monotone lower bound, raised to the queue's minimum pass whenever that rises and kept while
//!   the queue is empty. A waking budget's pass is `max(own, floor)`.
//! - **Rank** ([`Rank`]): `(pass, tie, id)`. Wakes take `front -= 1` (same-reconcile wakes processed in
//!   descending id, so the lowest id is frontmost), every deschedule of a still-runnable budget takes `back
//!   += 1`; both reset when the queue empties. So at an equal pass: wakers before requeued budgets; a later
//!   reconcile's wake first; within one reconcile the lower id first; requeues FIFO.
//! - **Inheritance**: a child enters at `max(floor, parent pass)` ([`entry`]); when destroyed, its work since
//!   entry is added to its parent's lead, normalized by weight ([`lift`]).
//! - **Weight change** ([`rescale`]): at a carve or a carve returned, what the budget owes, its lead over the
//!   floor and its remainder, is restated exactly at the new weight; a carve and its return cancel.
//! - **Deschedule**: a budget taken off the CPU is charged what it ran, and at least [`MIN_CHARGE`] (a run
//!   too short for the clock to see is not free).
//!
//! [`Harts`] is the wiring itself: one [`Runner`] per hart, the budget whose runtime is accruing
//! there, when it is folded, and the order of the steps at a deschedule, a pick, a creation, a
//! weight change and a destruction. A budget runs on at most one hart at a time. The kernel's
//! `sched.rs` drives it with its trap-boundary accounting and the budgets' frames; [`Cpu`] is the
//! same wiring for one hart, and the differential drives it against the model.

#![no_std]
#![forbid(unsafe_code)]

mod marks;

pub use marks::{Marks, Missed, Ready};

/// Stride scheduling numerator (`kernel/scheduling.md`).
pub const STRIDE: u64 = 1 << 20;

/// The most runtime one charge counts: `RUNTIME_CAP · STRIDE < 2^60`, so a charge plus a
/// remainder below 2^32 fits in 64 bits.
pub const RUNTIME_CAP: u64 = 1 << 40;

/// The least a deschedule charges, in the caller's time unit (the kernel's timebase ticks).
pub const MIN_CHARGE: u64 = 1;

/// The largest stride weight: the ABI's weights are 32-bit, and the arithmetic relies on it.
pub const MAX_WEIGHT: u64 = u32::MAX as u64;

/// A budget's scheduling state, as the kernel keeps it in the budget's frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub pass: u128,
    /// Where it entered virtual time when it was created.
    pub entry: u128,
    /// The exact remainder of charging: below the stride weight.
    pub rem: u64,
    /// Rank among equal passes.
    pub tie: i64,
    /// In the queue (it has a runnable thread, or is running).
    pub queued: bool,
}

/// Charge `runtime` to `s` at stride weight `weight` (nothing for weight 0, which holds no
/// process).
pub fn charge(s: &mut State, weight: u64, runtime: u64) {
    debug_assert!(weight <= MAX_WEIGHT, "stride weight {weight} above 32 bits");
    if weight == 0 {
        return;
    }
    // rem < weight <= 2^32 and runtime·STRIDE < 2^60: no overflow (saturating all the same).
    let t = s.rem.saturating_add(runtime.min(RUNTIME_CAP) * STRIDE);
    s.pass += u128::from(t / weight);
    s.rem = t % weight;
}

/// A budget's stride weight changes from `old` to `new` (a carve, or a carve returned), its
/// runtime already charged at `old`. What it owes, `W = (pass − floor)⁺·old + rem`, is kept exactly
/// and restated at the new weight: `pass = floor + W / new`, `rem = W mod new`
/// (`kernel/scheduling.md`, "The lead follows the weight"). So a carve and its return with no run
/// between leave the state as it was. A pass below the floor owes only its remainder (waking lifts
/// it to the floor anyway). A budget at weight 0 holds no process; what it owes is stated at
/// weight 1 meanwhile, so it is carried exactly through 0 (a carve of everything and the return of
/// part of it is the carve old to new).
pub fn rescale(s: &mut State, old: u64, new: u64, floor: u128) {
    let (old, new) = (old.max(1), new.max(1));
    // Saturating: far beyond any pass a machine reaches, but a weight change must not stop the
    // kernel.
    let owed = s.pass.saturating_sub(floor).saturating_mul(u128::from(old)).saturating_add(u128::from(s.rem));
    let w = u128::from(new);
    s.pass = floor.saturating_add(owed / w);
    s.rem = (owed % w) as u64;
}

/// One weight change ([`rescale`]): what went in and what came out, for a kernel that records it
/// (the test-only `sched-trace`, checked by the bench's oracle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Reweigh {
    pub before: State,
    pub after: State,
    pub old: u64,
    pub new: u64,
    pub floor: u128,
}

/// Where a new budget enters: `max(floor, parent's pass)` (the parent's runtime already charged).
pub fn entry(floor: u128, parent_pass: Option<u128>) -> u128 { parent_pass.map_or(floor, |p| p.max(floor)) }

/// A destroyed child's work since its entry moves to its parent: with `f` the floor,
/// `W = (child.pass − max(entry, f))⁺ · w_child + child.rem`, added to the parent's lead:
/// `parent = max(parent.pass, f) + W / w_parent`, the remainder carried. `w_parent` is the
/// parent's stride weight after the child's carve returned to it.
pub fn lift(parent: &mut State, child: &State, w_child: u64, w_parent: u64, f: u128) {
    if w_parent == 0 {
        return;
    }
    let from = child.entry.max(f);
    // Below 2^128 while the pass is (every charge adds under 2^60 to a pass that starts at 0),
    // but saturating: a destroy must not stop the kernel.
    let work = child
        .pass
        .saturating_sub(from)
        .saturating_mul(u128::from(w_child))
        .saturating_add(u128::from(child.rem));
    let wp = u128::from(w_parent);
    let (base, base_rem) = if parent.pass >= f { (parent.pass, u128::from(parent.rem)) } else { (f, 0) };
    let rem = base_rem + work % wp;
    parent.pass = base.saturating_add(work / wp).saturating_add(rem / wp);
    parent.rem = (rem % wp) as u64;
}

/// One destroyed child's work moving to its parent ([`lift`]): what went in and what came out,
/// for a kernel that records it (the test-only `sched-trace`, checked by the bench's oracle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lift {
    /// The parent before and after.
    pub parent: State,
    pub after: State,
    pub child: State,
    pub w_child: u64,
    pub w_parent: u64,
    pub floor: u128,
}

/// The order a pick minimizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rank {
    pub pass: u128,
    pub tie: i64,
    pub id: u64,
}

/// What the queue needs of the kernel's budgets.
pub trait Budgets<B> {
    fn state(&self, b: B) -> State;
    fn set_state(&mut self, b: B, s: State);
    /// The budget's id: never reused, it breaks ties.
    fn id(&self, b: B) -> u64;
    /// Its stride weight: its free weight.
    fn weight(&self, b: B) -> u64;
    /// Whether `b` still exists (a [`Cpu`] may name a budget destroyed since).
    fn live(&self, _b: B) -> bool { true }
    /// What the queue did, for a kernel that records it (the kernel's test-only `sched-trace`):
    /// `b` woke into the queue, was requeued behind its equals, or left it. Called after its new
    /// state is set. Nothing by default.
    fn woke(&mut self, _b: B) {}
    fn requeued(&mut self, _b: B) {}
    fn left(&mut self, _b: B) {}
    /// `child`'s work moved to `parent` (called after the parent's new state is set).
    fn lifted(&mut self, _parent: B, _child: B, _lift: &Lift) {}
    /// `b`'s weight changed and its state was converted (called before its new state is set, so
    /// the conversion is recorded ahead of the pass it may lower).
    fn reweighed(&mut self, _b: B, _r: &Reweigh) {}
}

/// The queue: every budget with a runnable thread (or running on a hart), at most `N` of them,
/// and the floor and tie counters.
#[derive(Clone, Debug)]
pub struct Queue<B, const N: usize> {
    pub floor: u128,
    pub front: i64,
    pub back: i64,
    /// The queued budgets are `slots[..len]`, in no order: an insert appends and a removal moves
    /// the last into the gap, so every scan visits only queued budgets. The pick is a minimum over
    /// a total order, so no order is needed.
    slots: [Option<B>; N],
    /// Beside each slot, the budget's rank as its frame holds it: what the floor and the pick read,
    /// so neither reads a frame. Written wherever the queue changes a queued budget's pass or tie
    /// (a charge, a requeue, a wake, a rescale, a lift). The frame stays the authority: a checked
    /// kernel audits the ranks against it ([`Queue::audit`]).
    ranks: [Rank; N],
    len: usize,
    /// Whether the minimum queued pass may have risen above the floor since it was last raised: a
    /// budget at the floor left or its pass rose. The floor is raised at the end of the next
    /// charge, deschedule, reconcile or destruction, as the rules have it (a weight change alone
    /// does not raise it), and only then.
    pending: bool,
}

impl<B: Copy + PartialEq, const N: usize> Default for Queue<B, N> {
    fn default() -> Self { Self::new() }
}

impl<B: Copy + PartialEq, const N: usize> Queue<B, N> {
    pub const fn new() -> Self {
        Queue {
            floor: 0,
            front: 0,
            back: 0,
            slots: [None; N],
            ranks: [Rank { pass: 0, tie: 0, id: 0 }; N],
            len: 0,
            pending: false,
        }
    }

    /// The queued budgets, in no order.
    pub fn queued(&self) -> impl Iterator<Item = B> + '_ { self.slots[..self.len].iter().flatten().copied() }

    pub fn contains(&self, b: B) -> bool { self.slot_of(b).is_some() }

    pub fn is_empty(&self) -> bool { self.len == 0 }

    /// `b`'s slot, if it is queued: a scan of the slots, no frame read.
    fn slot_of(&self, b: B) -> Option<usize> { self.slots[..self.len].iter().position(|s| *s == Some(b)) }

    /// Queue `b` with `id`, known not to be queued (its state says so: a reconcile's wakes, with
    /// no search), at the slot returned; its rank is the caller's to write. A queued budget has a
    /// runnable thread, so there are never more than there are processes, and `N` is the process
    /// count: a full queue is a broken invariant, and `b` is then left out (`None`) rather than
    /// anything stopping.
    fn push(&mut self, b: B, id: u64) -> Option<usize> {
        if self.len == N {
            debug_assert!(false, "stride queue full");
            return None;
        }
        let i = self.len;
        self.slots[i] = Some(b);
        self.ranks[i] = Rank { pass: 0, tie: 0, id };
        self.len += 1;
        Some(i)
    }

    /// Take out the budget at `slots[i]`, moving the last into its place. One at the floor may
    /// have held the minimum: the floor is raised at the end of the operation (a reconcile's
    /// leaves are one step, and the floor is their result, whatever their order: when they empty
    /// the queue it holds).
    fn remove_at(&mut self, i: usize) {
        self.pending |= self.at_floor(i);
        self.len -= 1;
        self.slots[i] = self.slots[self.len];
        self.ranks[i] = self.ranks[self.len];
        self.slots[self.len] = None;
    }

    fn reset_if_empty(&mut self) {
        if self.is_empty() {
            self.front = 0;
            self.back = 0;
        }
    }

    /// Whether the budget at `slots[i]` may hold the minimum pass: the floor is the minimum queued
    /// pass once raised, and a pass is never set below the floor, so a budget above the floor is
    /// not the minimum, and only one at the floor can move it by leaving or being charged.
    fn at_floor(&self, i: usize) -> bool { self.ranks[i].pass <= self.floor }

    /// Raise the floor to the queue's minimum pass, from the ranks, if the minimum may have risen
    /// since it was last raised. It never falls. A slice end at which no budget at the floor left
    /// or was charged compares nothing.
    fn raise_floor(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        if let Some(min) = self.ranks[..self.len].iter().map(|r| r.pass).min() {
            self.floor = self.floor.max(min);
        }
    }

    /// `b`'s pass or tie is now `s`'s (its frame already written): a queued budget's rank follows,
    /// and a pass risen from the floor leaves the floor to be raised. Nothing for a budget not
    /// queued.
    fn recorded(&mut self, b: B, s: &State) {
        if !s.queued {
            return;
        }
        if let Some(i) = self.slot_of(b) {
            self.pending |= self.at_floor(i) && s.pass > self.ranks[i].pass;
            self.ranks[i].pass = s.pass;
            self.ranks[i].tie = s.tie;
        }
    }

    /// Charge `runtime` to `b` (a fold), then raise the floor.
    pub fn fold(&mut self, bs: &mut impl Budgets<B>, b: B, runtime: u64) {
        let mut s = bs.state(b);
        charge(&mut s, bs.weight(b), runtime);
        bs.set_state(b, s);
        self.recorded(b, &s);
        self.raise_floor();
    }

    /// `b` was taken off the CPU (its runtime already folded). Still runnable, it is requeued
    /// behind its equals; otherwise it leaves the queue.
    pub fn deschedule(&mut self, bs: &mut impl Budgets<B>, b: B, still_runnable: bool) {
        let mut s = bs.state(b);
        let was = self.slot_of(b);
        let requeued = if still_runnable { was.or_else(|| self.push(b, bs.id(b))) } else { None };
        if let Some(i) = requeued {
            self.back = self.back.saturating_add(1);
            s.tie = self.back;
            s.queued = true;
            self.ranks[i] = Rank { pass: s.pass, tie: s.tie, id: bs.id(b) };
        } else {
            s.queued = false;
            if let Some(i) = was {
                self.remove_at(i);
            }
        }
        bs.set_state(b, s);
        if requeued.is_some() {
            bs.requeued(b);
        } else if was.is_some() {
            bs.left(b);
        }
        self.raise_floor();
        self.reset_if_empty();
    }

    /// The end of a kernel entry. It visits only the budgets whose runnable state may have changed
    /// since the last reconcile ([`Marks`]): `lost` those that may have lost their last runnable
    /// thread, `gained` those that may have gained one (each in any order, repeats allowed);
    /// `runnable` says whether a budget has a runnable thread now, and `running` whether a budget
    /// runs on a hart now: one that does stays queued while it runs, though its one thread is the
    /// running one. Of the others, budgets that lost their last runnable thread leave the queue;
    /// budgets that gained one wake at `max(own, floor)`, in descending id so the lowest id ranks
    /// first (`gained` is sorted so).
    pub fn reconcile<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        running: impl Fn(B) -> bool,
        lost: &[B],
        gained: &mut [B],
        runnable: impl Fn(&S, B) -> bool,
    ) {
        for &b in lost {
            if running(b) || !bs.live(b) {
                continue;
            }
            let mut s = bs.state(b);
            if !s.queued || runnable(bs, b) {
                continue;
            }
            s.queued = false;
            bs.set_state(b, s);
            bs.left(b);
            // Its slot is found by a scan of the slots, never by a read of every queued budget's
            // state.
            if let Some(i) = self.slot_of(b) {
                self.remove_at(i);
            }
        }
        self.raise_floor();
        self.reset_if_empty();
        // A wake into a non-empty queue is at or above the floor, the minimum: the floor can rise
        // only when the wakes fill an empty queue.
        self.pending |= self.is_empty();
        gained.sort_unstable_by_key(|b| core::cmp::Reverse(bs.id(*b)));
        for &b in gained.iter() {
            if !bs.live(b) || !runnable(bs, b) {
                continue;
            }
            let mut s = bs.state(b);
            if s.queued {
                continue;
            }
            // A full queue (never expected) ends it.
            let Some(i) = self.push(b, bs.id(b)) else { break };
            self.front = self.front.saturating_sub(1);
            s.pass = s.pass.max(self.floor);
            s.tie = self.front;
            s.queued = true;
            self.ranks[i].pass = s.pass;
            self.ranks[i].tie = s.tie;
            bs.set_state(b, s);
            bs.woke(b);
        }
        self.raise_floor();
    }

    /// The queued budget with the lowest rank, of those `elsewhere` does not rule out: a budget
    /// running on another hart, so that a budget runs on at most one hart at a time and its stride
    /// state has one runner (`kernel/scheduling.md`, "One flat stride queue"). A compare of the
    /// ranks beside the slots: no frame is read.
    pub fn pick(&self, elsewhere: impl Fn(B) -> bool) -> Option<B> {
        (0..self.len)
            .filter(|i| self.slots[*i].is_some_and(|b| !elsewhere(b)))
            .min_by_key(|i| self.ranks[*i])
            .and_then(|i| self.slots[i])
    }

    /// `b`'s stride weight changed from `old` to `new` (its runtime already folded at `old`): its
    /// lead and remainder are converted to the new weight ([`rescale`]).
    pub fn reweigh(&mut self, bs: &mut impl Budgets<B>, b: B, old: u64, new: u64) {
        let before = bs.state(b);
        let mut s = before;
        rescale(&mut s, old, new, self.floor);
        bs.reweighed(b, &Reweigh { before, after: s, old, new, floor: self.floor });
        bs.set_state(b, s);
        self.recorded(b, &s);
    }

    /// The checked kernel's audit of the ranks against the frames: each queued budget's cached
    /// pass and tie are its state's and its state says queued, and the floor is not below the
    /// minimum queued pass unless a raise is pending (a raise missed would leave it there).
    /// Returns the budget out of step: one whose rank is not its frame's, or the one holding a
    /// minimum above the floor. A walk of the queue, off the exit path.
    pub fn audit(&self, bs: &impl Budgets<B>) -> Result<(), B> {
        let mut min: Option<(u128, B)> = None;
        for (b, r) in self.slots[..self.len].iter().zip(&self.ranks[..self.len]) {
            let Some(b) = *b else { continue };
            let s = bs.state(b);
            if !s.queued || (s.pass, s.tie, bs.id(b)) != (r.pass, r.tie, r.id) {
                return Err(b);
            }
            if min.map_or(true, |(p, _)| r.pass < p) {
                min = Some((r.pass, b));
            }
        }
        match min {
            Some((p, b)) if p > self.floor && !self.pending => Err(b),
            _ => Ok(()),
        }
    }

    /// A new budget `child` under `parent` (whose runtime is already folded, and whose carve and
    /// [`Queue::reweigh`] follow): it enters at `max(floor, parent's pass)`.
    pub fn create(&mut self, bs: &mut impl Budgets<B>, child: B, parent: Option<B>) {
        let e = entry(self.floor, parent.map(|p| bs.state(p).pass));
        bs.set_state(child, State { pass: e, entry: e, rem: 0, tie: 0, queued: false });
    }

    /// `child` is being destroyed (its runtime folded, its carve already returned to `parent` and
    /// the parent reweighed): its work moves to the parent, and it leaves the queue.
    /// `w_child` is its stride weight (its limit, its own children being gone first); `w_parent`
    /// the parent's stride weight now.
    pub fn destroy(
        &mut self,
        bs: &mut impl Budgets<B>,
        child: B,
        parent: Option<B>,
        w_child: u64,
        w_parent: u64,
    ) {
        if let Some(p) = parent {
            let c = bs.state(child);
            let before = bs.state(p);
            let mut s = before;
            lift(&mut s, &c, w_child, w_parent, self.floor);
            bs.set_state(p, s);
            let l = Lift { parent: before, after: s, child: c, w_child, w_parent, floor: self.floor };
            bs.lifted(p, child, &l);
            self.recorded(p, &s);
        }
        if let Some(i) = self.slot_of(child) {
            self.remove_at(i);
            bs.left(child);
        }
        self.raise_floor();
        self.reset_if_empty();
    }
}

/// One hart's runner: the budget whose runtime is accruing there (`cur`: on the hart, or in the
/// kernel on its behalf) and what it has accrued and not yet been charged (`pending`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Runner<B> {
    pub cur: Option<B>,
    pub pending: u64,
}

impl<B> Runner<B> {
    pub const fn new() -> Self { Runner { cur: None, pending: 0 } }

    /// `cur` ran, or the kernel worked for it, `t` more.
    pub fn accrue(&mut self, t: u64) { self.pending = self.pending.saturating_add(t); }
}

impl<B> Default for Runner<B> {
    fn default() -> Self { Self::new() }
}

/// The queue wired to its runners, one per hart: when runtime is folded, and the order of the
/// steps at a deschedule, a pick, a creation, a weight change and a destruction. Runtime is folded
/// into a pass only here: at a deschedule ([`Wiring::switch`], at least [`MIN_CHARGE`]), before a
/// weight change or a creation under the budget ([`Wiring::settle`]), at a destruction, and for
/// work billed to a budget that runs on no hart ([`Wiring::bill`]). A budget is some runner's
/// `cur` on at most one hart at a time: a pick skips budgets other harts run.
struct Wiring<'a, B, const N: usize> {
    q: &'a mut Queue<B, N>,
    runners: &'a mut [Runner<B>],
}

impl<B: Copy + PartialEq, const N: usize> Wiring<'_, B, N> {
    /// The hart whose runner is `b`, if any.
    fn runner_of(&self, b: B) -> Option<usize> { self.runners.iter().position(|r| r.cur == Some(b)) }

    fn bill(&mut self, bs: &mut impl Budgets<B>, b: B, t: u64) {
        match self.runner_of(b) {
            Some(h) => self.runners[h].accrue(t),
            None if bs.live(b) => self.q.fold(bs, b, t),
            None => {}
        }
    }

    fn settle(&mut self, bs: &mut impl Budgets<B>, b: B) {
        if let Some(h) = self.runner_of(b) {
            let run = core::mem::take(&mut self.runners[h].pending);
            if bs.live(b) {
                self.q.fold(bs, b, run);
            }
        }
    }

    fn change_weight<S: Budgets<B>>(&mut self, bs: &mut S, b: B, change: impl FnOnce(&mut S)) {
        self.settle(bs, b);
        let old = bs.weight(b);
        change(bs);
        let new = bs.weight(b);
        self.q.reweigh(bs, b, old, new);
    }

    fn switch<S: Budgets<B>>(
        &mut self,
        h: usize,
        bs: &mut S,
        next: Option<B>,
        still_runnable: impl FnOnce(&S, B) -> bool,
    ) -> Option<B> {
        let r = &mut self.runners[h];
        if next == r.cur {
            return None;
        }
        let left = r.cur.take();
        let run = core::mem::take(&mut r.pending).max(MIN_CHARGE);
        r.cur = next;
        if let Some(c) = left.filter(|c| bs.live(*c)) {
            self.q.fold(bs, c, run);
            let still = still_runnable(bs, c);
            self.q.deschedule(bs, c, still);
        }
        left
    }

    fn reconcile<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        lost: &[B],
        gained: &mut [B],
        runnable: impl Fn(&S, B) -> bool,
    ) {
        let runners = &*self.runners;
        self.q.reconcile(bs, |b| runners.iter().any(|r| r.cur == Some(b)), lost, gained, runnable);
    }

    fn pick<S: Budgets<B>, T>(
        &mut self,
        h: usize,
        bs: &mut S,
        mut next: impl FnMut(&S, B) -> Option<T>,
    ) -> Option<(B, T)> {
        let runners = &*self.runners;
        let elsewhere = |b| runners.iter().enumerate().any(|(i, r)| i != h && r.cur == Some(b));
        loop {
            let b = self.q.pick(elsewhere)?;
            if let Some(t) = next(bs, b) {
                return Some((b, t));
            }
            self.q.deschedule(bs, b, false);
        }
    }

    fn create(&mut self, bs: &mut impl Budgets<B>, child: B, parent: Option<B>) {
        if let Some(p) = parent {
            self.settle(bs, p);
        }
        self.q.create(bs, child, parent);
    }

    fn destroy<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        child: B,
        parent: Option<B>,
        return_weight: impl FnOnce(&mut S),
    ) {
        self.settle(bs, child);
        for r in self.runners.iter_mut().filter(|r| r.cur == Some(child)) {
            *r = Runner::new();
        }
        let w_child = bs.weight(child);
        let w_parent = match parent {
            Some(p) => {
                self.change_weight(bs, p, return_weight);
                bs.weight(p)
            }
            None => 0,
        };
        self.q.destroy(bs, child, parent, w_child, w_parent);
    }
}

/// One CPU wired to the queue: the one-hart form, whose runner is `cur` and `pending`. The
/// differential drives it against the model.
#[derive(Clone, Debug)]
pub struct Cpu<B, const N: usize> {
    pub q: Queue<B, N>,
    pub cur: Option<B>,
    pub pending: u64,
}

impl<B: Copy + PartialEq, const N: usize> Default for Cpu<B, N> {
    fn default() -> Self { Self::new() }
}

impl<B: Copy + PartialEq, const N: usize> Cpu<B, N> {
    pub const fn new() -> Self { Cpu { q: Queue::new(), cur: None, pending: 0 } }

    /// Run `f` on the queue wired to this CPU's one runner.
    fn wired<R>(&mut self, f: impl FnOnce(&mut Wiring<'_, B, N>) -> R) -> R {
        let mut runner = [Runner { cur: self.cur, pending: self.pending }];
        let out = f(&mut Wiring { q: &mut self.q, runners: &mut runner });
        (self.cur, self.pending) = (runner[0].cur, runner[0].pending);
        out
    }

    /// `cur` ran, or the kernel worked for it, `t` more.
    pub fn accrue(&mut self, t: u64) { self.pending = self.pending.saturating_add(t); }

    /// `t` of work was done for `b`: `cur`'s joins its pending runtime, anyone else's is charged
    /// at once.
    pub fn bill(&mut self, bs: &mut impl Budgets<B>, b: B, t: u64) { self.wired(|w| w.bill(bs, b, t)) }

    /// If `b` is accruing runtime, charge it now, at its present weight: a weight change or a
    /// creation under it follows.
    pub fn settle(&mut self, bs: &mut impl Budgets<B>, b: B) { self.wired(|w| w.settle(bs, b)) }

    /// `b`'s stride weight changes by `change`: what it ran is charged at the old weight first,
    /// then its lead and remainder are converted to the new weight.
    pub fn change_weight<S: Budgets<B>>(&mut self, bs: &mut S, b: B, change: impl FnOnce(&mut S)) {
        self.wired(|w| w.change_weight(bs, b, change))
    }

    /// The CPU goes to `next` (`None`: to nobody's budget). If that is not `cur`, `cur` is taken
    /// off: charged what it ran and at least [`MIN_CHARGE`], then requeued behind its equals if it
    /// still has a runnable thread, or taken out of the queue. Returns the budget taken off.
    pub fn switch<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        next: Option<B>,
        still_runnable: impl FnOnce(&S, B) -> bool,
    ) -> Option<B> {
        self.wired(|w| w.switch(0, bs, next, still_runnable))
    }

    /// The end of a kernel entry: [`Queue::reconcile`] with `cur` running.
    pub fn reconcile<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        lost: &[B],
        gained: &mut [B],
        runnable: impl Fn(&S, B) -> bool,
    ) {
        self.wired(|w| w.reconcile(bs, lost, gained, runnable))
    }

    /// The lowest-ranked queued budget and what `next` chooses to run of it. A queued budget with
    /// nothing to run (never expected: reconciles keep the queue in step) is taken out.
    pub fn pick<S: Budgets<B>, T>(
        &mut self,
        bs: &mut S,
        next: impl FnMut(&S, B) -> Option<T>,
    ) -> Option<(B, T)> {
        self.wired(|w| w.pick(0, bs, next))
    }

    /// A new budget `child` under `parent`: a running parent is charged first, then the child
    /// enters at `max(floor, parent's pass)`. The carve ([`Cpu::change_weight`]) follows.
    pub fn create(&mut self, bs: &mut impl Budgets<B>, child: B, parent: Option<B>) {
        self.wired(|w| w.create(bs, child, parent))
    }

    /// `child` is destroyed (its own children already were, bottom-up): what it ran is charged,
    /// its carve goes back to `parent` (`return_weight`, which may do nothing if it went back
    /// already), and its work since entry moves to the parent at the parent's weight now.
    pub fn destroy<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        child: B,
        parent: Option<B>,
        return_weight: impl FnOnce(&mut S),
    ) {
        self.wired(|w| w.destroy(bs, child, parent, return_weight))
    }
}

/// `H` harts wired to one queue, a [`Runner`] each, indexed by the hart's boot index: what
/// [`Cpu`] is for one. A switch, an accrual and a pick act for one hart; a bill, a settle and a
/// destruction find the budget's runner on whichever hart runs it; a reconcile keeps a budget
/// queued while any hart runs it.
#[derive(Clone, Debug)]
pub struct Harts<B, const N: usize, const H: usize> {
    pub q: Queue<B, N>,
    pub runners: [Runner<B>; H],
}

impl<B: Copy + PartialEq, const N: usize, const H: usize> Default for Harts<B, N, H> {
    fn default() -> Self { Self::new() }
}

impl<B: Copy + PartialEq, const N: usize, const H: usize> Harts<B, N, H> {
    pub const fn new() -> Self { Harts { q: Queue::new(), runners: [const { Runner::new() }; H] } }

    fn wiring(&mut self) -> Wiring<'_, B, N> { Wiring { q: &mut self.q, runners: &mut self.runners } }

    /// Hart `h`'s budget.
    pub fn cur(&self, h: usize) -> Option<B> { self.runners[h].cur }

    /// Whether `b` runs on a hart now.
    pub fn running(&self, b: B) -> bool { self.runners.iter().any(|r| r.cur == Some(b)) }

    /// Hart `h`'s `cur` ran, or the kernel worked for it, `t` more.
    pub fn accrue(&mut self, h: usize, t: u64) { self.runners[h].accrue(t) }

    /// `t` of work was done for `b`: if a hart runs it, it joins that runner's pending runtime,
    /// and otherwise it is charged at once.
    pub fn bill(&mut self, bs: &mut impl Budgets<B>, b: B, t: u64) { self.wiring().bill(bs, b, t) }

    /// If `b` is accruing runtime on a hart, charge it now, at its present weight.
    pub fn settle(&mut self, bs: &mut impl Budgets<B>, b: B) { self.wiring().settle(bs, b) }

    /// [`Cpu::change_weight`], with `b`'s runner on whichever hart runs it.
    pub fn change_weight<S: Budgets<B>>(&mut self, bs: &mut S, b: B, change: impl FnOnce(&mut S)) {
        self.wiring().change_weight(bs, b, change)
    }

    /// Hart `h` goes to `next`: [`Cpu::switch`] on its runner. Other harts' runners are untouched.
    pub fn switch<S: Budgets<B>>(
        &mut self,
        h: usize,
        bs: &mut S,
        next: Option<B>,
        still_runnable: impl FnOnce(&S, B) -> bool,
    ) -> Option<B> {
        self.wiring().switch(h, bs, next, still_runnable)
    }

    /// The end of a kernel entry: [`Queue::reconcile`] with every hart's `cur` running.
    pub fn reconcile<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        lost: &[B],
        gained: &mut [B],
        runnable: impl Fn(&S, B) -> bool,
    ) {
        self.wiring().reconcile(bs, lost, gained, runnable)
    }

    /// Hart `h`'s pick: [`Cpu::pick`] over the budgets no other hart runs.
    pub fn pick<S: Budgets<B>, T>(
        &mut self,
        h: usize,
        bs: &mut S,
        next: impl FnMut(&S, B) -> Option<T>,
    ) -> Option<(B, T)> {
        self.wiring().pick(h, bs, next)
    }

    /// [`Cpu::create`], the parent's runner on whichever hart runs it.
    pub fn create(&mut self, bs: &mut impl Budgets<B>, child: B, parent: Option<B>) {
        self.wiring().create(bs, child, parent)
    }

    /// [`Cpu::destroy`]; the runner of whichever hart runs `child` is cleared.
    pub fn destroy<S: Budgets<B>>(
        &mut self,
        bs: &mut S,
        child: B,
        parent: Option<B>,
        return_weight: impl FnOnce(&mut S),
    ) {
        self.wiring().destroy(bs, child, parent, return_weight)
    }
}

#[cfg(test)]
mod tests;
