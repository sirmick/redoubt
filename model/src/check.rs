//! The property tests: families of random sequences, each checked step by step.
//!
//! | Family | What it checks |
//! | --- | --- |
//! | [`kernel_sequence`] | I1-I9, I11-I13 and the rule checks after every step of a random sequence |
//! | [`budget_lifecycle`] | I10: create a budget, use it, destroy it; everyone else's counters are unchanged |
//! | [`scheduler_fairness`] | R12: each budget's share over every interval it was runnable |
//! | `policy::steward_policy` | the steward's M1 policy (steward.rs) |
//! | `policy::steward_noninterference` | the policy's non-interference property |
//!
//! Every family is a function of one seed (and an optional mutation), so any failure is
//! reproduced by rerunning that seed; [`shrink`] cuts a failing kernel sequence down to the ops
//! that matter, and `trace::record` prints it. I14 (no panic) is checked by the test runner,
//! which catches panics around each family (tests/properties.rs).

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::gen::{Gen, Rng};
use crate::invariants::Checker;
use crate::kernel::{Boot, Kernel};
use crate::mutation::Mutation;
use crate::sched::Scheduler;
use crate::spec::SLICE;
use crate::syscall::Op;

/// A property that did not hold.
#[derive(Clone, Debug)]
pub struct Failure {
    pub family: &'static str,
    pub seed: u64,
    pub message: String,
    /// The kernel ops that led there (empty for families that do not drive the kernel model).
    pub ops: Vec<Op>,
}

/// Default length of a random kernel sequence.
pub const SEQUENCE_LEN: usize = 150;

/// Run `ops` from boot, checking every invariant after each step. Ops that are not legal events
/// at their point (possible after shrinking) are skipped.
pub fn replay(boot: &Boot, ops: &[Op], mutation: Option<Mutation>) -> Result<(), String> {
    let mut k = Kernel::boot(boot, mutation)?;
    let mut checker = Checker::new(&k);
    checker.check(&k)?;
    for op in ops {
        if k.step(op).is_some() {
            checker.check(&k)?;
        }
    }
    Ok(())
}

/// A random sequence from boot, with every invariant checked after each step.
pub fn kernel_sequence(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    let mut k = Kernel::boot(&Boot::testing(), mutation).expect("the testing boot is valid");
    let mut checker = Checker::new(&k);
    let mut gen = Gen::new(seed);
    let mut ops = Vec::new();
    let fail = |message: String, ops: &Vec<Op>| Failure {
        family: "kernel_sequence",
        seed,
        message,
        ops: ops.clone(),
    };
    checker.check(&k).map_err(|m| fail(m, &ops))?;
    for _ in 0..SEQUENCE_LEN {
        // Stop when nothing can happen any more (halted, or every thread blocked for ever).
        if k.halted.is_some() || (k.runnable().is_empty() && k.next_event().is_none()) {
            break;
        }
        let op = gen.next_op(&k);
        ops.push(op.clone());
        if k.step(&op).is_none() {
            return Err(fail(format!("generator produced an illegal op: {op:?}"), &ops));
        }
        checker.check(&k).map_err(|m| fail(m, &ops))?;
    }
    Ok(())
}

/// Handle indices the epilogue probes in each process.
pub const EPILOGUE_HANDLES: u64 = 64;

/// Probes that make the model's state visible in results, for the end of a conformance trace.
/// Comparing results alone misses a kernel whose state has diverged but not yet shown it (a
/// handle that should have been revoked but was never used again), so the epilogue, planned on a
/// copy of `k` step by step:
/// 1. lets time pass until every blocked call with a finite timeout has returned (so its thread can probe
///    too);
/// 2. has each process with a runnable thread, in pid order, read one word of every readable page it has
///    mapped (contents, zeroing, lends) and the usage of every budget it holds a handle to (charging);
/// 3. has `init` destroy the budgets it made, one at a time, reading the usage of every budget it still holds
///    after each (revocation, R10's returned limits);
/// 4. has each process still alive with a runnable thread close every handle index from 1 to
///    `EPILOGUE_HANDLES` (`Ok` or `BadHandle` shows its table, and so the stamps).
///
/// Not visible even so: a thread blocked for ever, a budget's account (it travels only in
/// messages), and an IRQ source's mask (it shows only as a later interrupt); traces meant to check
/// those must exercise them.
pub fn epilogue(k: &Kernel) -> Vec<Op> {
    use crate::kernel::{INIT_PID, MAX_TICK, MapState, Object, ROOT, SYSTEM, USERS};
    use crate::spec::{FLAG_R, PAGE_SIZE};
    use crate::syscall::Syscall;
    let mut k = k.clone();
    let mut ops = Vec::new();
    // A halted machine takes no more events.
    if k.halted.is_some() {
        return ops;
    }
    let go = |k: &mut Kernel, op: Op, ops: &mut Vec<Op>| {
        if k.step(&op).is_some() {
            ops.push(op);
        }
    };
    let one_thread_each = |k: &Kernel| {
        let mut out: Vec<(u64, u64)> = Vec::new();
        for (pid, tid) in k.runnable() {
            if !out.iter().any(|(p, _)| *p == pid) {
                out.push((pid, tid));
            }
        }
        out
    };
    // 1. Blocked calls with a timeout return.
    let last = k.threads.values().filter(|t| t.wait.is_some()).filter_map(|t| t.deadline).max();
    if let Some(d) = last {
        let dt = d.saturating_sub(k.now).clamp(1, MAX_TICK);
        go(&mut k, Op::Tick { dt }, &mut ops);
    }
    // 2. Pages and usage.
    for (pid, tid) in one_thread_each(&k) {
        let Some(p) = k.processes.get(&pid) else { continue };
        let mut probes = Vec::new();
        for (v, m) in &p.space {
            if m.flags & FLAG_R != 0 && matches!(m.state, MapState::Own | MapState::LentIn(_)) {
                probes.push(Op::Read { pid, tid, addr: v * PAGE_SIZE });
            }
        }
        for (i, h) in &p.handles {
            if matches!(h.object, Object::Budget(_)) {
                probes.push(Op::Sys { pid, tid, call: Syscall::BudgetUsage { h: *i } });
            }
        }
        for op in probes {
            go(&mut k, op, &mut ops);
        }
    }
    // 3. Revocation, one budget at a time.
    while let Some(init) = k.processes.get(&INIT_PID) {
        let Some(tid) = init.threads.iter().copied().find(|t| k.threads[t].wait.is_none()) else { break };
        let Some(h) = init
            .handles
            .iter()
            .find(|(_, h)| matches!(h.object, Object::Budget(b) if b != ROOT && b != SYSTEM && b != USERS))
            .map(|(i, _)| *i)
        else {
            break;
        };
        go(&mut k, Op::Sys { pid: INIT_PID, tid, call: Syscall::BudgetDestroy { h } }, &mut ops);
        let held: Vec<u64> = k.processes.get(&INIT_PID).map_or(Vec::new(), |p| {
            p.handles.iter().filter(|(_, h)| matches!(h.object, Object::Budget(_))).map(|(i, _)| *i).collect()
        });
        for h in held {
            go(&mut k, Op::Sys { pid: INIT_PID, tid, call: Syscall::BudgetUsage { h } }, &mut ops);
        }
    }
    // 4. Handle tables.
    for (pid, tid) in one_thread_each(&k) {
        for i in 1..=EPILOGUE_HANDLES {
            go(&mut k, Op::Sys { pid, tid, call: Syscall::HandleClose { h: i } }, &mut ops);
        }
    }
    ops
}

/// Shrink a failing sequence: drop ops (in halving chunks, then one at a time) while it still
/// fails, with any message.
pub fn shrink(boot: &Boot, ops: &[Op], mutation: Option<Mutation>) -> Vec<Op> {
    let mut ops = ops.to_vec();
    let mut chunk = ops.len().div_ceil(2).max(1);
    loop {
        let mut i = 0;
        let mut changed = false;
        while i < ops.len() {
            let end = (i + chunk).min(ops.len());
            let mut candidate = ops.clone();
            candidate.drain(i..end);
            if replay(boot, &candidate, mutation).is_err() {
                ops = candidate;
                changed = true;
            } else {
                i += chunk;
            }
        }
        if chunk == 1 && !changed {
            return ops;
        }
        if !changed {
            chunk = (chunk / 2).max(1);
        }
    }
}

/// I10: "Creating and then destroying a budget leaves its parent's usage and free limits
/// unchanged." A random prefix builds a world; then a runnable thread holding a budget handle
/// creates a child, only processes inside the child's subtree act for a while (no time passes,
/// so no other deadline can fire), and the creator destroys the child. Every other budget's
/// counters must be what they were.
pub fn budget_lifecycle(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    use crate::kernel::Object;
    use crate::syscall::Syscall;
    let mut k = Kernel::boot(&Boot::testing(), mutation).expect("the testing boot is valid");
    let mut checker = Checker::new(&k);
    let mut gen = Gen::new(seed);
    let mut ops = Vec::new();
    let fail = |message: String, ops: &Vec<Op>| Failure {
        family: "budget_lifecycle",
        seed,
        message,
        ops: ops.clone(),
    };
    let mut step = |k: &mut Kernel, op: Op, ops: &mut Vec<Op>| -> Result<crate::kernel::Step, Failure> {
        ops.push(op.clone());
        let s = k.step(&op).ok_or_else(|| fail(format!("illegal op {op:?}"), ops))?;
        checker.check(k).map_err(|m| fail(m, ops))?;
        Ok(s)
    };
    for _ in 0..gen.rng.range(20, 60) {
        if k.halted.is_some() {
            return Ok(());
        }
        let op = gen.next_op(&k);
        step(&mut k, op, &mut ops)?;
    }
    // A creator: a runnable thread holding a handle to a live budget with room for a child.
    let mut candidates = Vec::new();
    for (pid, tid) in k.runnable() {
        for (i, h) in &k.processes[&pid].handles {
            if let Object::Budget(b) = h.object {
                let bx = &k.budgets[&b];
                if bx.pages_limit.saturating_sub(bx.pages_used) >= 8 && bx.processes_limit > bx.processes_used
                {
                    candidates.push((pid, tid, *i, b));
                }
            }
        }
    }
    let Some((pid, tid, ph, parent)) = gen.rng.pick(&candidates) else { return Ok(()) };
    let snapshot = |k: &Kernel| -> Vec<(u64, u64, u64, u64)> {
        k.budgets.values().map(|b| (b.id, b.pages_used, b.processes_used, b.weight_used)).collect()
    };
    // (A receive right, so that the creator can receive the child's exit notice at the end; it
    // receives what is already pending first, so that only the child's notice is left then.)
    let exit = k.processes[&pid]
        .handles
        .iter()
        .find(|(_, h)| matches!(h.object, Object::Endpoint(_)) && h.badge == 0)
        .map(|(i, _)| *i);
    type StepFn<'a> = dyn FnMut(&mut Kernel, Op, &mut Vec<Op>) -> Result<crate::kernel::Step, Failure> + 'a;
    let drain = |k: &mut Kernel, ops: &mut Vec<Op>, step: &mut StepFn| -> Result<bool, Failure> {
        let Some(e) = exit else { return Ok(true) };
        loop {
            // This helper calls receive directly, outside the generator's actor selection.
            // Retire a one-shot call-output fault through an explicit event first: late
            // receive-output failure is outside the accepted oracle domain (question 171).
            if k.threads[&tid].record == crate::syscall::Record::CopyFault {
                step(k, Op::Record { pid, tid, record: crate::syscall::Record::Owned }, ops)?;
            }
            let recv = Syscall::Receive { h: Some(e), timeout: 0, max_transfer: 0 };
            let s = step(k, Op::Sys { pid, tid, call: recv }, ops)?;
            match s.outcome {
                crate::syscall::Outcome::Done(Ok(
                    crate::syscall::Ret::ExitNotice { .. } | crate::syscall::Ret::Abandoned { .. },
                )) => {}
                crate::syscall::Outcome::Done(Err(crate::spec::Error::Timeout)) => return Ok(true),
                _ => return Ok(false),
            }
        }
    };
    if !drain(&mut k, &mut ops, &mut step)? {
        return Ok(());
    }
    let before = snapshot(&k);
    let pb = &k.budgets[&parent];
    let free = pb.pages_limit - pb.pages_used;
    let create = Syscall::BudgetCreate {
        parent: ph,
        pages: gen.rng.range(4, free / 2 + 3),
        processes: 1,
        weight: gen.rng.range(0, pb.weight - pb.weight_used),

        labels: pb.labels.clone(),
        account: 0,
        deadline: crate::spec::FOREVER,
    };
    let s = step(&mut k, Op::Sys { pid, tid, call: create }, &mut ops)?;
    let crate::syscall::Outcome::Done(Ok(crate::syscall::Ret::Handle(bh))) = s.outcome else { return Ok(()) };
    let Some(Object::Budget(child)) =
        k.processes.get(&pid).and_then(|p| p.handles.get(&bh)).map(|h| h.object)
    else {
        return Ok(());
    };
    // Start one process in the child with only the child's handle, if there is an endpoint to
    // name as its exit endpoint.
    if let Some(e) = exit {
        let s = step(
            &mut k,
            Op::Sys { pid, tid, call: Syscall::ProcessCreate { budget: bh, exit_endpoint: e } },
            &mut ops,
        )?;
        if let crate::syscall::Outcome::Done(Ok(crate::syscall::Ret::Handle(proc_h))) = s.outcome {
            let start =
                Syscall::ProcessStart { process: proc_h, entry: 0, sp: 0, arg: 0, handles: alloc::vec![bh] };
            step(&mut k, Op::Sys { pid, tid, call: start }, &mut ops)?;
        }
    }
    // Only threads inside the child's subtree act; no ticks, no interrupts.
    for _ in 0..gen.rng.range(0, 40) {
        if !k.budgets.contains_key(&child) {
            break;
        }
        let Some(op) = gen.next_op_in(&k, child) else { break };
        step(&mut k, op, &mut ops)?;
    }
    if k.threads.get(&tid).is_none_or(|t| t.wait.is_some()) || !k.budgets.contains_key(&child) {
        return Ok(());
    }
    step(&mut k, Op::Sys { pid, tid, call: Syscall::BudgetDestroy { h: bh } }, &mut ops)?;
    // The child's processes' objects are charged to their creator (QUESTIONS 74) and stay until
    // their notices are received (I10: "once its processes' exit notices are received or
    // dropped"): the creator receives them. A message arriving instead changes the creator's state,
    // and the sequence proves nothing.
    if !drain(&mut k, &mut ops, &mut step)? {
        return Ok(());
    }
    let after = snapshot(&k);
    if before != after {
        return Err(fail(
            format!(
                "I10: budget {child} created under {parent} and destroyed changed usage: {before:?} -> {after:?}"
            ),
            &ops,
        ));
    }
    Ok(())
}

/// The flood (PLAN.md's milestone 1 attack case "endpoint flooding: 10,000 blocked senders on
/// `fsd`, and Alice is still served in her turn"), on a boot big enough to hold it. A system
/// server receives on one endpoint; Bob's processes (two label sets of one account, so two R2
/// groups) run up to 31 threads each, every thread calling with no timeout; a crowd of up to
/// eight other accounts queues `WAIT_CAP` calls each; Alice makes one call in the middle of the
/// flood. Most of Bob's calls get `Busy` (R2's cap per group); Alice's call must be taken within
/// as many receives as there are groups (I11). The server receives with two threads in turn; half
/// the seeds' servers hoard: they receive without replying, so open calls pile up to
/// `MAX_OPEN_CALLS` for the process, spread over both threads (R4a counts per process).
///
/// Up to 10,000 senders per seed, so the invariants are checked every 512 steps and at the end
/// (ghost violations are recorded as they happen and reported at the next check).
pub fn flood(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    use crate::kernel::{DeviceSpec, INIT_PID, Limits, Note};
    use crate::spec::{Error, FOREVER, WORDS};
    use crate::syscall::{Outcome, Ret, Syscall};
    let mut rng = Rng::new(seed);
    let fail = |message: String| Failure { family: "flood", seed, message, ops: Vec::new() };
    let boot = Boot {
        root: Limits { pages: 60_000, processes: 700, weight: 10_000 },
        system: Limits { pages: 1_000, processes: 10, weight: 1_000 },
        users: Limits { pages: 55_000, processes: 680, weight: 8_000 },
        devices: alloc::vec![DeviceSpec::Reset],
        ..Boot::default()
    };
    let mut k = Kernel::boot(&boot, mutation).map_err(fail)?;
    let mut checker = Checker::new(&k);
    let mut steps = 0u64;
    let mut run =
        |k: &mut Kernel, pid: u64, tid: u64, call: Syscall| -> Result<crate::kernel::Step, Failure> {
            let s = k
                .step(&Op::Sys { pid, tid, call })
                .ok_or_else(|| fail(String::from("illegal flood step")))?;
            steps += 1;
            if steps.is_multiple_of(512) {
                checker.check(k).map_err(fail)?;
            }
            Ok(s)
        };
    let handle = |s: &crate::kernel::Step| match s.outcome {
        Outcome::Done(Ok(Ret::Handle(h))) => Some(h),
        _ => None,
    };
    let started = |s: &crate::kernel::Step| {
        s.notes.iter().find_map(|n| match n {
            Note::Thread { pid, tid } => Some((*pid, *tid)),
            _ => None,
        })
    };
    let budget = |pages, processes, weight, labels: alloc::vec::Vec<u64>, account, parent| {
        Syscall::BudgetCreate { parent, pages, processes, weight, labels, account, deadline: FOREVER }
    };
    // init's slots: 1 root, 2 system, 3 users, 4 the Reset device.
    let e =
        handle(&run(&mut k, INIT_PID, 1, Syscall::EndpointCreate)?).ok_or_else(|| fail("endpoint".into()))?;
    let (system_h, users_h) = (2, 3);
    let hs = handle(&run(&mut k, INIT_PID, 1, budget(200, 2, 500, alloc::vec![], 0, system_h))?);
    let hs = hs.ok_or_else(|| fail("server budget".into()))?;
    let ps =
        handle(&run(&mut k, INIT_PID, 1, Syscall::ProcessCreate { budget: hs, exit_endpoint: e })?).unwrap();
    let (spid, stid) = started(&run(
        &mut k,
        INIT_PID,
        1,
        Syscall::ProcessStart { process: ps, entry: 0, sp: 0, arg: 0, handles: alloc::vec![e] },
    )?)
    .unwrap();
    let stid2 = match run(&mut k, spid, stid, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })?.outcome {
        Outcome::Done(Ok(Ret::Tid(t))) => t,
        _ => return Err(fail("the server's second thread".into())),
    };
    // Bob: two label sets of account 1002 (two R2 groups); Alice: account 1001.
    let senders = if rng.pct(20) { 10_000 } else { rng.range(40, 2_000) };
    let procs = senders.div_ceil(31);
    let hb =
        handle(&run(&mut k, INIT_PID, 1, budget(50_000, procs + 2, 4_000, alloc::vec![], 1002, users_h))?)
            .unwrap();
    let hb2 = handle(&run(&mut k, INIT_PID, 1, budget(20_000, procs / 2 + 1, 1_000, alloc::vec![7], 0, hb))?)
        .unwrap();
    let ha = handle(&run(&mut k, INIT_PID, 1, budget(100, 2, 1_000, alloc::vec![], 1001, users_h))?).unwrap();
    type Runner<'a> = dyn FnMut(&mut Kernel, u64, u64, Syscall) -> Result<crate::kernel::Step, Failure> + 'a;
    let mint = |k: &mut Kernel, run: &mut Runner, into: u64| {
        run(
            k,
            INIT_PID,
            1,
            Syscall::Mint { source: crate::syscall::MintSource::Handle(e), badge: 1, budget: Some(into) },
        )
        .map(|s| handle(&s))
    };
    let mut bobs = Vec::new();
    for i in 0..procs {
        let hbud = if i % 2 == 1 { hb2 } else { hb };
        let Some(me) = mint(&mut k, &mut run, hbud)? else { break };
        let Some(p) =
            handle(&run(&mut k, INIT_PID, 1, Syscall::ProcessCreate { budget: hbud, exit_endpoint: e })?)
        else {
            break;
        };
        let Some(t) = started(&run(
            &mut k,
            INIT_PID,
            1,
            Syscall::ProcessStart { process: p, entry: 0, sp: 0, arg: 0, handles: alloc::vec![me] },
        )?) else {
            break;
        };
        bobs.push(t);
        run(&mut k, INIT_PID, 1, Syscall::HandleClose { h: me })?;
    }
    // The crowd: other accounts, one process each, WAIT_CAP threads calling. The first two have
    // account 0, so each is its own group only by its budget id (QUESTIONS 87).
    // At least three, so that more than MAX_OPEN_CALLS calls can queue.
    let crowd = rng.range(3, 8);
    let mut crowd_threads = Vec::new();
    for i in 0..crowd {
        let hc = handle(&run(
            &mut k,
            INIT_PID,
            1,
            budget(200, 1, 10, alloc::vec![], if i < 2 { 0 } else { 2000 + i }, users_h),
        )?)
        .unwrap();
        let mc = mint(&mut k, &mut run, hc)?.unwrap();
        let p = handle(&run(&mut k, INIT_PID, 1, Syscall::ProcessCreate { budget: hc, exit_endpoint: e })?)
            .unwrap();
        let t = started(&run(
            &mut k,
            INIT_PID,
            1,
            Syscall::ProcessStart { process: p, entry: 0, sp: 0, arg: 0, handles: alloc::vec![mc] },
        )?)
        .unwrap();
        crowd_threads.push(t);
    }
    let me = mint(&mut k, &mut run, ha)?.unwrap();
    let pa =
        handle(&run(&mut k, INIT_PID, 1, Syscall::ProcessCreate { budget: ha, exit_endpoint: e })?).unwrap();
    let (apid, atid) = started(&run(
        &mut k,
        INIT_PID,
        1,
        Syscall::ProcessStart { process: pa, entry: 0, sp: 0, arg: 0, handles: alloc::vec![me] },
    )?)
    .unwrap();
    // The flood: each of Bob's threads calls once; each process spawns its next thread first.
    let call =
        Syscall::Call { h: 1, words: [0; WORDS], handles: alloc::vec![], lend: None, timeout: FOREVER };
    for (pid, first) in crowd_threads {
        let mut tid = first;
        for _ in 0..crate::spec::WAIT_CAP {
            let next = match run(&mut k, pid, tid, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })?.outcome
            {
                Outcome::Done(Ok(Ret::Tid(t))) => t,
                _ => break,
            };
            if matches!(
                run(&mut k, pid, tid, call.clone())?.outcome,
                Outcome::Done(Ok(Ret::Call(crate::syscall::CallCompletion { status: Err(Error::Busy), .. })))
            ) {
                return Err(fail(format!("R2: crowd process {pid} got Busy within its own group's cap")));
            }
            tid = next;
        }
    }
    let mut busy = 0u64;
    let mut sent = 0u64;
    let alice_at = rng.range(0, senders);
    let mut alice_msg = None;
    'flood: for (pid, first) in bobs {
        let mut tid = first;
        for _ in 0..31 {
            if sent == alice_at && alice_msg.is_none() {
                run(&mut k, apid, atid, call.clone())?;
                alice_msg = k.msgs.values().find(|m| m.sender_tid == atid).map(|m| m.id);
            }
            if sent >= senders {
                break 'flood;
            }
            let next = match run(&mut k, pid, tid, Syscall::ThreadCreate { entry: 0, sp: 0, arg: 0 })?.outcome
            {
                Outcome::Done(Ok(Ret::Tid(t))) => Some(t),
                _ => None,
            };
            let s = run(&mut k, pid, tid, call.clone())?;
            sent += 1;
            if matches!(
                s.outcome,
                Outcome::Done(Ok(Ret::Call(crate::syscall::CallCompletion { status: Err(Error::Busy), .. })))
            ) {
                busy += 1;
            }
            match next {
                Some(t) => tid = t,
                None => break,
            }
        }
    }
    if alice_msg.is_none() {
        run(&mut k, apid, atid, call.clone())?;
        alice_msg = k.msgs.values().find(|m| m.sender_tid == atid).map(|m| m.id);
    }
    // Two groups of Bob's may queue WAIT_CAP each; everything else got Busy.
    if sent > 2 * crate::spec::WAIT_CAP && busy + 2 * crate::spec::WAIT_CAP < sent {
        return Err(fail(format!("R2: of {sent} flooding calls only {busy} got Busy")));
    }
    // The server receives (and, unless it hoards, replies); Alice is served within as many turns
    // as there are groups waiting (Bob's two, the crowd's, and hers).
    let hoard = rng.pct(50);
    let groups = 3 + crowd;
    let mut turns = 0;
    let mut alice_served = false;
    for i in 0..(groups * crate::spec::WAIT_CAP + 80) {
        let stid = if i % 2 == 0 { stid } else { stid2 };
        let s = run(&mut k, spid, stid, Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 })?;
        let Outcome::Done(Ok(Ret::Message(m))) = s.outcome else { continue };
        turns += 1;
        if alice_msg.is_some() && m.account == 1001 {
            alice_served = true;
            if turns > groups {
                return Err(fail(format!(
                    "I11: Alice's call was taken at receive {turns}, of {groups} groups"
                )));
            }
        }
        if !hoard {
            run(
                &mut k,
                spid,
                stid,
                Syscall::Reply { msg_id: m.msg_id, words: [0; WORDS], handles: alloc::vec![] },
            )?;
        }
    }
    if alice_msg.is_some() && !alice_served {
        return Err(fail(String::from("I11: Alice's call was never taken")));
    }
    // A process at MAX_OPEN_CALLS takes no calls but still takes sends (R4a; QUESTIONS 81).
    if k.open_calls(spid) >= crate::spec::MAX_OPEN_CALLS {
        let send = Syscall::Send {
            h: e,
            words: [9; WORDS],
            handles: alloc::vec![],
            transfer: None,
            timeout: FOREVER,
        };
        run(&mut k, INIT_PID, 1, send)?;
        let s = run(&mut k, spid, stid, Syscall::Receive { h: Some(1), timeout: 0, max_transfer: 0 })?;
        if !matches!(s.outcome, Outcome::Done(Ok(Ret::Message(ref m))) if m.kind == crate::syscall::MsgKind::Send)
        {
            return Err(fail(format!(
                "R4a: a server at MAX_OPEN_CALLS did not take a send: {:?}",
                s.outcome
            )));
        }
    }
    checker.check(&k).map_err(fail)?;
    Ok(())
}

/// R12: random weighted budgets, spinners and sleepers in one flat queue.
/// For every budget runnable throughout (a spinner), over every
/// interval: it received at least its weight's share of the others' CPU time, less a bound of a
/// few slices. Sleepers do what RESOURCES.md's attack test describes: sleep (sometimes long), then
/// run a burst, so a budget that could bank credit while asleep would take it back in the burst.
///
/// The bound: stride keeps every runnable budget's pass within one slice's stride of the lowest,
/// so an always-runnable budget i lags its share by at most about SLICE x w_i / w_min, plus a
/// slice per budget for the pick order; a waking budget restarts at the current minimum pass,
/// so a sleeper cannot take back what it missed.
pub fn scheduler_fairness(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    fairness_run(seed, mutation).map(|_| ())
}

/// [`scheduler_fairness`], returning the largest lag seen as a fraction of the bound, in
/// thousandths (so tests can show how much room the bound leaves).
pub fn fairness_run(seed: u64, mutation: Option<Mutation>) -> Result<u64, Failure> {
    let mut rng = Rng::new(seed);
    let fail = |message: String| Failure { family: "scheduler_fairness", seed, message, ops: Vec::new() };
    let mut s = Scheduler { mutation, ..Scheduler::default() };
    let n = rng.range(2, 6);
    struct B {
        weight: u64,
        spinner: bool,
        /// Until when a sleeper sleeps (it is runnable when 0).
        asleep_until: u64,
        /// Slices left in a sleeper's current burst.
        burst: u64,
        runtime: u64,
        /// Max so far of (runtime x W - weight x T), for the drawdown bound.
        best: i128,
    }
    let mut bs = Vec::new();
    for id in 1..=n {
        let weight = rng.range(1, 100);
        let spinner = id == 1 || rng.pct(50);
        s.add_budget(id, weight);
        bs.push(B { weight, spinner, asleep_until: 0, burst: rng.range(1, 5), runtime: 0, best: 0 });
    }
    // Every budget has one thread (tid = budget id); all start runnable.
    for id in 1..=n {
        s.wake(id, id);
    }
    let mut now = 0u64;
    let mut user_time = 0u64;
    let mut worst = 0u64;
    let total_user_weight: u64 = bs.iter().map(|b| b.weight).sum();
    let w_min = bs.iter().map(|b| b.weight).min().unwrap_or(1);
    for _ in 0..rng.range(100, 1500) {
        // Wake sleepers whose time has come, each for a burst.
        for (i, b) in bs.iter_mut().enumerate() {
            if !b.spinner && b.asleep_until != 0 && b.asleep_until <= now {
                b.asleep_until = 0;
                b.burst = rng.range(1, 150);
                s.wake(i as u64 + 1, i as u64 + 1);
            }
        }
        let Some((id, _)) = s.pick() else {
            // Idle: jump to the next wake-up.
            now = bs
                .iter()
                .filter(|b| b.asleep_until != 0)
                .map(|b| b.asleep_until)
                .min()
                .unwrap_or(now + SLICE);
            continue;
        };
        let i = (id - 1) as usize;
        // A full slice, or the tail of a burst.
        let run = if bs[i].spinner || bs[i].burst > 1 { SLICE } else { rng.range(1, SLICE) };
        now += run;
        s.charge(id, run);
        bs[i].runtime += run;
        user_time += run;
        if !bs[i].spinner {
            bs[i].burst = bs[i].burst.saturating_sub(1);
            if bs[i].burst == 0 {
                // Sleep, sometimes for a long time.
                let d = if rng.pct(40) { rng.range(20, 300) * SLICE } else { rng.range(1, 3 * SLICE) };
                bs[i].asleep_until = now + d;
                s.block(id, id);
            }
        }
        // The share check, for always-runnable user budgets.
        for (j, b) in bs.iter_mut().enumerate() {
            if !b.spinner {
                continue;
            }
            let d = b.runtime as i128 * total_user_weight as i128 - b.weight as i128 * user_time as i128;
            b.best = b.best.max(d);
            let bound = (SLICE as i128)
                * (2 + 2 * b.weight as i128 / w_min as i128 + n as i128)
                * total_user_weight as i128;
            worst = worst.max(((b.best - d) * 1000 / bound) as u64);
            if b.best - d > bound {
                return Err(fail(format!(
                    "R12: budget {} (weight {} of {}) fell {} µs behind its share over an interval (bound {})",
                    j + 1,
                    b.weight,
                    total_user_weight,
                    (b.best - d) / total_user_weight as i128,
                    bound / total_user_weight as i128
                )));
            }
        }
    }
    Ok(worst)
}
