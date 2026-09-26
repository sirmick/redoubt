//! The kernel's stride queue: its arithmetic and its ranks (`kernel/scheduling.md` R12,
//! `kernel/budgets.md` R7). One flat queue over every runnable budget, lowest pass first.
//!
//! This crate holds only the rules, so they can be host-tested and checked against the executable
//! model (`tests/differential.rs`). The kernel keeps each budget's [`State`] in the budget's own
//! frame and supplies it through [`Budgets`]; it decides which budgets have runnable threads and
//! which thread of a budget runs.
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
//! - **Deschedule**: a budget taken off the CPU is charged what it ran, and at least [`MIN_CHARGE`] (a run too
//!   short for the clock to see is not free).
//!
//! [`Cpu`] is the wiring itself: the budget whose runtime is accruing, when it is folded, and the
//! order of the steps at a deschedule, a pick, a creation, a weight change and a destruction. The
//! kernel's `sched.rs` drives it with its trap-boundary accounting and the budgets' frames; the
//! differential drives the same code against the model.

#![no_std]
#![forbid(unsafe_code)]

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

/// A budget's stride weight changes from `old` to `new` (a carve, or a carve returned). Runtime
/// has already been charged at `old`; the remainder is rescaled, losing under one pass unit.
pub fn rescale(rem: u64, old: u64, new: u64) -> u64 {
    if old == 0 || new == 0 {
        return 0;
    }
    // rem < old < 2^32 and new < 2^32: the product fits in 64 bits.
    let r = rem.min(old - 1) * new / old;
    r.min(new - 1)
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
}

/// The queue: every budget with a runnable thread (or running), at most `N` of them, and the
/// floor and tie counters.
#[derive(Clone, Debug)]
pub struct Queue<B, const N: usize> {
    pub floor: u128,
    pub front: i64,
    pub back: i64,
    slots: [Option<B>; N],
}

impl<B: Copy + PartialEq, const N: usize> Default for Queue<B, N> {
    fn default() -> Self { Self::new() }
}

impl<B: Copy + PartialEq, const N: usize> Queue<B, N> {
    pub const fn new() -> Self { Queue { floor: 0, front: 0, back: 0, slots: [None; N] } }

    /// The queued budgets, in slot order.
    pub fn queued(&self) -> impl Iterator<Item = B> + '_ { self.slots.iter().flatten().copied() }

    pub fn contains(&self, b: B) -> bool { self.slots.contains(&Some(b)) }

    pub fn is_empty(&self) -> bool { self.slots.iter().all(Option::is_none) }

    /// Whether `b` is queued now. A queued budget has a runnable thread, so there are never more
    /// than there are processes, and `N` is the process count: a full queue is a broken invariant,
    /// and `b` is then left out rather than anything stopping.
    fn insert(&mut self, b: B) -> bool {
        if self.contains(b) {
            return true;
        }
        match self.slots.iter_mut().find(|s| s.is_none()) {
            Some(slot) => {
                *slot = Some(b);
                true
            }
            None => {
                debug_assert!(false, "stride queue full");
                false
            }
        }
    }

    fn take_out(&mut self, b: B) {
        for s in self.slots.iter_mut() {
            if *s == Some(b) {
                *s = None;
            }
        }
    }

    fn reset_if_empty(&mut self) {
        if self.is_empty() {
            self.front = 0;
            self.back = 0;
        }
    }

    /// Raise the floor to the queue's minimum pass. It never falls.
    pub fn raise_floor(&mut self, bs: &impl Budgets<B>) {
        if let Some(min) = self.queued().map(|b| bs.state(b).pass).min() {
            self.floor = self.floor.max(min);
        }
    }

    /// Charge `runtime` to `b` (a fold), then raise the floor.
    pub fn fold(&mut self, bs: &mut impl Budgets<B>, b: B, runtime: u64) {
        let mut s = bs.state(b);
        charge(&mut s, bs.weight(b), runtime);
        bs.set_state(b, s);
        self.raise_floor(bs);
    }

    /// `b` was taken off the CPU (its runtime already folded). Still runnable, it is requeued
    /// behind its equals; otherwise it leaves the queue.
    pub fn deschedule(&mut self, bs: &mut impl Budgets<B>, b: B, still_runnable: bool) {
        let mut s = bs.state(b);
        let was = self.contains(b);
        let requeued = still_runnable && self.insert(b);
        if requeued {
            self.back = self.back.saturating_add(1);
            s.tie = self.back;
            s.queued = true;
        } else {
            s.queued = false;
            self.take_out(b);
        }
        bs.set_state(b, s);
        if requeued {
            bs.requeued(b);
        } else if was {
            bs.left(b);
        }
        self.raise_floor(bs);
        self.reset_if_empty();
    }

    /// The end of a kernel entry. `runnable` is every budget that has a runnable thread now (in
    /// any order, each once); `running` the budget on the CPU, if any. Budgets that lost their
    /// last runnable thread leave the queue; budgets that gained one wake at `max(own, floor)`,
    /// in descending id so the lowest id ranks first.
    pub fn reconcile(&mut self, bs: &mut impl Budgets<B>, running: Option<B>, runnable: &[B]) {
        let gone: [Option<B>; N] =
            self.slots.map(|s| s.filter(|b| !runnable.contains(b) && running != Some(*b)));
        for b in gone.into_iter().flatten() {
            let mut s = bs.state(b);
            s.queued = false;
            bs.set_state(b, s);
            self.take_out(b);
            bs.left(b);
        }
        self.raise_floor(bs);
        self.reset_if_empty();
        loop {
            // The highest id among runnable budgets not yet queued.
            let Some(b) = runnable.iter().copied().filter(|b| !self.contains(*b)).max_by_key(|b| bs.id(*b))
            else {
                break;
            };
            // Each pass queues one more, so this ends; a full queue (never expected) ends it too.
            if !self.insert(b) {
                break;
            }
            self.front = self.front.saturating_sub(1);
            let mut s = bs.state(b);
            s.pass = s.pass.max(self.floor);
            s.tie = self.front;
            s.queued = true;
            bs.set_state(b, s);
            bs.woke(b);
        }
        self.raise_floor(bs);
    }

    /// The queued budget with the lowest rank.
    pub fn pick(&self, bs: &impl Budgets<B>) -> Option<B> {
        self.queued().min_by_key(|b| {
            let s = bs.state(*b);
            Rank { pass: s.pass, tie: s.tie, id: bs.id(*b) }
        })
    }

    /// `b`'s stride weight changed from `old` to `new` (its runtime already folded at `old`).
    pub fn reweigh(&mut self, bs: &mut impl Budgets<B>, b: B, old: u64, new: u64) {
        let mut s = bs.state(b);
        s.rem = rescale(s.rem, old, new);
        bs.set_state(b, s);
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
        }
        if self.contains(child) {
            self.take_out(child);
            bs.left(child);
        }
        self.raise_floor(bs);
        self.reset_if_empty();
    }
}

/// One CPU wired to the queue: the budget whose runtime is accruing (`cur`: on the CPU, or in
/// the kernel on its behalf) and what it has accrued and not yet been charged (`pending`). Runtime
/// is folded into a pass only here: at a deschedule ([`Cpu::switch`], at least [`MIN_CHARGE`]),
/// before a weight change or a creation under the budget ([`Cpu::settle`]), at a destruction, and
/// for work billed to a budget that is not running ([`Cpu::bill`]).
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

    /// `cur` ran, or the kernel worked for it, `t` more.
    pub fn accrue(&mut self, t: u64) { self.pending = self.pending.saturating_add(t); }

    /// `t` of work was done for `b`: `cur`'s joins its pending runtime, anyone else's is charged
    /// at once.
    pub fn bill(&mut self, bs: &mut impl Budgets<B>, b: B, t: u64) {
        if self.cur == Some(b) {
            self.accrue(t);
        } else if bs.live(b) {
            self.q.fold(bs, b, t);
        }
    }

    /// If `b` is accruing runtime, charge it now, at its present weight: a weight change or a
    /// creation under it follows.
    pub fn settle(&mut self, bs: &mut impl Budgets<B>, b: B) {
        if self.cur == Some(b) {
            let run = core::mem::take(&mut self.pending);
            if bs.live(b) {
                self.q.fold(bs, b, run);
            }
        }
    }

    /// `b`'s stride weight changes by `change`: what it ran is charged at the old weight first,
    /// then its remainder is rescaled.
    pub fn change_weight<S: Budgets<B>>(&mut self, bs: &mut S, b: B, change: impl FnOnce(&mut S)) {
        self.settle(bs, b);
        let old = bs.weight(b);
        change(bs);
        let new = bs.weight(b);
        self.q.reweigh(bs, b, old, new);
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
        if next == self.cur {
            return None;
        }
        let left = self.cur.take();
        let run = core::mem::take(&mut self.pending).max(MIN_CHARGE);
        if let Some(c) = left.filter(|c| bs.live(*c)) {
            self.q.fold(bs, c, run);
            let still = still_runnable(bs, c);
            self.q.deschedule(bs, c, still);
        }
        self.cur = next;
        left
    }

    /// The end of a kernel entry: [`Queue::reconcile`] with `cur` running.
    pub fn reconcile(&mut self, bs: &mut impl Budgets<B>, runnable: &[B]) {
        let running = self.cur.filter(|c| bs.live(*c));
        self.q.reconcile(bs, running, runnable);
    }

    /// The lowest-ranked queued budget and what `next` chooses to run of it. A queued budget with
    /// nothing to run (never expected: reconciles keep the queue in step) is taken out.
    pub fn pick<S: Budgets<B>, T>(
        &mut self,
        bs: &mut S,
        mut next: impl FnMut(&S, B) -> Option<T>,
    ) -> Option<(B, T)> {
        loop {
            let b = self.q.pick(bs)?;
            if let Some(t) = next(bs, b) {
                return Some((b, t));
            }
            self.q.deschedule(bs, b, false);
        }
    }

    /// A new budget `child` under `parent`: a running parent is charged first, then the child
    /// enters at `max(floor, parent's pass)`. The carve ([`Cpu::change_weight`]) follows.
    pub fn create(&mut self, bs: &mut impl Budgets<B>, child: B, parent: Option<B>) {
        if let Some(p) = parent {
            self.settle(bs, p);
        }
        self.q.create(bs, child, parent);
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
        self.settle(bs, child);
        if self.cur == Some(child) {
            self.cur = None;
            self.pending = 0;
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

#[cfg(test)]
mod tests;
