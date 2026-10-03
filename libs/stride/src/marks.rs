//! Which budgets a reconcile visits: the ones whose runnable state changed (`kernel/scheduling.md`,
//! "The current minimum and ties").
//!
//! The kernel marks a process (a slot, `0..N`) whenever its count of ready threads changes: a
//! thread becomes ready or stops being, the process is created or ends. [`Marks::settle`] then
//! moves each marked slot's count from the budget it was counted in to the budget it is in now, so
//! that whether a budget has a ready thread is one read ([`Ready::ready`]), and lists the budgets
//! whose counts moved: the only ones [`crate::Queue::reconcile`] visits. A checked kernel audits
//! the marks against a walk of every slot ([`Marks::audit`]).

use crate::{Budgets, Queue};

/// The count of ready threads the kernel keeps for each budget.
pub trait Ready<B>: Budgets<B> {
    fn ready(&self, b: B) -> u32;
    fn set_ready(&mut self, b: B, n: u32);
}

/// What the audit found wrong: a count changed with no mark, or a queue out of step with the
/// counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Missed {
    /// Slot `slot` has `has` ready threads, but `counted` are counted for it.
    Slot { slot: usize, counted: u32, has: u32 },
    /// Budget `id` keeps `kept` ready threads, but its slots count `sum`.
    Count { id: u64, kept: u32, sum: u32 },
    /// Budget `id` is queued with no ready thread and not running, or has one and is not queued.
    Queue { id: u64, queued: bool },
}

/// The marks since the last reconcile, and what each slot's ready threads were counted as.
pub struct Marks<B, const N: usize> {
    /// Per slot: the ready threads counted for it, and the budget they are counted in (any
    /// budget when none are).
    counted: [(u32, B); N],
    /// How many slots have ready threads counted.
    n_counted: usize,
    /// Whether the slot is listed in `slots[..n]`.
    marked: [bool; N],
    slots: [usize; N],
    n: usize,
    /// The budgets whose counts fell, and those whose counts rose, at the settles since the last
    /// reconcile.
    lost: [B; N],
    n_lost: usize,
    gained: [B; N],
    n_gained: usize,
    /// Whether the last reconcile visited a budget.
    visited: bool,
    /// When the last full walk ran, in the caller's time unit.
    walked: u64,
}

impl<B: Copy + PartialEq, const N: usize> Marks<B, N> {
    /// No slot counted and nothing marked; `none` fills the arrays and is never read.
    pub const fn new(none: B) -> Self {
        Marks {
            counted: [(0, none); N],
            n_counted: 0,
            marked: [false; N],
            slots: [0; N],
            n: 0,
            lost: [none; N],
            n_lost: 0,
            gained: [none; N],
            n_gained: 0,
            visited: false,
            walked: 0,
        }
    }

    /// Slot `i`'s ready threads changed.
    pub fn mark(&mut self, i: usize) {
        if !self.marked[i] {
            self.marked[i] = true;
            self.slots[self.n] = i;
            self.n += 1;
        }
    }

    /// Bring each marked slot's count up to date: `now` gives the slot's ready threads and its
    /// budget now (`None` for a slot with no process, or one in no budget). A count leaves the
    /// budget it was in, if that is still live, and joins the one it is in now; both are listed
    /// for the next reconcile.
    pub fn settle<S: Ready<B>>(&mut self, bs: &mut S, now: impl Fn(&S, usize) -> (u32, Option<B>)) {
        for k in 0..self.n {
            let i = self.slots[k];
            self.marked[i] = false;
            let (was, from) = self.counted[i];
            let (is, to) = match now(bs, i) {
                (n, Some(b)) if n > 0 => (n, b),
                _ => (0, from),
            };
            if was == is && (was == 0 || from == to) {
                continue;
            }
            if was > 0 && bs.live(from) {
                // Saturating: a broken count must not stop the kernel; the audit names it.
                bs.set_ready(from, bs.ready(from).saturating_sub(was));
                Self::list(&mut self.lost, &mut self.n_lost, from);
            }
            if is > 0 {
                bs.set_ready(to, bs.ready(to).saturating_add(is));
                Self::list(&mut self.gained, &mut self.n_gained, to);
            }
            self.counted[i] = (is, to);
            self.n_counted = self.n_counted + usize::from(is > 0) - usize::from(was > 0);
        }
        self.n = 0;
    }

    /// One entry per slot settled, and a reconcile after every settle: a full list is a broken
    /// invariant, and `b` is then left out (the audit finds the queue out of step) rather than
    /// anything stopping.
    fn list(xs: &mut [B; N], n: &mut usize, b: B) {
        if *n == N {
            debug_assert!(false, "marked budgets overflow");
            return;
        }
        xs[*n] = b;
        *n += 1;
    }

    /// The budgets a reconcile visits: those whose counts fell, and those whose counts rose (in
    /// any order, perhaps more than once; the reconcile sorts the second).
    pub fn changed(&mut self) -> (&[B], &mut [B]) {
        (&self.lost[..self.n_lost], &mut self.gained[..self.n_gained])
    }

    /// The reconcile visited them.
    pub fn clear(&mut self) {
        self.visited = self.n_lost > 0 || self.n_gained > 0;
        self.n_lost = 0;
        self.n_gained = 0;
    }

    /// The checked kernel's check of every reconcile, before [`Marks::clear`]: each budget it
    /// visited is queued exactly when it has a ready thread (the running one may stay queued
    /// without one). A walk of the budgets visited, never of every slot.
    pub fn check_visited<S: Ready<B>>(&self, bs: &S, running: Option<B>) -> Result<(), Missed> {
        let visited = self.lost[..self.n_lost].iter().chain(&self.gained[..self.n_gained]);
        for &b in visited.filter(|b| bs.live(**b)) {
            let (queued, ready) = (bs.state(b).queued, bs.ready(b) > 0);
            if queued != ready && !(queued && running == Some(b)) {
                return Err(Missed::Queue { id: bs.id(b), queued });
            }
        }
        Ok(())
    }

    /// Whether the checked kernel walks every live process now ([`Marks::audit`]), at `now`: at
    /// the first reconcile that visited a budget once `every` has passed since the last walk, and
    /// whenever the hart is about to idle (`idle`), so that a lone missed mark cannot leave a
    /// budget unqueued for good.
    pub fn audit_due(&mut self, now: u64, every: u64, idle: bool) -> bool {
        let due = idle || self.visited && now.saturating_sub(self.walked) >= every;
        if due {
            self.walked = now;
        }
        due
    }

    /// The checked kernel's audit, after a reconcile: every slot's count is what `now` (a walk of
    /// the slot) says, each budget's count is the sum of its slots', and the queue holds exactly
    /// the budgets with a ready thread, and `running` if it is queued. `live` names, once each,
    /// every slot that may have ready threads (those with a process in a budget); a slot counted
    /// and not among them ended with no mark. It walks those slots, never all `N`.
    pub fn audit<S: Ready<B>, const Q: usize>(
        &mut self,
        bs: &S,
        q: &Queue<B, Q>,
        running: Option<B>,
        live: impl Iterator<Item = usize>,
        now: impl Fn(&S, usize) -> (u32, Option<B>),
    ) -> Result<(), Missed> {
        // Between settles every slot is counted as it was last settled. The slots with ready
        // threads gather in the marked list's array, empty now, to be sorted by their budget's id.
        debug_assert!(self.n == 0, "the audit runs between settles");
        let mut m = 0;
        for i in live {
            let (was, from) = self.counted[i];
            let (has, b) = now(bs, i);
            let has = if b.is_some() { has } else { 0 };
            if was != has || (has > 0 && b != Some(from)) {
                return Err(Missed::Slot { slot: i, counted: was, has });
            }
            if was > 0 {
                self.slots[m] = i;
                m += 1;
            }
        }
        if m != self.n_counted {
            // Which slot: a scan only when the audit fails.
            let walked = &self.slots[..m];
            let slot = (0..N).find(|i| self.counted[*i].0 > 0 && !walked.contains(i)).unwrap_or(0);
            return Err(Missed::Slot { slot, counted: self.counted[slot].0, has: 0 });
        }
        let counted = &self.counted;
        let by_id = &mut self.slots[..m];
        by_id.sort_unstable_by_key(|i| bs.id(counted[*i].1));
        let mut budgets = 0;
        let mut k = 0;
        while k < m {
            let b = counted[by_id[k]].1;
            let mut sum = 0u32;
            while k < m && counted[by_id[k]].1 == b {
                sum = sum.saturating_add(counted[by_id[k]].0);
                k += 1;
            }
            budgets += 1;
            let kept = bs.ready(b);
            if kept != sum {
                return Err(Missed::Count { id: bs.id(b), kept, sum });
            }
        }
        // Every queued budget but the running one has ready threads, so is among them; and as many
        // of them are queued as there are, so all are.
        let mut queued = 0;
        for b in q.queued() {
            let found = by_id.binary_search_by_key(&bs.id(b), |i| bs.id(counted[*i].1)).is_ok();
            if !bs.state(b).queued || !found && running != Some(b) {
                return Err(Missed::Queue { id: bs.id(b), queued: true });
            }
            queued += usize::from(found);
        }
        if queued != budgets {
            // Which one is not: a search only when the audit fails.
            let b = by_id.iter().map(|i| counted[*i].1).find(|b| !q.contains(*b));
            return Err(Missed::Queue { id: b.map_or(0, |b| bs.id(b)), queued: false });
        }
        Ok(())
    }
}
