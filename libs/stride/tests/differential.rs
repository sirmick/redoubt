//! The kernel's stride rules against the executable model: the same random sequence of budget
//! creations and destructions, thread wakes and blocks, runs, slice ends and preemptions, driven
//! through `redoubt_model::sched::Scheduler` and through this crate's [`Harts`], the wiring the
//! kernel's `sched.rs` calls (a deschedule, a pick, a creation, a weight change, a destruction; the
//! budgets' state in a store), at 1, 2 and 4 harts. Only the thread bookkeeping and the clock are
//! this harness's own; a reconcile visits only the budgets whose threads changed since the last,
//! as the kernel's does, and each budget's threads no hart runs are counted at every change, as the
//! kernel's settle counts them before a switch or a reconcile. Every pass, entry, remainder, tie,
//! queue membership, the floor, the tie counters, the cap set and each hart's running thread must
//! agree after every step.
//!
//! Destructions come in the kernel's shapes too: a leaf whose threads were blocked first; a
//! budget on a hart, destroyed with its threads (a deadline: nothing deschedules it first); and a
//! whole subtree at once, bottom-up (R10's order), the top's carve returned first (the kernel's
//! `mark_dying`).

use std::collections::{BTreeMap, BTreeSet};

use redoubt_model::mutation::Mutation;
use redoubt_model::sched::{Current, Scheduler};
use redoubt_model::spec::SLICE;
use redoubt_stride::{Budgets, Harts, State};

type Thread = (u64, u64);

/// The most harts a run drives.
const HARTS: usize = 4;

struct Budget {
    state: State,
    parent: Option<u64>,
    limit: u64,
    carved: u64,
    threads: BTreeSet<Thread>,
    cursor: Option<Thread>,
}

#[derive(Default)]
struct Store(BTreeMap<u64, Budget>);

impl Budgets<u64> for Store {
    fn state(&self, b: u64) -> State { self.0[&b].state }

    fn set_state(&mut self, b: u64, s: State) { self.0.get_mut(&b).unwrap().state = s; }

    fn id(&self, b: u64) -> u64 { b }

    fn weight(&self, b: u64) -> u64 { self.0.get(&b).map_or(0, |x| x.limit - x.carved) }

    fn live(&self, b: u64) -> bool { self.0.contains_key(&b) }
}

/// The kernel's wiring ([`Harts`]) over a store, with each hart's thread and its slice.
struct Kernel {
    cpu: Harts<u64, 64, HARTS>,
    harts: usize,
    bs: Store,
    thread: [Option<Thread>; HARTS],
    slice_left: [u64; HARTS],
    /// Destructions begun: their carves are back with their parents.
    returned: BTreeSet<u64>,
    /// The budgets whose threads changed since the last reconcile, as the kernel marks them: the
    /// only ones a reconcile visits.
    marked: Vec<u64>,
}

impl Kernel {
    fn new(harts: usize) -> Kernel {
        let mut cpu = Harts::new();
        cpu.set_harts(harts as u32, |_| 0, |_| 0);
        Kernel {
            cpu,
            harts,
            bs: Store::default(),
            thread: [None; HARTS],
            slice_left: [0; HARTS],
            returned: BTreeSet::new(),
            marked: Vec::new(),
        }
    }

    fn current(&self, h: usize) -> Option<Current> {
        let budget = self.cpu.cur(h)?;
        let pending = self.cpu.runners[h].pending;
        Some(Current { budget, thread: self.thread[h]?, pending, slice_left: self.slice_left[h] })
    }

    /// `b`'s threads no hart runs.
    fn waiting(&self, b: u64) -> u32 {
        let busy: BTreeSet<Thread> = self.thread.iter().flatten().copied().collect();
        self.bs.0.get(&b).map_or(0, |x| x.threads.difference(&busy).count() as u32)
    }

    /// The kernel's settle for `b`: its threads no hart runs, counted.
    fn settle(&mut self, b: u64) {
        let n = self.waiting(b);
        self.cpu.set_waiting(b, n);
    }

    fn deschedule(&mut self, h: usize) {
        let Some(b) = self.cpu.cur(h) else { return };
        self.thread[h] = None;
        self.settle(b);
        let runnable = |bs: &Store, b: u64| !bs.0[&b].threads.is_empty();
        self.cpu.switch(h, &mut self.bs, None, runnable);
    }

    fn reconcile(&mut self) {
        let mut marked = std::mem::take(&mut self.marked);
        let lost = marked.clone();
        let busy: BTreeSet<Thread> = self.thread.iter().flatten().copied().collect();
        self.cpu.reconcile(&mut self.bs, &lost, &mut marked, |bs, b| {
            bs.0[&b].threads.difference(&busy).count() as u32
        });
    }

    fn add_budget(&mut self, id: u64, parent: Option<u64>, limit: u64) {
        self.bs.0.insert(
            id,
            Budget {
                state: State::default(),
                parent,
                limit,
                carved: 0,
                threads: BTreeSet::new(),
                cursor: None,
            },
        );
        self.cpu.create(&mut self.bs, id, parent);
        if let Some(p) = parent {
            self.cpu.change_weight(&mut self.bs, p, |bs| bs.0.get_mut(&p).unwrap().carved += limit);
        }
    }

    /// The first step of destroying `top` (the kernel's `mark_dying`): its carve goes back to its
    /// parent before anything else.
    fn return_carve(&mut self, top: u64) {
        let Some(p) = self.bs.0[&top].parent else { return };
        let limit = self.bs.0[&top].limit;
        self.cpu.change_weight(&mut self.bs, p, |bs| bs.0.get_mut(&p).unwrap().carved -= limit);
        self.returned.insert(top);
    }

    /// Destroy `b` (its children already gone): its carve goes back to its parent, unless it
    /// went back at the start.
    fn destroy_budget(&mut self, b: u64) {
        let parent = self.bs.0[&b].parent;
        let limit = self.bs.0[&b].limit;
        let returned = self.returned.remove(&b);
        self.cpu.destroy(&mut self.bs, b, parent, |bs| {
            if let (false, Some(p)) = (returned, parent) {
                bs.0.get_mut(&p).unwrap().carved -= limit;
            }
        });
        for h in 0..self.harts {
            if self.cpu.cur(h).is_none() {
                self.thread[h] = None;
            }
        }
        self.bs.0.remove(&b);
    }

    /// R10 for the subtree at `top` (`bottom_up`: every budget in it, each after its
    /// descendants): the top's carve returns first, its threads end with no deschedule, then each
    /// budget goes, the ones below the top returning their carves as they do.
    fn destroy_subtree(&mut self, top: u64, bottom_up: &[u64]) {
        self.return_carve(top);
        for b in bottom_up {
            self.bs.0.get_mut(b).unwrap().threads.clear();
            self.marked.push(*b);
        }
        for &b in bottom_up {
            self.destroy_budget(b);
        }
    }

    fn thread_runnable(&mut self, b: u64, t: Thread) {
        self.bs.0.get_mut(&b).unwrap().threads.insert(t);
        self.marked.push(b);
        self.settle(b);
    }

    fn thread_blocked(&mut self, b: u64, t: Thread) {
        self.bs.0.get_mut(&b).unwrap().threads.remove(&t);
        self.marked.push(b);
        match (0..self.harts).find(|h| self.cpu.cur(*h) == Some(b) && self.thread[*h] == Some(t)) {
            Some(h) => self.deschedule(h),
            None => self.settle(b),
        }
    }

    fn pick(&mut self, h: usize) -> Option<Current> {
        self.reconcile();
        if self.cpu.cur(h).is_some() {
            return self.current(h);
        }
        let busy: BTreeSet<Thread> = self.thread.iter().flatten().copied().collect();
        let (b, t) = self.cpu.pick(&mut self.bs, |bs, b| {
            let x = &bs.0[&b];
            let free = |t: &&Thread| !busy.contains(*t);
            x.cursor
                .and_then(|c| {
                    x.threads.range((std::ops::Bound::Excluded(c), std::ops::Bound::Unbounded)).find(free)
                })
                .or_else(|| x.threads.iter().find(free))
                .copied()
        })?;
        self.bs.0.get_mut(&b).unwrap().cursor = Some(t);
        self.thread[h] = Some(t);
        self.cpu.switch(h, &mut self.bs, Some(b), |_, _| true);
        self.settle(b);
        self.slice_left[h] = SLICE;
        self.current(h)
    }

    fn run(&mut self, h: usize, dt: u64) {
        if self.cpu.cur(h).is_some() {
            let dt = dt.min(self.slice_left[h]);
            self.cpu.accrue(h, dt);
            self.slice_left[h] -= dt;
        }
    }

    fn slice_end(&mut self, h: usize) {
        if self.cpu.cur(h).is_some() && self.slice_left[h] == 0 {
            self.deschedule(h);
        }
    }

    fn preempt(&mut self, h: usize) {
        if self.cpu.cur(h).is_some() {
            self.deschedule(h);
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: u64) -> u64 { self.next() % n.max(1) }

    fn pick<T: Copy>(&mut self, xs: &[T]) -> Option<T> {
        (!xs.is_empty()).then(|| xs[self.below(xs.len() as u64) as usize])
    }
}

fn compare(m: &Scheduler, k: &Kernel, step: usize, seed: u64) {
    let at = || format!("seed {seed} step {step} at {} harts", k.harts);
    assert_eq!(m.budgets.keys().collect::<Vec<_>>(), k.bs.0.keys().collect::<Vec<_>>(), "{} budgets", at());
    for (id, e) in &m.budgets {
        let s = k.bs.0[id].state;
        assert_eq!(
            (e.pass, e.entry, e.rem, e.tie, e.queued),
            (s.pass, s.entry, s.rem, s.tie, s.queued),
            "{} budget {id}",
            at()
        );
        assert_eq!(m.weight(*id), k.bs.weight(*id), "{} weight of {id}", at());
    }
    let q = &k.cpu.q;
    assert_eq!((m.floor, m.front, m.back), (q.floor, q.front, q.back), "{} floor and counters", at());
    assert_eq!(m.capped, q.capped().collect::<BTreeSet<_>>(), "{} cap set", at());
    // The ranks the queue keeps beside its slots are the stored states' after every step.
    assert_eq!(q.audit(&k.bs), Ok(()), "{} ranks", at());
    for h in 0..k.harts {
        assert_eq!(m.on(h), k.current(h), "{} hart {h}'s running thread", at());
    }
}

fn run(seed: u64, harts: usize) { run_with(seed, harts, None) }

fn run_with(seed: u64, harts: usize, mutation: Option<Mutation>) {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut m = Scheduler { mutation, ..Scheduler::default() };
    m.set_harts(harts);
    let mut k = Kernel::new(harts);
    const ROOT: u64 = 1;
    m.add_budget(ROOT, None, 1 << 31);
    k.add_budget(ROOT, None, 1 << 31);
    let mut next_id = 2;
    for step in 0..400 {
        let ids: Vec<u64> = m.budgets.keys().copied().collect();
        let h = rng.below(harts as u64) as usize;
        match rng.below(12) {
            // Create a budget under any budget, carving what the rules allow.
            0 | 1 => {
                let p = rng.pick(&ids).unwrap();
                let free = m.weight(p);
                let holds = !m.budgets[&p].runnable.is_empty();
                let max = if holds { free.saturating_sub(1) } else { free };
                if max == 0 || m.budgets.len() > 40 {
                    continue;
                }
                let limit = match rng.below(3) {
                    0 => 1 + rng.below(max.min(4)),
                    1 => 1 + rng.below(max.min(1000)),
                    _ => 1 + rng.below(max),
                };
                m.add_budget(next_id, Some(p), limit);
                k.add_budget(next_id, Some(p), limit);
                next_id += 1;
            }
            // Destroy a budget with no children (its threads end first, as R10 kills them).
            2 => {
                let leaves: Vec<u64> = ids
                    .iter()
                    .copied()
                    .filter(|b| *b != ROOT && !m.budgets.values().any(|x| x.parent == Some(*b)))
                    .collect();
                let Some(b) = rng.pick(&leaves) else { continue };
                let ts: Vec<Thread> = m.budgets[&b].runnable.iter().copied().collect();
                for t in ts {
                    m.thread_exited(b, t);
                    k.thread_blocked(b, t);
                }
                m.return_carve(b);
                m.destroy_budget(b);
                k.return_carve(b);
                k.destroy_budget(b);
            }
            // A thread becomes runnable, in a budget with free weight.
            3 | 4 => {
                let with_weight: Vec<u64> =
                    ids.iter().copied().filter(|b| *b != ROOT && m.weight(*b) > 0).collect();
                let Some(b) = rng.pick(&with_weight) else { continue };
                let t = (b, rng.below(3));
                m.thread_runnable(b, t);
                k.thread_runnable(b, t);
            }
            // A runnable thread blocks (one a hart runs, often).
            5 => {
                let t = if rng.below(2) == 0 {
                    m.on(h).map(|c| (c.budget, c.thread))
                } else {
                    let all: Vec<(u64, Thread)> = m
                        .budgets
                        .iter()
                        .flat_map(|(b, e)| e.runnable.iter().map(move |t| (*b, *t)))
                        .collect();
                    rng.pick(&all)
                };
                let Some((b, t)) = t else { continue };
                m.thread_blocked(b, t);
                k.thread_blocked(b, t);
            }
            6 => {
                m.reconcile();
                k.reconcile();
            }
            // Time passes for whatever a hart runs.
            7..=9 => {
                assert_eq!(m.pick_on(h), k.pick(h), "seed {seed} step {step} hart {h}'s pick");
                let dt = match rng.below(3) {
                    0 => SLICE,
                    1 => 1 + rng.below(SLICE),
                    _ => 1 + rng.below(20),
                };
                m.run_on(h, dt);
                k.run(h, dt);
                m.slice_end_on(h);
                k.slice_end(h);
            }
            // A budget deadline elsewhere: a hart's thread is preempted.
            10 => {
                m.preempt_on(h);
                k.preempt(h);
            }
            // A deadline (or a destroy) takes a whole subtree, a budget a hart runs perhaps in it:
            // its threads die with it, nothing descheduled first, bottom-up.
            11 if rng.below(2) == 0 => {
                let top = match m.on(h) {
                    Some(c) if c.budget != ROOT && rng.below(2) == 0 => c.budget,
                    _ => {
                        let others: Vec<u64> = ids.iter().copied().filter(|b| *b != ROOT).collect();
                        let Some(b) = rng.pick(&others) else { continue };
                        b
                    }
                };
                let mut subtree = vec![top];
                let mut i = 0;
                while i < subtree.len() {
                    let b = subtree[i];
                    subtree.extend(m.budgets.iter().filter(|(_, e)| e.parent == Some(b)).map(|(id, _)| *id));
                    i += 1;
                }
                // Deepest first: the reverse of the breadth-first order.
                subtree.reverse();
                m.return_carve(top);
                for b in &subtree {
                    m.destroy_budget(*b);
                }
                k.destroy_subtree(top, &subtree);
            }
            _ => {
                assert_eq!(m.pick_on(h), k.pick(h), "seed {seed} step {step} hart {h}'s pick");
            }
        }
        compare(&m, &k, step, seed);
    }
}

#[test]
fn the_crate_and_the_model_agree() {
    for harts in [1, 2, 4] {
        for seed in 0..3000 {
            run(seed, harts);
        }
    }
}

/// The comparison bites: a model with any of the scheduler's arithmetic or rank rules broken
/// disagrees with the crate on some sequence. Five R12 variants are not driven here:
/// `R12TimeoutWakePreempts` breaks the kernel model's timer path, not `Scheduler`, and
/// `R12SliceCountsExitWork`, `R12DeadlineWorkUnbilled`, `R12TimerWorkUnbilled` and
/// `R12SwitchBilledToPrevious` kernel work around a run (the exit work before a slice starts, a
/// deadline's destruction, a timer's expiry, the switch into a budget), which this harness
/// does not do. The model's own checks catch all five (`sched_contracts`, `scheduler_fairness`).
#[test]
fn a_broken_model_disagrees() {
    let quiet = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut missed = Vec::new();
    for m in [
        Mutation::R12IgnoreWeight,
        Mutation::R12WakeBanksCredit,
        Mutation::R12TieQueuedFirst,
        Mutation::R12RequeueAhead,
        Mutation::R12RequeueLifo,
        Mutation::R12NoFloorWhenIdle,
        Mutation::R12ShortRunsFree,
        Mutation::R12DropRemainder,
        Mutation::R12DestroyDropsDebt,
        Mutation::R12CreateAtFloorOnly,
        Mutation::R12LiftByMax,
        Mutation::R12StrideWeightIsLimit,
        Mutation::R12UnnormalizedLift,
        Mutation::R12LiftCountsEntryWait,
        Mutation::R12FoldAtNewWeight,
        Mutation::R12PriorityById,
        Mutation::R12PreemptOnWake,
        Mutation::R12NoMinimumCharge,
        Mutation::R12ExitRunsFree,
        Mutation::R12RescaleOnlyOnReturn,
    ] {
        let seen = (0..3000).any(|seed| std::panic::catch_unwind(|| run_with(seed, 1, Some(m))).is_err());
        if !seen {
            missed.push(m);
        }
    }
    // The rules of several harts, at two and at four.
    for m in [
        Mutation::R12CappedHoldsFloor,
        Mutation::R12CapOnce,
        Mutation::R12AllCappedHoldsFloor,
        Mutation::R12UncapBanksCredit,
        Mutation::R12OneRunnerPerBudget,
        Mutation::R12SpreadChargesOnce,
    ] {
        let seen = [2, 4].iter().any(|harts| {
            (0..3000).any(|seed| std::panic::catch_unwind(|| run_with(seed, *harts, Some(m))).is_err())
        });
        if !seen {
            missed.push(m);
        }
    }
    std::panic::set_hook(quiet);
    assert!(missed.is_empty(), "the differential missed {missed:?}");
}
