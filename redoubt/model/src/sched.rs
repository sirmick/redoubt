//! R12. Scheduling: two strictly ordered classes, one flat stride queue over budgets per class,
//! round-robin threads within a budget.
//!
//! The kernel model tells the scheduler when a thread becomes runnable ([`Scheduler::wake`]) or
//! stops being runnable ([`Scheduler::block`]), asks it what to run ([`Scheduler::pick`]) and
//! reports how long that ran ([`Scheduler::charge`]). Ties in pass go to the lower budget id (the
//! spec does not order them; any fixed order satisfies it).
//!
//! A budget with weight 0 never runs: its stride would be infinite. The spec allows weight 0 only
//! as part of a revocation scope, which has no processes, but a budget with pages and processes
//! and weight 0 can be created; its threads simply never run.

use alloc::collections::{BTreeMap, VecDeque};

use crate::mutation::Mutation;
use crate::spec::{Class, STRIDE};

#[derive(Clone, Debug)]
pub struct Entry {
    pub class: Class,
    pub weight: u64,
    pub pass: u64,
    /// Runnable threads, in round-robin order; the front one runs next.
    pub runnable: VecDeque<u64>,
}

#[derive(Clone, Debug, Default)]
pub struct Scheduler {
    pub mutation: Option<Mutation>,
    pub budgets: BTreeMap<u64, Entry>,
}

impl Scheduler {
    fn broken(&self, m: Mutation) -> bool { self.mutation == Some(m) }

    pub fn add_budget(&mut self, id: u64, class: Class, weight: u64) {
        self.budgets.insert(id, Entry { class, weight, pass: 0, runnable: VecDeque::new() });
    }

    pub fn remove_budget(&mut self, id: u64) { self.budgets.remove(&id); }

    /// The lowest pass among budgets of `class` that could run, other than `except`.
    fn min_pass(&self, class: Class, except: u64) -> Option<u64> {
        self.budgets
            .iter()
            .filter(|(id, e)| **id != except && e.class == class && e.weight > 0 && !e.runnable.is_empty())
            .map(|(_, e)| e.pass)
            .min()
    }

    /// Thread `tid` of budget `budget` became runnable. "On wake, pass = max(own pass, current
    /// minimum)": a budget that had no runnable thread cannot bank credit while asleep.
    pub fn wake(&mut self, budget: u64, tid: u64) {
        let Some(e) = self.budgets.get(&budget) else { return };
        if e.runnable.contains(&tid) {
            return;
        }
        let was_asleep = e.runnable.is_empty();
        let class = e.class;
        let min = self.min_pass(class, budget);
        let banks = self.broken(Mutation::R12WakeBanksCredit);
        let e = self.budgets.get_mut(&budget).unwrap();
        if was_asleep && !banks {
            if let Some(min) = min {
                e.pass = e.pass.max(min);
            }
        }
        e.runnable.push_back(tid);
    }

    /// Thread `tid` is no longer runnable (blocked or gone).
    pub fn block(&mut self, budget: u64, tid: u64) {
        if let Some(e) = self.budgets.get_mut(&budget) {
            e.runnable.retain(|t| *t != tid);
        }
    }

    /// What runs next: system-class budgets before user-class ones; within the class, the lowest
    /// pass; within the budget, the front of its round-robin queue.
    pub fn pick(&self) -> Option<(u64, u64)> {
        let ignore_class = self.broken(Mutation::R12NoClassOrder);
        self.budgets
            .iter()
            .filter(|(_, e)| e.weight > 0 && !e.runnable.is_empty())
            .min_by_key(|(id, e)| {
                let class_rank = if ignore_class || e.class == Class::System { 0 } else { 1 };
                (class_rank, e.pass, **id)
            })
            .map(|(id, e)| (*id, e.runnable[0]))
    }

    /// The front thread of `budget` ran for `runtime` µs and was descheduled: pass += runtime x
    /// `STRIDE` / weight, and the thread goes to the back of its budget's queue.
    pub fn charge(&mut self, budget: u64, runtime: u64) {
        let ignore_weight = self.broken(Mutation::R12IgnoreWeight);
        let Some(e) = self.budgets.get_mut(&budget) else { return };
        if e.weight == 0 {
            return;
        }
        let weight = if ignore_weight { 1 } else { e.weight };
        let step = (runtime as u128 * STRIDE as u128 / weight as u128).min(u64::MAX as u128) as u64;
        e.pass = e.pass.saturating_add(step);
        if let Some(t) = e.runnable.pop_front() {
            e.runnable.push_back(t);
        }
    }
}
