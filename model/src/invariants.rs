//! KERNEL-SPEC.md's invariants I1-I15, checked on the kernel model's state after every step.
//!
//! Each check recomputes what should be true from the objects themselves and from ghost records
//! (ghost.rs: taken from handle tables, budget objects and call arguments at the event), never from
//! the counters and derived fields the kernel keeps, so that a wrong update anywhere shows up here.
//! A thread's wait state counts as primary: whether a caller still waits for its reply is read
//! from the caller thread, not from the kernel's message record. I10 is a property of a pair of
//! calls and is checked by `check::budget_lifecycle`; I11's record-keeping is in ghost.rs
//! (`Ghost::took`); I14 is "the model never panics", which the property runner checks by catching
//! panics.
//!
//! Some checks are the direct statement of a rule (R2's cap, R3's lend staying with the server,
//! R4's delivery, R4a's limit); they are named after the rule.

use alloc::collections::{BTreeMap, BTreeSet};
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::ghost::{Blame, Flow, Key, group};
use crate::kernel::{Backing, Handle, Kernel, MapState, MsgKind, Object, Origin, ROOT, USERS, Wait};
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

/// The checker, with what it remembers between steps: the frames that existed after the last
/// step, so that a frame appearing now is known to be newly handed out (I9: it must read zero),
/// whatever the kernel's allocator believes; and the abandoned calls already reported (I15).
#[derive(Clone, Debug, Default)]
pub struct Checker {
    frames: BTreeSet<u64>,
    reported: BTreeSet<u64>,
}

impl Checker {
    pub fn new(k: &Kernel) -> Checker {
        Checker { frames: k.frames.keys().copied().collect(), reported: BTreeSet::new() }
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
        self.flows(k)?;
        for flow in &k.ghost.flows {
            if let Flow::Woken { tid, result: Ok(Ret::Call(c)) } = flow {
                use crate::syscall::LendDisposition as L;
                let (had_lend, taken, abandoned) = k.ghost.calls.get(tid).copied().unwrap_or_default();
                let expected = if !had_lend {
                    L::None
                } else if taken && abandoned {
                    L::Consumed
                } else {
                    L::Returned
                };
                ensure!(
                    c.lend == expected,
                    "IPC ownership: thread {tid} got {:?}, expected {expected:?}",
                    c.lend
                );
                ensure!(
                    c.reply.is_none() || matches!(c.status, Ok(()) | Err(Error::OutOfMemory)),
                    "IPC present reply with invalid status"
                );
                ensure!(
                    c.lend != L::Consumed
                        || (c.reply.is_none() && matches!(c.status, Err(Error::Timeout | Error::Dead))),
                    "IPC consumed lend has reply or invalid status"
                );
                ensure!(c.status.is_err() || c.reply.is_some(), "IPC success without committed reply");
            }
        }
        self.i9_memory(k)?;
        r2_r3_messages(k)?;
        self.i15_abandoned(k)?;
        r4_delivery(k)?;
        exits_owed(k)?;
        i13_timeouts(k)?;
        Ok(())
    }

    /// What the step did, judged against the ghost's records:
    /// - a delivered message is the one sent: its kind, badge, account, labels and handles are what the
    ///   sender's handles and budget gave (Messages), a handle revoked meanwhile arriving as 0 and every
    ///   other keeping its stamp (R9, R10); it went to a thread receiving through a badge-0 handle to its
    ///   endpoint (I4); between user budgets only equal label sets flow, the receiver being the endpoint's
    ///   owner (I7, R1); no transfer exceeds `max_transfer` (R4);
    /// - exit notices and usage reads flow only to ⊇ label sets or to a system-class budget, and a notice
    ///   reports what the ghost expects (I7; Messages);
    /// - a refusal (`LabelDenied` for a message or a usage read) happens only when R1 says so;
    /// - `Busy` for R2's cap only when the sender's group is full;
    /// - a minted handle's endpoint is one the minter holds a receive right to, or an open call of its thread
    ///   arrived on, with that source's default stamp (mint, R9);
    /// - a caller gets a reply only when its server replied (R4b), and not through a revoked handle (R10);
    /// - an abandoned-call notice goes to the thread holding the call, once (I15).
    fn flows(&mut self, k: &Kernel) -> Check {
        for f in &k.ghost.flows {
            match f {
                Flow::ReplyCompletion {
                    before,
                    after,
                    supplied,
                    output_valid,
                    completion,
                    delivered,
                    mask,
                } => {
                    ensure!(
                        *delivered == *output_valid && completion.reply.is_some() == *output_valid,
                        "IPC record/delivery commit disagrees with output validity"
                    );
                    for (slot, handle) in before {
                        ensure!(
                            after.get(slot) == Some(handle),
                            "IPC completion altered pre-existing handle {slot}"
                        );
                    }
                    if let Some(reply) = &completion.reply {
                        ensure!(reply.handles.len() == supplied.len(), "IPC reply lost positional slots");
                        let mut expected_mask = 0u8;
                        for (i, slot) in reply.handles.iter().enumerate() {
                            if *slot != 0 {
                                expected_mask |= 1 << i;
                                ensure!(
                                    !before.contains_key(slot) && after.get(slot) == Some(&supplied[i]),
                                    "IPC reply installed wrong handle"
                                );
                            }
                        }
                        ensure!(*mask == expected_mask, "IPC installed mask is not positional");
                        ensure!(
                            after.len() == before.len() + expected_mask.count_ones() as usize,
                            "IPC completion leaked handles outside reply"
                        );
                        ensure!(
                            completion.status
                                == if reply.handles.contains(&0) { Err(Error::OutOfMemory) } else { Ok(()) },
                            "IPC partial reply status"
                        );
                    } else {
                        ensure!(*mask == 0 && before == after, "IPC failed output leaked handle or mask");
                        ensure!(
                            completion.status == Err(Error::InvalidArgument),
                            "IPC failed output status precedence"
                        );
                    }
                }
                Flow::Delivered { tid, id, msg, via } => delivered(k, *tid, *id, msg, via)?,
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
                            "mint: process {pid} minted from handle {h}, not a receive right to endpoint {endpoint} \
                             stamped {default_stamp}"
                        );
                    }
                    MintSource::Message(rid) => {
                        let m = k.ghost.rids.get(&(*pid, *rid));
                        let open = m.is_some_and(|m| k.ghost.served.get(tid).is_some_and(|v| v.contains(m)));
                        let s = m.and_then(|m| k.ghost.sent.get(m));
                        ensure!(
                            open && s.is_some_and(|s| s.endpoint == *endpoint && s.stamp == *default_stamp),
                            "mint: thread {tid} minted from message {rid}, which is not its open call or came on \
                             another endpoint or stamp"
                        );
                    }
                },
                Flow::Woken {
                    tid,
                    result: Ok(Ret::Call(crate::syscall::CallCompletion { reply: Some(_), .. })),
                } => {
                    let replied = k.ghost.flows.iter().any(|g| match g {
                        Flow::Replied { msg } => k.ghost.sent.get(msg).is_some_and(|s| s.sender_tid == *tid),
                        _ => false,
                    });
                    ensure!(replied, "R4b: thread {tid} got a reply its server never sent");
                }
                Flow::Replied { msg } => {
                    let caller = k.ghost.sent.get(msg).map(|s| s.sender_tid);
                    let answered = k.ghost.flows.iter().any(
                        |g| matches!(g, Flow::Woken { tid, result: Ok(Ret::Call(crate::syscall::CallCompletion { reply: Some(_), .. })) } if Some(*tid) == caller),
                    );
                    let stamp = k.ghost.sent.get(msg).map_or(0, |s| s.stamp);
                    ensure!(
                        !answered || k.budgets.contains_key(&stamp),
                        "R10: the reply to {msg}, sent through a revoked handle, reached its caller"
                    );
                }
                // (A thread that died later in the step took its calls with it.)
                Flow::AbandonNotice { tid, msg } if k.threads.contains_key(tid) => {
                    ensure!(
                        k.ghost.served.get(tid).is_some_and(|v| v.contains(msg)),
                        "I15: thread {tid} was told of abandoned call {msg}, which it does not hold"
                    );
                    ensure!(
                        !awaited(k, *msg),
                        "I15: thread {tid} was told call {msg} was abandoned; its caller waits"
                    );
                    ensure!(
                        self.reported.insert(*msg),
                        "I15: thread {tid} was told twice that call {msg} was abandoned"
                    );
                }
                Flow::DeviceUsed { device, quarantined } => {
                    ensure!(!quarantined, "OD6: quarantined device {device} was handed out again");
                }
                Flow::Woken { .. } | Flow::AbandonNotice { .. } => {}
            }
        }
        Ok(())
    }

    /// I15: an abandoned call (taken, its caller no longer waiting, still open) is reported to the
    /// thread holding it: once it waits in `receive` on the call's endpoint, it has been told.
    fn i15_abandoned(&mut self, k: &Kernel) -> Check {
        self.reported.retain(|m| k.msgs.contains_key(m));
        for m in k.msgs.values() {
            let Some((_, stid)) = m.server else { continue };
            if m.kind != MsgKind::Call || awaited(k, m.id) || self.reported.contains(&m.id) {
                continue;
            }
            let waiting_here = k.threads.get(&stid).is_some_and(
                |t| matches!(t.wait, Some(Wait::Receive { endpoint, .. }) if endpoint == m.endpoint),
            );
            ensure!(
                !waiting_here,
                "I15: thread {stid} receives on endpoint {} and was never told its call {} was abandoned",
                m.endpoint,
                m.id
            );
        }
        Ok(())
    }

    /// I9 (R11): no mapping is writable and executable, or writable without being readable; a
    /// frame read zero when first handed out; a page is accessible in at most one address
    /// space, so a lent page is not the lender's until the call ends. R6 and R3: each frame is
    /// charged to whoever holds it (the lender while its call lasts, the server once the call is
    /// abandoned).
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
        // I-DMA (WP-K5b, answer 173): a frame waiting in the free pool for reuse (by any path,
        // DMA or not) is never one a device could still write: `dma_alloc`'s own frames are armed
        // in the same step they are created, so this checks the pool, not creation.
        for f in k.free_frames.keys() {
            ensure!(
                k.ghost.armed.get(f).is_none_or(BTreeSet::is_empty),
                "I-DMA: free frame {f} is still armed against {:?}",
                k.ghost.armed.get(f)
            );
        }
        let mut seen: BTreeMap<u64, Seen> = BTreeMap::new();
        for p in k.processes.values() {
            for (v, m) in &p.space {
                ensure!(
                    !(m.flags & FLAG_W != 0 && m.flags & FLAG_X != 0),
                    "I9: page {v:#x} of process {} is W+X",
                    p.pid
                );
                ensure!(
                    m.flags & FLAG_W == 0 || m.flags & FLAG_R != 0,
                    "I9: page {v:#x} of process {} is writable without being readable",
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
            if fr.quarantined {
                // WP-K5b, OD5: a quarantined frame outlives the process it was held by, mapped
                // nowhere, charged to its run's budget until that budget is destroyed (N1).
                ensure!(s.own.is_empty(), "I-DMA: quarantined frame {f} is still mapped by {:?}", s.own);
            } else if let Some(pid) = s.own.first() {
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
                let waiting = awaited(k, *mid);
                let want = if waiting { m.sender_budget } else { k.budget_of(*spid).unwrap() };
                ensure!(fr.payer == want, "R3: lent frame {f} is charged to {}, not {want}", fr.payer);
                if waiting {
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
            } else if fr.dma.is_some() && k.processes.values().any(|p| p.dma.contains(f)) {
                // WP-K5b, OD2: `unmap` keeps a DMA frame; it stays held (and charged) by its live
                // owner, mapped nowhere, until the process ends.
            } else {
                return Err(format!("R6: frame {f} is charged to {} but mapped nowhere", fr.payer));
            }
        }
        Ok(())
    }
}

/// Run the checks once on `k`, with no memory of earlier steps (so no frame counts as new).
pub fn check(k: &Kernel) -> Check { Checker::new(k).check(k) }

/// Does some thread wait for the reply to call `mid`? (Read from the caller threads.)
fn awaited(k: &Kernel, mid: u64) -> bool { k.threads.values().any(|t| t.wait == Some(Wait::Reply(mid))) }

/// A delivered message against the ghost's record of how it was sent (see `Checker::flows`).
fn delivered(
    k: &Kernel,
    tid: u64,
    id: u64,
    msg: &crate::syscall::Message,
    via: &Option<crate::ghost::Receiving>,
) -> Check {
    let Some(s) = k.ghost.sent.get(&id) else {
        return Err(format!("Messages: thread {tid} got message {id}, which was never sent"));
    };
    ensure!(
        msg.kind == s.kind && msg.badge == s.badge && msg.account == s.account && msg.labels == s.labels,
        "Messages: message {id} arrived as {:?} badge {} account {} labels {:?}; it was sent as {:?} badge {} account \
         {} labels {:?}",
        msg.kind,
        msg.badge,
        msg.account,
        msg.labels,
        s.kind,
        s.badge,
        s.account,
        s.labels
    );
    ensure!(
        msg.handles.len() == s.handles.len(),
        "R10: message {id} arrived with {} handles, it was sent with {}",
        msg.handles.len(),
        s.handles.len()
    );
    let pid = k.threads.get(&tid).map(|t| t.pid);
    for (i, (h, sent)) in msg.handles.iter().zip(&s.handles).enumerate() {
        // Closed while queued: revoked (its stamp destroyed), or its object destroyed.
        let live = match sent.object {
            Object::Budget(b) => k.budgets.contains_key(&b),
            Object::Process(p) => {
                k.ghost.slots.get(&p).is_some_and(|slot| k.budgets.contains_key(&slot.payer))
                    && (k.processes.contains_key(&p)
                        || k.ghost.owed.get(&p).is_some_and(|o| k.endpoints.contains_key(&o.endpoint)))
            }
            Object::Endpoint(e) => k.endpoints.contains_key(&e),
            Object::Device(d) => k.devices.contains_key(&d),
        };
        let closed = !k.budgets.contains_key(&sent.stamp) || !live;
        let stamp = sent.stamp;
        ensure!(
            (*h == NO_HANDLE) == closed,
            "R10: handle {i} of message {id} arrived as {h}; it was {}closed",
            if closed { "" } else { "not " }
        );
        if *h != NO_HANDLE {
            let hd = pid.and_then(|p| k.processes.get(&p)).and_then(|p| p.handles.get(h));
            ensure!(
                hd.is_some_and(|x| x.stamp == stamp),
                "R9: handle {i} of message {id} arrived as {h}, not stamped {stamp}"
            );
        }
    }
    let Some(via) = via else {
        return Err(format!("I4: thread {tid} got message {id} without receiving"));
    };
    ensure!(
        via.handle.badge == 0 && via.handle.object == Object::Endpoint(s.endpoint),
        "I4: thread {tid} received message {id} through {:?}",
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
    let pages = msg.buffer.filter(|b| b.kind == crate::syscall::BufferKind::Transfer).map_or(0, |b| b.pages);
    ensure!(
        pages <= via.max_transfer,
        "R4: {pages} pages transferred to a receiver that allowed {}",
        via.max_transfer
    );
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
        for h in m.handles.iter().flatten() {
            out.push((format!("message {}", m.id), *h));
        }
    }
    out
}

/// R2's group of a message, from the ghost's record of how it was sent.
fn ghost_key(k: &Kernel, m: u64) -> Option<Key> {
    k.ghost.sent.get(&m).map(|s| group(s.account, s.labels.clone(), s.sender_budget))
}

/// The object graph is well formed and alive: every process's budget, every thread's process,
/// every queued message and waiting receiver exist and agree; a queued message is in its sender's
/// group and was sent through a live handle (R2, R10); a thread blocked in `send`, `call` or
/// waiting for a reply waits on something that still exists (R10); no budget outlives its
/// deadline; a process's object is paid by a live budget, and its exit endpoint is a receive right
/// (QUESTIONS 74, 93); the scheduler's runnable threads are exactly the runnable threads.
fn structure(k: &Kernel) -> Check {
    for p in k.processes.values() {
        ensure!(
            k.budgets.contains_key(&p.budget),
            "R10: process {} lives in destroyed budget {}",
            p.pid,
            p.budget
        );
        let slot = k.ghost.slots.get(&p.pid);
        ensure!(
            slot.is_some_and(|s| k.budgets.contains_key(&s.payer)),
            "R10: process {} runs, but the budget its object is charged to is destroyed",
            p.pid
        );
        ensure!(
            slot.is_some_and(|s| s.badge == 0),
            "process_create: process {}'s exit endpoint is a badged handle",
            p.pid
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
                "R10: the exit notice of process {} on endpoint {} outlived its object's payer {}",
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
                    msg.is_some_and(|x| x.sender_tid == t.tid && x.server.is_some()),
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
    // Scheduler bookkeeping: each budget's weight limit and carve, and exactly its runnable
    // threads.
    for (b, e) in &k.sched.budgets {
        let Some(bx) = k.budgets.get(b) else {
            return Err(format!("R12: scheduler keeps destroyed budget {b}"));
        };
        ensure!(e.limit == bx.weight, "R12: scheduler has budget {b}'s weight wrong");
        ensure!(e.carved == bx.weight_used, "R7/R12: scheduler has budget {b}'s carve wrong");
        ensure!(
            u128::from(e.rem) < u128::from(k.sched.weight(*b)).max(1),
            "R12: budget {b}'s remainder is not below its weight"
        );
        for (pid, t) in &e.runnable {
            ensure!(
                k.threads
                    .get(t)
                    .is_some_and(|x| x.pid == *pid && x.wait.is_none() && k.budget_of(x.pid) == Some(*b)),
                "R12: scheduler queues thread {t}, which is not runnable in budget {b}"
            );
        }
    }
    for t in k.threads.values().filter(|t| t.wait.is_none()) {
        let b = k.budget_of(t.pid).unwrap();
        ensure!(
            k.sched.budgets.get(&b).is_some_and(|e| e.runnable.contains(&(t.pid, t.tid))),
            "R12: runnable thread {} is not queued",
            t.tid
        );
    }
    for b in k.budgets.keys() {
        ensure!(k.sched.budgets.contains_key(b), "R12: budget {b} is missing from the scheduler");
    }
    // R7/R12: a budget that holds a process has free weight (its stride weight) above 0.
    for p in k.processes.values() {
        let bx = &k.budgets[&p.budget];
        ensure!(
            bx.weight > bx.weight_used,
            "R7/R12: budget {} holds process {} with free weight 0",
            p.budget,
            p.pid
        );
    }
    // The floor never exceeds a queued budget's pass by more than the running budget's unfolded
    // runtime could explain: every queued pass is at least the floor, except a waker not yet
    // reconciled.
    for (b, e) in &k.sched.budgets {
        ensure!(
            !e.queued || e.pass >= k.sched.floor || e.runnable.is_empty(),
            "R12: queued budget {b} below the floor"
        );
    }
    Ok(())
}

/// What each thread serves (R4a): exactly the calls the ghost saw delivered to it and not yet
/// replied to; its current call is the one the ghost saw it take or `serve` last, none after a
/// `receive` returned anything else (QUESTIONS 82); every taken call is served by exactly one live
/// thread; a process holds at most `MAX_OPEN_CALLS`.
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
        let current = k.ghost.current.get(&t.tid).copied();
        ensure!(
            t.current == current,
            "Process: thread {}'s current call is {:?}, it should be {current:?}",
            t.tid,
            t.current
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
            Object::Process(p) => {
                k.ghost.slots.get(&p).is_some_and(|slot| k.budgets.contains_key(&slot.payer))
                    && (k.processes.contains_key(&p)
                        || k.ghost.owed.get(&p).is_some_and(|o| k.endpoints.contains_key(&o.endpoint)))
            }
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
/// budgets' own pages to their parents, process objects to their creators while the process
/// runs or its notice waits, page tables, threads, handle tables, endpoints, frames, open calls,
/// and lends to their receivers while the caller waits); usage fits the limits, always (QUESTIONS
/// 70); children's limits and own pages plus its own objects fit.
fn i5_charging(k: &Kernel) -> Check {
    let c = k.costs;
    let mut pages: BTreeMap<u64, u64> = BTreeMap::new();
    let mut procs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut add = |b: u64, n: u64| *pages.entry(b).or_default() += n;
    for b in k.budgets.values() {
        if let Some(p) = b.parent {
            add(p, c.budget + b.pages_limit);
            *procs.entry(p).or_default() += b.processes_limit;
        }
    }
    for p in k.processes.values() {
        add(
            p.budget,
            page_tables(&p.space) * c.page_table
                + p.handles.keys().map(|h| (h - 1) / c.handles_per_page).collect::<BTreeSet<_>>().len()
                    as u64
                + c.contexts
                + p.threads.len() as u64 * c.thread,
        );
        *procs.entry(p.budget).or_default() += 1;
    }
    // Process objects, as the ghost recorded them at `process_create`: charged to the creator's
    // budget while the process runs and while its notice waits (QUESTIONS 74).
    for (pid, s) in &k.ghost.slots {
        let waiting = k.ghost.owed.get(pid).is_some_and(|o| k.endpoints.contains_key(&o.endpoint));
        if k.budgets.contains_key(&s.payer) && (k.processes.contains_key(pid) || waiting) {
            add(s.payer, c.process);
        }
    }
    for e in k.endpoints.values() {
        add(e.owner, c.endpoint);
    }
    for f in k.frames.values() {
        add(f.payer, 1);
    }
    for m in k.msgs.values().filter(|m| m.kind == MsgKind::Call) {
        let Some((spid, _)) = m.server else { continue };
        let Some(sb) = k.budget_of(spid) else { continue };
        add(sb, c.open_call);
        if awaited(k, m.id) {
            add(sb, k.ghost.sent.get(&m.id).map_or(0, |s| s.lent_pages));
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
        ensure!(
            b.pages_used <= b.pages_limit,
            "I5: budget {} uses {} of {} pages",
            b.id,
            b.pages_used,
            b.pages_limit
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
/// I8: class(child) = class(parent); account is inherited unless the parent's is 0.
/// I12: budget ids ascend.
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
        let Some(p) = b.parent.and_then(|p| k.budgets.get(&p)) else { continue };
        ensure!(superset(&b.labels, &p.labels), "I6: budget {} lacks its parent's labels", b.id);
        if b.labels != p.labels {
            ensure!(
                k.ghost.creator_class.get(&b.id) == Some(&Class::System),
                "I6: budget {} got labels from a user-class creator",
                b.id
            );
        }
        // `users`, which the kernel makes class user under `root`, is the one exception (README
        // spec problem 9).
        ensure!(
            b.class == p.class || b.id == USERS,
            "I8: budget {} has class {:?}, its parent {:?}",
            b.id,
            b.class,
            p.class
        );
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

/// R2: at most `WAIT_CAP` queued messages per group per endpoint, grouped by the ghost's record of
/// each sender. R3: a server keeps a lent buffer mapped until it replies, even when its caller is
/// gone.
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

/// R4 and R4a: nothing deliverable waits while a receiver waits for it. After every step, a thread
/// waiting in `receive` on an endpoint has no exit notice pending there, and every message queued
/// there is a call its process cannot take (it holds `MAX_OPEN_CALLS`): a message it could take
/// would have been delivered or refused.
fn r4_delivery(k: &Kernel) -> Check {
    for e in k.endpoints.values() {
        let Some(r) = e.receivers.front() else { continue };
        ensure!(
            e.exits.is_empty(),
            "Messages: an exit notice waits on endpoint {} while thread {r} receives",
            e.id
        );
        for r in &e.receivers {
            let pid = k.threads[r].pid;
            let full = k.open_calls(pid) >= MAX_OPEN_CALLS;
            for m in e.queue.values().flatten() {
                let call = k.ghost.sent.get(m).is_some_and(|s| s.kind == MsgKind::Call);
                ensure!(
                    call && full,
                    "R4a: message {m} waits on endpoint {} while thread {r} (holding {} open calls) receives there",
                    e.id,
                    k.open_calls(pid)
                );
            }
        }
    }
    Ok(())
}

/// Exit notices (Messages; R1; R10; QUESTIONS 55, 74, 82): every notice the ghost saw owed, whose
/// endpoint still exists, waits on it (receiving it clears it) if its object's payer lives, and is
/// gone if the payer does not; it reports what the ghost expects; and every waiting notice is
/// owed.
fn exits_owed(k: &Kernel) -> Check {
    for (pid, o) in &k.ghost.owed {
        let Some(e) = k.endpoints.get(&o.endpoint) else { continue };
        let n = e.exits.iter().find(|n| n.pid == *pid);
        if !k.budgets.contains_key(&o.payer) {
            ensure!(
                n.is_none(),
                "R10: the exit notice of process {pid} outlived its object's payer {}",
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
