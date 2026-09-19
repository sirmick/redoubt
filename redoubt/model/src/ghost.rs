//! Ghost state: history the invariants need that the kernel itself does not keep.
//!
//! The kernel model records events here (a budget was created, a message was taken, an
//! interrupt fired); the checks in invariants.rs judge them. Nothing in the kernel model reads
//! ghost state to decide what to do, so a bug (or a mutation) in the kernel cannot hide itself by
//! also changing what the check believes.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::{Endpoint, Kernel};
use crate::spec::Class;

/// An information flow the kernel allowed in the current step (I7, R4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flow {
    Message {
        from_class: Class,
        from: Vec<u64>,
        to_class: Class,
        to: Vec<u64>,
        transfer: u64,
        max_transfer: u64,
    },
    Exit {
        from: Vec<u64>,
        to: Vec<u64>,
    },
    Usage {
        from: Vec<u64>,
        to: Vec<u64>,
    },
}

/// I11: while an account's oldest message waits on an endpoint, how often each other account
/// has been taken.
#[derive(Clone, Debug, Default)]
pub struct Waiting {
    pub head: u64,
    pub taken: BTreeMap<u64, u32>,
}

#[derive(Clone, Debug, Default)]
pub struct Irq {
    /// Fires since the last `receive` on the source began (R5: at most one, the source is
    /// masked until the next receive).
    pub fires_since_receive: u32,
    /// The line was raised and no `receive` has returned the interrupt since.
    pub undelivered: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Ghost {
    /// Flows allowed in the current step.
    pub flows: Vec<Flow>,
    /// Frames handed out by `map_anon`/`dma_alloc` and not written since (I9: they read 0).
    pub fresh: BTreeSet<u64>,
    /// What a freed frame still holds in RAM.
    pub freed_content: BTreeMap<u64, u64>,
    /// I6: each budget's labels when it was created, and its creator's class.
    pub labels_at_creation: BTreeMap<u64, Vec<u64>>,
    pub creator_class: BTreeMap<u64, Class>,
    /// I12: every budget id ever issued.
    pub ever_budgets: BTreeSet<u64>,
    /// I11, keyed by (endpoint, waiting account).
    pub waiting: BTreeMap<(u64, u64), Waiting>,
    pub irqs: BTreeMap<u64, Irq>,
    /// CPU time each budget received during ticks (R12).
    pub runtime: BTreeMap<u64, u64>,
    /// Violations found while a step ran (the checks run after it).
    pub violations: Vec<String>,
}

impl Ghost {
    pub fn begin_step(&mut self) {
        self.flows.clear();
    }

    pub fn budget_created(&mut self, id: u64, labels: &[u64], creator: Class) {
        if !self.ever_budgets.insert(id) {
            self.violations.push(format!("I12: budget id {id} reused"));
        }
        self.labels_at_creation.insert(id, labels.to_vec());
        self.creator_class.insert(id, creator);
    }

    /// A message of `account` is taken from endpoint `e` (delivered, or refused/denied in its
    /// turn). `ep` is the endpoint before the message leaves its queue.
    pub fn took(&mut self, e: u64, account: u64, ep: &Endpoint) {
        self.waiting.remove(&(e, account));
        for (other, q) in &ep.queue {
            if *other == account {
                continue;
            }
            let Some(head) = q.front().copied() else { continue };
            let w = self.waiting.entry((e, *other)).or_default();
            if w.head != head {
                *w = Waiting { head, taken: BTreeMap::new() };
            }
            let n = w.taken.entry(account).or_insert(0);
            *n += 1;
            if *n >= 2 {
                self.violations.push(format!(
                    "I11: on endpoint {e}, account {account} was taken twice while account {other}'s oldest message \
                     {head} waited"
                ));
            }
        }
        // Forget trackers for accounts that no longer wait.
        self.waiting.retain(|(ee, a), _| *ee != e || ep.queue.contains_key(a));
    }

    pub fn irq_raised(&mut self, d: u64) {
        self.irqs.entry(d).or_default().undelivered = true;
    }

    pub fn irq_fired(&mut self, d: u64) {
        let i = self.irqs.entry(d).or_default();
        i.fires_since_receive += 1;
        if i.fires_since_receive > 1 {
            self.violations.push(format!("R5: IRQ device {d} fired twice without a receive (not masked)"));
        }
    }

    pub fn irq_receive_begins(&mut self, d: u64) {
        self.irqs.entry(d).or_default().fires_since_receive = 0;
    }

    pub fn irq_delivered(&mut self, d: u64) {
        self.irqs.entry(d).or_default().undelivered = false;
    }

    /// A `receive` on the IRQ blocks or times out: there must be no raised, undelivered
    /// interrupt (it would be lost).
    pub fn irq_not_delivered(&mut self, d: u64) {
        if self.irqs.get(&d).is_some_and(|i| i.undelivered) {
            self.violations
                .push(format!("R5: a receive on IRQ device {d} waits while its interrupt is undelivered"));
        }
    }
}

/// R12, checked at every pick during a tick: a user-class budget never runs while a system-class
/// budget has a runnable thread; something runs whenever some budget with weight can.
pub fn check_pick(pick: Option<(u64, u64)>, k: &Kernel) -> Option<String> {
    let runnable_in = |class: Class| {
        k.threads.values().any(|t| {
            t.wait.is_none()
                && k.budget_of(t.pid)
                    .and_then(|b| k.budgets.get(&b))
                    .is_some_and(|b| b.class == class && b.weight > 0)
        })
    };
    match pick {
        None if runnable_in(Class::System) || runnable_in(Class::User) => {
            Some(String::from("R12: nothing scheduled although a thread is runnable"))
        }
        Some((b, tid)) => {
            let class = k.budgets.get(&b).map(|x| x.class);
            let t = k.threads.get(&tid);
            if t.is_none_or(|t| t.wait.is_some() || k.budget_of(t.pid) != Some(b)) {
                return Some(format!("R12: picked thread {tid} is not a runnable thread of budget {b}"));
            }
            if class == Some(Class::User) && runnable_in(Class::System) {
                return Some(format!("R12: user budget {b} ran while a system budget was runnable"));
            }
            None
        }
        None => None,
    }
}
