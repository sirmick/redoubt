//! Ghost state: what the invariants need that the kernel does not keep, recorded from the primary
//! objects (handle tables, budget objects, call arguments) at the moment of the event, not from
//! what the kernel derived from them.
//!
//! Nothing in the kernel model reads ghost state to decide what to do, so a bug (or a mutation) in
//! the kernel cannot hide itself by also changing what the check believes. The checks themselves
//! are in invariants.rs.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::kernel::{Endpoint, Handle, MsgKind};
use crate::spec::{Cause, Class, Error};
use crate::syscall::{Message, MintSource, Ret};

/// R2's key (QUESTIONS 17): blocked senders are grouped, capped and served round-robin by
/// (account, label set).
pub type Key = (u64, Vec<u64>);

/// A message as it was sent, recorded from the sender's handle table and budget object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sent {
    pub kind: MsgKind,
    pub sender_budget: u64,
    pub sender_tid: u64,
    pub sender_class: Class,
    /// The sender budget's labels, from the ghost record made when the budget was created.
    pub labels: Vec<u64>,
    pub account: u64,
    pub endpoint: u64,
    /// The endpoint's owner budget, its class and labels (R1 compares against it, QUESTIONS 4).
    pub owner_class: Class,
    pub owner_labels: Vec<u64>,
    /// The badge and stamp of the handle the message was sent through.
    pub badge: u64,
    pub stamp: u64,
    /// Pages lent by a `call` (0 for none).
    pub lent_pages: u64,
}

/// A thread's pending `receive` on an endpoint: the handle it named, as its table held it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Receiving {
    pub handle: Handle,
    pub max_transfer: u64,
}

/// What an exit notice reports: its cause, and whom it blames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Blame {
    pub cause: Cause,
    pub account: u64,
    pub labels: Vec<u64>,
}

impl Blame {
    /// `killed`, blaming nobody: what R10 reports.
    pub fn killed() -> Blame {
        Blame { cause: Cause::Killed, account: 0, labels: Vec::new() }
    }
}

/// A process's exit slot as `process_create` made it: who pays (the caller's budget, read from
/// the caller's process object), and the exit endpoint handle it named.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub payer: u64,
    pub endpoint: u64,
    pub stamp: u64,
}

/// An exit notice the kernel owes: to endpoint `endpoint`, paid for by budget `payer`, reporting
/// `want`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Owed {
    pub endpoint: u64,
    pub payer: u64,
    pub want: Blame,
}

/// Something the kernel did in the current step that a check must judge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Flow {
    /// A message was delivered to thread `tid`, which was receiving as `via` said.
    Delivered { tid: u64, msg: Message, via: Option<Receiving> },
    /// The exit notice of process `pid`, a budget with labels `from`, reached a receiver on an
    /// endpoint owned by a budget of class `to_class` with labels `to`, reporting `got`; the ghost
    /// expected `want` (none if it did not expect the notice).
    Exit { pid: u64, from: Vec<u64>, to_class: Class, to: Vec<u64>, got: Blame, want: Option<Blame> },
    /// `budget_usage` of a budget with labels `from` by a caller with class and labels `to`.
    Usage { from: Vec<u64>, to_class: Class, to: Vec<u64> },
    /// `budget_usage` refused (`LabelDenied`), with the same fields.
    UsageDenied { from: Vec<u64>, to_class: Class, to: Vec<u64> },
    /// A sender of class and labels `from` got `LabelDenied` from an endpoint whose owner has class
    /// and labels `to` (R1).
    LabelDenied { from_class: Class, from: Vec<u64>, to_class: Class, to: Vec<u64> },
    /// A sender got `Busy` (R2's cap) on endpoint `endpoint` for key `key`.
    Busy { endpoint: u64, key: Key },
    /// `mint` by thread `tid` of process `pid` made a handle to `endpoint` with default stamp
    /// `default_stamp`.
    Minted { pid: u64, tid: u64, endpoint: u64, via: MintSource, default_stamp: u64 },
    /// Blocked thread `tid` was woken with `result`.
    Woken { tid: u64, result: Result<Ret, Error> },
    /// Call `msg` was replied to by a thread the ghost saw take it.
    Replied { msg: u64 },
    /// A badge notice for `badge` was delivered on `endpoint`.
    BadgeClosed { endpoint: u64, badge: u64 },
}

/// I11: while a key's oldest message waits on an endpoint, how often each other key has been
/// taken.
#[derive(Clone, Debug, Default)]
pub struct Waiting {
    pub head: u64,
    pub taken: BTreeMap<Key, u32>,
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
    /// What the current step did.
    pub flows: Vec<Flow>,
    /// I6: each budget's labels when it was created, and its creator's class.
    pub labels_at_creation: BTreeMap<u64, Vec<u64>>,
    pub creator_class: BTreeMap<u64, Class>,
    /// I12: every budget id ever issued.
    pub ever_budgets: BTreeSet<u64>,
    /// I11, keyed by (endpoint, waiting key).
    pub waiting: BTreeMap<(u64, Key), Waiting>,
    pub irqs: BTreeMap<u64, Irq>,
    /// Every message sent, by id.
    pub sent: BTreeMap<u64, Sent>,
    /// Pending receives on endpoints, by thread.
    pub receiving: BTreeMap<u64, Receiving>,
    /// Calls delivered to each thread and not yet replied to (its open calls), in the order it
    /// took them. A `send` is never served (QUESTIONS 31).
    pub served: BTreeMap<u64, Vec<u64>>,
    /// Each process's exit slot, by pid (none for `init`).
    pub slots: BTreeMap<u64, Slot>,
    /// What a dying process's notice must report, recorded when it began to die.
    pub exit_expect: BTreeMap<u64, Blame>,
    /// Exit notices owed, by the exiting pid.
    pub owed: BTreeMap<u64, Owed>,
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

    /// Labels of budget `b` as created (empty if unknown).
    pub fn labels(&self, b: u64) -> Vec<u64> {
        self.labels_at_creation.get(&b).cloned().unwrap_or_default()
    }

    /// A message was delivered to `tid`: a call, as the ghost recorded it when sent, becomes its
    /// newest open call.
    pub fn delivered(&mut self, tid: u64, msg: &Message) {
        if self.sent.get(&msg.msg_id).is_some_and(|s| s.kind == MsgKind::Call) {
            self.served.entry(tid).or_default().push(msg.msg_id);
        }
        let via = self.receiving.remove(&tid);
        self.flows.push(Flow::Delivered { tid, msg: msg.clone(), via });
    }

    /// `tid` replied to `msg`, which must be one of its open calls.
    pub fn replied(&mut self, tid: u64, msg: u64) {
        let l = self.served.entry(tid).or_default();
        if !l.contains(&msg) {
            self.violations.push(format!("R4a: thread {tid} replied to {msg}, which it does not serve"));
        }
        l.retain(|m| *m != msg);
        self.flows.push(Flow::Replied { msg });
    }

    /// Process `pid` begins to die through thread `tid` (a fault if `fault`, else an exit); its
    /// threads are `threads`. Its notice must say `faulted` if it faulted or holds open calls
    /// (QUESTIONS 55), blaming the account and labels of `tid`'s most recently taken open call
    /// (QUESTIONS 37, 48); otherwise `exited`, blaming nobody.
    pub fn exiting(&mut self, pid: u64, tid: u64, threads: &[u64], fault: bool) {
        let open = threads.iter().any(|t| self.served.get(t).is_some_and(|v| !v.is_empty()));
        let blame = if fault || open {
            let newest = self.served.get(&tid).and_then(|v| v.last()).and_then(|m| self.sent.get(m));
            Blame {
                cause: Cause::Faulted,
                account: newest.map_or(0, |s| s.account),
                labels: newest.map_or(Vec::new(), |s| s.labels.clone()),
            }
        } else {
            Blame { cause: Cause::Exited, account: 0, labels: Vec::new() }
        };
        self.exit_expect.insert(pid, blame);
    }

    pub fn thread_gone(&mut self, tid: u64) {
        self.served.remove(&tid);
        self.receiving.remove(&tid);
    }

    /// A message with key `key` is taken from endpoint `e` (delivered, or refused in its turn).
    /// `ep` is the endpoint before the message leaves its queue.
    pub fn took(&mut self, e: u64, key: &Key, ep: &Endpoint) {
        self.waiting.remove(&(e, key.clone()));
        for (other, q) in &ep.queue {
            if other == key {
                continue;
            }
            let Some(head) = q.front().copied() else { continue };
            let w = self.waiting.entry((e, other.clone())).or_default();
            if w.head != head {
                *w = Waiting { head, taken: BTreeMap::new() };
            }
            let n = w.taken.entry(key.clone()).or_insert(0);
            *n += 1;
            if *n >= 2 {
                self.violations.push(format!(
                    "I11: on endpoint {e}, sender {key:?} was taken twice while {other:?}'s oldest message \
                     {head} waited"
                ));
            }
        }
        self.waiting.retain(|(ee, k), _| *ee != e || ep.queue.contains_key(k));
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
