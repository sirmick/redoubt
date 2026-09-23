//! R12/answer 103: one flat weighted stride queue over every runnable budget;
//! round-robin threads within each budget.
//!
//! The kernel model tells the scheduler when a thread becomes runnable ([`Scheduler::wake`]) or
//! stops being runnable ([`Scheduler::block`]), asks it what to run ([`Scheduler::pick`]) and
//! reports how long that ran ([`Scheduler::charge`]). Ties in pass go to the lower budget id (the
//! spec does not order them; any fixed order satisfies it).
//!
//! A budget with weight 0 never runs: its stride would be infinite, and a weight-0 budget holds
//! no process (QUESTIONS 12).

use alloc::collections::{BTreeMap, VecDeque};

use crate::mutation::Mutation;
use crate::spec::{SLICE, STRIDE};

#[derive(Clone, Debug)]
pub struct Entry {
    pub weight: u64,
    pub pass: u128,
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

    pub fn add_budget(&mut self, id: u64, weight: u64) {
        self.budgets.insert(id, Entry { weight, pass: 0, runnable: VecDeque::new() });
    }

    pub fn remove_budget(&mut self, id: u64) { self.budgets.remove(&id); }

    /// The lowest pass among budgets that could run, other than `except`.
    fn min_pass(&self, except: u64) -> Option<u128> {
        self.budgets
            .iter()
            .filter(|(id, e)| **id != except && e.weight > 0 && !e.runnable.is_empty())
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
        let min = self.min_pass(budget);
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

    /// What runs next: the lowest pass over all budgets; within the budget, the front of its round-robin
    /// queue.
    pub fn pick(&self) -> Option<(u64, u64)> {
        let priority = self.broken(Mutation::R12PriorityById);
        self.budgets
            .iter()
            .filter(|(_, e)| e.weight > 0 && !e.runnable.is_empty())
            .min_by_key(|(id, e)| (if priority { **id } else { 0 }, e.pass, **id))
            .map(|(id, e)| (*id, e.runnable[0]))
    }

    /// The front thread of `budget` ran for `runtime` µs and was descheduled: pass += runtime x
    /// `STRIDE` / weight, and the thread goes to the back of its budget's queue.
    pub fn charge(&mut self, budget: u64, runtime: u64) { self.charge_runs(budget, runtime, 1); }

    /// `charge(budget, SLICE)` `n` times over, at once.
    pub fn charge_slices(&mut self, budget: u64, n: u64) { self.charge_runs(budget, SLICE, n); }

    fn charge_runs(&mut self, budget: u64, runtime: u64, n: u64) {
        let ignore_weight = self.broken(Mutation::R12IgnoreWeight);
        let Some(e) = self.budgets.get_mut(&budget) else { return };
        if e.weight == 0 {
            return;
        }
        let weight = if ignore_weight { 1 } else { e.weight };
        // Logical elapsed time is u64; its product with STRIDE fits comfortably in u128.
        // Saturation would collapse unequal passes and turn the id tie-break into starvation.
        let step = runtime as u128 * STRIDE as u128 / weight as u128;
        e.pass += step * n as u128;
        if !e.runnable.is_empty() {
            let k = (n % e.runnable.len() as u64) as usize;
            e.runnable.rotate_left(k);
        }
    }

    /// How many budgets have a thread that could run.
    pub fn runnable_budgets(&self) -> usize {
        self.budgets.values().filter(|e| e.weight > 0 && !e.runnable.is_empty()).count()
    }
}
