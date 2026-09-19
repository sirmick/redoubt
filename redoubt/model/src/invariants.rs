//! KERNEL-SPEC.md's invariants I1-I14, checked on the kernel model's state after every step.
//!
//! Each check recomputes what should be true from the objects themselves (and from ghost
//! history), instead of trusting the counters and flags the kernel model maintains, so that a
//! wrong update anywhere shows up here. I10 is a property of a pair of calls and is checked by
//! the `budget_lifecycle` property (check.rs); I14 is "the model never panics", which the
//! property runner checks by catching panics.
//!
//! Some checks are the direct statement of a rule (R2's cap, R3's lend staying with the server,
//! R4's opt-in); they are named after the rule.

use alloc::collections::BTreeMap;
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ghost::Flow;
use crate::kernel::{Backing, Handle, Kernel, MapState, MsgKind, Object, Origin, Wait};
use crate::spec::*;

type Check = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($fmt:tt)*) => {
        if !$cond {
            return Err(format!($($fmt)*));
        }
    };
}

/// Every check, in invariant order. The error names the invariant (or rule) and what broke.
pub fn check(k: &Kernel) -> Check {
    if let Some(v) = k.ghost.violations.first() {
        return Err(v.clone());
    }
    structure(k)?;
    i1_i2_i3_i4_handles(k)?;
    i5_charging(k)?;
    i6_i8_budgets(k)?;
    i7_flows(k)?;
    i9_memory(k)?;
    r2_r3_messages(k)?;
    i13_timeouts(k)?;
    Ok(())
}

/// Every handle anywhere: (where, handle).
fn all_handles(k: &Kernel) -> Vec<(String, Handle)> {
    let mut out = Vec::new();
    for p in k.processes.values() {
        for (i, h) in &p.handles {
            out.push((format!("process {} slot {}", p.pid, i), *h));
        }
        if let Some(h) = p.exit_endpoint {
            out.push((format!("process {}'s exit endpoint", p.pid), h));
        }
    }
    for m in k.msgs.values() {
        for h in &m.handles {
            out.push((format!("message {}", m.id), *h));
        }
    }
    out
}

/// The object graph is well formed: every process's budget, every thread's process, every
/// queued message and waiting receiver exist and agree with each other; the scheduler's runnable
/// threads are exactly the runnable threads.
fn structure(k: &Kernel) -> Check {
    for p in k.processes.values() {
        ensure!(
            k.budgets.contains_key(&p.budget),
            "R10: process {} lives in destroyed budget {}",
            p.pid,
            p.budget
        );
        ensure!(p.started || p.threads.is_empty(), "process {} has threads before it started", p.pid);
        for t in &p.threads {
            ensure!(
                k.threads.get(t).is_some_and(|x| x.pid == p.pid),
                "process {} lists missing thread {t}",
                p.pid
            );
        }
    }
    for t in k.threads.values() {
        ensure!(
            k.processes.get(&t.pid).is_some_and(|p| p.threads.contains(&t.tid)),
            "thread {} belongs to no process",
            t.tid
        );
    }
    for b in k.budgets.values() {
        if let Some(p) = b.parent {
            ensure!(
                k.budgets.get(&p).is_some_and(|x| x.children.contains(&b.id)),
                "budget {} lost its parent",
                b.id
            );
        }
        for c in &b.children {
            ensure!(
                k.budgets.get(c).is_some_and(|x| x.parent == Some(b.id)),
                "budget {} lists stray child {c}",
                b.id
            );
        }
    }
    for e in k.endpoints.values() {
        ensure!(
            k.budgets.contains_key(&e.owner),
            "R10: endpoint {} charged to destroyed budget {}",
            e.id,
            e.owner
        );
        for (acct, q) in &e.queue {
            ensure!(!q.is_empty(), "endpoint {} keeps an empty queue", e.id);
            for m in q {
                let msg = k.msgs.get(m);
                ensure!(
                    msg.is_some_and(|x| x.account == *acct && x.endpoint == e.id && x.server.is_none()),
                    "endpoint {} queues bad message {m}",
                    e.id
                );
                let msg = msg.unwrap();
                ensure!(
                    k.threads.get(&msg.sender_tid).is_some_and(|t| t.wait == Some(Wait::Send(*m))),
                    "queued message {m}'s sender is not waiting for it"
                );
            }
        }
        for r in &e.receivers {
            ensure!(
                k.threads.get(r).is_some_and(
                    |t| matches!(t.wait, Some(Wait::Receive { endpoint, .. }) if endpoint == e.id)
                ),
                "endpoint {} lists thread {r} as receiving",
                e.id
            );
        }
    }
    for t in k.threads.values() {
        match t.wait {
            Some(Wait::Send(m)) => ensure!(
                k.msgs.get(&m).is_some_and(|x| x.server.is_none() && x.sender_tid == t.tid),
                "thread {} waits to send missing message {m}",
                t.tid
            ),
            Some(Wait::Reply(m)) => ensure!(
                k.msgs
                    .get(&m)
                    .is_some_and(|x| x.caller_waiting && x.sender_tid == t.tid && x.server.is_some()),
                "thread {} waits for a reply to message {m}, which is not in flight",
                t.tid
            ),
            Some(Wait::Receive { endpoint, .. }) => ensure!(
                k.endpoints.get(&endpoint).is_some_and(|e| e.receivers.contains(&t.tid)),
                "thread {} receives on endpoint {endpoint} unseen",
                t.tid
            ),
            _ => {}
        }
        if let Some(m) = t.serving {
            ensure!(
                k.msgs.get(&m).is_some_and(|x| x.server == Some((t.pid, t.tid))),
                "thread {} serves message {m}, which it did not take",
                t.tid
            );
        }
    }
    // Scheduler bookkeeping: runnable threads are exactly those queued in their budget.
    for (b, e) in &k.sched.budgets {
        ensure!(k.budgets.contains_key(b), "R12: scheduler keeps destroyed budget {b}");
        for t in &e.runnable {
            ensure!(
                k.threads.get(t).is_some_and(|x| x.wait.is_none() && k.budget_of(x.pid) == Some(*b)),
                "R12: scheduler queues thread {t}, which is not runnable in budget {b}"
            );
        }
    }
    for t in k.threads.values().filter(|t| t.wait.is_none()) {
        let b = k.budget_of(t.pid).unwrap();
        ensure!(
            k.sched.budgets.get(&b).is_some_and(|e| e.runnable.contains(&t.tid)),
            "R12: runnable thread {} is not queued",
            t.tid
        );
    }
    Ok(())
}

/// I1: every live handle names a live object (and process tables use indices from 1).
/// I2: no handle is stamped with a destroyed budget.
/// I3: a minted handle's badge is non-zero and its stamp is its source's default stamp or a
///     descendant; a created handle is stamped with its creator's budget (R9).
/// I4: badge-0 endpoint handles are `endpoint_create`'s results or copies of them.
fn i1_i2_i3_i4_handles(k: &Kernel) -> Check {
    for p in k.processes.values() {
        ensure!(!p.handles.contains_key(&0), "I1: process {} uses handle index 0", p.pid);
    }
    for (at, h) in all_handles(k) {
        let live = match h.object {
            Object::Budget(b) => k.budgets.contains_key(&b),
            Object::Process(p) => k.processes.contains_key(&p),
            Object::Endpoint(e) => k.endpoints.contains_key(&e),
            Object::Device(d) => k.devices.contains_key(&d),
        };
        ensure!(live, "I1: {at} names dead object {:?}", h.object);
        ensure!(k.budgets.contains_key(&h.stamp), "I2: {at} is stamped with destroyed budget {}", h.stamp);
        match h.origin {
            Origin::Boot => ensure!(h.stamp == k.root, "R9: {at}: a boot handle stamped {}", h.stamp),
            Origin::Created { by } => {
                ensure!(h.stamp == by, "R9: {at}: created by budget {by}, stamped {}", h.stamp)
            }
            Origin::Minted { default_stamp } => {
                ensure!(h.badge != 0, "I3: {at}: minted with badge 0");
                ensure!(
                    k.is_descendant_or_self(h.stamp, default_stamp),
                    "I3: {at}: minted stamp {} is not default {default_stamp} or below it",
                    h.stamp
                );
            }
        }
        if matches!(h.object, Object::Endpoint(_)) && h.badge == 0 {
            ensure!(!matches!(h.origin, Origin::Minted { .. }), "I4: {at}: a receive right that was minted");
        }
        if !matches!(h.object, Object::Endpoint(_)) {
            ensure!(h.badge == 0, "{at}: a non-endpoint handle with a badge");
        }
    }
    Ok(())
}

/// I5 and R6: recompute every budget's usage from the objects charged to it; usage fits the
/// limits except for R3's lends; children's limits plus own objects fit.
fn i5_charging(k: &Kernel) -> Check {
    let c = k.costs;
    let mut pages: BTreeMap<u64, u64> = BTreeMap::new();
    let mut procs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut threads: BTreeMap<u64, u64> = BTreeMap::new();
    let mut r3: BTreeMap<u64, u64> = BTreeMap::new();
    for b in k.budgets.values() {
        // A budget's own object is charged to itself, a revocation scope's to its parent.
        *pages.entry(b.id).or_default() += if b.is_scope() { 0 } else { c.budget };
        if let Some(p) = b.parent {
            *pages.entry(p).or_default() += if b.is_scope() { c.budget } else { b.pages_limit };
            *procs.entry(p).or_default() += b.processes_limit;
        }
    }
    for p in k.processes.values() {
        *pages.entry(p.budget).or_default() +=
            c.process + k.table_pages(p.handles.len() as u64) + p.threads.len() as u64 * c.thread;
        *procs.entry(p.budget).or_default() += 1;
        *threads.entry(p.budget).or_default() += p.threads.len() as u64;
    }
    for e in k.endpoints.values() {
        *pages.entry(e.owner).or_default() += c.endpoint;
    }
    for f in k.frames.values() {
        *pages.entry(f.payer).or_default() += 1;
    }
    for m in k.msgs.values().filter(|m| m.kind == MsgKind::Call && !m.caller_waiting) {
        if let (Some(b), Some((spid, _))) = (&m.buffer, m.server) {
            if let Some(sb) = k.budget_of(spid) {
                *r3.entry(sb).or_default() += b.frames.len() as u64;
            }
        }
    }
    for b in k.budgets.values() {
        let want = pages.get(&b.id).copied().unwrap_or(0);
        ensure!(
            b.pages_used == want,
            "R6: budget {} says {} pages used, its objects are {want}",
            b.id,
            b.pages_used
        );
        let want = procs.get(&b.id).copied().unwrap_or(0);
        ensure!(
            b.processes_used == want,
            "R6: budget {} says {} processes used, it has {want}",
            b.id,
            b.processes_used
        );
        let weights: u64 = b.children.iter().map(|c| k.budgets[c].weight).sum();
        ensure!(
            b.weight_used == weights,
            "R7: budget {} says {} weight carved, children have {weights}",
            b.id,
            b.weight_used
        );
        let lent = r3.get(&b.id).copied().unwrap_or(0);
        ensure!(
            b.pages_used - lent.min(b.pages_used) <= b.pages_limit,
            "I5: budget {} uses {} of {} pages (R3 lends: {lent})",
            b.id,
            b.pages_used,
            b.pages_limit
        );
        ensure!(
            lent <= MAX_LEND_PAGES * threads.get(&b.id).copied().unwrap_or(0),
            "I5: budget {} holds {lent} R3 pages, more than MAX_LEND_PAGES per thread",
            b.id
        );
        ensure!(b.processes_used <= b.processes_limit, "I5: budget {} over its process limit", b.id);
        ensure!(b.weight_used <= b.weight, "I5: budget {} carved more weight than it has", b.id);
    }
    Ok(())
}

/// I6: labels never change, contain the parent's, and only a system-class creator adds labels.
/// I8: class(child) <= class(parent); the account is the parent's unless that is 0.
/// I12: budget ids are never reused (the ghost records every id issued).
fn i6_i8_budgets(k: &Kernel) -> Check {
    for b in k.budgets.values() {
        ensure!(
            k.ghost.labels_at_creation.get(&b.id) == Some(&b.labels),
            "I6: budget {}'s labels changed",
            b.id
        );
        ensure!(
            b.labels.len() <= MAX_LABELS && b.labels.windows(2).all(|w| w[0] < w[1]),
            "I6: bad label set on {}",
            b.id
        );
        ensure!(b.depth < MAX_DEPTH, "budget {} is at depth {}", b.id, b.depth);
        let Some(p) = b.parent.and_then(|p| k.budgets.get(&p)) else { continue };
        ensure!(superset(&b.labels, &p.labels), "I6: budget {} lacks its parent's labels", b.id);
        if b.labels != p.labels {
            ensure!(
                k.ghost.creator_class.get(&b.id) == Some(&Class::System),
                "I6: budget {} got labels from a user-class creator",
                b.id
            );
        }
        ensure!(b.class <= p.class, "I8: budget {} outranks its parent's class", b.id);
        if p.account != 0 {
            ensure!(
                b.account == p.account,
                "I8: budget {} has account {}, parent {}",
                b.id,
                b.account,
                p.account
            );
        }
        ensure!(b.id > p.id, "I12: budget {} is older than its parent", b.id);
    }
    Ok(())
}

/// I7 (R1): messages between two user budgets only between equal label sets; exit notices and
/// usage reads only to ⊇ label sets. R4: no transfer larger than the receiver's `max_transfer`.
fn i7_flows(k: &Kernel) -> Check {
    for f in &k.ghost.flows {
        match f {
            Flow::Message { from_class, from, to_class, to, transfer, max_transfer } => {
                if *from_class == Class::User && *to_class == Class::User {
                    ensure!(from == to, "I7: a message flowed between user label sets {from:?} and {to:?}");
                }
                ensure!(
                    transfer <= max_transfer,
                    "R4: {transfer} pages transferred to a receiver that allowed {max_transfer}"
                );
            }
            Flow::Exit { from, to } => {
                ensure!(superset(to, from), "I7: an exit notice of {from:?} reached {to:?}")
            }
            Flow::Usage { from, to } => ensure!(superset(to, from), "I7: usage of {from:?} read by {to:?}"),
        }
    }
    Ok(())
}

/// I9 (R11): no mapping is writable and executable; pages handed out read zero until written; a
/// page is accessible in at most one address space, so a lent page is not the lender's until the
/// call ends. R6 and R3: each frame is charged to whoever holds it (the lender while its call
/// lasts, the server once the call is abandoned).
fn i9_memory(k: &Kernel) -> Check {
    #[derive(Default)]
    struct Seen {
        own: Vec<u64>,
        lent_in: Vec<(u64, u64)>,
        lent_out: Vec<(u64, u64)>,
    }
    let mut seen: BTreeMap<u64, Seen> = BTreeMap::new();
    for p in k.processes.values() {
        for (v, m) in &p.space {
            ensure!(
                !(m.flags & FLAG_W != 0 && m.flags & FLAG_X != 0),
                "I9: page {v:#x} of process {} is W+X",
                p.pid
            );
            let Backing::Frame(f) = m.backing else { continue };
            ensure!(k.frames.contains_key(&f), "I9: process {} maps freed frame {f}", p.pid);
            let s = seen.entry(f).or_default();
            match m.state {
                MapState::Own => s.own.push(p.pid),
                MapState::LentIn(mid) => s.lent_in.push((p.pid, mid)),
                MapState::LentOut(mid) => s.lent_out.push((p.pid, mid)),
            }
        }
    }
    for f in &k.ghost.fresh {
        ensure!(
            k.frames.get(f).is_none_or(|x| x.content == 0),
            "I9: frame {f} was handed out without being zeroed"
        );
    }
    for (f, fr) in &k.frames {
        let s = seen.remove(f).unwrap_or_default();
        ensure!(
            k.budgets.contains_key(&fr.payer),
            "R6: frame {f} is charged to destroyed budget {}",
            fr.payer
        );
        ensure!(
            s.own.len() + s.lent_in.len() <= 1,
            "I9: frame {f} is accessible in several address spaces ({:?} own, {:?} lent in)",
            s.own,
            s.lent_in
        );
        if let Some(pid) = s.own.first() {
            ensure!(
                k.budget_of(*pid) == Some(fr.payer),
                "R6: frame {f} owned by process {pid} is charged to {}",
                fr.payer
            );
        } else if let Some((spid, mid)) = s.lent_in.first() {
            let m = k.msgs.get(mid);
            ensure!(
                m.is_some_and(|m| m.server.map(|x| x.0) == Some(*spid)),
                "frame {f} lent in by a message not served by {spid}"
            );
            let m = m.unwrap();
            let want = if m.caller_waiting { m.sender_budget } else { k.budget_of(*spid).unwrap() };
            ensure!(fr.payer == want, "R3: lent frame {f} is charged to {}, not {want}", fr.payer);
            if m.caller_waiting {
                ensure!(
                    s.lent_out.iter().any(|(pid, x)| *pid == m.sender_pid && x == mid),
                    "I9: frame {f} lent by a waiting caller that no longer reserves it"
                );
            }
        } else if let Some((pid, mid)) = s.lent_out.first() {
            let m = k.msgs.get(mid);
            ensure!(
                m.is_some_and(|m| m.sender_pid == *pid && m.server.is_none()),
                "frame {f} lent out with no queued message"
            );
            ensure!(
                fr.payer == m.unwrap().sender_budget,
                "R6: frame {f} in flight is charged to {}",
                fr.payer
            );
        } else {
            return Err(format!("R6: frame {f} is charged to {} but mapped nowhere", fr.payer));
        }
    }
    Ok(())
}

/// R2: at most `WAIT_CAP` blocked senders per account per endpoint. R3: a server keeps a lent
/// buffer mapped until it replies, even when its caller is gone.
fn r2_r3_messages(k: &Kernel) -> Check {
    for e in k.endpoints.values() {
        for (a, q) in &e.queue {
            ensure!(
                q.len() as u64 <= WAIT_CAP,
                "R2: {} senders of account {a} wait on endpoint {}",
                q.len(),
                e.id
            );
        }
    }
    for m in k.msgs.values() {
        let Some((spid, _)) = m.server else { continue };
        if m.kind != MsgKind::Call {
            continue;
        }
        if m.lent_pages == 0 {
            continue;
        }
        let Some(b) = &m.buffer else {
            return Err(format!("R3: taken call {} lost its lend before the reply", m.id));
        };
        let rv = b.receiver_vpn.unwrap_or(u64::MAX);
        let p = &k.processes[&spid];
        for i in 0..b.frames.len() as u64 {
            ensure!(
                p.space.get(&(rv + i)).is_some_and(|x| x.state == MapState::LentIn(m.id)),
                "R3: server {spid} lost lent page {i} of call {} before replying",
                m.id
            );
        }
    }
    Ok(())
}

/// I13: every blocked call is due no later than its timeout (none is overdue).
fn i13_timeouts(k: &Kernel) -> Check {
    for t in k.threads.values() {
        if t.wait.is_some() {
            if let Some(d) = t.deadline {
                ensure!(
                    d > k.now,
                    "I13: thread {} is still blocked past its timeout ({d} <= {})",
                    t.tid,
                    k.now
                );
            }
        }
    }
    Ok(())
}
