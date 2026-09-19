//! KERNEL-SPEC.md's invariants I1-I14, checked on the kernel model's state after every step.
//!
//! Each check recomputes what should be true from the objects themselves and from ghost records
//! (ghost.rs: taken from handle tables, budget objects and call arguments at the event), never from
//! the counters and derived fields the kernel keeps, so that a wrong update anywhere shows up here.
//! I10 is a property of a pair of calls and is checked by `check::budget_lifecycle`; I11's
//! record-keeping is in ghost.rs (`Ghost::took`); I14 is "the model never panics", which the
//! property runner checks by catching panics.
//!
//! Some checks are the direct statement of a rule (R2's cap, R3's lend staying with the server,
//! R4's opt-in, R4a's limit); they are named after the rule.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ghost::{Blame, Flow, Key};
use crate::kernel::{Backing, Handle, Kernel, MapState, MsgKind, Object, Origin, ROOT, Wait};
use crate::spec::*;
use crate::syscall::{MintSource, Ret};

type Check = Result<(), String>;

macro_rules! ensure {
    ($cond:expr, $($fmt:tt)*) => {
        if !$cond {
            return Err(format!($($fmt)*));
        }
    };
}

/// Where the handles with each non-zero badge to each endpoint are: (endpoint, badge) -> the
/// label set of each holder (a process's budget, or a message's sender), from the ghost's records.
type Holders = BTreeMap<(u64, u64), Vec<Vec<u64>>>;

fn holders(k: &Kernel) -> Holders {
    let mut out: Holders = BTreeMap::new();
    for p in k.processes.values() {
        for h in p.handles.values().chain(p.exit_endpoint.iter()) {
            if let (Object::Endpoint(e), true) = (h.object, h.badge != 0) {
                out.entry((e, h.badge)).or_default().push(k.ghost.labels(p.budget));
            }
        }
    }
    for m in k.msgs.values() {
        let labels = k.ghost.sent.get(&m.id).map_or(Vec::new(), |s| s.labels.clone());
        for h in &m.handles {
            if let (Object::Endpoint(e), true) = (h.object, h.badge != 0) {
                out.entry((e, h.badge)).or_default().push(labels.clone());
            }
        }
    }
    out
}

fn pending_badges(k: &Kernel) -> BTreeSet<(u64, u64)> {
    k.endpoints
        .values()
        .flat_map(|e| e.badges.iter().filter(|(_, s)| s.pending).map(|(b, _)| (e.id, *b)))
        .collect()
}

/// The checker, with what it remembers between steps: the frames that existed after the last
/// step, so that a frame appearing now is known to be newly handed out (I9: it must read zero),
/// whatever the kernel's allocator believes; and who held each badge, so that a badge whose last
/// handle went is known to be owed a notice (QUESTIONS 53).
#[derive(Clone, Debug, Default)]
pub struct Checker {
    frames: BTreeSet<u64>,
    badges: Holders,
    pending: BTreeSet<(u64, u64)>,
}

impl Checker {
    pub fn new(k: &Kernel) -> Checker {
        Checker { frames: k.frames.keys().copied().collect(), badges: holders(k), pending: pending_badges(k) }
    }

    /// Every check, in invariant order. The error names the invariant (or rule) and what broke.
    pub fn check(&mut self, k: &Kernel) -> Check {
        if let Some(v) = k.ghost.violations.first() {
            return Err(v.clone());
        }
        structure(k)?;
        serving(k)?;
        i1_i2_i3_i4_handles(k)?;
        i5_charging(k)?;
        i6_i8_budgets(k)?;
        flows(k)?;
        self.i9_memory(k)?;
        self.badge_notices(k)?;
        r2_r3_messages(k)?;
        exits_owed(k)?;
        i13_timeouts(k)?;
        Ok(())
    }

    /// Badge slots and notices (QUESTIONS 53; I7, I15): each endpoint's slots count exactly the
    /// handles with their badge, and a slot is pending exactly when none is left; a badge whose
    /// last handle went this step is owed a notice if every holder it had after the last step
    /// passes the exit notices' label rule (the kernel judges the last one to go), and is owed none
    /// if none does; a notice is delivered only when no handle with its badge exists.
    fn badge_notices(&mut self, k: &Kernel) -> Check {
        let now = holders(k);
        for e in k.endpoints.values() {
            for (b, s) in &e.badges {
                let n = now.get(&(e.id, *b)).map_or(0, |v| v.len() as u64);
                ensure!(
                    s.count == n,
                    "Messages: badge {b} of endpoint {} counts {} handles, there are {n}",
                    e.id,
                    s.count
                );
                if n == 0 {
                    ensure!(
                        s.pending,
                        "Messages: badge {b} of endpoint {} kept a slot with no handle or notice",
                        e.id
                    );
                } else {
                    ensure!(
                        !s.pending,
                        "I15: a notice for badge {b} of endpoint {} is pending while it has handles",
                        e.id
                    );
                }
            }
        }
        for (e, b) in now.keys() {
            ensure!(
                k.endpoints.get(e).is_some_and(|x| x.badges.contains_key(b)),
                "Messages: badge {b} of endpoint {e} has handles but no slot"
            );
        }
        let delivered: BTreeSet<(u64, u64)> = k
            .ghost
            .flows
            .iter()
            .filter_map(|f| match f {
                Flow::BadgeClosed { endpoint, badge } => Some((*endpoint, *badge)),
                _ => None,
            })
            .collect();
        for ((e, b), was) in &self.badges {
            let Some(ep) = k.endpoints.get(e) else { continue };
            if now.contains_key(&(*e, *b)) {
                continue;
            }
            let owner = &k.budgets[&ep.owner];
            let to = k.ghost.labels(owner.id);
            let pass = |l: &Vec<u64>| owner.class == Class::System || superset(&to, l);
            let notice = ep.badges.get(b).is_some_and(|s| s.pending) || delivered.contains(&(*e, *b));
            if was.iter().all(pass) {
                ensure!(
                    notice,
                    "Messages: the last handle with badge {b} to endpoint {e} went, and no notice is owed"
                );
            }
            if !was.iter().any(pass) {
                ensure!(
                    !notice,
                    "I7: a notice for badge {b} of endpoint {e} is owed to {to:?} from holders {was:?}"
                );
            }
        }
        for (e, b) in &delivered {
            ensure!(
                !now.contains_key(&(*e, *b)),
                "I15: a notice for badge {b} of endpoint {e} arrived while it has handles"
            );
            ensure!(
                self.pending.contains(&(*e, *b)) || self.badges.contains_key(&(*e, *b)),
                "I15: a notice for badge {b} of endpoint {e} that was never owed"
            );
        }
        self.pending = pending_badges(k);
        self.badges = now;
        Ok(())
    }

    /// I9 (R11): no mapping is writable and executable; a frame read zero when first handed out;
    /// a page is accessible in at most one address space, so a lent page is not the lender's
    /// until the call ends. R6 and R3: each frame is charged to whoever holds it (the lender while
    /// its call lasts, the server once the call is abandoned).
    fn i9_memory(&mut self, k: &Kernel) -> Check {
        #[derive(Default)]
        struct Seen {
            own: Vec<u64>,
            lent_in: Vec<(u64, u64)>,
            lent_out: Vec<(u64, u64)>,
        }
        let now: BTreeSet<u64> = k.frames.keys().copied().collect();
        for f in now.difference(&self.frames) {
            ensure!(k.frames[f].content == 0, "I9: frame {f} was handed out without being zeroed");
        }
        self.frames = now;
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
}

/// Run the checks once on `k`, with no memory of earlier steps (so no frame counts as new).
pub fn check(k: &Kernel) -> Check {
    Checker::new(k).check(k)
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

/// R2's key of a message, from the ghost's record of how it was sent.
fn ghost_key(k: &Kernel, m: u64) -> Option<Key> {
    k.ghost.sent.get(&m).map(|s| (s.account, s.labels.clone()))
}

/// The object graph is well formed and alive: every process's budget, every thread's process,
/// every queued message and waiting receiver exist and agree; a thread blocked in `send`, `call`
/// or waiting for a reply waits on something that still exists (R10: calls to a destroyed
/// endpoint fail); no budget outlives its deadline; the scheduler's runnable threads are exactly
/// the runnable threads.
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
        ensure!(b.parent.is_none_or(|p| k.budgets.contains_key(&p)), "R10: budget {} lost its parent", b.id);
        ensure!(b.deadline.is_none_or(|d| d > k.now), "Budget: budget {} outlived its deadline", b.id);
    }
    for e in k.endpoints.values() {
        ensure!(
            k.budgets.contains_key(&e.owner),
            "R10: endpoint {} charged to destroyed budget {}",
            e.id,
            e.owner
        );
        for (key, q) in &e.queue {
            ensure!(!q.is_empty(), "endpoint {} keeps an empty queue", e.id);
            for m in q {
                let msg = k.msgs.get(m);
                ensure!(
                    msg.is_some_and(|x| &x.key == key && x.endpoint == e.id && x.server.is_none()),
                    "endpoint {} queues bad message {m}",
                    e.id
                );
                ensure!(
                    ghost_key(k, *m).as_ref() == Some(key),
                    "R2: endpoint {} queues message {m} as {key:?}, its sender is {:?}",
                    e.id,
                    ghost_key(k, *m)
                );
                ensure!(
                    k.ghost.sent.get(m).is_some_and(|s| k.budgets.contains_key(&s.stamp)),
                    "R10: endpoint {} still queues message {m}, sent through a revoked handle",
                    e.id
                );
                let msg = msg.unwrap();
                ensure!(
                    k.threads.get(&msg.sender_tid).is_some_and(|t| t.wait == Some(Wait::Send(*m))),
                    "queued message {m}'s sender is not waiting for it"
                );
            }
        }
        for n in &e.exits {
            ensure!(
                k.budgets.contains_key(&n.payer),
                "R10: the exit notice of process {} on endpoint {} outlived its slot's payer {}",
                n.pid,
                e.id,
                n.payer
            );
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
            Some(Wait::Send(m)) => {
                let msg = k.msgs.get(&m);
                ensure!(
                    msg.is_some_and(|x| x.server.is_none() && x.sender_tid == t.tid),
                    "thread {} waits to send missing message {m}",
                    t.tid
                );
                let e = msg.unwrap().endpoint;
                ensure!(
                    k.endpoints.get(&e).is_some_and(|ep| ep.queue.values().any(|q| q.contains(&m))),
                    "R10: thread {} waits to send on endpoint {e}, which no longer queues it",
                    t.tid
                );
            }
            Some(Wait::Reply(m)) => {
                let msg = k.msgs.get(&m);
                ensure!(
                    msg.is_some_and(|x| x.caller_waiting && x.sender_tid == t.tid),
                    "thread {} waits for a reply to message {m}, which is not in flight",
                    t.tid
                );
                let msg = msg.unwrap();
                ensure!(
                    k.ghost.sent.get(&m).is_some_and(|s| k.budgets.contains_key(&s.stamp)),
                    "R10: thread {} still waits for a reply to {m}, sent through a revoked handle",
                    t.tid
                );
                ensure!(
                    k.endpoints.contains_key(&msg.endpoint),
                    "R10: thread {} waits for a reply through destroyed endpoint {}",
                    t.tid,
                    msg.endpoint
                );
                let server = msg.server.and_then(|(_, stid)| k.threads.get(&stid));
                ensure!(
                    server.is_some_and(|s| s.serving.contains(&m)),
                    "R4b: thread {} waits for a reply to message {m} that no live thread serves",
                    t.tid
                );
            }
            Some(Wait::Receive { endpoint, .. }) => ensure!(
                k.endpoints.get(&endpoint).is_some_and(|e| e.receivers.contains(&t.tid)),
                "thread {} receives on endpoint {endpoint} unseen",
                t.tid
            ),
            _ => {}
        }
    }
    // Scheduler bookkeeping: each budget's class and weight, and exactly its runnable threads.
    for (b, e) in &k.sched.budgets {
        let Some(bx) = k.budgets.get(b) else {
            return Err(format!("R12: scheduler keeps destroyed budget {b}"));
        };
        ensure!(
            e.class == bx.class && e.weight == bx.weight,
            "R12: scheduler has budget {b}'s class or weight wrong"
        );
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

/// What each thread serves (R4a, QUESTIONS 2): exactly the messages the ghost saw delivered to it
/// and not yet replied to; every taken call is served by exactly one live thread; a process holds
/// at most `MAX_OPEN_CALLS`; the account a thread records is the newest served message's, as the
/// sender's budget gave it (Process; README choice 7).
fn serving(k: &Kernel) -> Check {
    let mut servers: BTreeMap<u64, u64> = BTreeMap::new();
    for t in k.threads.values() {
        let want: BTreeSet<u64> =
            k.ghost.served.get(&t.tid).map(|v| v.iter().copied().collect()).unwrap_or_default();
        let have: BTreeSet<u64> = t.serving.iter().copied().collect();
        ensure!(have == want, "R4a: thread {} serves {have:?}, but was delivered {want:?}", t.tid);
        for m in &t.serving {
            ensure!(
                k.msgs.get(m).is_some_and(|x| x.server == Some((t.pid, t.tid))),
                "thread {} serves message {m}, which it did not take",
                t.tid
            );
            *servers.entry(*m).or_default() += 1;
        }
        let newest = k.ghost.served.get(&t.tid).and_then(|v| v.last()).and_then(|m| k.ghost.sent.get(m));
        let account = newest.map_or(0, |s| s.account);
        ensure!(
            t.account == account,
            "Process: thread {} records account {}, it serves {account}'s",
            t.tid,
            t.account
        );
    }
    for m in k.msgs.values().filter(|m| m.server.is_some()) {
        ensure!(
            servers.get(&m.id) == Some(&1),
            "R4a: taken message {} is served by {:?} threads",
            m.id,
            servers.get(&m.id)
        );
    }
    for p in k.processes.values() {
        let open = k.open_calls(p.pid);
        ensure!(open <= MAX_OPEN_CALLS, "R4a: process {} holds {open} open calls", p.pid);
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
        ensure!(!p.handles.contains_key(&NO_HANDLE), "I1: process {} uses handle index 0", p.pid);
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
            Origin::Boot => ensure!(h.stamp == ROOT, "R9: {at}: a boot handle stamped {}", h.stamp),
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

/// Sv39 page-table pages a set of mapped pages needs, the root included.
fn page_tables(space: &BTreeMap<u64, crate::kernel::Mapping>) -> u64 {
    let mids: BTreeSet<u64> = space.keys().map(|v| v >> 18).collect();
    let leaves: BTreeSet<u64> = space.keys().map(|v| v >> 9).collect();
    1 + mids.len() as u64 + leaves.len() as u64
}

/// I5 and R6: recompute every budget's usage from the objects charged to it (the cost table:
/// budgets, processes, page tables, threads, handle tables, endpoints, frames, open calls, exit
/// slots); usage fits the limits except for R3's lends, at most `MAX_LEND_PAGES` per open call;
/// children's limits plus own objects fit.
fn i5_charging(k: &Kernel) -> Check {
    let c = k.costs;
    let mut pages: BTreeMap<u64, u64> = BTreeMap::new();
    let mut procs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut open: BTreeMap<u64, u64> = BTreeMap::new();
    let mut r3: BTreeMap<u64, u64> = BTreeMap::new();
    let mut add = |b: u64, n: u64| *pages.entry(b).or_default() += n;
    for b in k.budgets.values() {
        // A budget's own object is charged to itself, a revocation scope's to its parent.
        add(b.id, if b.is_scope() { 0 } else { c.budget });
        if let Some(p) = b.parent {
            add(p, if b.is_scope() { c.budget } else { b.pages_limit });
            *procs.entry(p).or_default() += b.processes_limit;
        }
    }
    for p in k.processes.values() {
        add(
            p.budget,
            c.process
                + page_tables(&p.space) * c.page_table
                + k.table_pages(p.handles.len() as u64)
                + p.threads.len() as u64 * c.thread,
        );
        *procs.entry(p.budget).or_default() += 1;
    }
    // Exit slots, as the ghost recorded them at `process_create`: charged to the creator's budget
    // while the process lives and while its notice waits (QUESTIONS 7).
    for (pid, s) in &k.ghost.slots {
        let waiting = k.ghost.owed.get(pid).is_some_and(|o| k.endpoints.contains_key(&o.endpoint));
        if k.budgets.contains_key(&s.payer) && (k.processes.contains_key(pid) || waiting) {
            add(s.payer, c.exit_slot);
        }
    }
    for e in k.endpoints.values() {
        add(e.owner, c.endpoint + (e.badges.len() as u64).div_ceil(c.badge_slots_per_page));
    }
    for f in k.frames.values() {
        add(f.payer, 1);
    }
    for m in k.msgs.values().filter(|m| m.kind == MsgKind::Call) {
        let Some((spid, _)) = m.server else { continue };
        let Some(sb) = k.budget_of(spid) else { continue };
        add(sb, c.open_call);
        *open.entry(sb).or_default() += 1;
        if let (false, Some(b)) = (m.caller_waiting, &m.buffer) {
            *r3.entry(sb).or_default() += b.frames.len() as u64;
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
        let weights: u64 = k.children(b.id).iter().map(|c| k.budgets[c].weight).sum();
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
            lent <= MAX_LEND_PAGES * open.get(&b.id).copied().unwrap_or(0),
            "I5: budget {} holds {lent} R3 pages, more than MAX_LEND_PAGES per open call",
            b.id
        );
        ensure!(b.processes_used <= b.processes_limit, "I5: budget {} over its process limit", b.id);
        ensure!(b.weight_used <= b.weight, "I5: budget {} carved more weight than it has", b.id);
    }
    for p in k.processes.values() {
        ensure!(k.budgets[&p.budget].weight > 0, "Budget: process {} lives in a weight-0 budget", p.pid);
    }
    Ok(())
}

/// I6: labels never change, contain the parent's, and only a system-class creator adds labels.
/// I8: class(child) <= class(parent), and only a system-class caller creates a system-class
/// budget; the account is the parent's unless that is 0. I12: budget ids ascend.
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
        // Depth from the parent chain, not the kernel's field.
        let mut depth = 0;
        let mut cur = b.parent;
        while let Some(p) = cur {
            depth += 1;
            cur = k.budgets.get(&p).and_then(|x| x.parent);
            ensure!(depth <= MAX_DEPTH, "budget {} has a parent cycle or is too deep", b.id);
        }
        ensure!(
            depth == b.depth && depth < MAX_DEPTH,
            "budget {} is at depth {depth} (recorded {})",
            b.id,
            b.depth
        );
        if b.class == Class::System {
            ensure!(
                k.ghost.creator_class.get(&b.id) == Some(&Class::System),
                "I8: system-class budget {} was created by a user-class caller",
                b.id
            );
        }
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

/// What the step did, judged against the ghost's records:
/// - a delivered message is the one sent: its kind, badge, account and labels are what the
///   sender's handle and budget gave (Messages); it went to a thread receiving through a badge-0
///   handle to its endpoint (I4); between user budgets only equal label sets flow, the receiver
///   being the endpoint's owner (I7, R1); no transfer exceeds `max_transfer` (R4);
/// - exit notices and usage reads flow only to ⊇ label sets or to a system-class budget (I7);
/// - `Busy` for R2's cap only when the sender's (account, label set) group is full;
/// - a minted handle's endpoint is one the minter holds a receive right to, or a message it
///   serves arrived on, with that source's default stamp (mint, R9).
fn flows(k: &Kernel) -> Check {
    for f in &k.ghost.flows {
        match f {
            Flow::Delivered { tid, msg, via } => {
                let Some(s) = k.ghost.sent.get(&msg.msg_id) else {
                    return Err(format!(
                        "Messages: thread {tid} got message {}, which was never sent",
                        msg.msg_id
                    ));
                };
                ensure!(
                    msg.kind == s.kind
                        && msg.badge == s.badge
                        && msg.account == s.account
                        && msg.labels == s.labels,
                    "Messages: message {} arrived as {:?} badge {} account {} labels {:?}; it was sent as {:?} \
                     badge {} account {} labels {:?}",
                    msg.msg_id,
                    msg.kind,
                    msg.badge,
                    msg.account,
                    msg.labels,
                    s.kind,
                    s.badge,
                    s.account,
                    s.labels
                );
                let Some(via) = via else {
                    return Err(format!("I4: thread {tid} got message {} without receiving", msg.msg_id));
                };
                ensure!(
                    via.handle.badge == 0 && via.handle.object == Object::Endpoint(s.endpoint),
                    "I4: thread {tid} received message {} through {:?}",
                    msg.msg_id,
                    via.handle
                );
                if s.sender_class == Class::User && s.owner_class == Class::User {
                    ensure!(
                        s.labels == s.owner_labels,
                        "I7: a message flowed between user label sets {:?} and {:?}",
                        s.labels,
                        s.owner_labels
                    );
                }
                let pages = msg
                    .buffer
                    .filter(|b| b.kind == crate::syscall::BufferKind::Transfer)
                    .map_or(0, |b| b.pages);
                ensure!(
                    pages <= via.max_transfer,
                    "R4: {pages} pages transferred to a receiver that allowed {}",
                    via.max_transfer
                );
            }
            Flow::Exit { pid, from, to_class, to, got, want } => {
                ensure!(
                    *to_class == Class::System || superset(to, from),
                    "I7: an exit notice of {from:?} reached {to:?}"
                );
                ensure!(
                    want.as_ref() == Some(got),
                    "Messages: the exit notice of process {pid} reports {got:?}, it should report {want:?}"
                );
            }
            Flow::Usage { from, to_class, to } => ensure!(
                *to_class == Class::System || superset(to, from),
                "I7: usage of {from:?} read by {to:?}"
            ),
            Flow::UsageDenied { from, to_class, to } => ensure!(
                *to_class == Class::User && !superset(to, from),
                "R1: usage of {from:?} refused to a {to_class:?} caller with {to:?}"
            ),
            Flow::LabelDenied { from_class, from, to_class, to } => ensure!(
                *from_class == Class::User && *to_class == Class::User && from != to,
                "R1: a {from_class:?} sender with {from:?} refused by a {to_class:?} owner with {to:?}"
            ),
            Flow::Woken { tid, result: Ok(Ret::Reply { .. }) } => {
                let replied = k.ghost.flows.iter().any(|g| match g {
                    Flow::Replied { msg } => k.ghost.sent.get(msg).is_some_and(|s| s.sender_tid == *tid),
                    _ => false,
                });
                ensure!(replied, "R4b: thread {tid} got a reply its server never sent");
            }
            Flow::Replied { msg } => {
                let caller = k.ghost.sent.get(msg).map(|s| s.sender_tid);
                let answered = k.ghost.flows.iter().any(
                    |g| matches!(g, Flow::Woken { tid, result: Ok(Ret::Reply { .. }) } if Some(*tid) == caller),
                );
                let stamp = k.ghost.sent.get(msg).map_or(0, |s| s.stamp);
                ensure!(
                    !answered || k.budgets.contains_key(&stamp),
                    "R10: the reply to {msg}, sent through a revoked handle, reached its caller"
                );
            }
            Flow::Woken { .. } | Flow::BadgeClosed { .. } => {}
            Flow::Busy { endpoint, key } => {
                let queued = k.endpoints.get(endpoint).map_or(0, |e| {
                    e.queue.values().flatten().filter(|m| ghost_key(k, **m).as_ref() == Some(key)).count()
                });
                ensure!(
                    queued as u64 >= WAIT_CAP,
                    "R2: a sender of {key:?} got Busy on endpoint {endpoint} with {queued} of its group waiting"
                );
            }
            Flow::Minted { pid, tid, endpoint, via, default_stamp } => match via {
                MintSource::Handle(h) => {
                    let hd = k.processes.get(pid).and_then(|p| p.handles.get(h));
                    ensure!(
                        hd.is_some_and(|x| x.object == Object::Endpoint(*endpoint)
                            && x.badge == 0
                            && x.stamp == *default_stamp),
                        "mint: process {pid} minted from handle {h}, not a receive right to endpoint {endpoint} stamped \
                         {default_stamp}"
                    );
                }
                MintSource::Message(m) => {
                    let served = k.ghost.served.get(tid).is_some_and(|v| v.contains(m));
                    let s = k.ghost.sent.get(m);
                    ensure!(
                        served && s.is_some_and(|s| s.endpoint == *endpoint && s.stamp == *default_stamp),
                        "mint: thread {tid} minted from message {m}, which it does not serve or which came on another \
                         endpoint or stamp"
                    );
                }
            },
        }
    }
    Ok(())
}

/// R2: at most `WAIT_CAP` blocked senders per (account, label set) group per endpoint, grouped by
/// the ghost's record of each sender. R3: a server keeps a lent buffer mapped until it replies,
/// even when its caller is gone.
fn r2_r3_messages(k: &Kernel) -> Check {
    for e in k.endpoints.values() {
        let mut groups: BTreeMap<Key, u64> = BTreeMap::new();
        for m in e.queue.values().flatten() {
            let key = ghost_key(k, *m).unwrap_or_default();
            *groups.entry(key).or_default() += 1;
        }
        for (key, n) in groups {
            ensure!(n <= WAIT_CAP, "R2: {n} senders of {key:?} wait on endpoint {}", e.id);
        }
    }
    for m in k.msgs.values() {
        let Some((spid, _)) = m.server else { continue };
        if m.kind != MsgKind::Call || k.ghost.sent.get(&m.id).is_none_or(|s| s.lent_pages == 0) {
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

/// Exit notices (Messages; R1; R10; QUESTIONS 7, 37, 55): every notice the ghost saw owed, whose
/// endpoint still exists, waits on it (receiving it clears it) if its slot's payer lives, and is
/// gone if the payer does not; it reports what the ghost expects; and every waiting notice is
/// owed.
fn exits_owed(k: &Kernel) -> Check {
    for (pid, o) in &k.ghost.owed {
        let Some(e) = k.endpoints.get(&o.endpoint) else { continue };
        let n = e.exits.iter().find(|n| n.pid == *pid);
        if !k.budgets.contains_key(&o.payer) {
            ensure!(
                n.is_none(),
                "R10: the exit notice of process {pid} outlived its slot's payer {}",
                o.payer
            );
            continue;
        }
        let Some(n) = n else {
            return Err(format!(
                "Messages: the exit notice of process {pid} is missing from endpoint {}",
                o.endpoint
            ));
        };
        let got = Blame { cause: n.cause, account: n.blamed_account, labels: n.blamed_labels.clone() };
        ensure!(
            got == o.want,
            "Messages: the exit notice of process {pid} reports {got:?}, it should report {:?}",
            o.want
        );
    }
    for e in k.endpoints.values() {
        for n in &e.exits {
            ensure!(
                k.ghost.owed.get(&n.pid).is_some_and(|o| o.endpoint == e.id),
                "I7: endpoint {} holds an exit notice of process {} that is not owed to it",
                e.id,
                n.pid
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
