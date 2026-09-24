//! The kernel's stride rules against the executable model: the same random sequence of budget
//! creations and destructions, thread wakes and blocks, runs, slice ends and preemptions, driven
//! through `redoubt_model::sched::Scheduler` and through this crate's [`Cpu`], the wiring the
//! kernel's `sched.rs` calls (a deschedule, a pick, a creation, a weight change, a destruction; the
//! budgets' state in a store). Only the thread bookkeeping and the clock are this harness's own.
//! Every pass, entry, remainder, tie, queue membership, the floor, the tie counters and the running
//! thread must agree after every step.
//!
//! Destructions come in the kernel's shapes too: a leaf whose threads were blocked first; the
//! budget on the CPU, destroyed with its threads (a deadline: nothing deschedules it first); and a
//! whole subtree at once, bottom-up (R10's order).

use std::collections::{BTreeMap, BTreeSet};

use redoubt_model::mutation::Mutation;
use redoubt_model::sched::{Current, Scheduler};
use redoubt_model::spec::SLICE;
use redoubt_stride::{Budgets, Cpu, State};

type Thread = (u64, u64);

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

/// The kernel's wiring ([`Cpu`]) over a store, with the thread on the CPU and its slice.
#[derive(Default)]
struct Kernel {
    cpu: Cpu<u64, 64>,
    bs: Store,
    thread: Option<Thread>,
    slice_left: u64,
}

impl Kernel {
    fn current(&self) -> Option<Current> {
        let budget = self.cpu.cur?;
        Some(Current { budget, thread: self.thread?, pending: self.cpu.pending, slice_left: self.slice_left })
    }

    fn deschedule(&mut self) {
        self.cpu.switch(&mut self.bs, None, |bs, b| !bs.0[&b].threads.is_empty());
        self.thread = None;
    }

    fn runnable(&self) -> Vec<u64> {
        self.bs.0.iter().filter(|(_, b)| !b.threads.is_empty()).map(|(id, _)| *id).collect()
    }

    fn reconcile(&mut self) {
        let runnable = self.runnable();
        self.cpu.reconcile(&mut self.bs, &runnable);
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

    /// Destroy `b` (its children already gone): its carve goes back to its parent.
    fn destroy_budget(&mut self, b: u64) {
        let parent = self.bs.0[&b].parent;
        let limit = self.bs.0[&b].limit;
        self.cpu.destroy(&mut self.bs, b, parent, |bs| {
            if let Some(p) = parent {
                bs.0.get_mut(&p).unwrap().carved -= limit;
            }
        });
        if self.cpu.cur.is_none() {
            self.thread = None;
        }
        self.bs.0.remove(&b);
    }

    /// R10 for a subtree (`bottom_up`: every budget in it, each after its descendants): its
    /// threads end with no deschedule, then each budget goes, returning its carve as it does.
    fn destroy_subtree(&mut self, bottom_up: &[u64]) {
        for b in bottom_up {
            self.bs.0.get_mut(b).unwrap().threads.clear();
        }
        for &b in bottom_up {
            self.destroy_budget(b);
        }
    }

    fn thread_runnable(&mut self, b: u64, t: Thread) { self.bs.0.get_mut(&b).unwrap().threads.insert(t); }

    fn thread_blocked(&mut self, b: u64, t: Thread) {
        self.bs.0.get_mut(&b).unwrap().threads.remove(&t);
        if self.cpu.cur == Some(b) && self.thread == Some(t) {
            self.deschedule();
        }
    }

    fn pick(&mut self) -> Option<Current> {
        self.reconcile();
        if self.cpu.cur.is_some() {
            return self.current();
        }
        let (b, t) = self.cpu.pick(&mut self.bs, |bs, b| {
            let x = &bs.0[&b];
            x.cursor
                .and_then(|c| {
                    x.threads.range((std::ops::Bound::Excluded(c), std::ops::Bound::Unbounded)).next()
                })
                .or_else(|| x.threads.iter().next())
                .copied()
        })?;
        self.bs.0.get_mut(&b).unwrap().cursor = Some(t);
        self.cpu.switch(&mut self.bs, Some(b), |_, _| true);
        self.thread = Some(t);
        self.slice_left = SLICE;
        self.current()
    }

    fn run(&mut self, dt: u64) {
        if self.cpu.cur.is_some() {
            let dt = dt.min(self.slice_left);
            self.cpu.accrue(dt);
            self.slice_left -= dt;
        }
    }

    fn slice_end(&mut self) {
        if self.cpu.cur.is_some() && self.slice_left == 0 {
            self.deschedule();
        }
    }

    fn preempt(&mut self) {
        if self.cpu.cur.is_some() {
            self.deschedule();
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
    let at = || format!("seed {seed} step {step}");
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
    assert_eq!(m.current, k.current(), "{} running thread", at());
}

fn run(seed: u64) { run_with(seed, None) }

fn run_with(seed: u64, mutation: Option<Mutation>) {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let mut m = Scheduler { mutation, ..Scheduler::default() };
    let mut k = Kernel::default();
    const ROOT: u64 = 1;
    m.add_budget(ROOT, None, 1 << 31);
    k.add_budget(ROOT, None, 1 << 31);
    let mut next_id = 2;
    for step in 0..400 {
        let ids: Vec<u64> = m.budgets.keys().copied().collect();
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
                m.destroy_budget(b);
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
            // A runnable thread blocks (the running one, often).
            5 => {
                let t = if rng.below(2) == 0 {
                    m.current.map(|c| (c.budget, c.thread))
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
            // Time passes for whatever runs.
            7..=9 => {
                assert_eq!(m.pick(), k.pick(), "seed {seed} step {step} pick");
                let dt = match rng.below(3) {
                    0 => SLICE,
                    1 => 1 + rng.below(SLICE),
                    _ => 1 + rng.below(20),
                };
                m.run(dt);
                k.run(dt);
                m.slice_end();
                k.slice_end();
            }
            // A budget deadline elsewhere: the running thread is preempted.
            10 => {
                m.preempt();
                k.preempt();
            }
            // A deadline (or a destroy) takes a whole subtree, the running budget perhaps in it:
            // its threads die with it, nothing descheduled first, bottom-up.
            11 if rng.below(2) == 0 => {
                let top = match m.current {
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
                for b in &subtree {
                    m.destroy_budget(*b);
                }
                k.destroy_subtree(&subtree);
            }
            _ => {
                assert_eq!(m.pick(), k.pick(), "seed {seed} step {step} pick");
            }
        }
        compare(&m, &k, step, seed);
    }
}

#[test]
fn the_crate_and_the_model_agree() {
    for seed in 0..3000 {
        run(seed);
    }
}

/// The comparison bites: a model with any of the scheduler's arithmetic or rank rules broken
/// disagrees with the crate on some sequence.
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
    ] {
        let seen = (0..3000).any(|seed| std::panic::catch_unwind(|| run_with(seed, Some(m))).is_err());
        if !seen {
            missed.push(m);
        }
    }
    std::panic::set_hook(quiet);
    assert!(missed.is_empty(), "the differential missed {missed:?}");
}
