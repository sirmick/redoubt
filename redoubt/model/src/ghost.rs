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

use crate::kernel::Endpoint;
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
    /// I6: each budget's labels when it was created, and its creator's class.
    pub labels_at_creation: BTreeMap<u64, Vec<u64>>,
    pub creator_class: BTreeMap<u64, Class>,
    /// I12: every budget id ever issued.
    pub ever_budgets: BTreeSet<u64>,
    /// I11, keyed by (endpoint, waiting account).
    pub waiting: BTreeMap<(u64, u64), Waiting>,
    pub irqs: BTreeMap<u64, Irq>,
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

    /// A `receive` on the IRQ that has just begun is about to block: no raised interrupt may be
    /// undelivered then, or it would wait unseen (R5: the receive unmasks the source, and a
    /// pending line fires at once). A waiter that times out while another waiter's interrupt
    /// keeps the source masked is not a lost interrupt: the next receive gets it.
    pub fn irq_not_delivered(&mut self, d: u64) {
        if self.irqs.get(&d).is_some_and(|i| i.undelivered) {
            self.violations
                .push(format!("R5: a receive on IRQ device {d} waits while its interrupt is undelivered"));
        }
    }
}
