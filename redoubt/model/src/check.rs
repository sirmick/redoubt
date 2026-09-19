//! The property tests: families of random sequences, each checked step by step.
//!
//! | Family | What it checks |
//! | --- | --- |
//! | [`kernel_sequence`] | I1-I9, I11-I13 and the rule checks after every step of a random sequence |
//! | [`budget_lifecycle`] | I10: create a budget, use it, destroy it; everyone else's counters are unchanged |
//! | [`scheduler_fairness`] | R12: class order, and each budget's share over every interval it was runnable |
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
use crate::invariants;
use crate::kernel::{Boot, Kernel};
use crate::mutation::Mutation;
use crate::sched::Scheduler;
use crate::spec::{Class, SLICE};
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
    let mut k = Kernel::boot(boot, mutation);
    invariants::check(&k)?;
    for op in ops {
        if k.step(op).is_some() {
            invariants::check(&k)?;
        }
    }
    Ok(())
}

/// A random sequence from boot, with every invariant checked after each step.
pub fn kernel_sequence(seed: u64, mutation: Option<Mutation>) -> Result<(), Failure> {
    let mut k = Kernel::boot(&Boot::default(), mutation);
    let mut gen = Gen::new(seed);
    let mut ops = Vec::new();
    let fail = |message: String, ops: &Vec<Op>| Failure {
        family: "kernel_sequence",
        seed,
        message,
        ops: ops.clone(),
    };
    invariants::check(&k).map_err(|m| fail(m, &ops))?;
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
        invariants::check(&k).map_err(|m| fail(m, &ops))?;
    }
    Ok(())
}

/// Handle indices the epilogue probes in each process.
pub const EPILOGUE_HANDLES: u64 = 64;

/// Probes that make the model's state visible in results, for the end of a conformance trace.
/// Comparing results alone misses a kernel whose state has diverged but not yet shown it (a
/// handle that should have been revoked but was never used again), so each process with a
/// runnable thread, in pid order, reads one word of every readable page it has mapped
/// (contents, zeroing, lends) and the usage of every budget it holds a handle to (charging);
/// then `init`, if runnable, destroys every budget it holds other than `root`, `system` and
/// `users` (revocation); then each of those processes that is still alive closes every handle
/// index from 1 to `EPILOGUE_HANDLES` (`Ok` or `BadHandle` shows its table, and so the stamps).
/// Ops of processes that died on the way are not legal events and `trace::record` leaves them
/// out.
///
/// Not visible even so: a budget's account (it travels only in messages), and an IRQ source's
/// mask (it shows only as a later interrupt); traces meant to check those must exercise them.
pub fn epilogue(k: &Kernel) -> Vec<Op> {
    use crate::kernel::{INIT_PID, MapState, Object};
    use crate::spec::{FLAG_R, PAGE_SIZE};
    use crate::syscall::Syscall;
    let mut ops = Vec::new();
    let mut actors = Vec::new();
    for (pid, tid) in k.runnable() {
        if actors.iter().any(|(p, _)| *p == pid) {
            continue;
        }
        actors.push((pid, tid));
        let p = &k.processes[&pid];
        for (v, m) in &p.space {
            if m.flags & FLAG_R != 0 && matches!(m.state, MapState::Own | MapState::LentIn(_)) {
                ops.push(Op::Read { pid, tid, addr: v * PAGE_SIZE });
            }
        }
        for (i, h) in &p.handles {
            if matches!(h.object, Object::Budget(_)) {
                ops.push(Op::Sys { pid, tid, call: Syscall::BudgetUsage { h: *i } });
            }
        }
    }
    if let Some((pid, tid)) = actors.iter().copied().find(|(p, _)| *p == INIT_PID) {
        for (i, h) in &k.processes[&pid].handles {
            if matches!(h.object, Object::Budget(b) if b != k.root && b != k.system && b != k.users) {
                ops.push(Op::Sys { pid, tid, call: Syscall::BudgetDestroy { h: *i } });
            }
        }
    }
    for (pid, tid) in actors {
        for i in 1..=EPILOGUE_HANDLES {
            ops.push(Op::Sys { pid, tid, call: Syscall::HandleClose { h: i } });
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
    let mut k = Kernel::boot(&Boot::default(), mutation);
    let mut gen = Gen::new(seed);
    let mut ops = Vec::new();
    let fail = |message: String, ops: &Vec<Op>| Failure {
        family: "budget_lifecycle",
        seed,
        message,
        ops: ops.clone(),
    };
    let step = |k: &mut Kernel, op: Op, ops: &mut Vec<Op>| -> Result<crate::kernel::Step, Failure> {
        ops.push(op.clone());
        let s = k.step(&op).ok_or_else(|| fail(format!("illegal op {op:?}"), ops))?;
        invariants::check(k).map_err(|m| fail(m, ops))?;
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
    let before = snapshot(&k);
    let pb = &k.budgets[&parent];
    let free = pb.pages_limit - pb.pages_used;
    let create = Syscall::BudgetCreate {
        parent: ph,
        pages: gen.rng.range(4, free / 2 + 3),
        processes: 1,
        weight: gen.rng.range(0, pb.weight - pb.weight_used),
        class: Class::User.raw().min(pb.class.raw()),
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
    let exit = k.processes[&pid]
        .handles
        .iter()
        .find(|(_, h)| matches!(h.object, Object::Endpoint(_)))
        .map(|(i, _)| *i);
    if let Some(e) = exit {
        let s = step(
            &mut k,
            Op::Sys { pid, tid, call: Syscall::ProcessCreate { budget: bh, exit_endpoint: e } },
            &mut ops,
        )?;
        if let crate::syscall::Outcome::Done(Ok(crate::syscall::Ret::Handle(proc_h))) = s.outcome {
            let start = Syscall::ProcessStart { process: proc_h, entry: 0, sp: 0, handles: alloc::vec![bh] };
            step(&mut k, Op::Sys { pid, tid, call: start }, &mut ops)?;
        }
    }
    // Only threads inside the child's subtree act; no ticks, no interrupts.
    for _ in 0..gen.rng.range(0, 40) {
        let inside: Vec<(u64, u64)> = k
            .runnable()
            .into_iter()
            .filter(|(p, _)| k.budget_of(*p).is_some_and(|b| k.is_descendant_or_self(b, child)))
            .collect();
        if inside.is_empty() || !k.budgets.contains_key(&child) {
            break;
        }
        let op = loop {
            let op = gen.next_op(&k);
            match &op {
                Op::Sys { pid, .. }
                | Op::Write { pid, .. }
                | Op::Read { pid, .. }
                | Op::Exec { pid, .. }
                | Op::Fault { pid, .. }
                    if inside.iter().any(|(p, _)| p == pid) =>
                {
                    break op;
                }
                _ => {}
            }
        };
        step(&mut k, op, &mut ops)?;
    }
    if k.threads.get(&tid).is_none_or(|t| t.wait.is_some()) || !k.budgets.contains_key(&child) {
        return Ok(());
    }
    step(&mut k, Op::Sys { pid, tid, call: Syscall::BudgetDestroy { h: bh } }, &mut ops)?;
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

/// R12: random budgets (classes, weights, spinners and sleepers) under the scheduler alone.
/// Checks at every pick that a user budget never runs while a system budget is runnable, and
/// for every user budget that is runnable throughout (a spinner), over every interval: it
/// received at least its weight's share of the user-class CPU time, less a bound of a few
/// slices. Sleepers do what RESOURCES.md's attack test describes: sleep (sometimes long), then
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
        class: Class,
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
        let class = if rng.pct(20) { Class::System } else { Class::User };
        let weight = rng.range(1, 100);
        let spinner = class == Class::User && (id == 1 || rng.pct(50));
        s.add_budget(id, class, weight);
        bs.push(B { class, weight, spinner, asleep_until: 0, burst: rng.range(1, 5), runtime: 0, best: 0 });
    }
    // Every budget has one thread (tid = budget id); all start runnable.
    for id in 1..=n {
        s.wake(id, id);
    }
    let mut now = 0u64;
    let mut user_time = 0u64;
    let mut worst = 0u64;
    let total_user_weight: u64 = bs.iter().filter(|b| b.class == Class::User).map(|b| b.weight).sum();
    let w_min = bs.iter().filter(|b| b.class == Class::User).map(|b| b.weight).min().unwrap_or(1);
    for _ in 0..rng.range(100, 1500) {
        // Wake sleepers whose time has come, each for a burst.
        for (i, b) in bs.iter_mut().enumerate() {
            if !b.spinner && b.asleep_until != 0 && b.asleep_until <= now {
                b.asleep_until = 0;
                b.burst = if b.class == Class::System { rng.range(1, 3) } else { rng.range(1, 150) };
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
        if bs[i].class == Class::User && bs.iter().any(|b| b.class == Class::System && b.asleep_until == 0) {
            return Err(fail(format!("R12: user budget {id} ran while a system budget was runnable")));
        }
        // A full slice, or the tail of a burst.
        let run = if bs[i].spinner || bs[i].burst > 1 { SLICE } else { rng.range(1, SLICE) };
        now += run;
        s.charge(id, run);
        bs[i].runtime += run;
        if bs[i].class == Class::User {
            user_time += run;
        }
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
            if !b.spinner || b.class != Class::User {
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
