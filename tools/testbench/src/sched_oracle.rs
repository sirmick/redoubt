//! An independent check of the kernel's stride ranks (kernel/scheduling.md R12, the four rank
//! clauses of "The current minimum and ties"), over the raw events a `sched-trace` kernel prints
//! at `system_reset`.
//!
//! The kernel's trace says what its queue did, never why: a budget woke (`W`), was requeued (`R`),
//! left the queue (`D`), had its pass changed (`P`), or was picked (`K`); each record carries the
//! kernel entry (reconcile) it belongs to, the budget's pass after the event (its low 64 bits),
//! and the hart that wrote it. A shootdown of a process on other harts (`S`) is recorded too; the
//! checks here pass it over, and [`fence`] reads it.
//! It holds no tie key. This module rebuilds the order from those events alone, with its own reading of the
//! rules, and checks every pick against it:
//! - the lowest pass first; at an equal pass,
//! - a budget that woke ranks ahead of one that was requeued;
//! - of two that woke, the later kernel entry's first, and within one entry the lower id;
//! - requeued ones in the order they were requeued.
//!
//! A destruction's lift (a child's work since entry moving to its parent) is recorded as a group
//! of nine records, every operand and the result, and recomputed here from the rule as the spec
//! states it (kernel/scheduling.md R12, "Inheritance"):
//! W = (child pass - max(entry, floor))+ x child weight + child remainder; the parent becomes
//! max(its pass, floor) + W / its weight, keeping its remainder only if it was not below the
//! floor, the remainders carried.
//!
//! It does not only trust the passes it is given. It keeps its own **floor**, the queue's lowest
//! pass, never lowered (so it holds while the queue is empty) and raised after a reconcile's wakes,
//! as the kernel's is, and requires every wake's pass to be at least that floor; and it requires
//! every budget's pass never to fall.
//!
//! Each destruction (R10) is bracketed by `X` and `Y` records carrying the time in µs in the pass
//! field; the check reports their durations, and a case can bound their p99
//! (`post_check = "sched_oracle r10_p99_us=30000"`) and, adding the worst steward decision-wake
//! p99, a lease's end (`lease_end_p99_us=145000`).
//!
//! **The latency targets count the kernel a release build runs.** A checked build's audits, full
//! scans a release build has none of, are bracketed by `U` and `V` records (which audit, and the
//! time in µs). The program prints each latency sample's window, its end on `time_now` and its
//! length (`LATENCY-SAMPLE N=16 deadline_notice <end> <gross>`), and how many it took
//! (`LATENCY-COUNT`, which the windows must number); this check subtracts the audit
//! time inside each window, judges the net p50 and p99 against the case's bounds
//! (`deadline_notice_p99_us=40000`), and reports the audit time beside each. An audit unpaired, or
//! inside a destruction (R10's own window, which then subtracts nothing), fails the check.
//!
//! **A share of the charged CPU** (`CHARGED-SHARE`) takes no count from a program: the program
//! prints its window and marks the budgets it means (an empty child of a weight of its choosing,
//! carved and destroyed, so the trace's lift names the parent), and this check sums the kernel's
//! charges in the window from each budget's pass, its rises times its weight, for the first marked
//! budget against every marked budget and the budgets lifted into them. The checked build's audits
//! are charged to no budget, so it is net of them by construction.
//!
//! Each process's threads ending, the pumps after them included, is bracketed by `T` and `t`
//! records (the time in µs): the check reports each destruction's time in its threads' ending
//! beside R10's (kernel/budgets.md, "Residual risks"). A span unpaired or nested fails it.
//!
//! A weight change (a carve, or a carve returned) is recorded as a group of six records ahead of
//! the pass it sets, and recomputed from the rule as the spec states it (kernel/scheduling.md
//! R12, "The lead follows the weight"): W = (pass - floor)+ x old weight + remainder; the budget
//! becomes floor + W / new weight, remainder W mod new weight; a weight of 0 on either side is
//! stated as 1, so what a budget owes is carried through 0. That is the one place a pass may fall.
//!
//! **A timer interrupt costs the budgets whose items it expired** (kernel/scheduling.md,
//! "Charging"). Each one from user mode is bracketed by `I` (the budget it interrupted, and the
//! time in µs) and `O` (its return, to user mode or to `kmain`), with every charge inside it (`B`:
//! the payer and the ticks) and the end of its expiry (`E`: the budget billed last, for an expired
//! item or for a wait that ended before its timeout and left the timer early). After an expiry
//! that billed someone, every charge after it goes to the budget billed last; after one that found
//! a wait ended early, every charge goes to that wait's budget; with neither, the only charge
//! allowed is to the interrupted budget, and only if the entry ends its slice (it returns to
//! `kmain`). So the budget it interrupted pays for its own items or its slice's end, never for
//! another budget's timer. The check counts the interrupts that found neither and ended no slice,
//! nobody's, inside each share's window (`HART-SHARE`), and those that found another budget's wait
//! ended early; a case can require some of the latter in a share's window
//! (`stale_waits_in=<share>`), so the
//! check it runs cannot pass for want of the interrupts it is about.
//!
//! **The kernel's time nobody pays for** (kernel/scheduling.md, "Residual risks") closes the
//! trace, one `C` record: the kernel's ticks on every hart in its id, the ticks charged to budgets
//! in its pass, and the checked build's audits' ticks in its entry field. The summary reports the
//! share of the kernel's time net of audits that no budget was charged, `nobody N of 1000`, with
//! the three numbers it divides, and judges nothing by it.
//!
//! **The cluster** (`post_check = "sched_oracle cluster ..."`, kernel/scheduling.md,
//! "Responsiveness") is judged on conservative kernel-clock envelopes, plan `v3-kernel-envelope`.
//! Each stand-in attempt reports B (a kernel reading before arming), its delay, L = B + delay, P (a
//! kernel reading after service) and U = P + 1; the check rebuilds every field, requires
//! R <= L <= P and U <= F, and credits only an audit's certified interior `[u + 1, v)` inside
//! `[L, U)`. The driver's lower witness, its RTC gross less the union of the audits' outer bins
//! `[u, v + 1)` inside the envelope, decides whether an old-control miss is latency
//! (`cluster_old_control`).
//!
//! **A sibling's first run** (`round`, kernel/scheduling.md, "Residual risks"): the program
//! destroys an empty budget and then makes the one it marks, which must run before any other
//! budget is picked twice.
//!
//! **A fresh lift's delay** (`lift-delay`, the same section): a sibling made under a parent just
//! lifted must wait the rounds the lift's own records predict, to within a round, with the lift
//! still fresh at its wake.
//!
//! **Several harts.** Each budget's threads waiting for a hart (`J`, the count in the pass field:
//! those no hart runs) and each hart's runner (`H`, the budget it runs, 0 for none, and its weight)
//! are recorded, and so is a budget lifted as it stops being capped (`u`, the pass it is lifted to,
//! at most the floor, ahead of the `P` that sets it, then `z` with the floor), so no share reads
//! the lift as a charge. A pick
//! takes the first budget in rank order with a thread waiting (a budget with no `J` yet counts as
//! having one); it may pass over a budget ranked ahead only if another hart runs it and none of
//! its threads waits. Each wait for the kernel lock from user mode, another hart holding it, is a
//! `Q` (its start and end in ticks), and an `F` before the `C` holds the ticks since boot and the
//! harts: the summary reports the waits as a share of the harts' time, `lock waits N of 1000`, and
//! judges nothing by it (kernel/scheduling.md, "Residual risks").
//!
//! **Frames in flight** (kernel/memory.md, R81). A hart zeroes the frames it freed after it gives
//! up the kernel lock, and at its next trap from user mode the kernel bills that time to the
//! frames' payers, not to the runner: a `z` (the ticks, the frames) after the entry's wait records.
//! The summary reports the total. An `inflight-trace` kernel also records each frame's way through
//! the in-flight state, which [`inflight`] alone reads.
//!
//! **A share across harts** (`HART-SHARE`) is judged of everything the kernel charged in the
//! program's window, each budget at the weight the trace states (its runner's `H`, or its weight
//! changes), net of every lock wait: a wait is billed to the waiting hart's runner, though no
//! thread of it ran. The part is the marked budget's, net of its own harts' waits; what it is owed
//! is its water-filling want (kernel/scheduling.md, R12) of the wants of it and the budgets the
//! program names against it, by weight and runnable threads, at the trace's harts. At one hart
//! that is its weight's share, as the count gave it.
//!
//! A trace that is malformed, incomplete, lost records or holds no pick is rejected: a check that
//! saw nothing proves nothing.

use std::collections::{BTreeMap, BTreeSet};

/// One trace record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Record {
    pub seq: u64,
    pub entry: u64,
    pub kind: char,
    pub id: u64,
    pub pass: u128,
    /// The boot index of the hart that wrote it (0 in a trace that does not say).
    pub hart: u64,
}

/// Where a queued budget ranks among those of equal pass: `(0, -entry)` for a wake, `(1, n)` for
/// the n-th requeue.
type Key = (u8, i128);

/// Parse the trace out of a console log: its records, or why it is unusable.
pub fn parse(log: &str) -> Result<Vec<Record>, String> {
    let mut records = Vec::new();
    let mut end = None;
    for line in log.lines() {
        let line = line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("SCHED-TRACE-END ") {
            let f: Vec<&str> = rest.split_whitespace().collect();
            let (Some(n), Some(&"dropped"), Some(d)) = (f.first(), f.get(1), f.get(2)) else {
                return Err(format!("malformed end line {line:?}"));
            };
            let n: u64 = n.parse().map_err(|_| format!("malformed end line {line:?}"))?;
            let d: u64 = d.parse().map_err(|_| format!("malformed end line {line:?}"))?;
            end = Some((n, d));
            continue;
        }
        let Some(rest) = line.strip_prefix("SCHED-TRACE ") else { continue };
        if end.is_some() {
            return Err(format!("a record after the end line: {line:?}"));
        }
        let f: Vec<&str> = rest.split_whitespace().collect();
        let bad = || format!("malformed record {line:?}");
        if f.len() != 5 && f.len() != 6 {
            return Err(bad());
        }
        let kind = f[2].chars().next().ok_or_else(bad)?;
        if f[2].len() != 1 || !"WRDPKLlefrqwAaGgvNnXYZUVTtIBEOMmCSJHQFukzxchjy0pidbos".contains(kind) {
            return Err(bad());
        }
        records.push(Record {
            seq: f[0].parse().map_err(|_| bad())?,
            entry: f[1].parse().map_err(|_| bad())?,
            kind,
            id: f[3].parse().map_err(|_| bad())?,
            pass: u128::from_str_radix(f[4], 16).map_err(|_| bad())?,
            hart: f.get(5).map_or(Ok(0), |h| h.parse()).map_err(|_| bad())?,
        });
    }
    let Some((n, dropped)) = end else {
        return Err("no SCHED-TRACE-END line: the trace is incomplete".into());
    };
    if dropped != 0 {
        return Err(format!("the kernel's trace ring dropped {dropped} records"));
    }
    if n != records.len() as u64 {
        return Err(format!("the end line counts {n} records; {} arrived", records.len()));
    }
    for (i, r) in records.iter().enumerate() {
        if r.seq != i as u64 {
            return Err(format!("record {i} is numbered {}", r.seq));
        }
        // The kernel's time (`C`) closes the trace, its entry field the audits' ticks.
        if r.kind == 'C' && i + 1 != records.len() {
            return Err(format!("record {i}: the kernel's time before the trace's end"));
        }
        if i > 0 && r.kind != 'C' && r.entry < records[i - 1].entry {
            return Err(format!("record {i} goes back to kernel entry {}", r.entry));
        }
    }
    Ok(records)
}

/// The kinds of a lift's group, in order.
const LIFT: &str = "LlefrqwAa";

/// Check the lift group starting at `records[0]` against the rule; (records used, whether the
/// child's work was above zero and the parent led the floor, so a lift by `max` would differ).
fn check_lift(records: &[Record]) -> Result<(usize, bool), String> {
    let g = records
        .get(..LIFT.len())
        .ok_or_else(|| format!("record {}: a lift group cut short", records[0].seq))?;
    let kinds: String = g.iter().map(|r| r.kind).collect();
    if kinds != LIFT {
        return Err(format!("record {}: a lift group of kinds {kinds}, not {LIFT}", g[0].seq));
    }
    let [pb, cp, e, f, cr, pr, w, pa, par] = [0, 1, 2, 3, 4, 5, 6, 7, 8].map(|i| g[i].pass);
    let (wc, wp) = (w >> 32, w & 0xffff_ffff);
    if wp == 0 {
        return Ok((LIFT.len(), false));
    }
    let work = cp.saturating_sub(e.max(f)) * wc + cr;
    let (base, base_rem) = if pb >= f { (pb, pr) } else { (f, 0) };
    let rem = base_rem + work % wp;
    let want = (base + work / wp + rem / wp, rem % wp);
    if (pa, par) != want {
        return Err(format!(
            "record {}: budget {}'s lift of budget {} gave pass {pa:#x} rem {par}, but the rule gives {:#x} rem {} \
             (parent {pb:#x} rem {pr}, child {cp:#x} rem {cr} entry {e:#x}, floor {f:#x}, weights {wc} and {wp})",
            g[0].seq, g[0].id, g[1].id, want.0, want.1
        ));
    }
    Ok((LIFT.len(), work / wp > 0 && pb > f))
}

/// The kinds of a weight change's group, in order.
const REWEIGH: &str = "GgvfNn";

/// Check the weight-change group starting at `records[0]` against the rule; (records used, the
/// budget's pass after).
fn check_reweigh(records: &[Record]) -> Result<(usize, u128), String> {
    let g = records
        .get(..REWEIGH.len())
        .ok_or_else(|| format!("record {}: a weight change cut short", records[0].seq))?;
    let kinds: String = g.iter().map(|r| r.kind).collect();
    if kinds != REWEIGH || g.iter().any(|r| r.id != g[0].id) {
        return Err(format!(
            "record {}: a weight change of kinds {kinds}, not {REWEIGH} for one budget",
            g[0].seq
        ));
    }
    let [pb, rb, w, f, pa, ra] = [0, 1, 2, 3, 4, 5].map(|i| g[i].pass);
    let (old, new) = (w >> 32, w & 0xffff_ffff);
    // A weight of 0 is stated as 1, so what a budget owes is carried through 0.
    let want = {
        let (old, new) = (old.max(1), new.max(1));
        let owed = pb.saturating_sub(f) * old + rb;
        (f + owed / new, owed % new)
    };
    if (pa, ra) != want {
        return Err(format!(
            "record {}: budget {}'s weight change {old} to {new} gave pass {pa:#x} rem {ra}, but the rule gives {:#x} rem {} \
             (pass {pb:#x} rem {rb}, floor {f:#x})",
            g[0].seq, g[0].id, want.0, want.1
        ));
    }
    Ok((REWEIGH.len(), pa))
}

/// What a check covered.
#[derive(Debug, Default)]
pub struct Summary {
    pub picks: usize,
    /// Budgets lifted, at most to the floor, as they stopped being capped (`u`).
    pub uncaps: usize,
    pub lifts: usize,
    /// Weight changes recomputed.
    pub reweighs: usize,
    /// Lifts a `max` rule would have got wrong (a leading parent, work to move).
    pub telling: usize,
    /// Each destruction's duration, µs, in trace order.
    pub r10_us: Vec<u64>,
    /// The most object frames present at a destruction's start (R10 walks them all).
    pub r10_frames: u64,
    /// Each checked-build audit's span, µs (`U` to `V`), in trace order.
    pub audits: Vec<(u64, u64)>,
    /// Each destruction's time in its threads' ending, µs (`T` to `t` inside its `X` and `Y`), in
    /// trace order.
    pub r10_threads_us: Vec<u64>,
    /// Timer interrupts from user mode checked, those whose expiry billed someone, those that
    /// found a wait ended before its timeout, and those that found neither and ended the
    /// interrupted budget's slice.
    pub timer_entries: usize,
    pub timer_expiring: usize,
    pub timer_stale: usize,
    pub timer_slice_ends: usize,
    /// Ticks charged after expiry to the budget billed last, over every timer interrupt.
    pub timer_tail_ticks: u64,
    /// Timer interrupts that charged a budget other than the one they interrupted, and those
    /// ticks: another budget's timer work in the interrupted budget's time on its hart, which no
    /// pick can repay a budget with one thread per hart (kernel/timer.md, "R12 (scheduling) for
    /// timer work"). Report-only.
    pub timer_foreign: usize,
    pub timer_foreign_ticks: u64,
    /// When each timer interrupt that found neither and ended no slice came, µs, in trace order.
    pub timer_empty: Vec<u64>,
    /// When each timer interrupt that found another budget's wait ended early came, µs.
    pub timer_stale_foreign: Vec<u64>,
    /// Device interrupts from user mode checked, and those that claimed nothing (another hart
    /// claimed the source first).
    pub external_entries: usize,
    pub external_empty: usize,
    /// Each walk's span, µs (`M` to `m`, a `walk-trace` kernel's): a receive's pump, a timer
    /// interrupt's expiry and a reconcile, in trace order.
    pub walks: [Vec<(u64, u64)>; 3],
    /// Each destruction's pumps: how many, and their time, µs (`M`/`m` of a pump inside its `X`
    /// and `Y`), in trace order.
    pub r10_pumps: Vec<(usize, u64)>,
    /// The floor as each record found it, by record.
    pub floor: Vec<u128>,
    /// Picks that passed over a budget ranked ahead, whose threads all ran on other harts.
    pub passed_over: usize,
    /// Each wait for the kernel lock from user mode, in ticks (`Q`), and the hart that waited.
    pub lock_waits: Vec<(u64, u64, u64)>,
    /// With `lock-trace`, each wait's ticket and the sections ahead of it (`k`), in the order the
    /// waits took the lock.
    pub lock_tickets: Vec<(u64, u64)>,
    /// The run's ticks and its harts (`F`): the harts' time the lock waits are a share of.
    pub hart_time: Option<(u64, u64)>,
    /// With `hold-trace`, each kernel lock section (`h`, then its `j`), in the order they released
    /// the lock.
    pub sections: Vec<Section>,
    /// The ticks of other harts' audits the lock waits waited through (`y`), not billed to anyone.
    pub excused: u64,
    /// The ticks harts spent zeroing frames in flight outside the lock (`z`), billed to the frames'
    /// payers and not to the runner they returned to, and the frames.
    pub zeroed: u64,
    pub zeroed_frames: u64,
    /// The zeroing entries destructions lifted from the budgets they ended to their payers (`p`),
    /// so no bill names a budget that is gone (R10).
    pub zeroing_lifted: u64,
}

/// One kernel lock section (`hold-trace`): held from tick `from` to `to` by `hart`, for `cause`
/// (a system call's number, `0x200` plus an interrupt's code, `0x300` plus an exception's, 0 for
/// `kmain`'s), of which `audits` ticks were the checked build's audits.
#[derive(Clone, Copy, Debug)]
pub struct Section {
    pub from: u64,
    pub to: u64,
    pub hart: u64,
    pub cause: u64,
    pub audits: u64,
}

/// A timer interrupt from user mode, from its `I` to its `O`; or a device interrupt, from its `x`.
struct TimerEntry {
    seq: u64,
    /// A device interrupt's: its claim (`c`), the source or 0, once made.
    external: Option<Option<u64>>,
    /// The budget it interrupted.
    interrupted: Option<u64>,
    /// When it came, µs.
    at: u64,
    /// Its expiry's end (`E`): the budget billed last (0 for none), and what for (1 an expired
    /// item, 2 a wait that ended before its timeout, 0 nothing).
    expired: Option<(u64, u128)>,
    /// Each charge: the payer, the ticks, and whether it came after the expiry's end.
    charges: Vec<(u64, u64, bool)>,
}

/// Judge a timer interrupt at its return (to user mode or not): after an expiry that billed
/// someone, every charge after it is that budget's; after one that found a wait ended early,
/// every charge is that wait's budget's; otherwise, the interrupted budget's, and only if the entry
/// ends its slice (it returns to `kmain`).
fn check_timer_entry(e: &TimerEntry, to_user: bool, sum: &mut Summary) -> Result<(), String> {
    sum.timer_entries += 1;
    let charged = e.charges.iter().filter(|c| c.1 > 0);
    let foreign: u64 = charged.clone().filter(|c| Some(c.0) != e.interrupted).map(|c| c.1).sum();
    if foreign > 0 {
        sum.timer_foreign += 1;
        sum.timer_foreign_ticks += foreign;
    }
    match e.expired {
        Some((last, what @ (1 | 2))) => {
            if what == 1 {
                sum.timer_expiring += 1;
            } else {
                sum.timer_stale += 1;
                if e.interrupted != Some(last) {
                    sum.timer_stale_foreign.push(e.at);
                }
            }
            for &(payer, ticks, after) in charged.filter(|c| what == 2 || c.2) {
                if payer != last {
                    return Err(format!(
                        "record {}: a timer interrupt whose expiry billed budget {last} last (for {}) charged \
                         {ticks} ticks {} to budget {payer} (it interrupted {:?})",
                        e.seq,
                        if what == 1 { "an expired item" } else { "a wait that ended early" },
                        if after { "after it" } else { "before it" },
                        e.interrupted
                    ));
                }
                sum.timer_tail_ticks += ticks;
            }
        }
        _ => {
            let mut ended_slice = false;
            for &(payer, ticks, _) in charged {
                if Some(payer) != e.interrupted || to_user {
                    return Err(format!(
                        "record {}: a timer interrupt that found nothing charged {ticks} ticks to budget {payer} \
                         (it interrupted {:?}, and {})",
                        e.seq,
                        e.interrupted,
                        if to_user { "returned to user mode" } else { "ended its slice" }
                    ));
                }
                ended_slice = true;
            }
            sum.timer_slice_ends += usize::from(ended_slice);
            if to_user {
                sum.timer_empty.push(e.at);
            }
        }
    }
    Ok(())
}

/// Judge a device interrupt from user mode at its return: one that claimed nothing is billed as a
/// timer interrupt that ends no slice, its expiry's last budget's after its expiry or nobody's,
/// never the interrupted budget's for the claim (kernel/scheduling.md, "Charging"). One that
/// claimed a source is counted: its handling is its device's owner's, which the trace does not
/// name.
fn check_external_entry(e: &TimerEntry, claim: Option<u64>, sum: &mut Summary) -> Result<(), String> {
    sum.external_entries += 1;
    match claim {
        None => Err(format!("record {}: a device interrupt returned without its claim", e.seq)),
        Some(irq) if irq != 0 => Ok(()),
        Some(_) => {
            sum.external_empty += 1;
            // As a timer interrupt's: an expired item's own bills come before its expiry's end.
            let last = e.expired.and_then(|(last, what)| (what != 0).then_some((last, what)));
            for &(payer, ticks, after) in e.charges.iter().filter(|c| c.1 > 0) {
                if last.is_some_and(|(_, what)| what == 1 && !after) {
                    continue;
                }
                if last.map(|l| l.0) != Some(payer) {
                    return Err(format!(
                        "record {}: a device interrupt that claimed nothing charged {ticks} ticks to budget {payer} \
                         (it interrupted {:?}, its expiry billed {:?} last)",
                        e.seq,
                        e.interrupted,
                        last.map(|l| l.0)
                    ));
                }
            }
            Ok(())
        }
    }
}

/// One search's length alone, µs, as `sched-lock-contention` prints it before its children run.
fn search_alone(log: &str) -> Option<u64> {
    log.lines().find_map(|l| {
        let (_, rest) = l.split_once("] one search alone: p50 ")?;
        rest.split_once(" µs")?.0.parse().ok()
    })
}

/// Check every pick in `records` against the four clauses, the floor and every pass's
/// monotonicity, and every lift against the rule.
pub fn check(records: &[Record]) -> Result<Summary, String> {
    let mut queued: BTreeMap<u64, (u128, Key)> = BTreeMap::new();
    let mut last_pass: BTreeMap<u64, u128> = BTreeMap::new();
    let mut floor: u128 = 0;
    let mut open_r10: Option<(u64, u128)> = None;
    let mut open_audit: Option<(u64, u128)> = None;
    let mut open_threads: Option<u128> = None;
    let mut open_walk: Option<(u64, u128)> = None;
    let mut open_timer: Option<TimerEntry> = None;
    let mut threads_us = 0;
    let mut pumps = (0, 0);
    let mut requeues: i128 = 0;
    // Each budget's threads waiting for a hart (`J`; a budget with no record yet counts as having
    // one, as every budget has on one hart when it is picked), and each hart's runner (`H`).
    let mut ready: BTreeMap<u64, u128> = BTreeMap::new();
    let mut runs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut sum = Summary::default();
    let mut i = 0;
    while i < records.len() {
        // A group's records find the floor its first did.
        sum.floor.resize(i + 1, floor);
        let r = &records[i];
        i += 1;
        // The kernel's time (`C`) is reported in the summary ([`nobody`]); it judges nothing.
        if r.kind == 'C' {
            continue;
        }
        // A budget's pass never falls (the records that carry one).
        if "WRDPKu".contains(r.kind) {
            if last_pass.get(&r.id).is_some_and(|p| r.pass < *p) {
                return Err(format!(
                    "record {}: budget {}'s pass fell to {:#x} from {:#x}",
                    r.seq, r.id, r.pass, last_pass[&r.id]
                ));
            }
            last_pass.insert(r.id, r.pass);
        }
        if r.kind == 'W' && r.pass < floor {
            return Err(format!(
                "record {}: budget {} woke at pass {:#x}, below the floor {floor:#x}",
                r.seq, r.id, r.pass
            ));
        }
        match r.kind {
            'X' => {
                if let Some((id, _)) = open_r10 {
                    return Err(format!("record {}: a destruction began inside budget {id}'s", r.seq));
                }
                open_r10 = Some((r.id, r.pass));
                threads_us = 0;
                pumps = (0, 0);
            }
            'Z' => sum.r10_frames = sum.r10_frames.max(r.pass as u64),
            // An audit is off R10's walk: none runs inside a destruction, so R10 subtracts none.
            'U' if open_r10.is_some() => {
                return Err(format!("record {}: an audit inside a destruction", r.seq));
            }
            'U' => {
                if let Some((id, _)) = open_audit {
                    return Err(format!("record {}: an audit began inside audit {id}", r.seq));
                }
                open_audit = Some((r.id, r.pass));
            }
            'V' => match open_audit.take() {
                Some((id, t)) if id == r.id && r.pass >= t => sum.audits.push((t as u64, r.pass as u64)),
                _ => return Err(format!("record {}: audit {} ended without beginning", r.seq, r.id)),
            },
            'T' => {
                if open_threads.replace(r.pass).is_some() {
                    return Err(format!("record {}: a threads span began inside another", r.seq));
                }
            }
            't' => match open_threads.take() {
                Some(t) if r.pass >= t => {
                    if open_r10.is_some() {
                        threads_us += (r.pass - t) as u64;
                    }
                }
                _ => return Err(format!("record {}: a threads span ended without beginning", r.seq)),
            },
            'M' => {
                if !(1..=3).contains(&r.id) || open_walk.replace((r.id, r.pass)).is_some() {
                    return Err(format!(
                        "record {}: walk {} began inside another, or is unknown",
                        r.seq, r.id
                    ));
                }
            }
            'm' => match open_walk.take() {
                Some((id, t)) if id == r.id && r.pass >= t => {
                    sum.walks[id as usize - 1].push((t as u64, r.pass as u64));
                    if id == 1 && open_r10.is_some() {
                        pumps = (pumps.0 + 1, pumps.1 + (r.pass - t) as u64);
                    }
                }
                _ => return Err(format!("record {}: walk {} ended without beginning", r.seq, r.id)),
            },
            'Y' => match open_r10.take() {
                Some((id, t)) if id == r.id => {
                    sum.r10_us.push(r.pass.saturating_sub(t) as u64);
                    sum.r10_threads_us.push(threads_us);
                    sum.r10_pumps.push(pumps);
                }
                _ => {
                    return Err(format!(
                        "record {}: budget {}'s destruction ended without beginning",
                        r.seq, r.id
                    ));
                }
            },
            'I' => {
                if open_timer.is_some() {
                    return Err(format!("record {}: a timer interrupt began inside another", r.seq));
                }
                let interrupted = (r.id != 0).then_some(r.id);
                open_timer = Some(TimerEntry {
                    seq: r.seq,
                    external: None,
                    interrupted,
                    at: r.pass as u64,
                    expired: None,
                    charges: Vec::new(),
                });
            }
            'x' => {
                if open_timer.is_some() {
                    return Err(format!("record {}: a device interrupt began inside another", r.seq));
                }
                open_timer = Some(TimerEntry {
                    seq: r.seq,
                    external: Some(None),
                    interrupted: (r.id != 0).then_some(r.id),
                    at: r.pass as u64,
                    expired: None,
                    charges: Vec::new(),
                });
            }
            'c' => match open_timer.as_mut().and_then(|e| e.external.as_mut()) {
                Some(claim @ None) => *claim = Some(r.id),
                _ => {
                    return Err(format!(
                        "record {}: a claim outside a device interrupt, or its second",
                        r.seq
                    ));
                }
            },
            'B' | 'E' | 'O' if open_timer.is_none() => {
                return Err(format!("record {}: a timer interrupt's record outside one", r.seq));
            }
            'B' => {
                let e = open_timer.as_mut().expect("checked above");
                e.charges.push((r.id, r.pass as u64, e.expired.is_some()));
            }
            'E' => {
                let e = open_timer.as_mut().expect("checked above");
                if r.pass > 2 || e.expired.replace((r.id, r.pass)).is_some() {
                    return Err(format!("record {}: a timer interrupt's second or malformed expiry", r.seq));
                }
            }
            'O' if r.pass > 2 => return Err(format!("record {}: a malformed return", r.seq)),
            'O' => {
                let e = open_timer.take().expect("checked above");
                match e.external {
                    Some(claim) => check_external_entry(&e, claim, &mut sum)?,
                    None => check_timer_entry(&e, r.pass == 1, &mut sum)?,
                }
            }
            'L' => {
                let (used, tells) = check_lift(&records[i - 1..])?;
                i += used - 1;
                sum.lifts += 1;
                sum.telling += usize::from(tells);
            }
            'G' => {
                let (used, after) = check_reweigh(&records[i - 1..])?;
                i += used - 1;
                // The one place a pass may fall: the rule just checked set it.
                last_pass.insert(r.id, after);
                sum.reweighs += 1;
            }
            'l' | 'e' | 'f' | 'r' | 'q' | 'w' | 'A' | 'a' | 'g' | 'v' | 'N' | 'n' => {
                return Err(format!("record {}: a group's record outside its group", r.seq));
            }
            'W' => {
                queued.insert(r.id, (r.pass, (0, -i128::from(r.entry))));
            }
            'R' => {
                requeues += 1;
                queued.insert(r.id, (r.pass, (1, requeues)));
            }
            'D' => {
                queued.remove(&r.id);
            }
            'P' => {
                if let Some(q) = queued.get_mut(&r.id) {
                    q.0 = r.pass;
                }
            }
            'J' => {
                ready.insert(r.id, r.pass);
            }
            'u' => {
                if !queued.contains_key(&r.id) {
                    return Err(format!(
                        "record {}: budget {} lifted out of the cap set, but not queued",
                        r.seq, r.id
                    ));
                }
                // Its `z`: the kernel's floor, which a lift out of the cap set never passes.
                let Some(floor) =
                    records.get(i).filter(|z| z.kind == 'z' && z.id == r.id && z.hart == r.hart)
                else {
                    return Err(format!(
                        "record {}: budget {}'s lift out of the cap set states no floor",
                        r.seq, r.id
                    ));
                };
                if r.pass > floor.pass {
                    return Err(format!(
                        "record {}: budget {} lifted out of the cap set to {:#x}, above the floor {:#x}",
                        r.seq, r.id, r.pass, floor.pass
                    ));
                }
                i += 1;
                sum.uncaps += 1;
            }
            'z' => return Err(format!("record {}: a lift's floor after no lift", r.seq)),
            'H' => {
                if r.id == 0 {
                    runs.remove(&r.hart);
                } else {
                    runs.insert(r.hart, r.id);
                }
            }
            'Q' => {
                if r.pass < u128::from(r.id) {
                    return Err(format!("record {}: a lock wait that ends before it starts", r.seq));
                }
                sum.lock_waits.push((r.id, r.pass as u64, r.hart));
            }
            'k' => {
                let wait = i.checked_sub(2).map(|j| &records[j]);
                if !wait.is_some_and(|q| q.kind == 'Q' && q.hart == r.hart) {
                    return Err(format!("record {}: a lock wait's ticket after no wait of its hart", r.seq));
                }
                sum.lock_tickets.push((r.id, r.pass as u64));
            }
            'y' => {
                let back = |k: usize| i.checked_sub(k).map(|j| records[j]);
                let wait = match back(2) {
                    Some(k) if k.kind == 'k' => back(3),
                    other => other,
                };
                let Some(q) = wait.filter(|q| q.kind == 'Q' && q.hart == r.hart) else {
                    return Err(format!("record {}: audits excused after no lock wait of its hart", r.seq));
                };
                if u128::from(r.id) > q.pass - u128::from(q.id) {
                    return Err(format!("record {}: a lock wait excused more than it waited", r.seq));
                }
                sum.excused += r.id;
            }
            'h' => {
                if r.pass < u128::from(r.id) {
                    return Err(format!("record {}: a kernel section that ends before it starts", r.seq));
                }
                sum.sections.push(Section {
                    from: r.id,
                    to: r.pass as u64,
                    hart: r.hart,
                    cause: 0,
                    audits: 0,
                });
            }
            'j' => {
                let held = i.checked_sub(2).map(|j| &records[j]);
                if !held.is_some_and(|h| h.kind == 'h' && h.hart == r.hart) {
                    return Err(format!(
                        "record {}: a kernel section's cause after no section of its hart",
                        r.seq
                    ));
                }
                let section = sum.sections.last_mut().expect("its h was pushed");
                if r.pass > u128::from(section.to - section.from) {
                    return Err(format!("record {}: a kernel section's audits outlast it", r.seq));
                }
                (section.cause, section.audits) = (r.id, r.pass as u64);
            }
            'F' => sum.hart_time = Some((r.id, r.pass as u64)),
            // A shootdown is `fence`'s: no rank or floor follows from it.
            'S' => {}
            '0' => {
                if r.pass == 0 || r.id == 0 {
                    return Err(format!("record {}: zeroing of no frames, or in no time", r.seq));
                }
                sum.zeroed += r.id;
                sum.zeroed_frames += r.pass as u64;
            }
            'p' if open_r10.is_none() => {
                return Err(format!("record {}: zeroing lifted outside a destruction", r.seq));
            }
            'p' => sum.zeroing_lifted += r.pass as u64,
            // A frame's way through R81, and each shootdown, are `inflight`'s.
            'i' | 'd' | 'b' | 'o' | 's' => {}
            'K' => {
                let Some(&(pass, _)) = queued.get(&r.id) else {
                    return Err(format!("record {}: picked budget {}, which is not queued", r.seq, r.id));
                };
                if pass != r.pass {
                    return Err(format!(
                        "record {}: picked budget {} at pass {:#x}, but its last recorded pass is {pass:#x}",
                        r.seq, r.id, r.pass
                    ));
                }
                // Across harts a pick passes over a budget with no thread waiting for a hart: one
                // whose threads all run on harts, which must be other harts than the picker.
                let waiting = |id: &u64| ready.get(id).is_none_or(|n| *n > 0);
                if !waiting(&r.id) {
                    return Err(format!(
                        "record {}: picked budget {}, which has no thread waiting",
                        r.seq, r.id
                    ));
                }
                let mut ahead: Vec<_> = queued.iter().map(|(id, (pass, key))| (*pass, *key, *id)).collect();
                ahead.sort_unstable();
                let want = ahead.iter().find(|x| waiting(&x.2)).map(|x| x.2);
                for &(_, _, id) in ahead.iter().take_while(|x| Some(x.2) != want) {
                    if !runs.iter().any(|(h, b)| *b == id && *h != r.hart) {
                        return Err(format!(
                            "record {}: hart {} passed over budget {}, which no other hart runs",
                            r.seq, r.hart, id
                        ));
                    }
                    sum.passed_over += 1;
                }
                if want != Some(r.id) {
                    let rank = |id: u64| queued.get(&id).map(|(p, k)| (*p, *k));
                    return Err(format!(
                        "record {}: picked budget {} {:?}, but the rank clauses put budget {} {:?} first",
                        r.seq,
                        r.id,
                        rank(r.id),
                        want.unwrap_or(0),
                        want.and_then(rank)
                    ));
                }
                sum.picks += 1;
            }
            _ => unreachable!("parse admits only these kinds"),
        }
        // The floor: the lowest queued pass, never lowered. The kernel raises it after a reconcile's
        // wakes, not between them (they all wake against the floor as it was), so a wake does not
        // raise it here either; the record after the wakes does.
        // A `P` for a budget not queued is a waker's pass, raised to the floor as it wakes: it
        // moves no floor either.
        let waking = r.kind == 'W' || (r.kind == 'P' && !queued.contains_key(&r.id));
        if !waking {
            if let Some(min) = queued.values().map(|(p, _)| *p).min() {
                floor = floor.max(min);
            }
        }
    }
    sum.floor.resize(records.len(), floor);
    if let Some((id, _)) = open_audit {
        return Err(format!("audit {id} began and never ended"));
    }
    if open_threads.is_some() {
        return Err("a threads span began and never ended".into());
    }
    if let Some(e) = open_timer {
        return Err(format!("record {}: a timer interrupt began and never returned", e.seq));
    }
    if sum.picks == 0 {
        return Err("the trace holds no pick: nothing was checked".into());
    }
    Ok(sum)
}

/// The `q`-th percentile of `v` (sorted in place): the smallest value at least `q`% of them do
/// not exceed.
fn percentile(v: &mut [u64], q: usize) -> u64 {
    v.sort_unstable();
    v.get((v.len() * q).div_ceil(100).saturating_sub(1)).copied().unwrap_or(0)
}

/// Prove the timeout wake happened while the spinner's slice was still running. Timer `I` and
/// `O` bracket the wake by record sequence, even when reconcile advances the entry number after
/// `I`. A wake from a device IRQ has no timer `I`, so it cannot satisfy this check. Each hart's
/// records are read apart, so on several harts the spinner is the one the waking hart ran: it
/// returns to user mode and runs to its own slice's end, while the other harts pick as they will.
/// The proofs are counted by sleeper, whichever spinner each interrupted.
fn check_wake_no_preempt(records: &[Record], expected: usize) -> Result<String, String> {
    // Each hart's timer intervals, `I` to `O`, by record index.
    let mut intervals = BTreeMap::<u64, Vec<(usize, usize)>>::new();
    let mut begin = BTreeMap::new();
    for (i, r) in records.iter().enumerate() {
        match r.kind {
            'I' | 'x' => {
                if begin.insert(r.hart, i).is_some() {
                    return Err(format!("record {}: nested timer interval", r.seq));
                }
            }
            'O' => {
                let b = begin
                    .remove(&r.hart)
                    .ok_or_else(|| format!("record {}: unmatched timer return", r.seq))?;
                // A device interrupt's interval proves nothing about the timer's.
                if records[b].kind == 'I' {
                    intervals.entry(r.hart).or_default().push((b, i));
                }
            }
            _ => {}
        }
    }
    if !begin.is_empty() {
        return Err("unended timer interval".into());
    }
    let mut proof = BTreeMap::<u64, BTreeMap<u64, usize>>::new();
    for (&hart, list) in &intervals {
        // The records this hart wrote, from `from` to `to`.
        let own = |from: usize, to: usize| records[from..to].iter().filter(move |r| r.hart == hart);
        for (n, &(b, e)) in list.iter().enumerate() {
            let interval = || own(b, e + 1);
            let spinner = records[b].id;
            if spinner == 0
                || records[e].pass != 1
                || !own(0, b).filter(|r| r.kind == 'K').last().is_some_and(|k| k.id == spinner)
                || interval().any(|r| "KDXY".contains(r.kind))
            {
                continue;
            }
            let wakes: Vec<_> = interval().filter(|r| r.kind == 'W').collect();
            if wakes.len() != 1 || wakes[0].id == spinner {
                continue;
            }
            let sleeper = wakes[0].id;
            if !interval().any(|r| r.kind == 'E' && r.id == sleeper && r.pass == 1) {
                continue;
            }
            let Some(&(next_b, next_e)) = list.get(n + 1) else { continue };
            if records[next_b].id != spinner || records[next_e].pass != 0 {
                continue;
            }
            if !own(next_b, next_e + 1).any(|r| r.kind == 'R' && r.id == spinner)
                || own(next_b, next_e + 1).any(|r| "WDXY".contains(r.kind))
                || !own(e + 1, next_b).all(|r| r.kind != 'K' && r.kind != 'X')
                || !own(next_e + 1, records.len())
                    .find(|r| r.kind == 'K' || r.kind == 'I')
                    .is_some_and(|r| r.kind == 'K')
            {
                continue;
            }
            *proof.entry(sleeper).or_default().entry(spinner).or_default() += 1;
        }
    }
    let matches: Vec<_> = proof.iter().filter(|(_, by)| by.values().sum::<usize>() == expected).collect();
    if matches.len() != 1 {
        return Err(format!(
            "wake-no-preempt: wanted one sleeper with {expected} I...W...O=1, later I...R...O=0 proofs on one hart; found {proof:?}"
        ));
    }
    let (sleeper, by) = matches[0];
    Ok(format!(
        "wake-no-preempt: {expected} timeout wakes of budget {sleeper} each continued the spinner its hart ran to that slice's end (by spinner {by:?})"
    ))
}

/// **A wake never preempts, on several harts** (kernel/scheduling.md, "Preemption points"). A
/// timeout is answered at whichever kernel entry first comes after it is due, on any hart: a timer
/// interrupt, another budget's call, or `kmain`'s own. The sleeper is the one budget that woke at
/// least `expected` times; each of its wakes is judged on the hart that recorded it:
/// - inside a timer or device interrupt (`I` or `x` to `O`): returning to user mode (`O` 1), the runner it
///   interrupted continued: a witness; returning to `kmain` with the runner's slice over at the entry (`O`
///   0), the slice ended there: not judged; returning to `kmain` before it (`O` 2), the entry took the hart,
///   and the check fails unless it destroyed a budget (`X`), as a deadline does;
/// - at another entry while the hart runs a budget (a call): that budget is not requeued (`R`) before the
///   hart's next interrupt, pick or switch: a witness;
/// - in `kmain`, after the hart's runner left: if it left still runnable (`R`) outside an interrupt, with no
///   destruction (`X`), that entry took its hart, and the check fails; if it blocked, ended, or its slice
///   ended, nothing ran to preempt.
///
/// At least `witnesses` of the wakes must be witnesses, so the check cannot pass for want of
/// wakes that could have preempted.
fn check_wake_no_preempt_harts(
    records: &[Record],
    expected: usize,
    witnesses: usize,
) -> Result<String, String> {
    let mut wakes = BTreeMap::<u64, Vec<usize>>::new();
    for (i, r) in records.iter().enumerate().filter(|(_, r)| r.kind == 'W') {
        wakes.entry(r.id).or_default().push(i);
    }
    let sleepers: Vec<_> = wakes.iter().filter(|(_, w)| w.len() >= expected).collect();
    let [(&sleeper, at)] = sleepers[..] else {
        let counts: BTreeMap<u64, usize> = wakes.iter().map(|(b, w)| (*b, w.len())).collect();
        return Err(format!(
            "wake-no-preempt: wanted one budget with at least {expected} wakes; wakes by budget {counts:?}"
        ));
    };
    let took = |w: &Record, r: &Record, runner: u64| {
        Err(format!(
            "wake-no-preempt: record {}: budget {sleeper}'s wake at record {} took hart {} from budget {runner}",
            r.seq, w.seq, w.hart
        ))
    };
    let (mut mid_slice, mut at_call, mut at_end, mut in_kmain) = (0, 0, 0, 0);
    for &i in at {
        let w = &records[i];
        let own = |r: &&Record| r.hart == w.hart;
        // The interrupt the wake is in, if any: its opening record and its return.
        let opened = records[..i]
            .iter()
            .enumerate()
            .rev()
            .filter(|(_, r)| own(r))
            .find(|(_, r)| "IxO".contains(r.kind));
        if let Some((b, open)) = opened.filter(|(_, r)| r.kind != 'O') {
            let Some((e, ret)) =
                records.iter().enumerate().skip(i).filter(|(_, r)| own(r)).find(|(_, r)| r.kind == 'O')
            else {
                return Err(format!("record {}: a wake inside an interrupt that never returns", w.seq));
            };
            match ret.pass {
                1 => mid_slice += 1,
                0 => at_end += 1,
                _ if records[b..e].iter().filter(own).any(|r| r.kind == 'X') => at_end += 1,
                _ => return took(w, ret, open.id),
            }
            continue;
        }
        let Some((h, runs)) =
            records[..i].iter().enumerate().rev().filter(|(_, r)| own(r)).find(|(_, r)| r.kind == 'H')
        else {
            in_kmain += 1;
            continue;
        };
        if runs.id != 0 {
            let after = records[i + 1..]
                .iter()
                .filter(own)
                .find(|r| "IxKHR".contains(r.kind) && (r.kind != 'R' || r.id == runs.id));
            if let Some(r) = after.filter(|r| r.kind == 'R') {
                return took(w, r, runs.id);
            }
            at_call += 1;
            continue;
        }
        // In `kmain`: how the hart's last runner left it, since the hart last picked it.
        let since = records[..h].iter().rev().filter(own).take_while(|r| r.kind != 'H').collect::<Vec<_>>();
        if let Some(k) = since.iter().position(|r| "RD".contains(r.kind)).filter(|k| since[*k].kind == 'R') {
            // Requeued: inside an interrupt (its slice's end), or after a destruction in its
            // section, it is excused.
            let section = || since[k + 1..].iter().take_while(|r| r.kind != 'O');
            let in_interrupt = section().any(|r| "Ix".contains(r.kind));
            if !in_interrupt && !since[..k].iter().chain(section()).any(|r| r.kind == 'X') {
                return took(w, since[k], since[k].id);
            }
        }
        in_kmain += 1;
    }
    if mid_slice + at_call < witnesses {
        return Err(format!(
            "wake-no-preempt: budget {sleeper}'s {} wakes: {mid_slice} mid-slice, {at_call} at another entry, {at_end} at a slice's end, {in_kmain} in `kmain`; fewer than {witnesses} could have preempted",
            at.len()
        ));
    }
    Ok(format!(
        "wake-no-preempt: budget {sleeper}'s {} wakes took no hart: {mid_slice} mid-slice, {at_call} at another entry, each runner continuing; {at_end} at a slice's end, {in_kmain} in `kmain`",
        at.len()
    ))
}

/// The fixture fixes both the start order and the sample plan. This parser rejects missing or
/// repeated setup acknowledgements and altered plans before any W is classified.
fn cluster_plan(log: &str) -> Result<(u64, u64, u64, u64), String> {
    let mut ready = Vec::new();
    let mut plan = 0;
    let mut window = None;
    let mut tpu = None;
    for line in log.lines().map(|x| x.trim_end_matches('\r')) {
        if let Some(n) = line.strip_prefix("CLUSTER-READY ") {
            ready.push(n.parse::<usize>().map_err(|_| format!("bad {line:?}"))?);
        } else if line == "CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000" {
            plan += 1;
        } else if line.starts_with("CLUSTER-PLAN ") {
            return Err(format!("unknown cluster plan version: {line:?}"));
        } else if let Some(rest) = line.strip_prefix("CLUSTER-WINDOW ") {
            let fields: Vec<_> = rest.split_whitespace().collect();
            if fields.len() != 3 || window.is_some() {
                return Err(format!("bad {line:?}"));
            }
            let parse = |s: &str| s.parse::<u64>().map_err(|_| format!("bad {line:?}"));
            window = Some((parse(fields[0])?, parse(fields[1])?, parse(fields[2])?));
        } else if let Some(rest) = line.strip_prefix("[cluster] calibrated: ") {
            let Some(ticks) = rest.split_whitespace().next() else { return Err(format!("bad {line:?}")) };
            if tpu.replace(ticks.parse::<u64>().map_err(|_| format!("bad {line:?}"))?).is_some() {
                return Err("two cluster calibrations".into());
            }
        }
    }
    if ready != (0..19).collect::<Vec<_>>() || plan != 1 {
        return Err(format!(
            "cluster setup: ready {ready:?}, plans {plan}; expected 0..18 and one fixed plan"
        ));
    }
    let (start, release, end) = window.ok_or("cluster window missing")?;
    let tpu = tpu.ok_or("cluster timebase missing")?;
    if tpu == 0 || start.checked_add(50_000) != Some(release) || release.checked_add(16_000_000) != Some(end)
    {
        return Err(format!("cluster window {start}/{release}/{end} or timebase {tpu} invalid"));
    }
    Ok((start, release, end, tpu))
}

#[derive(Clone, Copy)]
struct ClusterSample {
    intent_positive: bool,
    prep_us: i64,
    before: u64,
    lower: u64,
    early: u64,
    observed: u64,
    upper: u64,
    arm: u64,
    deadline: u64,
    service: u64,
    delay_us: u64,
    rtc_gross: u64,
}

/// Metadata is sent in original attempt order after measurement, after each stand-in's header
/// echoing the H and F it received. B, L, E, P and U are `time_now` microseconds; the driver's
/// a, d and s are goldfish RTC nanoseconds (`rtc_ns`), and the timer has none (`no_rtc`, all 0).
fn cluster_metadata(
    log: &str,
    start: u64,
    release: u64,
    end: u64,
) -> Result<[Vec<ClusterSample>; 2], String> {
    let mut out = [Vec::new(), Vec::new()];
    let mut headers = [0usize; 2];
    for line in log.lines().map(|x| x.trim_end_matches('\r')) {
        if let Some(rest) = line.strip_prefix("CLUSTER-HEADER ") {
            let f: Vec<_> = rest.split_whitespace().collect();
            let standin = match f.first().copied() {
                Some("driver_wake") => 0,
                Some("timer_wake") => 1,
                _ => return Err(format!("bad cluster header {line:?}")),
            };
            if f.len() != 3 || f[1].parse::<u64>() != Ok(start) || f[2].parse::<u64>() != Ok(end) {
                return Err(format!("bad cluster header {line:?}"));
            }
            headers[standin] += 1;
            continue;
        }
        let Some(rest) = line.strip_prefix("CLUSTER-SAMPLE ") else { continue };
        let f: Vec<_> = rest.split_whitespace().collect();
        let bad = || format!("malformed cluster metadata {line:?}");
        if f.len() != 16 {
            return Err(bad());
        }
        let standin = match (f[0], f[15]) {
            ("driver_wake", "rtc_ns") => 0,
            ("timer_wake", "no_rtc") => 1,
            _ => return Err(bad()),
        };
        if headers[standin] != 1 {
            return Err(format!("cluster metadata before its stand-in's window header: {line:?}"));
        }
        let num = |i: usize| f[i].parse::<u64>().map_err(|_| bad());
        let index = usize::try_from(num(1)?).map_err(|_| bad())?;
        let intent = num(2)?;
        let slot = num(3)?;
        let prep_us = f[4].parse::<i64>().map_err(|_| bad())?;
        let (before, delay_us, lower, early, observed, upper) =
            (num(5)?, num(6)?, num(7)?, num(8)?, num(9)?, num(10)?);
        let (arm, deadline, service, rtc_gross) = (num(11)?, num(12)?, num(13)?, num(14)?);
        let expected_phase = [100, 300, 600, 850][index % 4];
        let at = release.checked_add((index as u64).checked_mul(80_000).ok_or_else(bad)?).ok_or_else(bad)?;
        let target = at.checked_add(expected_phase).ok_or_else(bad)?;
        let signed_prep =
            i64::try_from(before).ok().and_then(|b| i64::try_from(at).ok().and_then(|a| b.checked_sub(a)));
        // L = B + delta is fixed before arming; U = P + 1 covers P's whole floored microsecond.
        let (Some(want_lower), Some(want_upper)) = (before.checked_add(delay_us), observed.checked_add(1))
        else {
            return Err(format!("cluster metadata arithmetic overflow at {line:?}"));
        };
        // The driver's RTC deadline d = a + 1000 delta; the timer has no E and no RTC fields.
        let driver_ok = || -> Option<bool> {
            let deadline_from_arm = arm.checked_add(delay_us.checked_mul(1_000)?)?;
            Some(
                before <= early
                    && early <= observed
                    && deadline == deadline_from_arm
                    && service >= deadline
                    && rtc_gross == (service - deadline) / 1_000,
            )
        };
        let raw_ok = if standin == 0 {
            driver_ok().ok_or_else(|| format!("cluster metadata arithmetic overflow at {line:?}"))?
        } else {
            [early, arm, deadline, service, rtc_gross] == [0; 5]
        };
        if index != out[standin].len()
            || index >= 200
            || intent > 1
            || (intent == 1) != (index / 4 % 2 == 1)
            || slot != index as u64 * 80_000
            || signed_prep != Some(prep_us)
            || (intent == 1 && before < at)
            || delay_us == 0
            || (intent == 1 && delay_us != expected_phase)
            || (intent == 0 && target.checked_sub(before) != Some(delay_us))
            || lower != want_lower
            || upper != want_upper
            || lower < release
            || lower > observed
            || upper > end
            || !raw_ok
        {
            return Err(format!("cluster metadata order/construction failure at {line:?}"));
        }
        out[standin].push(ClusterSample {
            intent_positive: intent == 1,
            prep_us,
            before,
            lower,
            early,
            observed,
            upper,
            arm,
            deadline,
            service,
            delay_us,
            rtc_gross,
        });
    }
    if headers != [1, 1] || out.iter().any(|samples| samples.len() != 200) {
        return Err(format!("cluster metadata counts {:?}, expected 200 per stand-in", out.map(|v| v.len())));
    }
    Ok(out)
}

/// Full v3 envelope, independently checked against the trusted latency record.
fn cluster_window_bounds(
    sample_end: u64,
    gross: u64,
    meta: ClusterSample,
) -> Result<(u64, u64), &'static str> {
    if sample_end != meta.upper || sample_end.checked_sub(gross) != Some(meta.lower) {
        return Err("cluster envelope and trusted window disagree");
    }
    Ok((meta.lower, meta.upper))
}

/// A report boundary must be identified independently of the wake count. Inside it, each one-
/// thread stand-in has one D, then one W and a service K for every reported attempt. A report
/// wake after the boundary therefore cannot replace a missing or immediate measurement wake.
fn cluster_waits_before(
    records: &[Record],
    id: u64,
    after_go: usize,
    before_report: usize,
) -> Result<Vec<usize>, String> {
    let events: Vec<_> = records[after_go + 1..before_report]
        .iter()
        .enumerate()
        .filter_map(|(offset, r)| {
            (r.id == id && (r.kind == 'D' || r.kind == 'W')).then_some((after_go + 1 + offset, r.kind))
        })
        .collect();
    if events.len() != 400 {
        return Err(format!(
            "cluster budget {id}: {} D/W events before report boundary, expected 400",
            events.len()
        ));
    }
    let mut wakes = Vec::with_capacity(200);
    for (index, pair) in events.chunks_exact(2).enumerate() {
        if pair[0].1 != 'D' || pair[1].1 != 'W' {
            return Err(format!("cluster budget {id} sample {index}: no single D...W blocked wait"));
        }
        let after_w = events.get(index * 2 + 2).map_or(before_report, |next| next.0);
        if !records[pair[1].0 + 1..after_w].iter().any(|r| r.kind == 'K' && r.id == id) {
            return Err(format!("cluster budget {id} sample {index}: no service K before next wait"));
        }
        wakes.push(pair[1].0);
    }
    Ok(wakes)
}

#[derive(Clone, Copy)]
struct ClusterFence {
    x: usize,
    y: usize,
    marker_id: u64,
}

/// Only the stand-ins destroy a budget after go. An empty marker has no W, so its handle and
/// budget ID cannot be equated: the most recent K at X identifies the running one-thread caller.
/// Require exactly one distinct, paired X/Y per caller and no deschedule after that K.
fn cluster_fences(
    records: &[Record],
    standins: [u64; 2],
    go_w: [usize; 2],
) -> Result<[ClusterFence; 2], String> {
    let after_go = go_w[0].max(go_w[1]);
    let mut last_k = records[..=after_go].iter().rposition(|r| r.kind == 'K');
    let mut fences = [None, None];
    let mut n = 0;
    for (i, r) in records.iter().enumerate().skip(after_go + 1) {
        if r.kind == 'K' {
            last_k = Some(i);
        }
        if r.kind != 'X' {
            continue;
        }
        n += 1;
        let k = last_k.ok_or_else(|| format!("cluster marker X {} has no caller K", r.seq))?;
        let caller = records[k].id;
        let standin = standins.iter().position(|id| *id == caller).ok_or_else(|| {
            format!("cluster marker X {} was called by budget {caller}, not a stand-in", r.seq)
        })?;
        if fences[standin].is_some()
            || records[k + 1..i]
                .iter()
                .any(|event| event.id == caller && (event.kind == 'R' || event.kind == 'D'))
        {
            return Err(format!("cluster marker X {} has duplicate or descheduled caller {caller}", r.seq));
        }
        let y = records[i + 1..]
            .iter()
            .position(|event| event.kind == 'Y')
            .map(|at| i + 1 + at)
            .ok_or_else(|| format!("cluster marker X {} has no Y", r.seq))?;
        if records[y].id != r.id
            || records[y].pass < r.pass
            || records[i + 1..y].iter().any(|event| event.kind == 'X')
        {
            return Err(format!("cluster marker X {} has no unique matching Y", r.seq));
        }
        fences[standin] = Some(ClusterFence { x: i, y, marker_id: r.id });
    }
    let [Some(driver), Some(timer)] = fences else {
        return Err(format!("cluster marker fences: found {n} X, expected one per stand-in"));
    };
    if n != 2 || driver.marker_id == timer.marker_id {
        return Err(format!("cluster marker fences: found {n} X or repeated marker ID"));
    }
    Ok([driver, timer])
}

/// Reconstruct the serial readiness and launcher-driven go protocol for all 19 children. A
/// FOREVER readiness send may complete immediately or block once; the window receive must block
/// once in either case. The server's first W after the final isolated spawn anchors go because
/// the launcher consumes every readiness report before sending any window, starting with server.
fn cluster_go_boundaries(records: &[Record], mapped: &[(usize, u64)]) -> Result<[usize; 19], String> {
    if mapped.len() != 19 {
        return Err(format!("cluster setup mapped {} children, expected 19", mapped.len()));
    }
    let final_spawn = mapped[18].0;
    let server = mapped[0].1;
    let anchor = records[final_spawn + 1..]
        .iter()
        .position(|r| r.kind == 'W' && r.id == server)
        .map(|offset| final_spawn + 1 + offset)
        .ok_or("cluster server lacked a go-phase W after final spawn")?;
    let launcher = records[..anchor]
        .iter()
        .rposition(|r| r.kind == 'K')
        .map(|at| records[at].id)
        .ok_or("cluster go anchor lacked a launcher pick")?;
    if mapped.iter().any(|(_, id)| *id == launcher) {
        return Err(format!("cluster go anchor caller {launcher} is a child"));
    }
    let mut go = [0usize; 19];
    for (child, &(spawn, id)) in mapped.iter().enumerate() {
        let readiness_limit = if child == 18 { anchor } else { mapped[child + 1].0 };
        let first_pick = records[spawn + 1..readiness_limit]
            .iter()
            .position(|r| r.kind == 'K' && r.id == id)
            .map(|offset| spawn + 1 + offset)
            .ok_or_else(|| format!("cluster child {child} budget {id} lacked isolated spawn pick"))?;
        let actual_go = records[anchor..]
            .iter()
            .position(|r| r.kind == 'W' && r.id == id)
            .map(|offset| anchor + offset)
            .ok_or_else(|| format!("cluster child {child} budget {id} lacked go W"))?;
        if (child == 0 && actual_go != anchor) || (child > 0 && actual_go <= go[child - 1]) {
            return Err(format!("cluster child {child} budget {id} go W was out of launcher order"));
        }
        let events: Vec<_> = records[spawn + 1..actual_go]
            .iter()
            .enumerate()
            .filter_map(|(offset, r)| {
                (r.id == id && (r.kind == 'D' || r.kind == 'W')).then_some((spawn + 1 + offset, r.kind))
            })
            .collect();
        let go_d = match events.as_slice() {
            [(d, 'D')] => *d, // readiness send completed immediately
            [(ready_d, 'D'), (ready_w, 'W'), (window_d, 'D')] => {
                if *ready_w >= readiness_limit
                    || *ready_d <= first_pick
                    || !records[ready_w + 1..*window_d].iter().any(|r| r.kind == 'K' && r.id == id)
                    || records[..*ready_w]
                        .iter()
                        .rposition(|r| r.kind == 'K')
                        .is_none_or(|at| records[at].id != launcher)
                {
                    return Err(format!(
                        "cluster child {child} budget {id} readiness pair was late or unserved"
                    ));
                }
                *window_d
            }
            _ => {
                return Err(format!(
                    "cluster child {child} budget {id} has ambiguous readiness/window D-W prefix {events:?}"
                ));
            }
        };
        if go_d <= first_pick
            || records[..actual_go]
                .iter()
                .rposition(|r| r.kind == 'K')
                .is_none_or(|at| records[at].id != launcher)
        {
            return Err(format!("cluster child {child} budget {id} go lacked blocked window or launcher"));
        }
        go[child] = actual_go;
    }
    Ok(go)
}

/// After its validated go, each spinner blocks once toward the common release. This next D/W,
/// rather than a fixed global W ordinal, names the release regardless of readiness-send cost.
fn cluster_spinner_releases(
    records: &[Record],
    mapped: &[(usize, u64)],
    go: &[usize; 19],
) -> Result<BTreeMap<u64, usize>, String> {
    let mut release = BTreeMap::new();
    for child in 1..=16 {
        let id = mapped[child].1;
        let mut events = records[go[child] + 1..].iter().enumerate().filter_map(|(offset, r)| {
            (r.id == id && (r.kind == 'D' || r.kind == 'W')).then_some((go[child] + 1 + offset, r.kind))
        });
        let Some((d, 'D')) = events.next() else {
            return Err(format!("cluster spinner {id} lacked a blocked release D"));
        };
        let Some((w, 'W')) = events.next() else {
            return Err(format!(
                "cluster spinner {id} lacked a matching release W after D {}",
                records[d].seq
            ));
        };
        if !records[go[child] + 1..d].iter().any(|r| r.kind == 'K' && r.id == id) {
            return Err(format!("cluster spinner {id} did not run after go before release D"));
        }
        release.insert(id, w);
    }
    Ok(release)
}

/// Check the actual common release and classify all stand-in wakes using independently replayed
/// ranks. The last 19 *first* budget wakes must belong to the 19 serially started, acknowledged
/// one-thread children; no numeric budget-id assumption enters the mapping. A child must first
/// be picked before the next child's first W, matching the readiness handshake.
struct ClusterProof {
    report: String,
    metrics: [Vec<ClusterMetric>; 2],
    /// Each stand-in's positive-lead wakes: the spinners ranked before each.
    positive_ahead: [Vec<usize>; 2],
}

#[derive(Clone, Copy)]
struct ClusterMetric {
    gross: u64,
    credit: u64,
    net: u64,
    lower_witness: u64,
}

/// Certified audit interiors credit the whole reported envelope; outer bins bound physical
/// audit overlap in the opposite direction for the driver's negative-control witness.
fn cluster_metric(
    audits: &[(u64, u64)],
    lower: u64,
    upper: u64,
    rtc_gross: u64,
) -> Result<ClusterMetric, String> {
    let gross = upper.checked_sub(lower).ok_or("reversed cluster envelope")?;
    let mut credit = 0u64;
    let mut outer = 0u64;
    let mut outer_end = lower;
    for &(u, v) in audits {
        if v < u {
            return Err("reversed cluster audit".into());
        }
        let interior_start = u.checked_add(1).ok_or("cluster audit interior overflow")?;
        let part = v.min(upper).saturating_sub(interior_start.max(lower));
        credit = credit.checked_add(part).ok_or("cluster audit credit overflow")?;
        let outer_stop = v.checked_add(1).ok_or("cluster audit outer overflow")?.min(upper);
        let outer_start = u.max(lower).max(outer_end);
        if outer_stop > outer_start {
            outer =
                outer.checked_add(outer_stop - outer_start).ok_or("cluster audit outer credit overflow")?;
        }
        outer_end = outer_end.max(outer_stop);
    }
    let net = gross.checked_sub(credit).ok_or("cluster audit credit exceeds envelope")?;
    Ok(ClusterMetric { gross, credit, net, lower_witness: rtc_gross.saturating_sub(outer) })
}

fn check_cluster(
    log: &str,
    records: &[Record],
    samples: &BTreeMap<(usize, &str), Vec<(u64, u64)>>,
    audits: &[(u64, u64)],
) -> Result<ClusterProof, String> {
    let (start, release, end, tpu) = cluster_plan(log)?;
    let metadata = cluster_metadata(log, start, release, end)?;
    let mut first = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, r) in records.iter().enumerate() {
        if r.kind == 'W' && seen.insert(r.id) {
            first.push((i, r.id));
        }
    }
    let map_start = first.len().checked_sub(19).ok_or("fewer than 19 first budget wakes")?;
    let mapped = &first[map_start..];
    let go = cluster_go_boundaries(records, mapped)?;
    let go_w = [go[17], go[18]];
    let server = mapped[0].1;
    let spinners: BTreeSet<u64> = mapped[1..=16].iter().map(|(_, id)| *id).collect();
    let standins = [mapped[17].1, mapped[18].1];
    if spinners.len() != 16 || standins[0] == standins[1] {
        return Err("cluster setup budget mapping is ambiguous".into());
    }
    let spinner_release = cluster_spinner_releases(records, mapped, &go)?;
    let fences = cluster_fences(records, standins, go_w)?;
    let measurement_wakes = [
        cluster_waits_before(records, standins[0], go_w[0], fences[0].x)?,
        cluster_waits_before(records, standins[1], go_w[1], fences[1].x)?,
    ];
    let mut queues: BTreeMap<u64, (u128, Key)> = BTreeMap::new();
    let mut floor = 0u128;
    let mut requeues = 0i128;
    let mut released = None;
    let mut clustered = BTreeSet::new();
    let mut first_cluster_w = None;
    let mut wake_counts = BTreeMap::<u64, usize>::new();
    let mut wakes = [Vec::new(), Vec::new()];
    let mut witnesses: [Vec<(u128, u128, usize, Option<bool>)>; 2] = [Vec::new(), Vec::new()];
    let mut categories = [[0usize; 4]; 4]; // driver zero/positive, timer zero/positive
    let mut pick_positions: BTreeMap<u64, Vec<usize>> = BTreeMap::new();
    let spread_limit = (u128::from(100 * tpu) * u128::from(redoubt_stride::STRIDE)).div_ceil(100);
    let mut spread = 0u128;
    for (i, r) in records.iter().enumerate() {
        if r.kind == 'W' {
            wake_counts.entry(r.id).and_modify(|n| *n += 1).or_insert(1);
            // Readiness can itself block; the validated D/W after each spinner's go is release.
            // Later reporting can itself block and wake after measurement.
            if spinners.contains(&r.id) {
                if spinner_release.get(&r.id) == Some(&i) {
                    first_cluster_w.get_or_insert(i);
                    clustered.insert(r.id);
                } else if spinner_release.get(&r.id).is_some_and(|release| i > *release) && released.is_none()
                {
                    return Err(format!(
                        "cluster spinner {} woke again before the release pick at record {}",
                        r.id, r.seq
                    ));
                }
            }
            if let Some(standin) = standins.iter().position(|id| *id == r.id) {
                if i > go_w[standin] && i < fences[standin].x {
                    let sample_index = wakes[standin].len();
                    if measurement_wakes[standin].get(sample_index) != Some(&i) {
                        return Err(format!(
                            "cluster stand-in {} W {} did not join a bounded sample before marker X",
                            r.id, r.seq
                        ));
                    }
                    let phase = sample_index % 4;
                    let intended_positive = metadata[standin][sample_index].intent_positive;
                    let lead = r.pass.saturating_sub(floor);
                    let key = (0, -i128::from(r.entry));
                    let ahead = spinners
                        .iter()
                        .filter(|id| {
                            queues
                                .get(id)
                                .is_some_and(|&(pass, rank)| (pass, rank, **id) < (r.pass, key, r.id))
                        })
                        .count();
                    // A positive lead is a pass above the replayed floor. How many spinners rank
                    // before the wake is the slice's doing, not the debt's: under 1 ms a waker
                    // is picked within one slice of the lowest spinner, so `ahead` stays a
                    // reported witness, not a qualifier (the old control's non-vacuity uses it).
                    let qualified = if intended_positive && lead > 0 {
                        Some(true)
                    } else if !intended_positive && lead == 0 {
                        Some(false)
                    } else {
                        None
                    };
                    if let Some(positive) = qualified {
                        categories[standin * 2 + usize::from(positive)][phase] += 1;
                    }
                    witnesses[standin].push((floor, lead, ahead, qualified));
                    wakes[standin].push(i);
                }
            }
        }
        match r.kind {
            'W' => {
                queues.insert(r.id, (r.pass, (0, -i128::from(r.entry))));
            }
            'R' => {
                requeues += 1;
                queues.insert(r.id, (r.pass, (1, requeues)));
            }
            'D' => {
                queues.remove(&r.id);
            }
            'P' => {
                if let Some(q) = queues.get_mut(&r.id) {
                    q.0 = r.pass;
                }
            }
            'K' => {
                pick_positions.entry(r.id).or_default().push(i);
                if released.is_none() && !clustered.is_empty() {
                    if clustered.len() != 16 || !spinners.iter().all(|id| queues.contains_key(id)) {
                        return Err(format!(
                            "cluster release: only {} spinner W before next K {}",
                            clustered.len(),
                            r.seq
                        ));
                    }
                    let low = spinners.iter().map(|id| queues[id].0).min().unwrap();
                    let high = spinners.iter().map(|id| queues[id].0).max().unwrap();
                    spread = high - low;
                    if spread > spread_limit {
                        return Err(format!(
                            "cluster pass spread {spread:#x} exceeds 100 us equivalent {spread_limit:#x}"
                        ));
                    }
                    released = Some(i);
                }
            }
            _ => {}
        }
        let waking = r.kind == 'W' || (r.kind == 'P' && !queues.contains_key(&r.id));
        if !waking {
            if let Some(min) = queues.values().map(|(pass, _)| *pass).min() {
                floor = floor.max(min);
            }
        }
    }
    let release_at = released.ok_or("cluster release had no qualifying sixteen-W interval")?;
    if wake_counts.get(&server).is_none_or(|n| *n < 2)
        || !spinners.iter().all(|id| wake_counts.get(id).is_some_and(|n| *n >= 3))
        || standins.iter().any(|id| wake_counts.get(id).is_none_or(|n| *n < 202))
    {
        return Err(format!(
            "cluster setup/sample W counts lack one spawn, one go and 200 bounded waits: server {:?}, spinners {:?}, stand-ins {:?}",
            wake_counts.get(&server),
            spinners.iter().map(|id| wake_counts.get(id)).collect::<Vec<_>>(),
            standins.map(|id| wake_counts.get(&id))
        ));
    }
    let first_release = first_cluster_w.ok_or("cluster release lacked a spinner W")?;
    let last_release = *spinner_release.values().max().ok_or("cluster release lacked its last spinner W")?;
    let last_pick =
        records[..first_release].iter().rposition(|r| r.kind == 'K').ok_or("no pick before release")?;
    if records[last_pick].id != server {
        return Err(format!(
            "busy server {} was not running across release; last pick was {}",
            server, records[last_pick].id
        ));
    }
    let mut lines = Vec::new();
    let mut metrics: [Vec<ClusterMetric>; 2] = [Vec::new(), Vec::new()];
    let mut positive_ahead: [Vec<usize>; 2] = [Vec::new(), Vec::new()];
    for (standin, measure) in ["driver_wake", "timer_wake"].iter().enumerate() {
        let windows =
            samples.get(&(standin, "cluster")).ok_or(format!("cluster {measure} windows missing"))?;
        if windows.len() != 200 || wakes[standin].len() != 200 {
            return Err(format!(
                "cluster {measure}: {} windows and {} bounded measurement W, expected exactly 200 each",
                windows.len(),
                wakes[standin].len()
            ));
        }
        let mut picks = 0usize;
        let mut peer_r10_overlap_us = 0u64;
        let mut net = Vec::new();
        let mut category_net = [Vec::new(), Vec::new()];
        let mut details = Vec::new();
        for (sample_index, (&w, &(sample_end, gross))) in
            wakes[standin][..200].iter().zip(windows).enumerate()
        {
            let meta = metadata[standin][sample_index];
            let next = wakes[standin].get(sample_index + 1).copied().unwrap_or(fences[standin].x);
            let service =
                pick_positions[&standins[standin]].iter().copied().find(|&k| k > w && k < next).ok_or_else(
                    || {
                        format!(
                            "cluster {measure} sample {sample_index}: no unique service K after W {}",
                            records[w].seq
                        )
                    },
                )?;
            let earlier = records[w + 1..service].iter().filter(|r| r.kind == 'K').count();
            picks += earlier;
            let (sample_start, sample_stop) = cluster_window_bounds(sample_end, gross, meta)
                .map_err(|e| format!("cluster {measure} sample {sample_index}: {e}"))?;
            let metric = cluster_metric(audits, sample_start, sample_stop, meta.rtc_gross)
                .map_err(|e| format!("cluster {measure} sample {sample_index}: {e}"))?;
            metrics[standin].push(metric);
            let peer = fences[1 - standin];
            let peer_begin = records[peer.x].pass as u64;
            let peer_end = records[peer.y].pass as u64;
            let overlap = sample_stop.min(peer_end).saturating_sub(sample_start.max(peer_begin));
            peer_r10_overlap_us += overlap;
            let elapsed = metric.net;
            net.push(elapsed);
            let (floor, lead, ahead, qualified) = witnesses[standin][sample_index];
            if let Some(positive) = qualified {
                category_net[usize::from(positive)].push(elapsed);
                // The timer reads no E and no RTC: it reports B, L, P and U only.
                let raw = if standin == 0 {
                    format!(
                        "B/L/E/P/U {}/{}/{}/{}/{} µs RTC a/d/s {}/{}/{} ns RTC-gross {} µs lower-witness {} µs",
                        meta.before,
                        meta.lower,
                        meta.early,
                        meta.observed,
                        meta.upper,
                        meta.arm,
                        meta.deadline,
                        meta.service,
                        meta.rtc_gross,
                        metric.lower_witness
                    )
                } else {
                    format!("B/L/P/U {}/{}/{}/{} µs", meta.before, meta.lower, meta.observed, meta.upper)
                };
                details.push(format!(
                    "{measure} #{sample_index} programmed-offset {} {} prep-slip {} µs delay {} µs {raw} W {} floor {floor:#x} lead {lead:#x} ahead {ahead} earlier-picks {earlier} peer-R10-overlap {overlap} µs envelope/certified-audit-credit/net {}/{}/{} µs",
                    [100, 300, 600, 850][sample_index % 4],
                    if positive { "positive" } else { "zero" },
                    meta.prep_us,
                    meta.delay_us,
                    records[w].seq,
                    metric.gross,
                    metric.credit,
                    metric.net,
                ));
            }
        }
        for category in 0..2 {
            let counts = categories[standin * 2 + category];
            let total: usize = counts.iter().sum();
            if total < 25 || counts.iter().any(|n| *n < 5) {
                return Err(format!(
                    "cluster {measure} {}-lead coverage {counts:?}, total {total}, needs >=25 and >=5 per offset",
                    if category == 0 { "zero" } else { "positive" }
                ));
            }
        }
        // `ahead` per category, as a distribution: min, median, max.
        let ahead_in = |positive: bool| -> Vec<u64> {
            witnesses[standin].iter().filter(|w| w.3 == Some(positive)).map(|w| w.2 as u64).collect()
        };
        let spread_of = |mut v: Vec<u64>| {
            let median = percentile(&mut v, 50);
            format!("{}/{median}/{}", v.first().copied().unwrap_or(0), v.last().copied().unwrap_or(0))
        };
        let ahead = format!(
            "; spinners ranked before the wake, min/median/max: zero {}, positive {}",
            spread_of(ahead_in(false)),
            spread_of(ahead_in(true))
        );
        positive_ahead[standin] = ahead_in(true).into_iter().map(|a| a as usize).collect();
        let (p50, p99) = (percentile(&mut net.clone(), 50), percentile(&mut net, 99));
        let zero = category_net[0].len();
        let positive = category_net[1].len();
        let (z50, z99) = (percentile(&mut category_net[0].clone(), 50), percentile(&mut category_net[0], 99));
        let (p50_cat, p99_cat) =
            (percentile(&mut category_net[1].clone(), 50), percentile(&mut category_net[1], 99));
        let mut lower: Vec<u64> = metrics[standin].iter().map(|m| m.lower_witness).collect();
        let (l50, l99) = (percentile(&mut lower.clone(), 50), percentile(&mut lower, 99));
        let witness = if standin == 0 {
            format!("; RTC gross less outer audit bins (lower witness) p50/p99 {l50}/{l99} µs")
        } else {
            String::new()
        };
        lines.push(format!(
            "{measure}: 200 fenced D-W-service/sample envelopes, {} later report wakes, marker {} X {} Y {} duration {} µs, peer R10 overlap {peer_r10_overlap_us} µs retained; {zero} zero and {positive} positive, {picks} earlier picks before service; all envelope net (an upper bound on non-audit latency) p50/p99 {p50}/{p99} µs{witness}; zero {z50}/{z99}; positive {p50_cat}/{p99_cat}{ahead}\n      {}",
            wake_counts[&standins[standin]] - 202,
            fences[standin].marker_id,
            records[fences[standin].x].seq,
            records[fences[standin].y].seq,
            records[fences[standin].y].pass - records[fences[standin].x].pass,
            details.join("\n      ")
        ));
    }
    let report = format!(
        "cluster: 16 queued spinner W before K {}, W entries {}..{}, sequences {}..{}, spread {spread:#x} <= {spread_limit:#x}; {}",
        records[release_at].seq,
        records[first_release].entry,
        records[last_release].entry,
        records[first_release].seq,
        records[last_release].seq,
        lines.join("; ")
    );
    Ok(ClusterProof { report, metrics, positive_ahead })
}

/// The old 10-ms build is a demonstrated negative control only if, with every construction and
/// coverage gate already passed, it shows the attack (at least 25 positive-lead wakes per
/// stand-in with eight or more spinners ranked before them), misses at least one unchanged
/// envelope target **and** a driver lower-witness percentile exceeds its target. An envelope miss
/// alone may be only the observation's uncertainty (arming, the reads, P's resolution bin), not
/// latency.
fn cluster_old_control_verdict(
    envelope_missed: bool,
    mut lower_witness: Vec<u64>,
    (b50, b99): (u64, u64),
    positive_ahead: &[Vec<usize>; 2],
) -> Result<String, String> {
    let behind = positive_ahead.each_ref().map(|v| v.iter().filter(|a| **a >= 8).count());
    if behind.iter().any(|n| *n < 25) {
        return Err(format!(
            "old control is vacuous: positive-lead wakes with >=8 spinners ahead {behind:?} (driver, timer), needs >=25 each"
        ));
    }
    let l50 = percentile(&mut lower_witness, 50);
    let l99 = percentile(&mut lower_witness, 99);
    if !envelope_missed {
        return Err("old control met every unchanged cluster envelope target".into());
    }
    if l50 <= b50 && l99 <= b99 {
        return Err(format!(
            "old control envelope-only failure: driver lower witness p50/p99 {l50}/{l99} µs within {b50}/{b99}"
        ));
    }
    Ok(format!(
        "old control: an envelope target missed, and driver lower witness p50/p99 {l50}/{l99} µs passes a target of {b50}/{b99}"
    ))
}

/// The guest's own gates, which the control's case does not expect by line (it expects the
/// trace instead, so that a classified failure reaches this check). Without a `CLUSTER-FAIL`, the
/// program must have passed. With one, the only failures allowed are those stand-ins' own
/// "took all 200 real waits": any other (a spinner or the server that did not run, a report
/// count, a setup acknowledgement) is not the classified kind.
fn cluster_control_guest_gates(log: &str) -> Result<(), String> {
    let lines = || log.lines().map(|x| x.trim_end_matches('\r'));
    let failed: Vec<&str> = lines()
        .filter_map(|l| {
            l.strip_prefix("CLUSTER-FAIL ")?.split_whitespace().find_map(|f| f.strip_prefix("role="))
        })
        .collect();
    if failed.is_empty() {
        if !lines().any(|l| l == "SCHED-CLUSTER TEST PASSED") {
            return Err(
                "control: the program did not pass, and no stand-in failed of the classified kind".into()
            );
        }
        return Ok(());
    }
    for fail in lines().filter(|l| l.starts_with("[cluster] FAIL: ")) {
        let allowed = failed
            .iter()
            .any(|role| fail == format!("[cluster] FAIL: stand-in {role} took all 200 real waits"));
        if !allowed {
            return Err(format!("control: a failure other than a classified stand-in's: {fail:?}"));
        }
    }
    Ok(())
}

/// An old control whose stand-in could not reach its next slot (kernel/scheduling.md,
/// "Responsiveness"; the control ruling). It is the demonstrated negative only if every
/// `CLUSTER-FAIL` is a zero-intent attempt `i >= 1` whose target had passed at B (branch 2,
/// result 0), the stand-in's attempts before `i` joined the trace as blocked waits after its go,
/// and for at least one such stand-in the miss `B - target`, less the certified audit time
/// inside `[target, B)`, exceeds that stand-in's p99 target. The trace is required: `parse`
/// has already refused a log without one.
fn cluster_control_classify(
    log: &str,
    records: &[Record],
    audits: &[(u64, u64)],
    bounds: &BTreeMap<&str, u64>,
) -> Result<String, String> {
    let (_, release, _, _) = cluster_plan(log)?;
    let mut first = Vec::new();
    let mut seen = BTreeSet::new();
    for (i, r) in records.iter().enumerate() {
        if r.kind == 'W' && seen.insert(r.id) {
            first.push((i, r.id));
        }
    }
    let map_start = first.len().checked_sub(19).ok_or("fewer than 19 first budget wakes")?;
    let mapped = &first[map_start..];
    let go = cluster_go_boundaries(records, mapped)?;
    let mut failed = [None, None];
    for line in log.lines().map(|x| x.trim_end_matches('\r')) {
        let Some(rest) = line.strip_prefix("CLUSTER-FAIL ") else { continue };
        let bad = || format!("control: malformed {line:?}");
        let field = |name: &str| -> Result<i64, String> {
            rest.split_whitespace()
                .find_map(|f| f.strip_prefix(name)?.strip_prefix('=')?.parse::<i64>().ok())
                .ok_or_else(bad)
        };
        let (role, index, branch, result) =
            (field("role")?, field("index")?, field("branch")?, field("result")?);
        let (target, current) = (field("target_us")?, field("current_us")?);
        let standin = match role {
            17 => 0,
            18 => 1,
            _ => return Err(bad()),
        };
        let index = usize::try_from(index).map_err(|_| bad())?;
        let phase = [100i64, 300, 600, 850][index % 4];
        if branch != 2
            || result != 0
            || index == 0
            || index >= 200
            || index / 4 % 2 == 1
            || target != index as i64 * 80_000 + phase
            || current <= target
            || failed[standin].is_some()
        {
            return Err(format!("control: {line:?} is not a zero-intent attempt whose target had passed"));
        }
        failed[standin] = Some((index, phase, target, current));
    }
    let mut lines = Vec::new();
    let mut demonstrated = false;
    for (standin, measure) in ["driver_wake", "timer_wake"].iter().enumerate() {
        let id = mapped[17 + standin].1;
        let Some((index, phase, target, current)) = failed[standin] else {
            lines.push(format!("stand-in {measure} reported no failure (its samples are not judged)"));
            continue;
        };
        // Attempts 0..index each took a real blocked wait after go: a D, its W, and a pick of the
        // stand-in before its next D, as the full check joins every sample.
        let after = go[17 + standin] + 1;
        let waits: Vec<(usize, char)> = records[after..]
            .iter()
            .enumerate()
            .filter(|(_, r)| r.id == id && (r.kind == 'D' || r.kind == 'W'))
            .map(|(at, r)| (after + at, r.kind))
            .take(2 * index + 1)
            .collect();
        for attempt in 0..index {
            let (Some(&(_, 'D')), Some(&(w, 'W'))) = (waits.get(2 * attempt), waits.get(2 * attempt + 1))
            else {
                return Err(format!("control: {measure} attempt {attempt} did not join a blocked wait"));
            };
            let next = waits.get(2 * attempt + 2).map_or(records.len(), |e| e.0);
            if !records[w + 1..next].iter().any(|r| r.kind == 'K' && r.id == id) {
                return Err(format!("control: {measure} attempt {attempt} has no service pick"));
            }
        }
        let at = |us: i64| -> Result<u64, String> {
            release
                .checked_add_signed(us)
                .ok_or_else(|| format!("control: {measure} time {us} outside the clock"))
        };
        let (from, to) = (at(target)?, at(current)?);
        let miss = to - from;
        let credit = cluster_metric(audits, from, to, 0)?.credit;
        let net = miss - credit;
        let bound = *bounds.get(&*format!("{measure}_p99_us")).ok_or("control: p99 target missing")?;
        let over = net > bound;
        demonstrated |= over;
        lines.push(format!(
            "control: stand-in {measure} could not take attempt {index} (zero intent, offset {phase}): B exceeded its target by {miss} µs, {credit} µs of audits inside, net {net} µs {} {bound}",
            if over { ">" } else { "<=" }
        ));
    }
    if !demonstrated {
        return Err(format!("control: no net miss above its p99 target: {}", lines.join("; ")));
    }
    Ok(lines.join("; "))
}

/// The 999/1000 carve must return while the same parent is still in the turn picked before the
/// down-weight. `G` carries both weights and the parent's low-weight lead before the up-weight;
/// the ordinary oracle independently checks both reweigh calculations and the later share.
fn check_carve_return(log: &str, records: &[Record]) -> Result<String, String> {
    let observations: Vec<_> = log.lines().filter_map(|line| line.strip_prefix("CARVE-OBS ")).collect();
    let [observation] = observations.as_slice() else {
        return Err(format!("carve-return: expected one observation, found {}", observations.len()));
    };
    let fields: Vec<_> = observation.split_whitespace().collect();
    if fields.len() != 3 {
        return Err(format!("carve-return: malformed observation {observation:?}"));
    }
    let mut numbers = fields.iter().map(|s| s.parse::<u64>());
    let (create, returned, absolute) = (
        numbers.next().unwrap().map_err(|_| "bad carve create duration")?,
        numbers.next().unwrap().map_err(|_| "bad carve return duration")?,
        numbers.next().unwrap().map_err(|_| "bad carve return time")?,
    );
    if create == 0 || returned < create || absolute == 0 {
        return Err(format!("carve-return: invalid create {create} or return {returned} µs"));
    }
    let changes: Vec<_> = records
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            if r.kind != 'G' {
                return None;
            }
            let packed = records.get(i + 2)?.pass;
            Some((i, r.id, packed >> 32, packed & 0xffff_ffff, records.get(i + 3)?.pass, r.pass))
        })
        .collect();
    let downs: Vec<_> = changes.iter().filter(|(_, _, old, new, _, _)| (*old, *new) == (1000, 1)).collect();
    let ups: Vec<_> = changes.iter().filter(|(_, _, old, new, _, _)| (*old, *new) == (1, 1000)).collect();
    let ([down], [up]) = (downs.as_slice(), ups.as_slice()) else {
        return Err(format!(
            "carve-return: expected one 1000->1 and one 1->1000 change, found {}/{}",
            downs.len(),
            ups.len()
        ));
    };
    let (start, parent) = (down.0, down.1);
    let (finish, low_pass, floor) = (up.0, up.5, up.4);
    if up.1 != parent || finish <= start || low_pass <= floor {
        return Err(format!(
            "carve-return: parent {parent} lacked positive low-weight lead ({low_pass:#x} vs floor {floor:#x})"
        ));
    }
    let pick = records[..start]
        .iter()
        .rposition(|r| r.kind == 'K')
        .ok_or("carve-return: no pick before the down-weight")?;
    if records[pick].id != parent {
        return Err(format!(
            "carve-return: last pick before carve was budget {}, not parent {parent}",
            records[pick].id
        ));
    }
    if let Some(r) = records[pick + 1..finish].iter().find(|r| {
        r.kind == 'K' || (r.id == parent && "RD".contains(r.kind)) || (r.kind == 'O' && r.pass != 1)
    }) {
        return Err(format!(
            "carve-return: parent {parent} lost its running turn at record {} ({})",
            r.seq, r.kind
        ));
    }
    Ok(format!(
        "carve-return: parent {parent} K {} through 1000->1 and 1->1000 with low-weight lead {:#x}; create {create} µs, return {returned} µs after wake at time_now {absolute}",
        records[pick].seq,
        low_pass - floor
    ))
}

/// A budget made after a marker runs before any other budget is picked twice: the program
/// destroys an empty marker budget (one never woken, picked or queued), then makes a budget; the
/// first wake after the marker's `Y` must be that new budget's first (a waker's `P`, its pass
/// raised to the floor, comes just before its `W`), and it must be picked before any other budget
/// is picked twice. A lineage's raw debt, unlifted, would cost it several rounds; the lift's own
/// arithmetic is [`check_lift`]'s.
fn check_round(records: &[Record]) -> Result<String, String> {
    let queued: BTreeSet<u64> = records.iter().filter(|r| "WKRD".contains(r.kind)).map(|r| r.id).collect();
    let markers: Vec<usize> =
        (0..records.len()).filter(|&i| records[i].kind == 'X' && !queued.contains(&records[i].id)).collect();
    let [x] = markers.as_slice() else {
        return Err(format!("round: expected one empty marker destruction, found {}", markers.len()));
    };
    let marker = records[*x].id;
    let y = records[x + 1..]
        .iter()
        .position(|r| r.kind == 'Y' && r.id == marker)
        .map(|at| x + 1 + at)
        .ok_or_else(|| format!("round: marker {marker} has no Y"))?;
    let w = records[y + 1..]
        .iter()
        .position(|r| r.kind == 'W')
        .map(|at| y + 1 + at)
        .ok_or_else(|| format!("round: no wake after marker {marker}"))?;
    let marked = records[w].id;
    if records[..w].iter().any(|r| r.id == marked && "WRDK".contains(r.kind)) {
        return Err(format!(
            "round: the first wake after marker {marker}, record {}, is budget {marked}'s, which is not new",
            records[w].seq
        ));
    }
    let mut picks = BTreeMap::<u64, usize>::new();
    for r in &records[w + 1..] {
        if r.kind != 'K' {
            continue;
        }
        if r.id == marked {
            return Ok(format!(
                "round: budget {marked}, new at record {}, picked at record {} after {} picks of other budgets, none twice",
                records[w].seq,
                r.seq,
                picks.len()
            ));
        }
        let n = picks.entry(r.id).or_default();
        *n += 1;
        if *n == 2 {
            return Err(format!(
                "round: budget {} was picked twice (record {}) before budget {marked}, new at record {}",
                r.id, r.seq, records[w].seq
            ));
        }
    }
    Err(format!("round: budget {marked}, new at record {}, was never picked", records[w].seq))
}

/// The most a lift's lead may have faded, in rounds, by the sibling's wake for `lift-delay` to
/// judge it.
const LIFT_FRESH: f64 = 0.5;

/// **A fresh lift's delay** (`lift-delay`, kernel/scheduling.md, "Residual risks"). Each phase is
/// the first lift into a budget P that never queues (it holds no process) of a child that did,
/// and the first budget new to the trace to wake after it: the sibling S made under P. The lift
/// gave P a lead of `W / w_P` over the floor (the child's work and P's weight, from the lift's
/// records), and a round moves the floor by one step of the budgets that take turns: the median
/// pass step between a budget's consecutive picks. So S, entering at P's pass, should wait
/// `W / w_P / step` rounds: the most any other budget is picked before S's first pick must be
/// less than a round from that, and from S's own lead over the floor at its wake. S must enter at
/// P's lifted pass, and the floor must have moved less than [`LIFT_FRESH`] of a round between
/// the lift and S's wake: a lift the floor has passed, or nearly, proves nothing. The phases must
/// number the program's `[lift-delay] phase` lines.
fn check_lift_delay(log: &str, records: &[Record], floor: &[u128]) -> Result<String, String> {
    let queued: BTreeSet<u64> = records.iter().filter(|r| "WRDPK".contains(r.kind)).map(|r| r.id).collect();
    let mut last = BTreeMap::new();
    let mut steps: Vec<u128> = records
        .iter()
        .filter(|r| r.kind == 'K')
        .filter_map(|r| last.insert(r.id, r.pass).map(|p| r.pass.saturating_sub(p)))
        .collect();
    steps.sort_unstable();
    let step = steps
        .get(steps.len() / 2)
        .copied()
        .filter(|s| *s > 0)
        .ok_or("lift-delay: no budget's pass rose between two picks: no round to measure")?;
    let (mut lifted, mut phases, mut failed) = (BTreeSet::new(), Vec::new(), false);
    for (i, r) in records.iter().enumerate() {
        let Some(g) = records.get(i..i + LIFT.len()).filter(|g| g[0].kind == 'L') else { continue };
        if queued.contains(&r.id) || !queued.contains(&g[1].id) || !lifted.insert(r.id) {
            continue;
        }
        let n = phases.len() + 1;
        let [cp, e, f, cr, w, pa] = [1, 2, 3, 4, 6, 7].map(|k| g[k].pass);
        let (wc, wp) = (w >> 32, w & 0xffff_ffff);
        let work = cp.saturating_sub(e.max(f)) * wc + cr;
        let expected = work as f64 / wp.max(1) as f64 / step as f64;
        let after = i + LIFT.len();
        let new = |j: usize| !records[..j].iter().any(|x| x.id == records[j].id && "WRDK".contains(x.kind));
        let w = (after..records.len()).find(|&j| records[j].kind == 'W' && new(j)).ok_or_else(|| {
            format!("lift-delay: phase {n}: no new budget woke after budget {}'s lift", r.id)
        })?;
        let (s, sp) = (records[w].id, records[w].pass);
        let head = format!("lift-delay: phase {n}: W={work} w_P={wp} expected {expected:.1} rounds");
        if sp != pa || sp <= floor[w] {
            failed = true;
            phases.push(format!(
                "{head}, but budget {s} woke at pass {sp:#x}, not above the floor {:#x} at P's lifted pass {pa:#x}: \
                 the floor had passed the lift",
                floor[w]
            ));
            continue;
        }
        let lead = (sp - floor[w]) as f64 / step as f64;
        let moved = floor[w].saturating_sub(f) as f64 / step as f64;
        let (mut picks, mut k) = (BTreeMap::<u64, usize>::new(), None);
        for x in records[w + 1..].iter().filter(|x| x.kind == 'K') {
            if x.id == s {
                k = Some(x);
                break;
            }
            *picks.entry(x.id).or_default() += 1;
        }
        let k = k.ok_or_else(|| {
            format!("lift-delay: phase {n}: budget {s}, new at record {}, was never picked", records[w].seq)
        })?;
        let most = picks.values().max().copied().unwrap_or(0);
        let fresh = moved < LIFT_FRESH;
        let within = [expected, lead].iter().all(|x| (most as f64 - x).abs() < 1.0);
        failed |= !(fresh && within);
        phases.push(format!(
            "{head}, observed {} picks, max {most} of one budget (budget {s} new at record {}, its lead at the wake \
             {lead:.1} rounds, the floor moved {moved:.2} rounds since the lift, picked at record {}){}",
            picks.values().sum::<usize>(),
            records[w].seq,
            k.seq,
            if !fresh {
                ": the lift was not fresh"
            } else if !within {
                ": not within a round"
            } else {
                ""
            }
        ));
    }
    let printed = log.lines().filter(|l| l.trim_end_matches('\r').starts_with("[lift-delay] phase ")).count();
    if phases.is_empty() || phases.len() != printed {
        return Err(format!(
            "lift-delay: the trace shows {} phases, the program printed {printed}",
            phases.len()
        ));
    }
    let out = format!("{} (a round's step {step:#x})", phases.join("; "));
    if failed { Err(out) } else { Ok(out) }
}

/// A share of the CPU the kernel charged, judged on the trace alone: `CHARGED-SHARE <name>
/// <start> <end> <tolerance>[@<harts>] <mark>...`, a window `[start, end]` on `time_now` in µs, how
/// far in thousandths the share may lie from what it is owed (with `@`, judged only on a trace of
/// that many harts and reported on any other), and the weights of the marks. A mark is an
/// empty budget of that weight the program carves from a budget and destroys, so the trace names
/// the budget (its lift's parent). The first mark's budget is the one judged; it and every marked
/// budget, with all the budgets lifted into them, are the whole it is judged of. A mark may name
/// its budget's runnable threads, `<weight>:<threads>`, which the share across harts is capped at
/// ([`water_fill`]); one that does not is never capped.
struct ChargedShare<'a> {
    name: &'a str,
    window: (u64, u64),
    tolerance: u64,
    /// The harts the share is judged at, if only at some (`@`).
    at: Option<u64>,
    marks: Vec<u64>,
    threads: Vec<Option<u64>>,
}

/// Each budget's share of `harts` harts by water-filling (kernel/scheduling.md, R12), in
/// thousandths of a hart, from its weight and its runnable threads (`None`: as many as it can use):
/// every budget whose weight's share of the harts left is more than its threads gets its threads,
/// and its threads and weight leave the harts and the weight shared, until none does; the rest
/// share what is left by weight.
fn water_fill(budgets: &BTreeMap<u64, (u64, Option<u64>)>, harts: u64) -> BTreeMap<u64, u64> {
    let mut capped: BTreeMap<u64, u64> = BTreeMap::new();
    loop {
        let left = u128::from(harts).saturating_sub(capped.values().map(|k| u128::from(*k)).sum());
        let weight: u128 =
            budgets.iter().filter(|(b, _)| !capped.contains_key(b)).map(|(_, (w, _))| u128::from(*w)).sum();
        let over: Vec<(u64, u64)> = budgets
            .iter()
            .filter(|(b, _)| !capped.contains_key(b))
            .filter_map(|(b, (w, k))| {
                k.filter(|k| u128::from(*w) * left > u128::from(*k) * weight).map(|k| (*b, k))
            })
            .collect();
        if over.is_empty() {
            return budgets
                .iter()
                .map(|(b, (w, _))| match capped.get(b) {
                    Some(k) => (*b, k * 1000),
                    None => (*b, (u128::from(*w) * left * 1000 / weight.max(1)) as u64),
                })
                .collect();
        }
        capped.extend(over);
    }
}

/// The share, in thousandths, that weight `judged` is owed among `weights` (its own included):
/// what R12 gives it of the CPU the runnable budgets share.
fn expected_share(judged: u64, weights: &BTreeMap<u64, u64>) -> u64 {
    judged * 1000 / weights.values().sum::<u64>().max(1)
}

/// A share's tolerance, `<tolerance>[+|-][@<harts>]`, and the harts after its `@`, if it names
/// some: the share is judged only on a trace of that many harts. `None` if they are not a count.
fn judged_harts(tolerance: &str) -> Option<(&str, Option<u64>)> {
    match tolerance.split_once('@') {
        Some((t, h)) => h.parse().ok().filter(|h| *h > 0).map(|h| (t, Some(h))),
        None => Some((tolerance, None)),
    }
}

/// A share's line and verdict on a trace of `harts` harts: one judged only at other harts is
/// reported, not judged.
fn judged_at((line, met): (String, bool), at: Option<u64>, harts: u64) -> (String, bool) {
    match at {
        Some(h) if h != harts => (format!("{line}, judged at {h} harts only: not judged"), true),
        _ => (line, met),
    }
}

/// The charged shares the program printed, in order.
fn charged_shares(log: &str) -> Result<Vec<ChargedShare<'_>>, String> {
    let mut out = Vec::new();
    for line in log.lines().map(|line| line.trim_end_matches('\r')) {
        let Some(rest) = line.strip_prefix("CHARGED-SHARE ") else { continue };
        let bad = || format!("malformed {line:?}");
        let mut f: Vec<&str> = rest.split_whitespace().collect();
        let mut at = None;
        if let Some(tolerance) = f.get_mut(3) {
            (*tolerance, at) = judged_harts(tolerance).ok_or_else(bad)?;
        }
        let mut nums = Vec::new();
        let mut threads = Vec::new();
        for v in f.iter().skip(1) {
            let (n, k) = match v.split_once(':') {
                Some((n, k)) => (n, Some(k.parse::<u64>().map_err(|_| bad())?)),
                None => (*v, None),
            };
            nums.push(n.parse::<u64>().map_err(|_| bad())?);
            threads.push(k);
        }
        let [start, end, tolerance, ref marks @ ..] = nums[..] else { return Err(bad()) };
        if threads[..3].iter().any(Option::is_some) {
            return Err(bad());
        }
        let distinct: BTreeSet<&u64> = marks.iter().collect();
        if marks.is_empty() || distinct.len() != marks.len() || start >= end || tolerance > 1000 {
            return Err(bad());
        }
        out.push(ChargedShare {
            name: f[0],
            window: (start, end),
            tolerance,
            at,
            marks: marks.to_vec(),
            threads: threads[3..].to_vec(),
        });
    }
    Ok(out)
}

/// Each budget's stride weight through the trace, as the trace states it: a hart's runner (`H`,
/// its weight in the pass field, 0 in a trace that does not say), each weight change (`G`, the old
/// and the new), and a destroyed child's own weight (its lift's `w`).
struct Weights {
    /// Each budget's records that state its weight: (the record's index, its weight before the
    /// record where the record says, after it).
    events: BTreeMap<u64, Vec<(usize, Option<u64>, u64)>>,
    /// A destroyed child's weight, from its lift.
    lifted: BTreeMap<u64, u64>,
}

impl Weights {
    fn new(records: &[Record]) -> Weights {
        let mut w = Weights { events: BTreeMap::new(), lifted: BTreeMap::new() };
        for (i, r) in records.iter().enumerate() {
            match r.kind {
                'H' if r.id != 0 && r.pass != 0 => {
                    w.events.entry(r.id).or_default().push((i, Some(r.pass as u64), r.pass as u64));
                }
                'G' => {
                    let v = records[i + 2].pass;
                    let (old, new) = ((v >> 32) as u64, (v & 0xffff_ffff) as u64);
                    w.events.entry(r.id).or_default().push((i, Some(old), new));
                }
                'L' => {
                    w.lifted.insert(records[i + 1].id, (records[i + 6].pass >> 32) as u64);
                }
                _ => {}
            }
        }
        w
    }

    /// `b`'s weight at record `i`: the last one stated before it; with none, the one the first
    /// record after it states it had before; with none, its lift's.
    fn at(&self, b: u64, i: usize) -> Option<u64> {
        let es = self.events.get(&b).map(Vec::as_slice).unwrap_or(&[]);
        match es.iter().rposition(|e| e.0 < i) {
            Some(at) => Some(es[at].2),
            None => es.first().and_then(|e| e.1).or_else(|| self.lifted.get(&b).copied()),
        }
    }
}

/// The records a share's window `[start, end]` on `time_now` in µs holds: from the first stamped
/// with a time in it (a timer interrupt, an audit, a destruction, a threads span or a walk) to the
/// last.
fn window_records(records: &[Record], (start, end): (u64, u64)) -> Option<(usize, usize)> {
    let stamped = |r: &Record| "IUVXYTtMm".contains(r.kind) && (start..=end).contains(&(r.pass as u64));
    Some((records.iter().position(stamped)?, records.iter().rposition(stamped)?))
}

/// What the kernel charged inside records `first..=last`, to the budgets `counted` admits.
struct Charges {
    /// Each budget's charge: its pass's rises times its weight (ticks times `STRIDE`).
    charged: BTreeMap<u64, u128>,
    /// The budgets charged that `counted` refuses.
    outside: BTreeSet<u64>,
    /// Each budget's lock waits (`Q`), in ticks: those of a hart while it ran the budget, which
    /// was billed for them though no thread of it ran.
    waited: BTreeMap<u64, u128>,
    /// The wakes of the budgets counted.
    wakes: usize,
}

/// Sum the kernel's charges inside records `first..=last` (`what` names the share in a failure).
/// A budget's pass rises by its charge times `STRIDE` over its stride weight, so between two of
/// its records that carry its pass the kernel charged it the rise times that weight: every such
/// rise inside the window is summed, at the weight the budget had then ([`Weights`]). A rise is
/// not a charge across a wake or a lift out of the cap set (`u`: the floor lifts the pass), a
/// weight change or a lift (the rule restates the pass, and the oracle has checked it), so each of
/// those starts the count again. A
/// budget counted and charged with no weight in the trace fails it. With `refuse`, the window in
/// µs, a weight change or a lift of a budget counted inside it fails it too: its competitors
/// changed.
fn charges(
    records: &[Record],
    (first, last): (usize, usize),
    weights: &Weights,
    counted: impl Fn(u64) -> bool,
    what: &str,
    refuse: Option<(u64, u64)>,
) -> Result<Charges, String> {
    let mut out =
        Charges { charged: BTreeMap::new(), outside: BTreeSet::new(), waited: BTreeMap::new(), wakes: 0 };
    // Each budget's pass last seen and whether it is out of the queue (left it, or never woke).
    let mut seen: BTreeMap<u64, (u128, bool)> = BTreeMap::new();
    // Each hart's runner (`H`).
    let mut runs: BTreeMap<u64, u64> = BTreeMap::new();
    let mut i = 0;
    while i < records.len() {
        let (r, inside) = (records[i], (first..=last).contains(&i));
        match r.kind {
            'H' => {
                runs.insert(r.hart, r.id);
            }
            'Q' if inside => {
                if let Some(&b) = runs.get(&r.hart).filter(|b| **b != 0 && counted(**b)) {
                    *out.waited.entry(b).or_default() += r.pass.saturating_sub(u128::from(r.id));
                }
            }
            // The other harts' audits the wait waited through were billed to nobody.
            'y' if inside => {
                if let Some(&b) = runs.get(&r.hart).filter(|b| **b != 0 && counted(**b)) {
                    let w = out.waited.entry(b).or_default();
                    *w = w.saturating_sub(u128::from(r.id));
                }
            }
            _ => {}
        }
        let out_of_queue = seen.get(&r.id).is_none_or(|s| s.1);
        // Each pass the record states: (the budget, the pass, whether the rise to it is a charge,
        // whether the budget is out of the queue after it).
        let passes = match r.kind {
            'W' => vec![(r.id, r.pass, false, false)],
            // Out of the cap set, lifted at most to the floor ahead of the `P` that sets it: no charge.
            'u' => vec![(r.id, r.pass, false, out_of_queue)],
            'D' => vec![(r.id, r.pass, true, true)],
            'K' | 'R' => vec![(r.id, r.pass, true, false)],
            // Out of the queue, a pass changes only on the way back in: the floor's lift.
            'P' => vec![(r.id, r.pass, !out_of_queue, out_of_queue)],
            // A weight change: the rise to its pass before is charged at the old weight; the pass
            // after starts the count again.
            'G' => vec![(r.id, r.pass, true, out_of_queue), (r.id, records[i + 4].pass, false, out_of_queue)],
            // A lift: the parent's rise and the child's are charged; the parent's pass after starts
            // the count again.
            'L' => vec![
                (r.id, r.pass, true, out_of_queue),
                (records[i + 1].id, records[i + 1].pass, true, true),
                (r.id, records[i + 7].pass, false, out_of_queue),
            ],
            _ => Vec::new(),
        };
        out.wakes += usize::from(inside && r.kind == 'W' && counted(r.id));
        let changed = match r.kind {
            'G' => Some(r.id),
            'L' => Some(records[i + 1].id),
            _ => None,
        };
        if let (Some(b), Some((start, end))) = (changed.filter(|b| inside && counted(*b)), refuse) {
            return Err(format!(
                "{what}: budget {b} under its marks was {} at record {}, inside [{start}, {end}]: its competitors changed",
                if r.kind == 'G' { "reweighed" } else { "ended" },
                r.seq
            ));
        }
        for (b, pass, charge, out_after) in passes {
            let Some((before, _)) = seen.insert(b, (pass, out_after)) else { continue };
            let rise = pass.saturating_sub(before);
            if !(charge && inside && rise > 0) {
                continue;
            }
            if !counted(b) {
                out.outside.insert(b);
                continue;
            }
            let w = weights.at(b, i).ok_or_else(|| {
                format!("{what}: budget {b} was charged in the window, but the trace states no weight for it")
            })?;
            *out.charged.entry(b).or_default() += rise * u128::from(w);
        }
        i += match r.kind {
            'G' => REWEIGH.len(),
            'L' => LIFT.len(),
            _ => 1,
        };
    }
    Ok(out)
}

/// Judge a charged share on what the kernel charged in its window ([`charges`]), to the marked
/// budgets and the budgets lifted into them. The checked build's audits are charged to no budget,
/// so the share is net of them by construction. What a budget out of the queue is charged shows
/// only in its next wake's pass, where the floor's lift hides it; the wakes in the window are
/// counted beside. A budget charged in the window that no lift places under a mark is named beside
/// too, so a whole the trace could not place is seen.
///
/// What the judged budget is owed is its weight over the weights of the budgets under the marks
/// that the kernel charged in the window (its competitors, itself included), each as the trace
/// states it; the share must lie within the tolerance of that. A window in which the competitors
/// could change is refused: a weight change or a lift of a budget under the marks inside it (a
/// budget made, carved from or ended).
fn check_charged_share(records: &[Record], s: &ChargedShare) -> Result<(String, bool), String> {
    let (parent, marked) = lifts(records);
    let what = format!("charged share {}", s.name);
    let roots = s.marks.iter().map(|w| mark(&marked, *w, &what)).collect::<Result<Vec<u64>, String>>()?;
    let judged = roots[0];
    let weights = Weights::new(records);
    let (start, end) = s.window;
    let Some((first, last)) = window_records(records, s.window) else {
        return Err(format!("charged share {}: no record stamped inside [{start}, {end}]", s.name));
    };
    let Charges { charged, outside, waited, wakes } = charges(
        records,
        (first, last),
        &weights,
        |b| root_of(&roots, &parent, b).is_some(),
        &what,
        Some(s.window),
    )?;
    let all: u128 = charged.values().sum();
    if all == 0 {
        return Err(format!("charged share {}: nothing charged under its marks in [{start}, {end}]", s.name));
    }
    let harts = harts(records);
    if harts > 1 {
        return judge_across_harts(s, &roots, |b| root_of(&roots, &parent, b), &charged, &waited, harts, |b| {
            weights.at(b, first)
        })
        .map(|(line, met)| {
            (
                format!(
                    "{line}; [{start}, {end}] µs (records {}..={}), {wakes} wakes in the window, charged outside its marks: {outside:?}",
                    records[first].seq, records[last].seq
                ),
                met,
            )
        });
    }
    let its = charged.get(&judged).copied().unwrap_or(0);
    let share = (its * 1000 / all) as u64;
    // The competitors' weights in the window (none changes inside it).
    let by_weight: BTreeMap<u64, u64> =
        charged.keys().map(|&b| (b, weights.at(b, first).unwrap_or(0))).collect();
    let expected = expected_share(by_weight.get(&judged).copied().unwrap_or(0), &by_weight);
    let (min, max) = (expected.saturating_sub(s.tolerance), (expected + s.tolerance).min(1000));
    let met = (min..=max).contains(&share);
    let stride = u128::from(redoubt_stride::STRIDE);
    Ok((
        format!(
            "charged share {}: {share} of 1000 of the CPU the kernel charged to the budgets under its marks in [{start}, {end}] µs (records {}..={}): budget {judged} {} ticks of {}, audits charged to none; competitors by weight {by_weight:?}, expected {expected}; {wakes} wakes in the window; charged outside its marks: {:?}: target {} ({min} <= share <= {max})",
            s.name,
            records[first].seq,
            records[last].seq,
            its / stride,
            all / stride,
            outside,
            if met { "met" } else { "missed" }
        ),
        met,
    ))
}

/// Each lift's child and its parent, and the marks: the empty budgets destroyed (never queued),
/// their parents by the mark's weight.
fn lifts(records: &[Record]) -> (BTreeMap<u64, u64>, BTreeMap<u64, Vec<u64>>) {
    let queued: BTreeSet<u64> = records.iter().filter(|r| "WKRDP".contains(r.kind)).map(|r| r.id).collect();
    let (mut parent, mut marked) = (BTreeMap::new(), BTreeMap::<u64, Vec<u64>>::new());
    for (i, r) in records.iter().enumerate().filter(|(_, r)| r.kind == 'L') {
        let (child, w) = (records[i + 1].id, (records[i + 6].pass >> 32) as u64);
        parent.insert(child, r.id);
        if !queued.contains(&child) {
            marked.entry(w).or_default().push(r.id);
        }
    }
    (parent, marked)
}

/// The budget a mark of weight `w` names: the parent of the one empty budget of that weight.
fn mark(marked: &BTreeMap<u64, Vec<u64>>, w: u64, what: &str) -> Result<u64, String> {
    match marked.get(&w).map(Vec::as_slice) {
        Some([b]) => Ok(*b),
        other => Err(format!(
            "{what}: {} empty budgets of mark weight {w} destroyed, not one",
            other.map_or(0, <[u64]>::len)
        )),
    }
}

/// The mark `b` is under: the first of `roots` its lifts lead to.
fn root_of(roots: &[u64], parent: &BTreeMap<u64, u64>, mut b: u64) -> Option<u64> {
    loop {
        if roots.contains(&b) {
            return Some(b);
        }
        match parent.get(&b) {
            Some(&p) if p != b => b = p,
            _ => return None,
        }
    }
}

/// A charged share across harts: each marked budget's charge in the window, with what was lifted
/// into it, less its harts' lock waits there (billed to it, though no thread of it ran), of all of
/// them; against its water-filling want ([`water_fill`]) of the wants of all of them, from the
/// marks' weights as the trace states them at the window's start and their threads as the marks
/// name them, at the trace's harts (`F`). Every want is stated, so a failure names its numbers.
fn judge_across_harts(
    s: &ChargedShare,
    roots: &[u64],
    root_of: impl Fn(u64) -> Option<u64>,
    charged: &BTreeMap<u64, u128>,
    waited: &BTreeMap<u64, u128>,
    harts: u64,
    weight: impl Fn(u64) -> Option<u64>,
) -> Result<(String, bool), String> {
    let stride = u128::from(redoubt_stride::STRIDE);
    let mut net: BTreeMap<u64, u128> = roots.iter().map(|r| (*r, 0)).collect();
    for (b, c) in charged {
        if let Some(r) = root_of(*b) {
            *net.entry(r).or_default() += c;
        }
    }
    let mut lost: BTreeMap<u64, u128> = BTreeMap::new();
    for (b, w) in waited {
        if let Some(r) = root_of(*b) {
            *lost.entry(r).or_default() += w;
            let n = net.entry(r).or_default();
            *n = n.saturating_sub(w * stride);
        }
    }
    let mut budgets = BTreeMap::new();
    for (r, k) in roots.iter().zip(&s.threads) {
        let w =
            weight(*r).ok_or_else(|| format!("charged share {}: no weight for marked budget {r}", s.name))?;
        budgets.insert(*r, (w, *k));
    }
    let wants = water_fill(&budgets, harts);
    let all: u128 = net.values().sum();
    let judged = roots[0];
    let share = (net[&judged] * 1000 / all.max(1)) as u64;
    let expected = wants[&judged] * 1000 / wants.values().sum::<u64>().max(1);
    let (min, max) = (expected.saturating_sub(s.tolerance), (expected + s.tolerance).min(1000));
    let met = (min..=max).contains(&share);
    Ok((
        format!(
            "charged share {}: {share} of 1000 of the CPU the kernel charged to the budgets under its marks, net of their harts' lock waits, at {harts} harts: budget {judged} {} ticks of {}; water-filling wants in thousandths of a hart {wants:?} (weight, threads {budgets:?}), expected {expected}; lock waits taken out, ticks {lost:?}: target {} ({min} <= share <= {max})",
            s.name,
            net[&judged] / stride,
            all / stride,
            if met { "met" } else { "missed" }
        ),
        met,
    ))
}

/// A share across harts of the CPU the kernel charged, judged on the trace alone: `HART-SHARE
/// <name> <start> <end> <tolerance>[+|-][@<harts>] <mark>[:<threads>] <weight>[:<threads>]...`, a
/// window `[start, end]` on `time_now` in µs, how far in thousandths the share may lie from what it
/// is owed (with `+` only below it, so it may be any more; with `-` only above it; with `@`, judged
/// only on a trace of that many harts and reported on any other), the weight of the mark
/// naming the budget judged ([`ChargedShare`]), and the weight of each budget the program runs against it.
/// Each may name its runnable threads, which its share across harts is capped at ([`water_fill`]); one that
/// does not is never capped.
struct HartShare<'a> {
    name: &'a str,
    window: (u64, u64),
    tolerance: u64,
    /// `+` (at least the want less the tolerance), `-` (at most the want and the tolerance), or
    /// neither.
    side: Option<char>,
    /// The harts the share is judged at, if only at some.
    at: Option<u64>,
    mark: (u64, Option<u64>),
    others: Vec<(u64, Option<u64>)>,
}

/// The shares across harts the program printed, in order.
fn hart_shares(log: &str) -> Result<Vec<HartShare<'_>>, String> {
    let mut out = Vec::new();
    for line in log.lines().map(|line| line.trim_end_matches('\r')) {
        let Some(rest) = line.strip_prefix("HART-SHARE ") else { continue };
        let bad = || format!("malformed {line:?}");
        let num = |v: &str| v.parse::<u64>().map_err(|_| bad());
        let budget = |v: &str| match v.split_once(':') {
            Some((w, k)) => match num(k)? {
                0 => Err(bad()),
                k => Ok((num(w)?, Some(k))),
            },
            None => Ok((num(v)?, None)),
        };
        let f: Vec<&str> = rest.split_whitespace().collect();
        let [name, start, end, tolerance, judged, ref others @ ..] = f[..] else { return Err(bad()) };
        let (tolerance, at) = judged_harts(tolerance).ok_or_else(bad)?;
        let (tolerance, side) = match tolerance.strip_suffix(['+', '-']) {
            Some(t) => (t, tolerance.chars().last()),
            None => (tolerance, None),
        };
        let (start, end, tolerance) = (num(start)?, num(end)?, num(tolerance)?);
        if start >= end || tolerance > 1000 {
            return Err(bad());
        }
        out.push(HartShare {
            name,
            window: (start, end),
            tolerance,
            side,
            at,
            mark: budget(judged)?,
            others: others.iter().map(|v| budget(v)).collect::<Result<_, _>>()?,
        });
    }
    Ok(out)
}

/// Judge a share across harts. The whole is everything the kernel charged in the window
/// ([`charges`]), net of every lock wait in it: a wait is billed to the runner of the hart that
/// waited, though no thread of it ran (kernel/scheduling.md, "Residual risks"). The part is what
/// it charged the marked budget and the budgets lifted into it, net of the waits of the harts
/// running them. What the budget is owed is its water-filling want of the wants of it and the
/// budgets the program runs against it ([`water_fill`]), at the trace's harts (`F`), its weight as
/// the trace states it at the window's start; at one hart that is its weight over theirs. A
/// budget the program does not name (its own, `init`'s) is in the whole and owed nothing, so it
/// counts against the share. Every want is stated, so a failure names its numbers.
fn check_hart_share(records: &[Record], s: &HartShare) -> Result<(String, bool), String> {
    let what = format!("hart share {}", s.name);
    let (parent, marked) = lifts(records);
    let judged = mark(&marked, s.mark.0, &what)?;
    let weights = Weights::new(records);
    let (start, end) = s.window;
    let (first, last) = window_records(records, s.window)
        .ok_or_else(|| format!("{what}: no record stamped inside [{start}, {end}]"))?;
    let c = charges(records, (first, last), &weights, |_| true, &what, None)?;
    let stride = u128::from(redoubt_stride::STRIDE);
    // What the budgets `pick` admits were charged net of their waits, and their waits in ticks.
    let net = |pick: &dyn Fn(u64) -> bool| {
        let charged: u128 = c.charged.iter().filter(|(b, _)| pick(**b)).map(|(_, v)| v).sum();
        let waited: u128 = c.waited.iter().filter(|(b, _)| pick(**b)).map(|(_, v)| v).sum();
        (charged.saturating_sub(waited * stride), waited)
    };
    let ((its, its_waits), (all, all_waits)) =
        (net(&|b| root_of(&[judged], &parent, b).is_some()), net(&|_| true));
    if all == 0 {
        return Err(format!("{what}: nothing charged in [{start}, {end}]"));
    }
    let w = weights
        .at(judged, first)
        .ok_or_else(|| format!("{what}: the trace states no weight for budget {judged}"))?;
    let budgets: BTreeMap<u64, (u64, Option<u64>)> = std::iter::once((w, s.mark.1))
        .chain(s.others.iter().copied())
        .enumerate()
        .map(|(i, b)| (i as u64, b))
        .collect();
    let harts = harts(records);
    let wants = water_fill(&budgets, harts);
    let expected = wants[&0] * 1000 / wants.values().sum::<u64>().max(1);
    let share = (its * 1000 / all) as u64;
    let min = if s.side == Some('-') { 0 } else { expected.saturating_sub(s.tolerance) };
    let max = if s.side == Some('+') { 1000 } else { (expected + s.tolerance).min(1000) };
    let met = (min..=max).contains(&share);
    let stated: Vec<String> = budgets
        .iter()
        .map(|(i, (w, k))| format!("{w}:{} {}", k.map_or("-".to_string(), |k| k.to_string()), wants[i]))
        .collect();
    Ok((
        format!(
            "hart share {}: {share} of 1000 of the CPU the kernel charged in [{start}, {end}] µs (records {}..={}), net of lock waits, at {harts} harts: budget {judged} {} ticks of {}, its lock waits {its_waits} of {all_waits} ticks taken out; water-filling wants in thousandths of a hart (weight:threads want, the judged first) [{}], expected {expected}; {} budgets charged, {} wakes in the window: target {} ({})",
            s.name,
            records[first].seq,
            records[last].seq,
            its / stride,
            all / stride,
            stated.join(", "),
            c.charged.len(),
            c.wakes,
            if met { "met" } else { "missed" },
            match s.side {
                Some('+') => format!("share >= {min}"),
                Some(_) => format!("share <= {max}"),
                None => format!("{min} <= share <= {max}"),
            }
        ),
        met,
    ))
}

/// The harts the trace's `F` states, 1 in a trace with none.
fn harts(records: &[Record]) -> u64 {
    records.iter().rev().find(|r| r.kind == 'F').map_or(1, |r| r.pass as u64)
}

/// The audit time inside `[from, to]`, µs: the part of each audit's span that falls in it, so an
/// audit straddling an edge counts only its inside. `audits` is in trace order, so by time.
pub fn audit_inside(audits: &[(u64, u64)], from: u64, to: u64) -> u64 {
    let first = audits.partition_point(|a| a.1 <= from);
    audits[first..].iter().take_while(|a| a.0 < to).map(|a| a.1.min(to).saturating_sub(a.0.max(from))).sum()
}

/// The measures a program times on `time_now` and prints sample by sample, each a window
/// `[end - gross, end]` in µs: `LATENCY-SAMPLE <group> <measure> <end> <gross>`, the group being
/// what the program judges apart (`N=16`). With them, how many it took of each
/// (`LATENCY-COUNT <group> <measure> <n>`), so a window lost on the way fails the check.
const MEASURES: [&str; 4] = ["driver_wake", "timer_wake", "decision_wake", "deadline_notice"];

/// The walks a `walk-trace` kernel brackets, by their ids 1 to 3, each with a `<name>_max_us`
/// bound: a receive's pump, a timer expiry's own walk, and a reconcile.
const WALKS: [&str; 3] = ["pump", "expiry", "reconcile"];

/// The windows the program printed, `(end, gross)` in µs, and the count it took, by measure (its
/// index in [`MEASURES`]) and group. Every group's windows must number its count.
fn samples(log: &str) -> Result<BTreeMap<(usize, &str), Vec<(u64, u64)>>, String> {
    let mut windows: BTreeMap<(usize, &str), Vec<(u64, u64)>> = BTreeMap::new();
    let mut counts = BTreeMap::new();
    for line in log.lines().map(|line| line.trim_end_matches('\r')) {
        let (sample, rest) = match (line.strip_prefix("LATENCY-SAMPLE "), line.strip_prefix("LATENCY-COUNT "))
        {
            (Some(rest), _) => (true, rest),
            (_, Some(rest)) => (false, rest),
            _ => continue,
        };
        let bad = || format!("malformed {line:?}");
        let f: Vec<&str> = rest.split_whitespace().collect();
        let (group, measure) = (f.first().ok_or_else(bad)?, f.get(1).ok_or_else(bad)?);
        let m = MEASURES.iter().position(|x| x == measure).ok_or_else(bad)?;
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).ok_or_else(bad);
        if sample && f.len() == 4 && num(3)? <= num(2)? {
            windows.entry((m, *group)).or_default().push((num(2)?, num(3)?));
        } else if !sample && f.len() == 3 && counts.insert((m, *group), num(2)?).is_none() {
            windows.entry((m, *group)).or_default();
        } else {
            return Err(bad());
        }
    }
    for (&(m, group), w) in &windows {
        if counts.get(&(m, group)) != Some(&(w.len() as u64)) {
            return Err(format!(
                "{group} {}: {} windows printed, but the program counted {:?}",
                MEASURES[m],
                w.len(),
                counts.get(&(m, group))
            ));
        }
    }
    Ok(windows)
}

/// The kernel's time a `C` record states, and the share of it, net of the checked build's audits,
/// that no budget was charged: report-only (kernel/scheduling.md, "Residual risks").
fn nobody(r: &Record) -> String {
    let (kernel, audits, charged) = (r.id, r.entry, r.pass as u64);
    let net = kernel.saturating_sub(audits);
    let per_mille = net.saturating_sub(charged).saturating_mul(1000) / net.max(1);
    format!("nobody {per_mille} of 1000 (kernel {kernel} ticks, audits {audits}, charged {charged})")
}

/// The waits for the kernel lock from user mode (`Q`) as a share of the harts' time (`F`): the
/// time one hart's runner lost to another hart's kernel section, which no budget is charged for and
/// R78 bounds by count, not by share (kernel/scheduling.md, "Residual risks"); and of them, the
/// ticks behind other harts' audits, which a checked build bills to nobody (`y`). Report-only.
fn lock_waits(waits: &[(u64, u64, u64)], excused: u64, (ticks, harts): (u64, u64)) -> String {
    let waited: u64 = waits.iter().map(|(from, to, _)| to - from).sum();
    let per_mille = waited.saturating_mul(1000) / ticks.saturating_mul(harts).max(1);
    format!(
        "lock waits {per_mille} of 1000 ({} waits, {waited} ticks, {excused} of them behind other harts' \
         audits and billed to nobody, over {harts} hart(s) x {ticks} ticks)",
        waits.len()
    )
}

/// With `lock-trace`, FIFO kernel entry (R78) from the waits' tickets (`k`): each record is written
/// holding the lock, so the waits appear in the order they took it, and that order must be the
/// tickets' (modulo the counter's wrap), each drawn behind fewer sections than the harts (`F`). So
/// no wait outlasts the sections queued ahead of it. Its lengths in ticks are reported beside.
fn lock_order(tickets: &[(u64, u64)], waits: &[(u64, u64, u64)], harts: u64) -> Result<String, String> {
    for (k, w) in tickets.windows(2).enumerate() {
        let step = (w[1].0 as u32).wrapping_sub(w[0].0 as u32);
        if step == 0 || step > u32::MAX / 2 {
            return Err(format!(
                "lock wait {} took the kernel lock on ticket {}, after ticket {}: not in ticket order (R78)",
                k + 1,
                w[1].0,
                w[0].0
            ));
        }
    }
    let most = tickets.iter().map(|t| t.1).max().unwrap_or(0);
    if most >= harts {
        return Err(format!("a lock wait was drawn behind {most} sections, {harts} hart(s) (R78)"));
    }
    let mut lengths: Vec<u64> = waits.iter().map(|(from, to, _)| to - from).collect();
    let (p50, p99) = (percentile(&mut lengths, 50), percentile(&mut lengths, 99));
    Ok(format!(
        "lock order: {} waits took the kernel lock in ticket order, at most {most} section(s) ahead of one \
         ({harts} harts); waits in ticks p50/p99/max {p50}/{p99}/{}",
        tickets.len(),
        lengths.last().copied().unwrap_or(0)
    ))
}

/// The causes of a section a page fault began (`hold-trace`): an instruction, load or store page
/// fault, `0x300` plus its exception code.
const PAGE_FAULTS: [u64; 3] = [0x30c, 0x30d, 0x30f];

/// The cause of a section the supervisor timer interrupt began (`hold-trace`), `0x200` plus its
/// code: from user mode, or in `kmain`'s idle.
const TIMER_INTERRUPT: u64 = 0x205;

/// A section's cause by name: a system call's, an interrupt's or an exception's code, or `kmain`.
fn cause_name(cause: u64) -> String {
    match cause {
        0 => "kmain".into(),
        0x200.. if cause < 0x300 => format!("interrupt {}", cause - 0x200),
        0x300.. => format!("exception {}", cause - 0x300),
        _ => redoubt_sys::Number::from_raw(cause).map_or("an unknown call".into(), |n| n.name().into()),
    }
}

/// With `hold-trace`, the kernel lock's sections (kernel/scheduling.md, "Fair kernel entry is
/// bounded by count"): their lengths net of the audits inside them, the longest by cause, and
/// what the lock waits (`Q`) waited behind: the part of each wait that another hart's section
/// covers (its audits pro rata) and the part with the lock free, the hand-off to the waiting hart.
/// Report-only.
fn sections(sections: &[Section], waits: &[(u64, u64, u64)]) -> String {
    let net = |s: &Section| s.to - s.from - s.audits;
    let mut lengths: Vec<u64> = sections.iter().map(net).collect();
    let (p50, p99) = (percentile(&mut lengths, 50), percentile(&mut lengths, 99));
    let mut by_cause: BTreeMap<u64, (usize, u64, u64)> = BTreeMap::new();
    for s in sections {
        let c = by_cause.entry(s.cause).or_default();
        *c = (c.0 + 1, c.1 + net(s), c.2.max(net(s)));
    }
    let mut longest: Vec<_> = by_cause.into_iter().collect();
    longest.sort_by_key(|(_, (_, _, max))| std::cmp::Reverse(*max));
    let longest: Vec<String> = longest
        .iter()
        .take(5)
        .map(|(cause, (n, total, max))| format!("{} {max} ({n}, {total} in all)", cause_name(*cause)))
        .collect();
    // Sections by start, to find the ones each wait overlaps.
    let mut by_start: Vec<&Section> = sections.iter().collect();
    by_start.sort_by_key(|s| s.from);
    let longest_section = sections.iter().map(|s| s.to - s.from).max().unwrap_or(0);
    let (mut waited, mut behind, mut audits) = (0u64, 0u64, 0u64);
    for &(from, to, hart) in waits {
        waited += to - from;
        let first = by_start.partition_point(|s| s.from + longest_section < from);
        for s in by_start[first..].iter().take_while(|s| s.from < to).filter(|s| s.hart != hart) {
            let overlap = s.to.min(to).saturating_sub(s.from.max(from));
            behind += overlap;
            audits +=
                (u128::from(overlap) * u128::from(s.audits) / u128::from((s.to - s.from).max(1))) as u64;
        }
    }
    format!(
        "kernel sections: {} held, ticks net of audits p50/p99/max {p50}/{p99}/{}; longest by cause: {}; \
         lock waits {waited} ticks: behind other harts' sections {behind} (their audits {audits}), the lock \
         free {}",
        sections.len(),
        lengths.last().copied().unwrap_or(0),
        longest.join(", "),
        waited.saturating_sub(behind)
    )
}

/// The bench's post-check: parse the case's console log and check it. `args` may bound the p99
/// of R10's durations, `r10_p99_us=N`; each measure's p50 and p99 net of audits,
/// `<measure>_p50_us=N` and `<measure>_p99_us=N`, in each group the program printed; and a
/// lease's end from the steward's decision, `lease_end_p99_us=N`: the worst net decision-wake p99
/// (over every group) plus R10's p99 (kernel/scheduling.md, "Responsiveness"). With
/// `gate_harts=N`, those targets are judged on a trace of at most N harts (`F`) and only reported
/// on more: the targets are gated at one hart and two and recorded at four. A `walk-trace`
/// kernel's walks may each be bounded, `pump_max_us=N`, `expiry_max_us=N` and
/// `reconcile_max_us=N`, the longest net of audits, judged before R10's p99; a `hold-trace`
/// kernel's longest section a page fault caused, `fault_section_max_ticks=N`, and a timer interrupt,
/// `timer_section_max_ticks=N` and their p99, `timer_section_p99_ticks=N` (both gated up to
/// `gate_harts`), net of audits. Every window a
/// target judges has the checked build's audit time inside it subtracted; R10's has none. Each charged
/// share the program printed is judged on the kernel's charges in the trace
/// ([`check_charged_share`]), and each share across harts on the same charges net of lock waits
/// ([`check_hart_share`]), with the timer interrupts inside its window nobody paid for and those
/// that found another budget's wait ended early beside; `stale_waits_in=<share>` requires one of
/// the latter.
/// `round` requires the budget the program marks to run before any other budget is picked twice
/// ([`check_round`]).
pub fn run(log: &str, args: &str) -> Result<String, String> {
    let records = parse(log)?;
    let mut sum = check(&records)?;
    let samples = samples(log)?;
    let n = sum.r10_us.len();
    let (p50, p99, max) = (
        percentile(&mut sum.r10_us, 50),
        percentile(&mut sum.r10_us, 99),
        sum.r10_us.last().copied().unwrap_or(0),
    );
    let threads = &mut sum.r10_threads_us;
    let (t50, t99, tmax) =
        (percentile(threads, 50), percentile(threads, 99), threads.last().copied().unwrap_or(0));
    // Each bound, by its argument's name.
    let mut bounds = BTreeMap::new();
    let mut stale_in = Vec::new();
    let mut wake_no_preempt = None;
    let mut cluster = false;
    let mut cluster_old_control = false;
    let mut carve_return = false;
    let mut round = false;
    let mut lift_delay = false;
    for arg in args.split_whitespace() {
        if arg == "cluster" {
            if cluster {
                return Err("duplicate cluster check".into());
            }
            cluster = true;
            continue;
        }
        if arg == "cluster_old_control" {
            if cluster_old_control {
                return Err("duplicate cluster old-control check".into());
            }
            cluster_old_control = true;
            continue;
        }
        if arg == "carve_return" {
            if carve_return {
                return Err("duplicate carve-return check".into());
            }
            carve_return = true;
            continue;
        }
        if arg == "round" {
            if round {
                return Err("duplicate round check".into());
            }
            round = true;
            continue;
        }
        if arg == "lift-delay" {
            if lift_delay {
                return Err("duplicate lift-delay check".into());
            }
            lift_delay = true;
            continue;
        }
        if let Some(share) = arg.strip_prefix("stale_waits_in=") {
            stale_in.push(share);
            continue;
        }
        if let Some(count) = arg.strip_prefix("wake_no_preempt=") {
            wake_no_preempt = Some(count.parse::<usize>().map_err(|_| format!("bad wake count {arg:?}"))?);
            continue;
        }
        let (name, bound) = arg
            .split_once('=')
            .and_then(|(name, v)| Some((name, v.parse::<u64>().ok()?)))
            .ok_or_else(|| format!("unknown sched_oracle argument {arg:?}"))?;
        let measure = name.strip_suffix("_p50_us").or_else(|| name.strip_suffix("_p99_us"));
        let walk = name.strip_suffix("_max_us");
        if !([
            "r10_p99_us",
            "lease_end_p99_us",
            "gate_harts",
            "fault_section_max_ticks",
            "timer_section_max_ticks",
            "timer_section_p99_ticks",
            "driver_wake_p50_searches",
            "driver_wake_p99_searches",
            "wake_witnesses",
        ]
        .contains(&name)
            || measure.is_some_and(|m| MEASURES.contains(&m))
            || walk.is_some_and(|w| WALKS.contains(&w)))
        {
            return Err(format!("unknown sched_oracle argument {arg:?}"));
        }
        bounds.insert(name, bound);
    }
    // One hart proves each wake against the slice that follows it; several judge the entry that
    // answered each wake, and need witnesses (`wake_witnesses`).
    let several = records.iter().any(|r| r.hart != 0);
    let wake_proof = match (wake_no_preempt, several) {
        (Some(count), false) => Some(check_wake_no_preempt(&records, count)?),
        (Some(count), true) => {
            let witnesses = bounds.get("wake_witnesses").copied().unwrap_or(0) as usize;
            Some(check_wake_no_preempt_harts(&records, count, witnesses)?)
        }
        (None, _) => None,
    };
    if cluster_old_control && !cluster {
        return Err("cluster_old_control requires cluster".into());
    }
    // An old control whose stand-in could not reach a slot took no 200 samples: it is judged by
    // the classification alone, never by the full cluster check or its partial samples.
    let control_failed =
        cluster_old_control && log.lines().any(|l| l.trim_end_matches('\r').starts_with("CLUSTER-FAIL "));
    let cluster_proof = (cluster && !control_failed)
        .then(|| check_cluster(log, &records, &samples, &sum.audits))
        .transpose()?;
    if cluster_old_control {
        cluster_control_guest_gates(log)?;
    }
    let control_classified =
        control_failed.then(|| cluster_control_classify(log, &records, &sum.audits, &bounds)).transpose()?;
    let carve_proof = carve_return.then(|| check_carve_return(log, &records)).transpose()?;
    let round_proof = round.then(|| check_round(&records)).transpose()?;
    let lift_delay_proof = lift_delay.then(|| check_lift_delay(log, &records, &sum.floor)).transpose()?;
    // Each walk net of the audits inside it, as a release kernel runs it; a walk's bound is
    // judged before R10's, so a case whose R10 must fail still holds its walks.
    let walks: Vec<(Vec<u64>, u64)> = sum
        .walks
        .iter()
        .map(|spans| {
            let inside: Vec<u64> = spans.iter().map(|&(b, e)| audit_inside(&sum.audits, b, e)).collect();
            let net = spans.iter().zip(&inside).map(|(&(b, e), a)| (e - b).saturating_sub(*a)).collect();
            (net, inside.iter().sum())
        })
        .collect();
    for (name, (net, _)) in WALKS.iter().zip(&walks) {
        let Some(bound) = bounds.get(&*format!("{name}_max_us")) else { continue };
        let max =
            net.iter().max().ok_or(format!("a {name} bound is set, but the trace holds no {name} walk"))?;
        if max > bound {
            return Err(format!("the {name} walk's max is {max} µs net of audits, above {bound}"));
        }
    }
    // With `hold-trace`, the longest kernel section a page fault from user mode caused, net of
    // audits: a fault's handling may not cost what the faulting process holds.
    if let Some(bound) = bounds.get("fault_section_max_ticks") {
        let max = sum
            .sections
            .iter()
            .filter(|s| PAGE_FAULTS.contains(&s.cause))
            .map(|s| s.to - s.from - s.audits)
            .max()
            .ok_or("a fault section bound is set, but the trace holds no section a page fault caused")?;
        if max > *bound {
            return Err(format!(
                "a page fault's kernel section held the lock {max} ticks net of audits, above {bound}"
            ));
        }
    }
    if n == 0 && (bounds.contains_key("r10_p99_us") || bounds.contains_key("lease_end_p99_us")) {
        return Err("an R10 bound is set, but the trace holds no destruction".into());
    }
    if let Some(bound) = bounds.get("r10_p99_us").filter(|b| p99 > **b) {
        return Err(format!("R10's p99 is {p99} µs over {n} destructions, above {bound}"));
    }
    // The latency targets are gated on a trace of at most `gate_harts` harts, and recorded above it
    // (kernel/scheduling.md, "Responsiveness").
    let harts = sum.hart_time.map_or(1, |(_, h)| h);
    let gated = bounds.get("gate_harts").is_none_or(|g| harts <= *g);
    // With `hold-trace`, the kernel sections a timer interrupt caused, net of audits: the slice's
    // end, the pick and the return cost what they do, not what the kernel holds. The p99 bounds the
    // steady entry, the max the cold ones (the first of a program's phases). Gated up to
    // `gate_harts` as the latency targets are: under `icount` the harts share one clock, so a
    // section's ticks on several count the other harts' instructions too.
    let mut timer_sections = None;
    let timer_bounds =
        [("p99", bounds.get("timer_section_p99_ticks")), ("max", bounds.get("timer_section_max_ticks"))];
    if timer_bounds.iter().any(|(_, b)| b.is_some()) {
        let mut net: Vec<u64> = sum
            .sections
            .iter()
            .filter(|s| s.cause == TIMER_INTERRUPT)
            .map(|s| s.to - s.from - s.audits)
            .collect();
        let (p50, p99) = (percentile(&mut net, 50), percentile(&mut net, 99));
        let max = *net
            .last()
            .ok_or("a timer section bound is set, but the trace holds no section a timer interrupt caused")?;
        let mut judged = Vec::new();
        for (q, bound) in timer_bounds {
            let Some(bound) = bound else { continue };
            let held = if q == "p99" { p99 } else { max };
            if held > *bound && gated {
                return Err(format!(
                    "timer interrupts' kernel sections: their {q} held the lock {held} ticks net of audits, above {bound}"
                ));
            }
            judged.push(format!("{q} <= {bound}"));
        }
        timer_sections = Some(format!(
            "timer interrupts' kernel sections: {}, ticks net of audits p50/p99/max {p50}/{p99}/{max}: {}{}",
            net.len(),
            judged.join(", "),
            if gated { String::new() } else { format!(", recorded at {harts} harts, not gated") }
        ));
    }
    // Each measure in each group, net of the audits inside its windows.
    let (mut lines, mut missed, mut decision_p99) = (Vec::new(), false, None::<u64>);
    // The old control's cluster envelope targets: whether any of them missed.
    let mut cluster_missed = false;
    for (m, measure) in MEASURES.iter().enumerate() {
        let want = ["p50", "p99"].map(|q| (q, bounds.get(&*format!("{measure}_{q}_us"))));
        let groups: Vec<_> = samples.range((m, "")..(m + 1, "")).filter(|(_, w)| !w.is_empty()).collect();
        if groups.is_empty() && want.iter().any(|(_, b)| b.is_some()) {
            return Err(format!("a {measure} bound is set, but the log holds no {measure} sample"));
        }
        for ((_, group), windows) in groups {
            if control_failed && *group == "cluster" {
                continue;
            }
            let cluster_metrics = if *group == "cluster" {
                let proof = cluster_proof.as_ref().ok_or("cluster samples without cluster check")?;
                Some(
                    proof
                        .metrics
                        .get(m)
                        .ok_or_else(|| format!("cluster {measure} samples: not a stand-in measure"))?,
                )
            } else {
                None
            };
            let mut gross: Vec<u64> = if let Some(metrics) = cluster_metrics {
                metrics.iter().map(|x| x.gross).collect()
            } else {
                windows.iter().map(|(_, g)| *g).collect()
            };
            let inside: Vec<u64> = if let Some(metrics) = cluster_metrics {
                metrics.iter().map(|x| x.credit).collect()
            } else {
                windows.iter().map(|(end, g)| audit_inside(&sum.audits, end - g, *end)).collect()
            };
            let mut net: Vec<u64> = gross.iter().zip(&inside).map(|(g, a)| g - a).collect();
            let stats =
                |v: &mut Vec<u64>| (percentile(v, 50), percentile(v, 99), v.last().copied().unwrap_or(0));
            let ((g50, g99, gmax), (n50, n99, nmax)) = (stats(&mut gross), stats(&mut net));
            let judged: Vec<(String, bool)> = want
                .iter()
                .filter_map(|(q, b)| {
                    b.map(|b| (format!("{q} <= {b}"), if *q == "p50" { n50 } else { n99 } <= *b))
                })
                .collect();
            let met = judged.iter().all(|(_, ok)| *ok);
            if cluster_old_control && *group == "cluster" {
                cluster_missed |= !met;
            } else {
                missed |= !met && gated;
            }
            if *measure == "decision_wake" {
                decision_p99 = Some(decision_p99.unwrap_or(0).max(n99));
            }
            let target = if judged.is_empty() {
                "no target".to_string()
            } else {
                let texts: Vec<&str> = judged.iter().map(|(text, _)| text.as_str()).collect();
                let recorded =
                    if gated { String::new() } else { format!(", recorded at {harts} harts, not gated") };
                format!("target {} ({}){recorded}", if met { "met" } else { "missed" }, texts.join(", "))
            };
            // Where the clock is the host's, the driver wake's p99 in units of one search of the
            // same run, alone: the host's speed moves both (kernel/scheduling.md, R78). Gated at any
            // hart count, beside targets `gate_harts` may only record.
            let mut target = target;
            for (q, n) in [("p50", n50), ("p99", n99)] {
                let Some(k) =
                    bounds.get(&*format!("driver_wake_{q}_searches")).filter(|_| *measure == "driver_wake")
                else {
                    continue;
                };
                let alone = search_alone(log).ok_or_else(|| {
                    format!("driver_wake_{q}_searches is set, but the log holds no 'one search alone' line")
                })?;
                let ok = n <= k * alone;
                missed |= !ok;
                target = format!(
                    "{target}; {} ({q} <= {k} searches of {alone} µs alone, {:.1})",
                    if ok { "searches met" } else { "searches missed" },
                    n as f64 / alone.max(1) as f64
                );
            }
            // A cluster window is the conservative envelope, net of certified audit interiors.
            let (kind, credit) = if cluster_metrics.is_some() {
                ("envelope ", "certified audit credit")
            } else {
                ("", "audits")
            };
            lines.push(format!(
                "{group} {measure} ({}): {kind}net p50/p99/max {n50}/{n99}/{nmax} µs, {kind}gross {g50}/{g99}/{gmax}, {credit} {} µs: {target}",
                gross.len(),
                inside.iter().sum::<u64>()
            ));
        }
    }
    // Each share of the charged CPU, of the kernel's charges alone.
    for s in charged_shares(log)? {
        let (line, met) = judged_at(check_charged_share(&records, &s)?, s.at, harts);
        missed |= !met;
        lines.push(line);
    }
    // Each share across harts, of the kernel's charges net of lock waits.
    let hart_shares = hart_shares(log)?;
    if let Some(name) = stale_in.iter().find(|n| !hart_shares.iter().any(|s| s.name == **n)) {
        return Err(format!("stale_waits_in names {name}, but the log holds no such share"));
    }
    for s in hart_shares {
        let (line, met) = judged_at(check_hart_share(&records, &s)?, s.at, harts);
        let inside = |t: &&u64| (s.window.0..s.window.1).contains(*t);
        let stale = sum.timer_stale_foreign.iter().filter(inside).count();
        let stale_missed = stale_in.contains(&s.name) && stale == 0;
        missed |= !met || stale_missed;
        lines.push(format!(
            "{line}; {} timer interrupts nobody's, {stale} finding another budget's wait ended early{}",
            sum.timer_empty.iter().filter(inside).count(),
            if stale_missed { " (none, but the case requires some)" } else { "" }
        ));
    }
    let mut lease_end = String::new();
    if let Some(bound) = bounds.get("lease_end_p99_us") {
        let wake =
            decision_p99.ok_or("lease_end_p99_us is set, but the log holds no decision_wake sample")?;
        missed |= wake + p99 > *bound && gated;
        lease_end = format!(
            "; lease end p99, net decision wake {wake} + R10 {p99} = {} µs: target {} (<= {bound}){}",
            wake + p99,
            if wake + p99 <= *bound { "met" } else { "missed" },
            if gated { String::new() } else { format!(", recorded at {harts} harts, not gated") }
        );
    }
    let audit_total: u64 = sum.audits.iter().map(|(b, e)| e - b).sum();
    let kernel_time = records.last().filter(|r| r.kind == 'C').map_or(String::new(), |r| {
        format!(
            "; {}{}",
            nobody(r),
            sum.hart_time
                .map_or(String::new(), |t| format!("; {}", lock_waits(&sum.lock_waits, sum.excused, t)))
        )
    });
    let kernel_time = if sum.zeroed_frames > 0 {
        format!(
            "{kernel_time}; {} frames zeroed outside the lock in {} ticks, billed to their payers ({} of destroyed payers lifted)",
            sum.zeroed_frames, sum.zeroed, sum.zeroing_lifted
        )
    } else {
        kernel_time
    };
    let head = format!(
        "sched_oracle: {} records, {} picks in rank order, every wake at or above the floor, no pass falling but by a weight change; {} passing over a budget whose threads all ran on other harts; {} lifted out of the cap set, at most to the floor; {} lifts by the rule ({} with a leading parent and work to move); {} weight changes by the rule; R10 {n} destructions over up to {} object frames, µs p50/p99/max {p50}/{p99}/{max}, their threads' ending {t50}/{t99}/{tmax}, no audit inside one; {} audits, {audit_total} µs; {} timer interrupts billed by the rule ({} expiring, {} finding a wait ended early, {} ticks after expiry to the budget billed last; {} ending a slice, {} nobody's; {} charging another budget than the one interrupted, {} ticks); {} device interrupts from user mode billed by the rule ({} claiming nothing){kernel_time}{lease_end}",
        records.len(),
        sum.picks,
        sum.passed_over,
        sum.uncaps,
        sum.lifts,
        sum.telling,
        sum.reweighs,
        sum.r10_frames,
        sum.audits.len(),
        sum.timer_entries,
        sum.timer_expiring,
        sum.timer_stale,
        sum.timer_tail_ticks,
        sum.timer_slice_ends,
        sum.timer_empty.len(),
        sum.timer_foreign,
        sum.timer_foreign_ticks,
        sum.external_entries,
        sum.external_empty
    );
    let mut report = Vec::new();
    if let Some(proof) = wake_proof {
        report.push(proof);
    }
    if let Some(line) = control_classified {
        report.push(line);
    }
    if let Some(proof) = cluster_proof {
        if cluster_old_control {
            let lower: Vec<u64> = proof.metrics[0].iter().map(|m| m.lower_witness).collect();
            let b50 = bounds.get("driver_wake_p50_us").ok_or("old control driver p50 target missing")?;
            let b99 = bounds.get("driver_wake_p99_us").ok_or("old control driver p99 target missing")?;
            report.push(cluster_old_control_verdict(
                cluster_missed,
                lower,
                (*b50, *b99),
                &proof.positive_ahead,
            )?);
        }
        report.push(proof.report);
    }
    if let Some(proof) = carve_proof {
        report.push(proof);
    }
    if let Some(proof) = round_proof {
        report.push(proof);
    }
    if let Some(proof) = lift_delay_proof {
        report.push(proof);
    }
    for (name, (net, audits)) in WALKS.iter().zip(walks) {
        let mut net = net;
        if net.is_empty() {
            continue;
        }
        let n = net.len();
        let (p50, p99) = (percentile(&mut net, 50), percentile(&mut net, 99));
        report.push(format!(
            "{name} {n}, {p50}/{p99}/{}, audits {audits} µs",
            net.last().copied().unwrap_or(0)
        ));
    }
    if !report.is_empty() {
        let inside: Vec<String> = sum.r10_pumps.iter().map(|(n, us)| format!("{n} ({us} µs)")).collect();
        lines.push(format!(
            "walks, net of audits, µs p50/p99/max: {}; pumps inside each destruction: {}",
            report.join("; "),
            inside.join(", ")
        ));
    }
    if !sum.lock_tickets.is_empty() {
        let harts = sum.hart_time.ok_or("lock wait tickets, but no record of the harts (F)")?.1;
        lines.push(lock_order(&sum.lock_tickets, &sum.lock_waits, harts)?);
    }
    if !sum.sections.is_empty() {
        lines.push(sections(&sum.sections, &sum.lock_waits));
    }
    lines.extend(timer_sections);
    let out = std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n      ");
    if missed { Err(out) } else { Ok(out) }
}

/// A shootdown record's why for a page made executable (`S`, its pass field's bits 32 and up).
const SHOT_FETCH: u128 = 2;

/// `post_check = "smp_fence"` (kernel/memory.md, "Instruction fetch after mapping"): the trace
/// holds a shootdown for a page made executable that asked a hart other than its own, and every
/// hart it asked acknowledged, having run `fence.i`. With none, no hart running the process was
/// fenced, and it fails. It reads the shootdown records alone: the rank checks judge one-hart
/// runs.
pub fn fence(log: &str) -> Result<String, String> {
    let records = parse(log)?;
    let fetches: Vec<&Record> =
        records.iter().filter(|r| r.kind == 'S' && r.pass >> 32 == SHOT_FETCH).collect();
    let fenced = fetches
        .iter()
        .filter(|r| {
            let (asked, acked) = (r.pass & 0xffff, r.pass >> 16 & 0xffff);
            asked != 0 && acked == asked && asked & 1 << r.hart == 0
        })
        .count();
    if fenced == 0 {
        return Err(format!(
            "no shootdown for a page made executable fenced another hart ({} such records)",
            fetches.len()
        ));
    }
    Ok(format!(
        "{fenced} shootdown(s) for a page made executable fenced every other hart running the process"
    ))
}

/// `post_check = "smp_inflight"` (kernel/memory.md, R81): every RAM frame's way through the
/// in-flight state, from an `inflight-trace` kernel's records, by frame. A frame retired (`i`) is
/// pending (`d`) only on the hart that retired it, and, if an entry for it was cleared in a
/// process's space (the PID in its pass field), only after that hart shot the process down (`s`);
/// it is given back to the bitmap (`b`) only once pending, after which its hart zeroed it; and it
/// is never taken (`o`) between its retiring and its giving back. The run must race: frames
/// retired from a live process's space, given back and taken again.
pub fn inflight(log: &str) -> Result<String, String> {
    /// Where a frame is: retired by `hart` from `pid`'s space (0 for none), shot down since if
    /// `shot`; or pending.
    #[derive(Clone, Copy)]
    enum Flight {
        Retired { hart: u64, pid: u64, shot: bool },
        Pending,
    }
    let records = parse(log)?;
    let mut flying: BTreeMap<u64, Flight> = BTreeMap::new();
    let mut given_back = BTreeSet::new();
    let (mut retired, mut unmapped, mut back, mut taken, mut reused, mut most) = (0, 0, 0, 0, 0, 0);
    for r in &records {
        let frame = r.id;
        match r.kind {
            'i' => {
                if flying.contains_key(&frame) {
                    return Err(format!("record {}: frame {frame} retired while in flight", r.seq));
                }
                let pid = r.pass as u64;
                flying.insert(frame, Flight::Retired { hart: r.hart, pid, shot: pid == 0 });
                retired += 1;
                unmapped += usize::from(pid != 0);
                most = most.max(flying.len());
            }
            's' => {
                for f in flying.values_mut() {
                    if let Flight::Retired { hart, pid, shot } = f {
                        if *hart == r.hart && *pid == r.id {
                            *shot = true;
                        }
                    }
                }
            }
            'd' => match flying.get(&frame) {
                Some(Flight::Retired { hart, pid, shot }) => {
                    if *hart != r.hart {
                        return Err(format!(
                            "record {}: frame {frame} pending on hart {}, retired on hart {hart}",
                            r.seq, r.hart
                        ));
                    }
                    if !shot {
                        return Err(format!(
                            "record {}: frame {frame} pending before PID {pid}, whose entry for it was cleared, was shot down",
                            r.seq
                        ));
                    }
                    flying.insert(frame, Flight::Pending);
                }
                _ => return Err(format!("record {}: frame {frame} pending, but not retired", r.seq)),
            },
            'b' => {
                if !matches!(flying.remove(&frame), Some(Flight::Pending)) {
                    return Err(format!(
                        "record {}: frame {frame} given back to the bitmap before it was pending and zeroed",
                        r.seq
                    ));
                }
                given_back.insert(frame);
                back += 1;
            }
            'o' => {
                if flying.contains_key(&frame) {
                    return Err(format!(
                        "record {}: frame {frame} taken from the bitmap while in flight",
                        r.seq
                    ));
                }
                taken += 1;
                reused += usize::from(given_back.remove(&frame));
            }
            _ => {}
        }
    }
    if unmapped == 0 || back == 0 || reused == 0 {
        return Err(format!(
            "nothing raced: {retired} frames retired ({unmapped} from a live space), {back} given back, {reused} taken again"
        ));
    }
    Ok(format!(
        "R81: {retired} frames retired ({unmapped} from a live space, pending only after its shootdown), at most {most} in flight; {back} given back only once pending; {taken} taken, none in flight, {reused} of them given back before; {} still in flight at the end",
        flying.len()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_inflight_check_follows_each_frame_from_its_retiring_to_its_taking() {
        // Frame 7 unmapped from PID 5 on hart 1, shot down, pending, given back, taken again.
        let good = [('i', 7, 5, 1), ('s', 5, 0, 1), ('d', 7, 0, 1), ('b', 7, 0, 0), ('o', 7, 0, 0)];
        assert!(inflight(&shot_trace(&good)).is_ok());
        // Pending before the shootdown (`inflight-zero-unshot`).
        let unshot = [('i', 7, 5, 1), ('d', 7, 0, 1), ('s', 5, 0, 1), ('b', 7, 0, 0), ('o', 7, 0, 0)];
        assert!(inflight(&shot_trace(&unshot)).is_err_and(|e| e.contains("before PID 5")));
        // Given back unzeroed (`inflight-early-commit`).
        let early = [('i', 7, 5, 1), ('b', 7, 0, 1), ('o', 7, 0, 0)];
        assert!(inflight(&shot_trace(&early)).is_err_and(|e| e.contains("before it was pending")));
        // Taken while in flight.
        let taken = [('i', 7, 5, 1), ('s', 5, 0, 1), ('d', 7, 0, 1), ('o', 7, 0, 0)];
        assert!(
            inflight(&shot_trace(&taken)).is_err_and(|e| e.contains("taken from the bitmap while in flight"))
        );
        // Pending on another hart, or never retired.
        let moved = [('i', 7, 0, 1), ('d', 7, 0, 0)];
        assert!(inflight(&shot_trace(&moved)).is_err_and(|e| e.contains("retired on hart 1")));
        assert!(inflight(&shot_trace(&[('d', 7, 0, 0)])).is_err_and(|e| e.contains("not retired")));
        // A shootdown of another process, or on another hart, does not count.
        let other = [('i', 7, 5, 1), ('s', 6, 0, 1), ('s', 5, 0, 0), ('d', 7, 0, 1)];
        assert!(inflight(&shot_trace(&other)).is_err_and(|e| e.contains("before PID 5")));
        // No race at all.
        let idle = [('i', 7, 0, 1), ('d', 7, 0, 1), ('b', 7, 0, 0)];
        assert!(inflight(&shot_trace(&idle)).is_err_and(|e| e.contains("nothing raced")));
    }

    /// A trace of `records` (kind, id, pass, hart), in one kernel entry, with its end line.
    fn shot_trace(records: &[(char, u64, u128, u64)]) -> String {
        let mut log = String::new();
        for (seq, (kind, id, pass, hart)) in records.iter().enumerate() {
            log += &format!("SCHED-TRACE {seq} 1 {kind} {id} {pass:x} {hart}\n");
        }
        log + &format!("SCHED-TRACE-END {} dropped 0\n", records.len())
    }

    #[test]
    fn the_fence_check_needs_an_acknowledged_shootdown_of_another_hart_for_a_page_made_executable() {
        let fetch = |asked: u128, acked: u128| SHOT_FETCH << 32 | acked << 16 | asked;
        // Hart 0 shot PID 5 down on hart 1, which acknowledged.
        assert!(fence(&shot_trace(&[('K', 9, 1, 0), ('S', 5, fetch(0b10, 0b10), 0)])).is_ok());
        // None at all: the recorded negative.
        assert!(fence(&shot_trace(&[('K', 9, 1, 0)])).is_err());
        // A removal's shootdown, or an ending's, is not a fence for new code.
        assert!(fence(&shot_trace(&[('S', 5, 1 << 32 | 0b10 << 16 | 0b10, 0)])).is_err());
        assert!(fence(&shot_trace(&[('S', 5, 3 << 32 | 0b10 << 16 | 0b10, 0)])).is_err());
        // Not acknowledged by every hart asked, or asking only its own hart.
        assert!(fence(&shot_trace(&[('S', 5, fetch(0b110, 0b010), 0)])).is_err());
        assert!(fence(&shot_trace(&[('S', 5, fetch(0b1, 0b1), 0)])).is_err());
    }

    #[test]
    fn a_record_without_its_hart_still_parses() {
        let old = "SCHED-TRACE 0 1 K 9 1\nSCHED-TRACE-END 1 dropped 0\n";
        assert_eq!(parse(old).unwrap()[0].hart, 0);
        assert_eq!(parse(&shot_trace(&[('K', 9, 1, 3)])).unwrap()[0].hart, 3);
    }

    #[test]
    fn cluster_plan_rejects_the_old_construction() {
        let mut setup = String::from("[cluster] calibrated: 10 ticks/us, 1 iterations/ms\n");
        for i in 0..19 {
            setup.push_str(&format!("CLUSTER-READY {i}\n"));
        }
        setup.push_str("CLUSTER-WINDOW 950000 1000000 17000000\n");
        let new = format!("{setup}CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000\n");
        assert!(cluster_plan(&new).is_ok());
        let old = format!("{setup}CLUSTER-PLAN v2-relative 100 300 600 850 200 80000 50000\n");
        assert!(cluster_plan(&old).is_err());
        assert!(
            cluster_plan(
                &(new.clone() + "CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000\n")
            )
            .is_err()
        );
    }

    /// Two headers, then 200 consistent attempts per stand-in: release R = 1,000,000 µs, fixed end
    /// F = 17,000,000 µs. Zero intent arms 20 µs before its slot, positive intent 2 µs after it.
    fn cluster_fixture() -> Vec<String> {
        let mut lines = vec![
            "CLUSTER-HEADER driver_wake 950000 17000000".to_string(),
            "CLUSTER-HEADER timer_wake 950000 17000000".to_string(),
        ];
        for (measure, driver) in [("driver_wake", true), ("timer_wake", false)] {
            for i in 0..200u64 {
                let phase = [100, 300, 600, 850][i as usize % 4];
                let positive = i / 4 % 2 == 1;
                let at = 1_000_000 + i * 80_000;
                let before = if positive { at + 2 } else { at - 20 };
                let delay = if positive { phase } else { phase + 20 };
                let lower = before + delay;
                let observed = lower + 10;
                let (early, arm, deadline, service, rtc_gross, clock) = if driver {
                    let arm = 50_000_000_000 + i * 80_000_000;
                    (before, arm, arm + delay * 1_000, arm + delay * 1_000 + 10_000, 10, "rtc_ns")
                } else {
                    (0, 0, 0, 0, 0, "no_rtc")
                };
                lines.push(format!(
                    "CLUSTER-SAMPLE {measure} {i} {} {} {} {before} {delay} {lower} {early} {observed} {} {arm} {deadline} {service} {rtc_gross} {clock}",
                    usize::from(positive),
                    i * 80_000,
                    before as i64 - at as i64,
                    observed + 1
                ));
            }
        }
        lines
    }

    /// `lines` with fields of line `at` replaced, numbered as the parser numbers them (0 is the
    /// measure, after the `CLUSTER-SAMPLE` tag).
    fn edited(lines: &[String], at: usize, fields: &[(usize, String)]) -> Vec<String> {
        let mut out = lines.to_vec();
        let mut f: Vec<String> = out[at].split_whitespace().map(str::to_owned).collect();
        for (i, v) in fields {
            f[*i + 1] = v.clone();
        }
        out[at] = f.join(" ");
        out
    }

    fn metadata_of(lines: &[String]) -> Result<[Vec<ClusterSample>; 2], String> {
        cluster_metadata(&lines.join("\n"), 950_000, 1_000_000, 17_000_000)
    }

    #[test]
    fn cluster_metadata_rejects_bad_records_and_bounds() {
        let lines = cluster_fixture();
        assert!(metadata_of(&lines).is_ok());
        let mut missing = lines.clone();
        missing.remove(4);
        assert!(metadata_of(&missing).is_err());
        let mut reordered = lines.clone();
        reordered.swap(4, 5);
        assert!(metadata_of(&reordered).is_err());
        let mut duplicate = lines.clone();
        duplicate.insert(2, duplicate[2].clone());
        assert!(metadata_of(&duplicate).is_err());
        // Line 2 is driver attempt 0: each field wrong on its own, the units mixed or old.
        for (field, value) in [
            (2, "1"),
            (3, "1"),
            (4, "0"),
            (5, "2000000"),
            (6, "0"),
            (7, "999999"),
            (8, "2000000"),
            (9, "999999"),
            (10, "17000001"),
            (12, "0"),
            (13, "0"),
            (14, "999"),
            (15, "no_rtc"),
            (15, "timer_us"),
        ] {
            assert!(metadata_of(&edited(&lines, 2, &[(field, value.into())])).is_err(), "field {field}");
        }
        // The timer has no E and no RTC: any of them set, or an RTC unit, is rejected.
        let timer0 = 2 + 200;
        for field in [8, 11, 12, 13, 14] {
            assert!(metadata_of(&edited(&lines, timer0, &[(field, "1".into())])).is_err(), "timer {field}");
        }
        assert!(metadata_of(&edited(&lines, timer0, &[(15, "rtc_ns".into())])).is_err());
        // Plan and stand-in window headers must match, once each, before the samples.
        let mut header = lines.clone();
        header[0] = "CLUSTER-HEADER driver_wake 950000 17000001".into();
        assert!(metadata_of(&header).is_err());
        let mut header = lines.clone();
        header.push(lines[0].clone());
        assert!(metadata_of(&header).is_err());
        let mut late_header = lines.clone();
        let h = late_header.remove(0);
        late_header.push(h);
        assert!(metadata_of(&late_header).is_err());
    }

    #[test]
    fn cluster_metadata_rejects_arming_and_containment_failures() {
        let lines = cluster_fixture();
        let n = |v: u64| v.to_string();
        // Zero intent, attempt 0 (slot 1,000,000, phase 100): a target already passed, or reached.
        let late = |b: u64, delay: u64| {
            // a = 50,000,000,000 ns and s = a + 130,000 ns, as in the fixture; d follows the delay.
            let d = 1_000 * delay;
            let f = [
                (4, n(b - 1_000_000)),
                (5, n(b)),
                (6, n(delay)),
                (7, n(b + delay)),
                (8, n(b)),
                (12, n(50_000_000_000 + d)),
                (14, n((130_000 - d) / 1_000)),
            ];
            edited(&lines, 2, &f)
        };
        assert!(metadata_of(&late(1_000_099, 1)).is_ok());
        assert!(metadata_of(&late(1_000_101, 1)).is_err());
        let reached = late(1_000_100, 0);
        assert!(metadata_of(&reached).is_err());
        // Positive intent, attempt 4 (line 6, slot 1,320,000): B 5 µs after the slot is accepted,
        // B 1 µs before it is not, the rest kept consistent.
        let slot = 1_320_000;
        let positive = |b: u64| {
            let mut f = vec![(5, n(b)), (7, n(b + 100)), (8, n(b))];
            f.push((4, (b as i64 - slot as i64).to_string()));
            edited(&lines, 6, &f)
        };
        assert!(metadata_of(&positive(slot + 5)).is_ok());
        assert!(metadata_of(&positive(slot - 1)).is_err());
        // L above P, and E before B, E after P.
        assert!(metadata_of(&edited(&lines, 6, &[(9, n(slot + 101)), (10, n(slot + 102))])).is_err());
        assert!(metadata_of(&edited(&lines, 6, &[(8, n(slot + 1))])).is_err());
        assert!(metadata_of(&edited(&lines, 6, &[(8, n(slot + 113))])).is_err());
        // The last attempt: P = F - 1 gives U = F and is inside; P = F gives U > F and is not.
        let last = 2 + 199;
        assert!(metadata_of(&edited(&lines, last, &[(9, n(16_999_999)), (10, n(17_000_000))])).is_ok());
        assert!(metadata_of(&edited(&lines, last, &[(9, n(17_000_000)), (10, n(17_000_001))])).is_err());
        // d - a must be the delay in ns, and s >= d.
        let wrong_d = edited(&lines, 2, &[(12, n(50_000_000_000 + 120_001))]);
        assert!(metadata_of(&wrong_d).is_err());
        let early_s = edited(&lines, 2, &[(13, n(50_000_000_000 + 119_999)), (14, n(0))]);
        assert!(metadata_of(&early_s).is_err());
        // Overflow anywhere is a rejection, not a wrap: d, U and L = B + delta.
        let max = u64::MAX;
        assert!(metadata_of(&edited(&lines, 2, &[(11, n(max - 5))])).is_err());
        assert!(metadata_of(&edited(&lines, 2, &[(9, n(max)), (10, n(0))])).is_err());
        assert!(metadata_of(&edited(&lines, 2, &[(5, n(max))])).is_err());
    }

    #[test]
    fn cluster_metadata_rejects_a_preparation_slip_it_cannot_sign() {
        // A window just below 2^63 µs, so every slot is a signed number and nothing else
        // overflows. Driver attempts 0 to 4; the stand-in counts are short, so a record the
        // parser accepts ends in the count error, and one it refuses in a construction error.
        let release = (1u64 << 63) - 1_000_000;
        let (start, end) = (release - 50_000, release + 16_000_000);
        let log = |last_before: u64| {
            let mut lines = vec![
                format!("CLUSTER-HEADER driver_wake {start} {end}"),
                format!("CLUSTER-HEADER timer_wake {start} {end}"),
            ];
            for i in 0..5u64 {
                let phase = [100, 300, 600, 850][i as usize % 4];
                let positive = i / 4 % 2 == 1;
                let at = release + i * 80_000;
                let before = if i == 4 {
                    last_before
                } else if positive {
                    at + 2
                } else {
                    at - 20
                };
                let delay = if positive { phase } else { phase + 20 };
                let (lower, arm) = (before + delay, 50_000_000_000 + i * 80_000_000);
                let (deadline, observed) = (arm + delay * 1_000, lower + 10);
                lines.push(format!(
                    "CLUSTER-SAMPLE driver_wake {i} {} {} {} {before} {delay} {lower} {before} {observed} {} {arm} {deadline} {} 10 rtc_ns",
                    usize::from(positive),
                    i * 80_000,
                    before.wrapping_sub(at) as i64,
                    observed + 1,
                    deadline + 10_000
                ));
            }
            cluster_metadata(&lines.join("\n"), start, release, end)
        };
        let at = release + 4 * 80_000;
        // B 5 µs after its slot: every field holds, and only the counts are short.
        assert!(log(at + 5).is_err_and(|e| e.contains("expected 200 per stand-in")));
        // B at 2^63: still at or after its slot, L, P and U inside the window, but B - slot has
        // no signed reading, so the record is refused for it.
        assert!(1u64 << 63 > at && (1u64 << 63) + 112 < end);
        assert!(log(1 << 63).is_err_and(|e| e.contains("construction failure")));
    }

    #[test]
    fn cluster_envelope_qualifies_where_the_old_rtc_interval_did_not() {
        // Driver attempt 0 serviced 36,267 µs late on the RTC, with E and P 200 µs after L: the
        // old interval [E - RTC gross, E] starts before release, but [L, U) is inside [R, F].
        let lines = cluster_fixture();
        let (lower, arm, deadline) = (1_000_100u64, 50_000_000_000u64, 50_000_120_000u64);
        let service = deadline + 36_267_192;
        let early = lower + 200;
        let ok = edited(
            &lines,
            2,
            &[
                (8, early.to_string()),
                (9, early.to_string()),
                (10, (early + 1).to_string()),
                (13, service.to_string()),
                (14, "36267".into()),
            ],
        );
        let meta = metadata_of(&ok).unwrap()[0][0];
        assert_eq!((meta.arm, meta.deadline), (arm, deadline));
        assert!(early - 36_267 < 1_000_000);
        assert_eq!(cluster_window_bounds(meta.upper, meta.upper - meta.lower, meta), Ok((lower, early + 1)));
        // A printed window that does not match the envelope, here one reaching before release.
        assert!(cluster_window_bounds(meta.upper, 36_267, meta).is_err());
        assert!(cluster_window_bounds(meta.upper - 1, meta.upper - meta.lower - 1, meta).is_err());
    }

    #[test]
    fn cluster_credit_is_the_certified_interior_only() {
        // Envelope [100, 200). Interiors [91, 120) and [181, 250) credit 20 and 19 at the edges.
        let m = cluster_metric(&[(90, 120), (180, 250)], 100, 200, 0).unwrap();
        assert_eq!((m.gross, m.credit, m.net), (100, 39, 61));
        // Empty and sub-microsecond interiors, and audits wholly outside, credit nothing.
        for audits in [[(99, 100)], [(150, 151)], [(150, 150)], [(10, 50)], [(200, 300)], [(300, 400)]] {
            assert_eq!(cluster_metric(&audits, 100, 200, 0).unwrap().net, 100, "{audits:?}");
        }
        // The uncertain leading bin of a stamp is never credited: never more than `audit_inside`.
        let edge = [(100, 105), (150, 160)];
        let m = cluster_metric(&edge, 100, 200, 0).unwrap();
        assert_eq!(m.credit, 4 + 9);
        assert!(m.credit <= audit_inside(&edge, 100, 200));
        // Invalid audits fail closed: reversed, overflowing, or crediting more than the envelope.
        assert!(cluster_metric(&[(20, 10)], 0, 100, 0).is_err());
        assert!(cluster_metric(&[(u64::MAX, u64::MAX)], 0, 100, 0).is_err());
        assert!(cluster_metric(&[(0, 20), (0, 20)], 10, 15, 0).is_err());
        let huge = [(0, u64::MAX - 1), (0, u64::MAX - 1)];
        assert!(cluster_metric(&huge, 0, u64::MAX, 0).is_err());
        assert!(cluster_metric(&[], 20, 10, 0).is_err());
    }

    #[test]
    fn cluster_lower_witness_counts_the_union_of_outer_bins() {
        // Outer bins [10, 14) and [13, 17) overlap in 13: their union is 7 µs, not 8.
        assert_eq!(cluster_metric(&[(10, 13), (13, 16)], 10, 17, 10).unwrap().lower_witness, 3);
        assert_eq!(cluster_metric(&[(10, 13), (13, 16)], 10, 17, 7).unwrap().lower_witness, 0);
        // Only the part inside the envelope counts; none inside, the RTC gross stands.
        assert_eq!(cluster_metric(&[(5, 12)], 10, 17, 10).unwrap().lower_witness, 7);
        assert_eq!(cluster_metric(&[(30, 40)], 10, 17, 10).unwrap().lower_witness, 10);
    }

    #[test]
    fn the_old_control_must_fail_on_its_lower_witness() {
        let bounds = (15_000, 50_000);
        // The attack shown: 25 positive wakes per stand-in behind eight or more spinners.
        let behind = [vec![12usize; 25], [vec![9usize; 25], vec![1; 30]].concat()];
        let mut lower = vec![10u64; 200];
        // Met every envelope target: no negative control at all.
        assert!(cluster_old_control_verdict(false, lower.clone(), bounds, &behind).is_err());
        // An envelope miss whose lower witness meets both targets is observation, not latency.
        assert!(cluster_old_control_verdict(true, lower.clone(), bounds, &behind).is_err());
        // A lower witness past the p99 target (three of 200 above it), or past the p50 one.
        lower[..3].fill(60_000);
        assert!(cluster_old_control_verdict(true, lower.clone(), bounds, &behind).is_ok());
        assert!(cluster_old_control_verdict(false, lower.clone(), bounds, &behind).is_err());
        assert!(cluster_old_control_verdict(true, vec![20_000; 200], bounds, &behind).is_ok());
        // Vacuous: one stand-in has only 24 positive wakes behind eight or more spinners, however
        // it misses.
        let short = [vec![12usize; 25], [vec![8usize; 24], vec![7; 40]].concat()];
        let vacuous = cluster_old_control_verdict(true, lower, bounds, &short);
        assert!(vacuous.is_err_and(|e| e.contains("vacuous") && e.contains("[25, 24]")));
        assert!(run(&audited(""), "cluster_old_control").is_err_and(|e| e.contains("requires cluster")));
        assert!(
            run(&audited(""), "cluster cluster_old_control cluster_old_control")
                .is_err_and(|e| e.contains("duplicate"))
        );
    }

    /// A control whose driver failed at attempt 1: the plan lines, the failure, and a trace in
    /// which all 19 children start and go, and the driver's attempt 0 is a blocked wait.
    fn control_fixture(fail: &str) -> (String, Vec<Record>) {
        let mut log = String::from("[cluster] calibrated: 10 ticks/us, 1 iterations/ms\n");
        for i in 0..19 {
            log += &format!("CLUSTER-READY {i}\n");
        }
        log += "CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000\n";
        log += "CLUSTER-WINDOW 950000 1000000 17000000\n";
        log += fail;
        let mut records = Vec::new();
        let mut add = |kind: char, id: u64| {
            let at = records.len() as u64;
            records.push(Record { seq: at, entry: at, kind, id, pass: 1, hart: 0 });
        };
        for child in 0..19 {
            let id = 100 + child;
            add('W', id); // isolated spawn
            add('K', id);
            add('D', id); // the window receive blocks
            add('K', 1);
        }
        for child in 0..19 {
            add('K', 1);
            add('W', 100 + child); // go
        }
        for kind in ['K', 'D', 'W', 'K'] {
            add(kind, 117); // the driver's attempt 0
        }
        (log, records)
    }

    #[test]
    fn a_control_that_misses_its_slot_is_classified_net_of_audits() {
        let bounds = BTreeMap::from([("driver_wake_p99_us", 50_000), ("timer_wake_p99_us", 50_000)]);
        let fail = "CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80300 current_us=184488\n";
        let (log, records) = control_fixture(fail);
        // Target R + 80,300, B at R + 184,488: a 104,188 µs miss; an audit stamped 1,100,000 to
        // 1,110,000 certifies 9,999 µs inside it.
        let audits = [(1_100_000, 1_110_000)];
        let ok = cluster_control_classify(&log, &records, &audits, &bounds);
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains(
                "control: stand-in driver_wake could not take attempt 1 (zero intent, offset 300): B exceeded its target by 104188 µs, 9999 µs of audits inside, net 94189 µs > 50000"
            ) && s.contains("stand-in timer_wake reported no failure (its samples are not judged)")),
            "{ok:?}"
        );
        // Net of audits under the target: not a control.
        let heavy = [(1_080_300, 1_150_000)];
        assert!(
            cluster_control_classify(&log, &records, &heavy, &bounds)
                .is_err_and(|e| e.contains("no net miss"))
        );
        // Another kind of failure: another branch, a positive-intent attempt, attempt 0, a target
        // not yet passed, or a target that is not the attempt's.
        for other in [
            "CLUSTER-FAIL role=17 index=1 branch=3 result=0 target_us=80300 current_us=184488\n",
            "CLUSTER-FAIL role=17 index=4 branch=2 result=0 target_us=320100 current_us=400000\n",
            "CLUSTER-FAIL role=17 index=0 branch=2 result=0 target_us=100 current_us=60000\n",
            "CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80300 current_us=80300\n",
            "CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80000 current_us=184488\n",
            "CLUSTER-FAIL role=16 index=1 branch=2 result=0 target_us=80300 current_us=184488\n",
        ] {
            let (log, records) = control_fixture(other);
            assert!(cluster_control_classify(&log, &records, &audits, &bounds).is_err(), "{other}");
        }
        // Attempt 0 must have joined a blocked wait in the trace.
        let mut unjoined = records.clone();
        unjoined.truncate(unjoined.len() - 2);
        assert!(
            cluster_control_classify(&log, &unjoined, &audits, &bounds)
                .is_err_and(|e| e.contains("did not join"))
        );
        // A stand-in's attempt 0 with no service pick before the failure, and a second failure
        // for one stand-in.
        let mut unpicked = records.clone();
        unpicked.pop();
        assert!(
            cluster_control_classify(&log, &unpicked, &audits, &bounds)
                .is_err_and(|e| e.contains("no service pick"))
        );
        let twice = format!("{log}{fail}");
        assert!(cluster_control_classify(&twice, &records, &audits, &bounds).is_err());
        // No trace, no control: the log is refused before any classification.
        let no_trace = run(&log, "cluster cluster_old_control driver_wake_p99_us=50000");
        assert!(no_trace.is_err_and(|e| e.contains("no SCHED-TRACE-END")));
    }

    #[test]
    fn a_control_keeps_the_programs_own_gates() {
        let fail = "CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80300 current_us=184488\n";
        let own = "[cluster] FAIL: stand-in 17 took all 200 real waits\n";
        // Classified: only the failing stand-in's own failure line.
        assert!(cluster_control_guest_gates(&format!("{fail}{own}")).is_ok());
        for other in [
            "[cluster] FAIL: spinner/server 3 ran\n",
            "[cluster] FAIL: child 5 has 2 reports (expected one)\n",
            "[cluster] FAIL: child 9 acknowledged its isolated setup wake\n",
            "[cluster] FAIL: stand-in 18 took all 200 real waits\n",
        ] {
            assert!(cluster_control_guest_gates(&format!("{fail}{own}{other}")).is_err(), "{other}");
        }
        // Unclassified: the program must have passed.
        assert!(cluster_control_guest_gates("SCHED-CLUSTER TEST PASSED\n").is_ok());
        assert!(cluster_control_guest_gates("[cluster] FAIL: spinner/server 3 ran\n").is_err());
        assert!(cluster_control_guest_gates("").is_err());
    }

    #[test]
    fn cluster_go_protocol_distinguishes_readiness_from_window_and_release() {
        fn add(records: &mut Vec<Record>, kind: char, id: u64) -> usize {
            let at = records.len();
            records.push(Record { seq: at as u64, entry: at as u64, kind, id, pass: 1, hart: 0 });
            at
        }
        fn mapped(records: &[Record]) -> Vec<(usize, u64)> {
            let mut seen = BTreeSet::new();
            records
                .iter()
                .enumerate()
                .filter_map(|(at, r)| (r.kind == 'W' && seen.insert(r.id)).then_some((at, r.id)))
                .collect()
        }
        let mut records = Vec::new();
        let mut ready_w = [None; 19];
        let mut window_d = [0usize; 19];
        for child in 0..19 {
            let id = 100 + child as u64;
            add(&mut records, 'W', id); // isolated spawn
            add(&mut records, 'K', id);
            if [0, 5, 17, 18].contains(&child) {
                add(&mut records, 'D', id); // reliable readiness send blocked
                add(&mut records, 'K', 1); // launcher receives the readiness report
                ready_w[child] = Some(add(&mut records, 'W', id));
                add(&mut records, 'K', id);
            }
            window_d[child] = add(&mut records, 'D', id); // window receive always blocks
            add(&mut records, 'K', 1);
        }
        let mut expected_go = [0usize; 19];
        for (child, go) in expected_go.iter_mut().enumerate() {
            add(&mut records, 'K', 1);
            *go = add(&mut records, 'W', 100 + child as u64);
        }
        for child in 1..=16 {
            let id = 100 + child as u64;
            add(&mut records, 'K', id);
            add(&mut records, 'D', id); // common timeout wait
            add(&mut records, 'K', 100);
            add(&mut records, 'W', id);
        }
        let starts = mapped(&records);
        assert_eq!(starts.len(), 19);
        let go = cluster_go_boundaries(&records, &starts).unwrap();
        assert_eq!(go, expected_go);
        let releases = cluster_spinner_releases(&records, &starts, &go).unwrap();
        assert_eq!(releases.len(), 16);
        assert!(releases.values().all(|at| *at > go[18]));

        let mut extra_ready = records.clone();
        extra_ready.insert(window_d[5], Record { kind: 'W', id: 105, ..records[window_d[5]] });
        assert!(cluster_go_boundaries(&extra_ready, &mapped(&extra_ready)).is_err());

        let mut late_ready = records.clone();
        let w = late_ready.remove(ready_w[5].unwrap());
        let next_spawn = late_ready.iter().position(|r| r.kind == 'W' && r.id == 106).unwrap();
        late_ready.insert(next_spawn + 1, w);
        assert!(cluster_go_boundaries(&late_ready, &mapped(&late_ready)).is_err());

        let mut no_window_wait = records.clone();
        no_window_wait.remove(window_d[18]);
        assert!(cluster_go_boundaries(&no_window_wait, &mapped(&no_window_wait)).is_err());

        let mut out_of_order = records.clone();
        out_of_order.swap(go[5], go[6]);
        assert!(cluster_go_boundaries(&out_of_order, &mapped(&out_of_order)).is_err());

        let mut extra_release = records.clone();
        extra_release.insert(go[1] + 1, Record { kind: 'W', id: 101, ..records[go[1]] });
        let starts = mapped(&extra_release);
        let go = cluster_go_boundaries(&extra_release, &starts).unwrap();
        assert!(cluster_spinner_releases(&extra_release, &starts, &go).is_err());
    }

    #[test]
    fn cluster_wait_boundary_rejects_report_substitution_and_extra_wakes() {
        let mut records = Vec::new();
        let push = |kind: char, records: &mut Vec<Record>| {
            records.push(Record {
                seq: records.len() as u64,
                entry: records.len() as u64,
                kind,
                id: 17,
                pass: 1,
                hart: 0,
            });
        };
        push('W', &mut records); // The already validated go wake.
        for _ in 0..200 {
            for kind in ['D', 'W', 'K'] {
                push(kind, &mut records);
            }
        }
        let boundary = records.len();
        push('X', &mut records);
        for kind in ['D', 'W', 'K'] {
            push(kind, &mut records); // A later handover wake cannot fill a missing sample.
        }
        assert_eq!(cluster_waits_before(&records, 17, 0, boundary).unwrap().len(), 200);
        let mut immediate = records.clone();
        immediate.remove(1 + 4 * 3 + 1); // No W for sample 4; later report W remains.
        assert!(cluster_waits_before(&immediate, 17, 0, boundary - 1).is_err());
        let mut extra = records.clone();
        extra.insert(4, Record { seq: 0, entry: 0, kind: 'W', id: 17, pass: 1, hart: 0 });
        assert!(cluster_waits_before(&extra, 17, 0, boundary + 1).is_err());
        let mut reordered = records;
        reordered.swap(1, 2);
        assert!(cluster_waits_before(&reordered, 17, 0, boundary).is_err());
    }

    #[test]
    fn cluster_fences_reject_missing_duplicate_and_wrong_callers() {
        let rec = |kind, id, time| Record { seq: 0, entry: 0, kind, id, pass: time, hart: 0 };
        let good = vec![
            rec('W', 17, 0),
            rec('K', 17, 0),
            rec('X', 101, 100),
            rec('Y', 101, 105),
            rec('K', 18, 0),
            rec('X', 102, 110),
            rec('Y', 102, 115),
        ];
        assert_eq!(cluster_fences(&good, [17, 18], [0, 0]).unwrap().map(|f| f.marker_id), [101, 102]);
        let mut missing = good.clone();
        missing.truncate(5);
        assert!(cluster_fences(&missing, [17, 18], [0, 0]).is_err());
        let mut duplicate = good.clone();
        duplicate.extend([rec('K', 17, 0), rec('X', 103, 120), rec('Y', 103, 125)]);
        assert!(cluster_fences(&duplicate, [17, 18], [0, 0]).is_err());
        let mut wrong_caller = good.clone();
        wrong_caller[4].id = 42;
        assert!(cluster_fences(&wrong_caller, [17, 18], [0, 0]).is_err());
        let mut descheduled = good.clone();
        descheduled.insert(2, rec('R', 17, 0));
        assert!(cluster_fences(&descheduled, [17, 18], [0, 0]).is_err());
        let mut repeated_marker = good;
        repeated_marker[5].id = 101;
        repeated_marker[6].id = 101;
        assert!(cluster_fences(&repeated_marker, [17, 18], [0, 0]).is_err());
    }

    #[test]
    fn timeout_wake_proof_rejects_preemption_and_an_unframed_wake() {
        let events = [
            ('K', 7, 0),
            ('I', 7, 100),
            ('E', 8, 1),
            ('W', 8, 10),
            ('O', 0, 1),
            ('I', 7, 900),
            ('E', 0, 0),
            ('R', 7, 20),
            ('O', 0, 0),
            ('K', 8, 10),
        ];
        let records = |events: &[(char, u64, u128)]| {
            events
                .iter()
                .enumerate()
                .map(|(i, &(kind, id, pass))| Record {
                    seq: i as u64,
                    entry: i as u64,
                    kind,
                    id,
                    pass,
                    hart: 0,
                })
                .collect::<Vec<_>>()
        };
        assert!(check_wake_no_preempt(&records(&events), 1).is_ok());
        let mut preempted = events.to_vec();
        preempted.insert(4, ('K', 8, 10));
        assert!(check_wake_no_preempt(&records(&preempted), 1).is_err());
        let mut unframed = events;
        unframed[3] = ('B', 8, 10);
        assert!(check_wake_no_preempt(&records(&unframed), 1).is_err());
        // Two harts: hart 1 picks and runs spinner 9 between the records of hart 0's proof, which
        // still holds; a pick on hart 0 inside it does not.
        let on = |events: &[(char, u64, u128, u64)]| {
            events
                .iter()
                .enumerate()
                .map(|(i, &(kind, id, pass, hart))| Record {
                    seq: i as u64,
                    entry: i as u64,
                    kind,
                    id,
                    pass,
                    hart,
                })
                .collect::<Vec<_>>()
        };
        let mut two: Vec<_> = events.iter().map(|&(k, id, p)| (k, id, p, 0)).collect();
        for (at, r) in [(5, ('K', 9, 5, 1)), (7, ('I', 9, 500, 1)), (8, ('O', 0, 1, 1))] {
            two.insert(at, r);
        }
        assert!(check_wake_no_preempt(&on(&two), 1).is_ok_and(|s| s.contains("by spinner {7: 1}")));
        two.insert(6, ('K', 9, 5, 0));
        assert!(check_wake_no_preempt(&on(&two), 1).is_err());
        // Several harts: sleeper 8 wakes three times. Mid-slice on hart 0 (spinner 7 resumes),
        // at a call on hart 1 (runner 9 goes on), and at a slice's end on hart 0.
        let harts = [
            ('H', 7, 100, 0),
            ('H', 9, 100, 1),
            ('I', 7, 300, 0),
            ('E', 8, 1, 0),
            ('W', 8, 10, 0),
            ('O', 0, 1, 0),
            ('W', 8, 20, 1),
            ('I', 9, 900, 1),
            ('R', 9, 40, 1),
            ('O', 0, 0, 1),
            ('I', 7, 1000, 0),
            ('E', 8, 1, 0),
            ('W', 8, 30, 0),
            ('R', 7, 50, 0),
            ('O', 0, 0, 0),
        ];
        let judged = check_wake_no_preempt_harts(&on(&harts), 3, 2);
        assert!(
            judged.as_ref().is_ok_and(
                |s| s.contains("1 mid-slice, 1 at another entry") && s.contains("1 at a slice's end")
            ),
            "{judged:?}"
        );
        // Two witnesses are not three.
        assert!(check_wake_no_preempt_harts(&on(&harts), 3, 3).is_err_and(|e| e.contains("fewer than 3")));
        // The call's wake requeues hart 1's runner before its next interrupt: it took the hart.
        let mut took = harts.to_vec();
        took.insert(7, ('R', 9, 25, 1));
        assert!(
            check_wake_no_preempt_harts(&on(&took), 3, 0)
                .is_err_and(|e| e.contains("took hart 1 from budget 9"))
        );
        // The last wake's interrupt came before spinner 7's slice was over (`O` 2): it took the
        // hart; with a destruction in it, a deadline did, which may.
        let mut early = harts.to_vec();
        early[14].2 = 2;
        assert!(
            check_wake_no_preempt_harts(&on(&early), 3, 0)
                .is_err_and(|e| e.contains("took hart 0 from budget 7"))
        );
        early.insert(12, ('X', 11, 0, 0));
        assert!(check_wake_no_preempt_harts(&on(&early), 3, 0).is_ok());
        // The call on hart 1 requeues runner 9 and leaves for `kmain`, which records the wake:
        // it took the hart. Had runner 9 blocked (`D`), nothing ran to preempt.
        let mut left = harts.to_vec();
        left.splice(6..7, [('R', 9, 25, 1), ('H', 0, 0, 1), ('W', 8, 20, 1)]);
        assert!(
            check_wake_no_preempt_harts(&on(&left), 3, 0)
                .is_err_and(|e| e.contains("took hart 1 from budget 9"))
        );
        left[6].0 = 'D';
        assert!(
            check_wake_no_preempt_harts(&on(&left), 3, 0).is_ok_and(|s| s.contains("1 in `kmain`")),
            "{:?}",
            check_wake_no_preempt_harts(&on(&left), 3, 0)
        );
        // No budget woke three times.
        assert!(
            check_wake_no_preempt_harts(&on(&harts), 4, 0).is_err_and(|e| e.contains("at least 4 wakes"))
        );
    }

    #[test]
    fn carve_return_proof_requires_the_same_running_turn() {
        let mut events = vec![('K', 9, 110)];
        for (before, old, new, floor) in [(110, 1000, 1, 100), (210, 1, 1000, 100)] {
            events.extend([
                ('G', 9, before),
                ('g', 9, 0),
                ('v', 9, (old << 32) | new),
                ('f', 9, floor),
                ('N', 9, before),
                ('n', 9, 0),
            ]);
        }
        let records = |events: &[(char, u64, u128)]| {
            events
                .iter()
                .enumerate()
                .map(|(i, &(kind, id, pass))| Record {
                    seq: i as u64,
                    entry: i as u64,
                    kind,
                    id,
                    pass,
                    hart: 0,
                })
                .collect::<Vec<_>>()
        };
        assert!(check_carve_return("CARVE-OBS 900 950 12345\n", &records(&events)).is_ok());
        events.insert(7, ('K', 5, 150));
        assert!(check_carve_return("CARVE-OBS 900 950 12345\n", &records(&events)).is_err());
    }

    fn records_of(events: &[(char, u64)]) -> Vec<Record> {
        events
            .iter()
            .enumerate()
            .map(|(i, &(kind, id))| Record { seq: i as u64, entry: i as u64, kind, id, pass: 0, hart: 0 })
            .collect()
    }

    /// Budgets 2 and 3 run, 4 is destroyed after its runs (not a marker), 9 is the empty marker,
    /// and 7 is made after it, picked after one pick each of 3 and 2.
    fn round_events() -> Vec<(char, u64)> {
        vec![
            ('W', 4),
            ('K', 4),
            ('X', 4),
            ('Y', 4),
            ('K', 2),
            ('X', 9),
            ('Y', 9),
            ('P', 7),
            ('W', 7),
            ('K', 3),
            ('K', 2),
            ('K', 7),
        ]
    }

    #[test]
    fn a_marked_wake_picked_within_one_round_passes() {
        let proof = check_round(&records_of(&round_events())).unwrap();
        assert!(proof.contains("budget 7") && proof.contains("after 2 picks"), "{proof}");
    }

    /// The raw debt, unlifted, leaves the new budget behind second picks of those ahead.
    #[test]
    fn a_budget_picked_twice_before_the_marked_one_fails() {
        let mut events = round_events();
        events.insert(11, ('K', 3));
        let err = check_round(&records_of(&events)).unwrap_err();
        assert!(err.contains("budget 3 was picked twice"), "{err}");
    }

    #[test]
    fn a_round_check_without_its_marker_or_new_budget_fails() {
        let mut events = round_events();
        events.retain(|&(_, id)| id != 9);
        let err = check_round(&records_of(&events)).unwrap_err();
        assert!(err.contains("found 0"), "{err}");
        let mut twice = round_events();
        twice.extend([('X', 11), ('Y', 11)]);
        assert!(check_round(&records_of(&twice)).unwrap_err().contains("found 2"));
        // The first wake after the marker is an old budget's, not the new one's.
        let mut old = round_events();
        old.insert(7, ('W', 2));
        assert!(check_round(&records_of(&old)).unwrap_err().contains("not new"));
        let unpicked: Vec<_> = round_events().into_iter().take(11).collect();
        assert!(check_round(&records_of(&unpicked)).unwrap_err().contains("never picked"));
    }

    /// Phases of a lift delay, the n-th (from 0) with P 10n + 11, C 10n + 15 and S 10n + 17: the
    /// lift gives P a
    /// lead of 90 over the floor (C's 300 at weight 3 into P at 10), a round's step is 10, so S
    /// should wait 9 rounds; budgets 2 and 3 are each picked `picks` times before S. With the
    /// floor at each record.
    fn lift_delay_trace(picks: &[usize]) -> (Vec<Record>, Vec<u128>) {
        let (mut events, mut floor, mut t) = (Vec::new(), Vec::new(), 100u128);
        for (n, &k) in picks.iter().enumerate() {
            let (p, c, s) = (10 * n as u64 + 11, 10 * n as u64 + 15, 10 * n as u64 + 17);
            let lift = [
                ('W', c, t),
                ('K', c, t),
                ('L', p, t),
                ('l', c, t + 300),
                ('e', c, t),
                ('f', 0, t),
                ('r', c, 0),
                ('q', p, 0),
                ('w', 0, 3 << 32 | 10),
                ('A', p, t + 90),
                ('a', p, 0),
                ('W', s, t + 90),
            ];
            let wake = t;
            events.extend(lift);
            floor.extend([wake; 12]);
            for _ in 0..k {
                events.extend([('K', 2, t), ('K', 3, t)]);
                floor.extend([t; 2]);
                t += 10;
            }
            events.push(('K', s, wake + 90));
            floor.push(t);
        }
        let records = events
            .iter()
            .enumerate()
            .map(|(i, &(kind, id, pass))| Record { seq: i as u64, entry: i as u64, kind, id, pass, hart: 0 })
            .collect();
        (records, floor)
    }

    fn phase_lines(n: usize) -> String {
        (1..=n).map(|i| format!("[lift-delay] phase {i}: S ran\n")).collect()
    }

    #[test]
    fn a_sibling_delayed_as_its_parents_lift_predicts_passes() {
        let (records, floor) = lift_delay_trace(&[9]);
        let proof = check_lift_delay(&phase_lines(1), &records, &floor).unwrap();
        assert!(proof.contains("expected 9.0 rounds") && proof.contains("max 9 of one budget"), "{proof}");
    }

    #[test]
    fn a_sibling_held_a_round_past_its_parents_lift_fails() {
        let (records, floor) = lift_delay_trace(&[10]);
        let err = check_lift_delay(&phase_lines(1), &records, &floor).unwrap_err();
        assert!(err.contains("max 10 of one budget") && err.contains("not within a round"), "{err}");
        // A round short of it fails too.
        let (records, floor) = lift_delay_trace(&[8]);
        assert!(check_lift_delay(&phase_lines(1), &records, &floor).is_err());
    }

    #[test]
    fn a_lift_the_floor_had_passed_fails() {
        // S wakes at P's lifted pass, but the floor has reached it.
        let (records, mut floor) = lift_delay_trace(&[9]);
        let w = records.iter().position(|r| r.kind == 'W' && r.id == 17).unwrap();
        floor[w] = records[w].pass;
        let err = check_lift_delay(&phase_lines(1), &records, &floor).unwrap_err();
        assert!(err.contains("the floor had passed the lift"), "{err}");
        // S entered at a floor above P's lifted pass.
        let (mut records, floor) = lift_delay_trace(&[9]);
        records[w].pass += 5;
        assert!(
            check_lift_delay(&phase_lines(1), &records, &floor).unwrap_err().contains("the floor had passed")
        );
        // The floor moved half a round between the lift and S's wake: the lift has faded.
        let (records, mut floor) = lift_delay_trace(&[9]);
        floor[w] += 5;
        let err = check_lift_delay(&phase_lines(1), &records, &floor).unwrap_err();
        assert!(err.contains("moved 0.50 rounds") && err.contains("not fresh"), "{err}");
        floor[w] -= 1;
        assert!(check_lift_delay(&phase_lines(1), &records, &floor).is_ok());
    }

    /// The floor `check()` keeps is recorded at each record, the members of a group at its first's.
    #[test]
    fn the_floor_is_recorded_at_each_record() {
        let records = parse(&trace(&[
            (1, 'W', 1, 100),
            (1, 'W', 2, 120),
            (2, 'K', 1, 100),
            (3, 'R', 1, 130),
            (3, 'G', 2, 120),
            (3, 'g', 2, 0),
            (3, 'v', 2, 1 << 32 | 1),
            (3, 'f', 2, 120),
            (3, 'N', 2, 120),
            (3, 'n', 2, 0),
            (4, 'K', 2, 120),
        ]))
        .unwrap();
        let sum = check(&records).unwrap();
        assert_eq!(sum.floor, [0, 0, 0, 100, 120, 120, 120, 120, 120, 120, 120]);
    }

    #[test]
    fn each_phase_is_reported_on_its_own() {
        let (records, floor) = lift_delay_trace(&[9, 9, 9]);
        let proof = check_lift_delay(&phase_lines(3), &records, &floor).unwrap();
        assert!((1..=3).all(|n| proof.contains(&format!("phase {n}: W=900 w_P=10 expected 9.0"))), "{proof}");
        // One phase late fails the check, and the report names it.
        let (records, floor) = lift_delay_trace(&[9, 11, 9]);
        let err = check_lift_delay(&phase_lines(3), &records, &floor).unwrap_err();
        assert!(
            err.contains("phase 2: W=900 w_P=10 expected 9.0 rounds, observed 22 picks, max 11"),
            "{err}"
        );
        // The phases must number the program's lines.
        let (records, floor) = lift_delay_trace(&[9, 9]);
        assert!(check_lift_delay(&phase_lines(3), &records, &floor).unwrap_err().contains("shows 2 phases"));
    }

    /// A trace from `(entry, kind, id, pass)` records, with its end line.
    fn trace(records: &[(u64, char, u64, u128)]) -> String {
        let mut s = String::from("boot noise\n");
        for (i, (entry, kind, id, pass)) in records.iter().enumerate() {
            s += &format!("SCHED-TRACE {i} {entry} {kind} {id} {pass:x}\n");
        }
        s += &format!("SCHED-TRACE-END {} dropped 0\n", records.len());
        s
    }

    fn verdict(records: &[(u64, char, u64, u128)]) -> Result<String, String> { run(&trace(records), "") }

    /// A trace whose records name the hart that wrote them: `(entry, kind, id, pass, hart)`.
    fn verdict_on(records: &[(u64, char, u64, u128, u64)]) -> Result<String, String> {
        run(&verdict_log(records), "")
    }

    /// The log of `records`, each `(entry, kind, id, pass, hart)`, numbered in order.
    fn verdict_log(records: &[(u64, char, u64, u128, u64)]) -> String {
        let mut s = String::from("boot noise\n");
        for (i, (entry, kind, id, pass, hart)) in records.iter().enumerate() {
            s += &format!("SCHED-TRACE {i} {entry} {kind} {id} {pass:x} {hart}\n");
        }
        s + &format!("SCHED-TRACE-END {} dropped 0\n", records.len())
    }

    /// Across harts a pick passes over a budget ranked ahead only if no thread of it waits for a
    /// hart (`J`) and another hart runs it (`H`); it never picks a budget with no thread waiting.
    #[test]
    fn a_pick_passes_over_only_a_budget_that_other_harts_run() {
        // Budget 5 ranks first; hart 0 runs it, its one thread, so hart 1 takes 6.
        let base = [
            (1, 'W', 5, 0x10, 0),
            (1, 'W', 6, 0x20, 0),
            (1, 'J', 5, 1, 0),
            (1, 'J', 6, 1, 0),
            (1, 'K', 5, 0x10, 0),
            (2, 'J', 5, 0, 0),
            (2, 'H', 5, 0, 0),
        ];
        let ok = verdict_on(&[base.as_slice(), &[(3, 'K', 6, 0x20, 1)]].concat()).unwrap();
        assert!(ok.contains("2 picks in rank order") && ok.contains("1 passing over"), "{ok}");
        // The hart that runs it cannot pass it over itself.
        let own = verdict_on(&[base.as_slice(), &[(3, 'K', 6, 0x20, 0)]].concat()).unwrap_err();
        assert!(own.contains("hart 0 passed over budget 5, which no other hart runs"), "{own}");
        // Nor can any hart once no hart runs it.
        let left = verdict_on(&[base.as_slice(), &[(3, 'H', 0, 0, 0), (3, 'K', 6, 0x20, 1)]].concat());
        assert!(left.unwrap_err().contains("hart 1 passed over budget 5, which no other hart runs"));
        // A thread of it waits again: it is first.
        let waits = verdict_on(&[base.as_slice(), &[(3, 'J', 5, 1, 0), (3, 'K', 6, 0x20, 1)]].concat());
        assert!(waits.unwrap_err().contains("the rank clauses put budget 5"));
        // A budget with no thread waiting is never picked.
        let none = verdict_on(&[base.as_slice(), &[(3, 'K', 5, 0x10, 1)]].concat()).unwrap_err();
        assert!(none.contains("picked budget 5, which has no thread waiting"), "{none}");
    }

    /// A shootdown (`S`) among the queue's records changes no rank: the check passes over it.
    #[test]
    fn a_shootdown_record_is_passed_over() {
        let ok = verdict_on(&[
            (1, 'W', 5, 0x10, 0),
            (1, 'S', 7, 1 << 32 | 0b10 << 16 | 0b10, 0),
            (1, 'K', 5, 0x10, 0),
        ]);
        assert!(ok.is_ok_and(|s| s.contains("1 picks in rank order")));
    }

    /// The waits for the lock (`Q`) are reported as a share of the harts' time (`F`), judging nothing,
    /// with the ticks of other harts' audits they waited through (`y`, just after its hart's `Q`, or
    /// its `k`); a wait that ends before it starts is malformed, and so is an excuse after no wait
    /// of its hart or for more than the wait.
    #[test]
    fn lock_waits_are_reported_per_mille_of_the_harts_time() {
        let ok = verdict_on(&[
            (1, 'W', 5, 0x10, 0),
            (1, 'K', 5, 0x10, 0),
            (2, 'Q', 100, 300, 1),
            (2, 'y', 150, 0, 1),
            (3, 'Q', 400, 500, 0),
            (3, 'k', 7, 1, 0),
            (3, 'y', 20, 0, 0),
            (3, 'F', 1000, 2, 0),
            (300, 'C', 10_300, 9_900, 0),
        ])
        .unwrap();
        assert!(
            ok.contains(
                "; lock waits 150 of 1000 (2 waits, 300 ticks, 170 of them behind other harts' audits and \
                 billed to nobody, over 2 hart(s) x 1000 ticks)"
            ),
            "{ok}"
        );
        let bad = verdict_on(&[(1, 'W', 5, 0x10, 0), (1, 'K', 5, 0x10, 0), (2, 'Q', 300, 100, 1)]);
        assert!(bad.unwrap_err().contains("a lock wait that ends before it starts"));
        let excuse = |records: &[(u64, char, u64, u128, u64)]| verdict_on(records).unwrap_err();
        let w = (1, 'W', 5, 0x10, 0);
        assert!(excuse(&[w, (2, 'y', 10, 0, 1)]).contains("audits excused after no lock wait of its hart"));
        assert!(
            excuse(&[w, (2, 'Q', 100, 300, 0), (2, 'y', 10, 0, 1)])
                .contains("audits excused after no lock wait of its hart")
        );
        assert!(
            excuse(&[w, (2, 'Q', 100, 300, 1), (2, 'y', 201, 0, 1)])
                .contains("a lock wait excused more than it waited")
        );
    }

    /// With `lock-trace`, the waits' tickets (`k`, each just after its hart's `Q`) must rise in the
    /// order the waits took the lock, each drawn behind fewer sections than harts (R78).
    #[test]
    fn lock_waits_take_the_lock_in_ticket_order() {
        let trace = |tickets: [(u64, u128); 2]| {
            verdict_on(&[
                (1, 'W', 5, 0x10, 0),
                (1, 'K', 5, 0x10, 0),
                (2, 'Q', 100, 300, 1),
                (2, 'k', tickets[0].0, tickets[0].1, 1),
                (3, 'Q', 400, 500, 0),
                (3, 'k', tickets[1].0, tickets[1].1, 0),
                (3, 'F', 1000, 2, 0),
                (300, 'C', 10_300, 9_900, 0),
            ])
        };
        let ok = trace([(7, 1), (8, 0)]).unwrap();
        assert!(
            ok.contains(
                "lock order: 2 waits took the kernel lock in ticket order, at most 1 section(s) ahead of \
                 one (2 harts); waits in ticks p50/p99/max 100/200/200"
            ),
            "{ok}"
        );
        // The counter wraps.
        assert!(trace([(u64::from(u32::MAX), 1), (0, 1)]).is_ok());
        assert!(
            trace([(8, 1), (7, 1)]).unwrap_err().contains("on ticket 7, after ticket 8: not in ticket order")
        );
        assert!(trace([(7, 1), (7, 1)]).unwrap_err().contains("not in ticket order"));
        assert!(trace([(7, 2), (8, 0)]).unwrap_err().contains("drawn behind 2 sections, 2 hart(s)"));
        let alone = verdict_on(&[(1, 'W', 5, 0x10, 0), (1, 'K', 5, 0x10, 0), (2, 'k', 7, 0, 1)]);
        assert!(alone.unwrap_err().contains("a lock wait's ticket after no wait of its hart"));
        let other = verdict_on(&[(1, 'W', 5, 0x10, 0), (2, 'Q', 100, 300, 1), (2, 'k', 7, 0, 0)]);
        assert!(other.unwrap_err().contains("a lock wait's ticket after no wait of its hart"));
    }

    /// With `hold-trace` the kernel's sections are reported by cause net of their audits, and each
    /// lock wait is split into what other harts' sections covered (their audits pro rata) and the
    /// time the lock stood free; a section's cause must follow it, on its hart, and its audits fit.
    #[test]
    fn kernel_sections_say_what_the_lock_waits_waited_behind() {
        let ok = verdict_on(&[
            (1, 'W', 5, 0x10, 0),
            (1, 'K', 5, 0x10, 0),
            // Hart 0 holds [100, 300] for budget_destroy (0x115), 100 ticks of it an audit; hart 1
            // waits [150, 350]: 150 behind it, 75 of them its audit, 50 with the lock free.
            (2, 'h', 100, 300, 0),
            (2, 'j', 0x115, 100, 0),
            (2, 'Q', 150, 350, 1),
            (2, 'h', 350, 360, 1),
            (2, 'j', 0x205, 0, 1),
            (3, 'F', 1000, 2, 0),
            (300, 'C', 10_300, 9_900, 0),
        ])
        .unwrap();
        assert!(
            ok.contains(
                "kernel sections: 2 held, ticks net of audits p50/p99/max 10/100/100; longest by cause: \
                 budget_destroy 100 (1, 100 in all), interrupt 5 10 (1, 10 in all); lock waits 200 ticks: \
                 behind other harts' sections 150 (their audits 75), the lock free 50"
            ),
            "{ok}"
        );
        // A page fault's section is bounded net of its audits.
        let fault = |bound: &str| {
            let records = [
                (1, 'W', 5, 0x10, 0),
                (1, 'K', 5, 0x10, 0),
                (2, 'h', 100, 300, 0),
                (2, 'j', 0x30f, 50, 0),
                (2, 'h', 300, 900, 0),
                (2, 'j', 0x115, 0, 0),
            ];
            run(&verdict_log(&records), bound)
        };
        assert!(fault("fault_section_max_ticks=150").is_ok());
        assert!(fault("fault_section_max_ticks=149").is_err_and(|e| {
            e.contains("a page fault's kernel section held the lock 150 ticks net of audits, above 149")
        }));
        let none =
            run(&verdict_log(&[(1, 'W', 5, 0x10, 0), (1, 'K', 5, 0x10, 0)]), "fault_section_max_ticks=1");
        assert!(none.as_ref().is_err_and(|e| e.contains("no section a page fault caused")), "{none:?}");
        // So is a timer interrupt's, and no other cause's: the fault's longer section is not it.
        let timer = |bound: &str| {
            let records = [
                (1, 'W', 5, 0x10, 0),
                (1, 'K', 5, 0x10, 0),
                (2, 'h', 100, 220, 0),
                (2, 'j', 0x205, 20, 0),
                (2, 'h', 300, 900, 0),
                (2, 'j', 0x30f, 0, 0),
            ];
            run(&verdict_log(&records), bound)
        };
        let ok = timer("timer_section_max_ticks=100").unwrap();
        assert!(ok.contains(
            "timer interrupts' kernel sections: 1, ticks net of audits p50/p99/max 100/100/100: max <= 100"
        ));
        assert!(timer("timer_section_max_ticks=99").is_err_and(|e| {
            e.contains("timer interrupts' kernel sections: their max held the lock 100 ticks net of audits, above 99")
        }));
        // The p99 bounds the steady entry: one cold section in a hundred is the max's alone.
        let steady = |bound: &str| {
            let mut records = vec![(1, 'W', 5, 0x10, 0), (1, 'K', 5, 0x10, 0)];
            for i in 0..100u64 {
                let length = if i == 0 { 50 } else { 10 };
                records.push((2, 'h', 1000 * i, u128::from(1000 * i + length), 0));
                records.push((2, 'j', 0x205, 0, 0));
            }
            run(&verdict_log(&records), bound)
        };
        let ok = steady("timer_section_p99_ticks=10 timer_section_max_ticks=50").unwrap();
        assert!(ok.contains("p50/p99/max 10/10/50: p99 <= 10, max <= 50"), "{ok}");
        assert!(
            steady("timer_section_p99_ticks=9").is_err_and(|e| e.contains("their p99 held the lock 10 "))
        );
        assert!(
            steady("timer_section_max_ticks=49").is_err_and(|e| e.contains("their max held the lock 50 "))
        );
        // Above `gate_harts` harts it is recorded, not judged.
        let two = |bound: &str| {
            let records = [
                (1, 'W', 5, 0x10, 0),
                (1, 'K', 5, 0x10, 0),
                (2, 'h', 100, 220, 1),
                (2, 'j', 0x205, 20, 1),
                (3, 'F', 1000, 2, 0),
                (300, 'C', 10_300, 9_900, 0),
            ];
            run(&verdict_log(&records), bound)
        };
        assert!(two("timer_section_max_ticks=99 gate_harts=2").is_err());
        let recorded = two("timer_section_max_ticks=99 gate_harts=1").unwrap();
        assert!(
            recorded.contains("p50/p99/max 100/100/100: max <= 99, recorded at 2 harts, not gated"),
            "{recorded}"
        );
        let none =
            run(&verdict_log(&[(1, 'W', 5, 0x10, 0), (1, 'K', 5, 0x10, 0)]), "timer_section_max_ticks=1");
        assert!(none.as_ref().is_err_and(|e| e.contains("no section a timer interrupt caused")), "{none:?}");
        let orphan = verdict_on(&[(1, 'W', 5, 0x10, 0), (2, 'h', 100, 300, 0), (2, 'j', 0x115, 0, 1)]);
        assert!(orphan.unwrap_err().contains("a kernel section's cause after no section of its hart"));
        let over = verdict_on(&[(1, 'W', 5, 0x10, 0), (2, 'h', 100, 300, 0), (2, 'j', 0x115, 201, 0)]);
        assert!(over.unwrap_err().contains("a kernel section's audits outlast it"));
        let back = verdict_on(&[(1, 'W', 5, 0x10, 0), (2, 'h', 300, 100, 0)]);
        assert!(back.unwrap_err().contains("a kernel section that ends before it starts"));
    }

    /// The kernel's time closes the trace (`C`): the share of it, net of audits, charged to no
    /// budget is reported with the numbers it divides, whatever its entry field, and judges nothing.
    #[test]
    fn the_kernel_time_nobody_paid_is_reported_per_mille() {
        let ok = verdict(&[(1, 'W', 5, 0x10), (1, 'K', 5, 0x10), (300, 'C', 10_300, 9_900)]).unwrap();
        assert!(ok.contains("; nobody 10 of 1000 (kernel 10300 ticks, audits 300, charged 9900)"), "{ok}");
        let none = verdict(&[(1, 'W', 5, 0x10), (1, 'K', 5, 0x10), (0, 'C', 0, 0)]).unwrap();
        assert!(none.contains("; nobody 0 of 1000 (kernel 0 ticks, audits 0, charged 0)"), "{none}");
        let without = verdict(&[(1, 'W', 5, 0x10), (1, 'K', 5, 0x10)]).unwrap();
        assert!(!without.contains("nobody 0 of 1000"), "{without}");
    }

    #[test]
    fn walks_are_reported_net_of_audits_and_never_nested() {
        let ok = verdict(&[
            (1, 'W', 5, 0x10),
            (1, 'K', 5, 0x10),
            (1, 'M', 1, 100),
            (1, 'U', 7, 110),
            (1, 'V', 7, 130),
            (1, 'm', 1, 150),
            (2, 'M', 2, 200),
            (2, 'm', 2, 205),
            (3, 'X', 9, 300),
            (3, 'M', 1, 310),
            (3, 'm', 1, 320),
            (3, 'Y', 9, 400),
        ]);
        let want = "walks, net of audits, µs p50/p99/max: pump 2, 10/30/30, audits 20 µs; expiry 1, 5/5/5, audits 0 µs; \
                    pumps inside each destruction: 1 (10 µs)";
        assert!(ok.as_ref().is_ok_and(|s| s.contains(want)), "{ok:?}");
        for bad in [
            [(1, 'M', 1, 100), (1, 'M', 2, 110)],
            [(1, 'M', 3, 100), (1, 'm', 1, 110)],
            [(1, 'M', 4, 100), (1, 'm', 4, 110)],
        ] {
            let err = verdict(&[[(1, 'W', 5, 0x10), (1, 'K', 5, 0x10)].as_slice(), &bad].concat());
            assert!(err.as_ref().is_err_and(|e| e.contains("walk")), "{err:?}");
        }
    }

    /// A walk's bound is its longest net of audits, judged before R10's: a case whose R10 must
    /// fail still fails on a walk past its bound, with a message of its own.
    #[test]
    fn a_walk_is_bounded_net_of_audits_before_r10() {
        let t = trace(&[
            (1, 'W', 5, 0x10),
            (1, 'K', 5, 0x10),
            (1, 'M', 1, 100),
            (1, 'U', 7, 110),
            (1, 'V', 7, 130),
            (1, 'm', 1, 150),
            (2, 'M', 2, 200),
            (2, 'm', 2, 205),
            (3, 'X', 9, 300),
            (3, 'Y', 9, 400),
        ]);
        assert!(run(&t, "pump_max_us=30 expiry_max_us=5").is_ok());
        let err = run(&t, "pump_max_us=29 r10_p99_us=50");
        assert!(
            err.as_ref().is_err_and(|e| e.contains("pump walk's max is 30 µs") && e.contains("above 29")),
            "{err:?}"
        );
        assert!(run(&t, "pump_max_us=30 r10_p99_us=50").is_err_and(|e| e.contains("R10's p99")));
        assert!(run(&t, "reconcile_max_us=10").is_err_and(|e| e.contains("no reconcile walk")));
        assert!(run(&t, "walk_max_us=10").is_err_and(|e| e.contains("unknown")));
    }

    #[test]
    fn a_trace_that_keeps_every_clause_passes() {
        let good = [
            // Entry 1: 3 and 2 wake at pass 5; the lower id first (clause 3).
            (1, 'W', 3, 5),
            (1, 'W', 2, 5),
            (1, 'K', 2, 5),
            // 2 runs and is requeued above; 4 wakes at 5 in a later entry.
            (1, 'P', 2, 9),
            (1, 'R', 2, 9),
            (2, 'W', 4, 5),
            // Entry 3: 4 (the later entry's wake) before 3 (clause 2).
            (3, 'K', 4, 5),
            (3, 'R', 4, 5),
            // 4, requeued at 5, now ranks behind 3, a waker at the same pass (clause 1).
            (3, 'K', 3, 5),
            (3, 'R', 3, 5),
            // 4 and 3, both requeued at 5: in requeue order (clause 4).
            (3, 'K', 4, 5),
            (3, 'D', 4, 5),
            (3, 'K', 3, 5),
        ];
        assert!(verdict(&good).is_ok(), "{:?}", verdict(&good));
    }

    #[test]
    fn each_broken_clause_is_caught() {
        // Clause 1: a requeued budget picked ahead of a waker at the same pass.
        let c1 = [(1, 'W', 1, 5), (1, 'K', 1, 5), (1, 'R', 1, 5), (2, 'W', 2, 5), (2, 'K', 1, 5)];
        // Clause 2: the earlier entry's waker picked first.
        let c2 = [(1, 'W', 1, 5), (2, 'W', 2, 5), (2, 'K', 1, 5)];
        // Clause 3: within one entry, the higher id picked first.
        let c3 = [(1, 'W', 2, 5), (1, 'W', 1, 5), (1, 'K', 2, 5)];
        // Clause 4: the later requeue picked first.
        let c4 = [
            (1, 'W', 1, 5),
            (1, 'W', 2, 5),
            (1, 'K', 1, 5),
            (1, 'R', 1, 5),
            (1, 'K', 2, 5),
            (1, 'R', 2, 5),
            (1, 'K', 2, 5),
        ];
        // And the pass: a higher pass picked over a lower one.
        let pass = [(1, 'W', 1, 7), (1, 'W', 2, 5), (1, 'K', 1, 7)];
        for (what, t) in [("1", &c1[..]), ("2", &c2), ("3", &c3), ("4", &c4), ("pass", &pass)] {
            let v = verdict(t);
            assert!(v.as_ref().is_err_and(|e| e.contains("rank clauses")), "clause {what}: {v:?}");
        }
    }

    /// A lift group, then a pick so the trace is complete: parent 1 at `pb` rem `pr`, child 2 at
    /// `cp` rem `cr` entry `e`, floor `f`, weights `wc` and `wp`, parent after `pa` rem `par`.
    #[allow(clippy::too_many_arguments)]
    fn lift_trace(
        pb: u128,
        pr: u128,
        cp: u128,
        cr: u128,
        e: u128,
        f: u128,
        wc: u128,
        wp: u128,
        pa: u128,
        par: u128,
    ) -> String {
        trace(&[
            (1, 'L', 1, pb),
            (1, 'l', 2, cp),
            (1, 'e', 2, e),
            (1, 'f', 0, f),
            (1, 'r', 2, cr),
            (1, 'q', 1, pr),
            (1, 'w', 0, wc << 32 | wp),
            (1, 'A', 1, pa),
            (1, 'a', 1, par),
            (1, 'W', 1, pa),
            (1, 'K', 1, pa),
        ])
    }

    /// A weight change for budget 1, then its pass, a wake and a pick so the trace is complete:
    /// pass `pb` rem `rb`, weights `old` to `new`, floor `f`, pass after `pa` rem `ra`.
    fn reweigh_trace(pb: u128, rb: u128, old: u128, new: u128, f: u128, pa: u128, ra: u128) -> String {
        trace(&[
            (1, 'W', 1, pb),
            (1, 'G', 1, pb),
            (1, 'g', 1, rb),
            (1, 'v', 1, old << 32 | new),
            (1, 'f', 1, f),
            (1, 'N', 1, pa),
            (1, 'n', 1, ra),
            (1, 'P', 1, pa),
            (1, 'K', 1, pa),
        ])
    }

    #[test]
    fn weight_changes_are_recomputed() {
        // A carve: lead 20 over floor 100 at weight 10, remainder 3: W = 203; at weight 1, 100 +
        // 203. Its return: back to exactly where it was, the one place a pass falls.
        assert!(run(&reweigh_trace(120, 3, 10, 1, 100, 303, 0), "").is_ok());
        assert!(run(&reweigh_trace(303, 0, 1, 10, 100, 120, 3), "").is_ok());
        // A remainder-only rescale (the old rule) is caught, and so is a pass that falls with no
        // weight change behind it.
        let rem_only = run(&reweigh_trace(303, 0, 1, 10, 100, 303, 0), "");
        assert!(rem_only.as_ref().is_err_and(|e| e.contains("the rule gives")), "{rem_only:?}");
        let fell = run(&trace(&[(1, 'W', 1, 303), (1, 'P', 1, 120), (1, 'K', 1, 120)]), "");
        assert!(fell.as_ref().is_err_and(|e| e.contains("fell")), "{fell:?}");
        // Weight 0 is stated as 1: 10 to 0 holds W = 203 as 100 + 203; 0 to 5 restates it.
        assert!(run(&reweigh_trace(120, 3, 10, 0, 100, 303, 0), "").is_ok());
        assert!(run(&reweigh_trace(303, 0, 0, 5, 100, 140, 3), "").is_ok());
        let dropped = run(&reweigh_trace(120, 3, 10, 0, 100, 120, 0), "");
        assert!(dropped.as_ref().is_err_and(|e| e.contains("the rule gives")), "{dropped:?}");
    }

    #[test]
    fn lifts_are_recomputed() {
        // Parent leads the floor (pb 150 > f 100); the child did 30 over max(entry 90, floor 100)
        // at weight 4, remainder 3: W = 123; parent weight 10: 150 + 12, remainder 5 + 3 = 8.
        assert!(run(&lift_trace(150, 5, 130, 3, 90, 100, 4, 10, 162, 8), "").is_ok());
        // The carry: remainder 9 + 3 = 12 is a pass unit and 2.
        assert!(run(&lift_trace(150, 9, 130, 3, 90, 100, 4, 10, 163, 2), "").is_ok());
        // A parent below the floor starts from the floor, remainder dropped.
        assert!(run(&lift_trace(80, 9, 130, 3, 90, 100, 4, 10, 112, 3), "").is_ok());
        // The same lift by max (the model's R12LiftByMax): max(150, 100 + 12) = 150.
        let by_max = run(&lift_trace(150, 5, 130, 3, 90, 100, 4, 10, 150, 5), "");
        assert!(by_max.as_ref().is_err_and(|e| e.contains("the rule gives")), "{by_max:?}");
        // Measured from the floor, not the entry (R12LiftCountsEntryWait): entry 60 under floor
        // 100 changes nothing, but a child that entered above the floor moves only its own work.
        assert!(run(&lift_trace(150, 0, 130, 0, 120, 100, 4, 10, 154, 0), "").is_ok());
        assert!(run(&lift_trace(150, 0, 130, 0, 120, 100, 4, 10, 162, 0), "").is_err());
        // A group cut short, or a lift record on its own.
        let cut = "SCHED-TRACE 0 1 L 1 5\nSCHED-TRACE 1 1 W 1 5\nSCHED-TRACE 2 1 K 1 5\nSCHED-TRACE-END 3 dropped 0\n";
        assert!(run(cut, "").is_err());
        let stray = "SCHED-TRACE 0 1 a 1 5\nSCHED-TRACE 1 1 W 1 5\nSCHED-TRACE 2 1 K 1 5\nSCHED-TRACE-END 3 dropped 0\n";
        assert!(run(stray, "").is_err());
    }

    #[test]
    fn the_floor_and_the_passes_are_checked_on_their_own() {
        // 1 is queued at 9; 2 wakes at 5, below that floor (a wake that kept its own pass).
        let below = [(1, 'W', 1, 9), (1, 'K', 1, 9), (1, 'R', 1, 9), (2, 'W', 2, 5), (2, 'K', 2, 5)];
        let v = verdict(&below);
        assert!(v.as_ref().is_err_and(|e| e.contains("below the floor")), "{v:?}");
        // Two wake into an empty queue above the floor, one entry: each against the floor as it was
        // (the waker's own raised pass, `P`, moves nothing).
        let together = [
            (1, 'W', 1, 9),
            (1, 'K', 1, 9),
            (1, 'D', 1, 9),
            (2, 'P', 3, 12),
            (2, 'W', 3, 12),
            (2, 'P', 2, 9),
            (2, 'W', 2, 9),
            (2, 'K', 2, 9),
        ];
        assert!(verdict(&together).is_ok(), "{:?}", verdict(&together));
        // The floor holds while the queue is empty: 1 leaves at 9, 2 wakes at 5 later.
        let held = [(1, 'W', 1, 9), (1, 'K', 1, 9), (1, 'D', 1, 9), (2, 'W', 2, 5), (2, 'K', 2, 5)];
        assert!(verdict(&held).is_err());
        // A pass that falls.
        let fell = [(1, 'W', 1, 9), (1, 'K', 1, 9), (1, 'P', 1, 7), (1, 'K', 1, 7)];
        let v = verdict(&fell);
        assert!(v.as_ref().is_err_and(|e| e.contains("fell")), "{v:?}");
    }

    #[test]
    fn destructions_are_timed_and_bounded() {
        let t = trace(&[
            (1, 'X', 7, 100),
            (1, 'Y', 7, 130),
            (1, 'X', 8, 200),
            (1, 'Y', 8, 250),
            (1, 'W', 1, 5),
            (1, 'K', 1, 5),
        ]);
        let ok = run(&t, "r10_p99_us=50");
        assert!(
            ok.as_ref().is_ok_and(
                |s| s.contains("R10 2 destructions over up to 0 object frames, µs p50/p99/max 30/50/50")
            ),
            "{ok:?}"
        );
        assert!(run(&t, "r10_p99_us=49").is_err_and(|e| e.contains("above 49")));
        assert!(run(&t, "r10_p99=49").is_err());
        // A lease's end: the worst net decision-wake p99 over every group, plus R10's p99 (50).
        assert!(run(&t, "lease_end_p99_us=1000").is_err_and(|e| e.contains("no decision_wake sample")));
        let t = t + "LATENCY-SAMPLE N=1 decision_wake 1900 900\nLATENCY-COUNT N=1 decision_wake 1\n\
                     LATENCY-SAMPLE N=16 decision_wake 2950 950\nLATENCY-COUNT N=16 decision_wake 1\n";
        let ok = run(&t, "r10_p99_us=50 lease_end_p99_us=1000");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains("decision wake 950 + R10 50 = 1000 µs: target met")),
            "{ok:?}"
        );
        assert!(run(&t, "lease_end_p99_us=999").is_err_and(|e| e.contains("target missed (<= 999)")));
        // Unpaired or nested brackets.
        assert!(verdict(&[(1, 'Y', 7, 1), (1, 'W', 1, 5), (1, 'K', 1, 5)]).is_err());
        assert!(verdict(&[(1, 'X', 7, 1), (1, 'X', 8, 2), (1, 'W', 1, 5), (1, 'K', 1, 5)]).is_err());
    }

    #[test]
    fn a_destructions_threads_ending_is_timed_inside_it() {
        let pick = [(2, 'W', 1, 5), (2, 'K', 1, 5)];
        // Two process endings inside the first destruction (4 + 6 µs), none in the second, and one
        // outside both, which counts in neither.
        let v = verdict(
            &[
                vec![
                    (1, 'T', 0, 10),
                    (1, 't', 0, 15),
                    (1, 'X', 7, 100),
                    (1, 'T', 0, 104),
                    (1, 't', 0, 108),
                    (1, 'T', 0, 110),
                    (1, 't', 0, 116),
                    (1, 'Y', 7, 130),
                    (1, 'X', 8, 200),
                    (1, 'Y', 8, 250),
                ],
                pick.to_vec(),
            ]
            .concat(),
        );
        assert!(v.as_ref().is_ok_and(|s| s.contains("30/50/50, their threads' ending 0/10/10")), "{v:?}");
        for (what, head) in [
            ("a begin with no end", vec![(1, 'T', 0, 130)]),
            ("an end with no begin", vec![(1, 't', 0, 154)]),
            ("an end before its begin", vec![(1, 'T', 0, 154), (1, 't', 0, 130)]),
            ("nested", vec![(1, 'T', 0, 130), (1, 'T', 0, 131), (1, 't', 0, 132), (1, 't', 0, 133)]),
        ] {
            let v = verdict(&[head, pick.to_vec()].concat());
            assert!(v.as_ref().is_err_and(|e| e.contains("threads span")), "{what}: {v:?}");
        }
    }

    /// A destruction from 100 to 130 µs, an audit from 130 to 154 (after `Y`, as the kernel runs
    /// it) and one from 300 to 308 (at a process object's free), then `samples`.
    fn audited(samples: &str) -> String {
        trace(&[
            (1, 'X', 7, 100),
            (1, 'Y', 7, 130),
            (1, 'U', 1, 130),
            (1, 'V', 1, 154),
            (2, 'U', 2, 300),
            (2, 'V', 2, 308),
            (2, 'W', 1, 5),
            (2, 'K', 1, 5),
        ]) + samples
    }

    /// `gate_harts=N` judges the latency targets on a trace of at most N harts (`F`) and only
    /// reports them on more: the targets are gated at one hart and two and recorded at four.
    #[test]
    fn latency_targets_are_gated_up_to_gate_harts() {
        let on = |harts: u128| {
            trace(&[(1, 'W', 1, 5), (1, 'K', 1, 5), (2, 'F', 1000, harts)])
                + "LATENCY-SAMPLE N=1 driver_wake 160 60\nLATENCY-COUNT N=1 driver_wake 1\n"
        };
        let bounds = "gate_harts=2 driver_wake_p99_us=50";
        let two = run(&on(2), bounds);
        assert!(
            two.as_ref().is_err_and(|e| e.contains("target missed (p99 <= 50)") && !e.contains("not gated")),
            "{two:?}"
        );
        let four = run(&on(4), bounds);
        assert!(
            four.as_ref()
                .is_ok_and(|s| s.contains("target missed (p99 <= 50), recorded at 4 harts, not gated")),
            "{four:?}"
        );
        assert!(run(&on(4), "driver_wake_p99_us=50").is_err());
        assert!(run(&on(1), "gate_harts=x").is_err_and(|e| e.contains("unknown sched_oracle argument")));
    }

    /// `driver_wake_p50_searches=K` and `driver_wake_p99_searches=K` judge the driver wake's net
    /// p50 and p99 against K of the run's own searches alone, read from its `one search alone` line.
    #[test]
    fn the_driver_wake_is_judged_in_searches_of_its_own_run() {
        let log = |p99: u64| {
            trace(&[(1, 'W', 1, 5), (1, 'K', 1, 5)])
                + "[lock-contention] one search alone: p50 100 µs, max 120 µs (21 searches)\n"
                + &format!(
                    "LATENCY-SAMPLE contention driver_wake 1000 {p99}\nLATENCY-COUNT contention driver_wake 1\n"
                )
        };
        let met = run(&log(700), "driver_wake_p99_searches=7");
        assert!(
            met.as_ref().is_ok_and(|s| s.contains("searches met (p99 <= 7 searches of 100 µs alone, 7.0)")),
            "{met:?}"
        );
        let missed = run(&log(701), "driver_wake_p99_searches=7");
        assert!(
            missed
                .as_ref()
                .is_err_and(|e| e.contains("searches missed (p99 <= 7 searches of 100 µs alone, 7.0)")),
            "{missed:?}"
        );
        // `gate_harts` records the absolute targets above its count, never this one.
        let ungated = run(&log(701), "gate_harts=0 driver_wake_p99_us=50 driver_wake_p99_searches=7");
        assert!(ungated.as_ref().is_err_and(|e| e.contains("not gated; searches missed")), "{ungated:?}");
        // The p50 alone: one sample is both its p50 and its p99.
        let p50 = run(&log(301), "driver_wake_p50_searches=3");
        assert!(
            p50.as_ref()
                .is_err_and(|e| e.contains("searches missed (p50 <= 3 searches of 100 µs alone, 3.0)")),
            "{p50:?}"
        );
        let no_line = trace(&[(1, 'W', 1, 5), (1, 'K', 1, 5)])
            + "LATENCY-SAMPLE contention driver_wake 1000 5\nLATENCY-COUNT contention driver_wake 1\n";
        assert!(
            run(&no_line, "driver_wake_p99_searches=7")
                .is_err_and(|e| e.contains("no 'one search alone' line"))
        );
    }

    #[test]
    fn audits_are_subtracted_inside_each_window() {
        let spans = [(130, 154), (300, 308)];
        // Inside: [120, 200] holds the first whole; [120, 400] both.
        assert_eq!(audit_inside(&spans, 120, 200), 24);
        assert_eq!(audit_inside(&spans, 120, 400), 32);
        // Straddling an edge: only the part inside counts.
        assert_eq!(audit_inside(&spans, 140, 200), 14);
        assert_eq!(audit_inside(&spans, 100, 304), 28);
        // None inside, an edge touching one included.
        assert_eq!(audit_inside(&spans, 154, 300), 0);
        assert_eq!(audit_inside(&spans, 0, 100), 0);
        // The deadline notice: a 40 µs target. Its window [100, 160] holds 24 µs of audit (gross 60,
        // net 36); [145, 185] the audit's last 9 (net 31); [200, 235] none (net 35).
        let log = audited(
            "LATENCY-SAMPLE N=1 deadline_notice 160 60\n\
             LATENCY-SAMPLE N=1 deadline_notice 185 40\n\
             LATENCY-SAMPLE N=1 deadline_notice 235 35\n\
             LATENCY-COUNT N=1 deadline_notice 3\n",
        );
        let ok = run(&log, "deadline_notice_p99_us=36");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains(
                "N=1 deadline_notice (3): net p50/p99/max 35/36/36 µs, gross 40/60/60, audits 33 µs: target met (p99 <= 36)"
            ) && s.contains("2 audits, 32 µs")),
            "{ok:?}"
        );
        let missed = run(&log, "deadline_notice_p99_us=35");
        assert!(missed.as_ref().is_err_and(|e| e.contains("target missed (p99 <= 35)")), "{missed:?}");
        // Unsubtracted, the same windows miss by the audit: a stamp the kernel left out is time
        // the oracle never saw.
        let unstamped = trace(&[(1, 'X', 7, 100), (1, 'Y', 7, 130), (2, 'W', 1, 5), (2, 'K', 1, 5)])
            + "LATENCY-SAMPLE N=1 deadline_notice 160 60\nLATENCY-COUNT N=1 deadline_notice 1\n";
        assert!(run(&unstamped, "deadline_notice_p99_us=40").is_err_and(|e| e.contains("target missed")));
        // Groups are judged apart, and each measure's p50 too.
        let log = audited(
            "LATENCY-SAMPLE N=1 driver_wake 160 50\nLATENCY-COUNT N=1 driver_wake 1\n\
             LATENCY-SAMPLE N=16 driver_wake 260 50\nLATENCY-COUNT N=16 driver_wake 1\n",
        );
        let v = run(&log, "driver_wake_p50_us=30 driver_wake_p99_us=50");
        let n1 = "N=1 driver_wake (1): net p50/p99/max 26/26/26 µs, gross 50/50/50, audits 24 µs: target met";
        let n16 =
            "N=16 driver_wake (1): net p50/p99/max 50/50/50 µs, gross 50/50/50, audits 0 µs: target missed";
        assert!(v.as_ref().is_err_and(|e| e.contains(n1) && e.contains(n16)), "{v:?}");
        // A bound with no sample, a malformed sample, an unknown measure.
        assert!(run(&audited(""), "timer_wake_p99_us=5").is_err_and(|e| e.contains("no timer_wake sample")));
        assert!(run(&audited("LATENCY-SAMPLE N=1 timer_wake 5\n"), "").is_err());
        assert!(run(&audited("LATENCY-SAMPLE N=1 timer_wake 5 6\n"), "").is_err());
        assert!(run(&audited("LATENCY-SAMPLE N=1 lunch 9 6\n"), "").is_err());
        // A window lost on the way (the program counted two), a count with no windows, and windows
        // with no count.
        let lost = audited("LATENCY-SAMPLE N=1 timer_wake 9 6\nLATENCY-COUNT N=1 timer_wake 2\n");
        assert!(
            run(&lost, "").is_err_and(|e| e.contains("1 windows printed, but the program counted Some(2)"))
        );
        assert!(run(&audited("LATENCY-COUNT N=1 timer_wake 2\n"), "").is_err());
        assert!(run(&audited("LATENCY-SAMPLE N=1 timer_wake 9 6\n"), "").is_err());
        assert!(run(&audited("LATENCY-COUNT N=1 timer_wake 0\n"), "").is_ok());
        assert!(run(&audited(""), "lunch_p99_us=5").is_err());
    }

    type Rec = (u64, char, u64, u128);

    /// A weight change's group by the rule; the pass after.
    fn push_reweigh(t: &mut Vec<Rec>, e: u64, id: u64, pb: u128, old: u128, new: u128, f: u128) -> u128 {
        let owed = pb.saturating_sub(f) * old.max(1);
        let (pa, ra) = (f + owed / new.max(1), owed % new.max(1));
        for (kind, pass) in [('G', pb), ('g', 0), ('v', old << 32 | new), ('f', f), ('N', pa), ('n', ra)] {
            t.push((e, kind, id, pass));
        }
        pa
    }

    /// A destruction's lift group by the rule (the child entered at 0, no remainders); the
    /// parent's pass after.
    fn push_lift(
        t: &mut Vec<Rec>,
        e: u64,
        (parent, pb): (u64, u128),
        (child, cp): (u64, u128),
        f: u128,
        wc: u128,
        wp: u128,
    ) -> u128 {
        let work = cp.saturating_sub(f) * wc;
        let base = pb.max(f);
        let pa = base + work / wp;
        let group = [
            ('L', parent, pb),
            ('l', child, cp),
            ('e', child, 0),
            ('f', 0, f),
            ('r', child, 0),
            ('q', parent, 0),
            ('w', 0, wc << 32 | wp),
            ('A', parent, pa),
            ('a', parent, work % wp),
        ];
        t.extend(group.map(|(kind, id, pass)| (e, kind, id, pass)));
        pa
    }

    /// The containment gate's steady window as the 1 ms slice runs it: the bystander (44, weight
    /// 100, marked by an empty child of weight 2) beside both slots' leases (55 and 65, under
    /// sessions 50, marked 3), each of free weight `lease` less its sub-agent's 1 (57 and 67). Every
    /// slice is 1,200 µs charged, of which the bystander's counting thread runs 1,000: the rest is
    /// the per-switch kernel time the page states as today's (the program's gross count read 758
    /// of 1000 against a floor of 783 on rv64 seed 13). The picks follow the rank clauses. Halfway
    /// through, the bystander leaves the queue for `away` slices. The log, and the share of the
    /// window the bystander's count stands for (the old clause's).
    fn containment(lease: u128, slices: u64, away: u64, marks: &str) -> (String, u64) {
        const SLICE_US: u64 = 1_200;
        let stride = u128::from(redoubt_stride::STRIDE);
        let mut t: Vec<Rec> = Vec::new();
        // The marks: carved, destroyed and lifted, each parent's weight restated both ways.
        for (parent, mark, w) in [(44, 60, 2), (50, 61, 3)] {
            push_reweigh(&mut t, 1, parent, 0, 100, 100 - w, 0);
            t.push((1, 'X', mark, 10));
            push_reweigh(&mut t, 1, parent, 0, 100 - w, 100, 0);
            push_lift(&mut t, 1, (parent, 0), (mark, 0), 0, w, 100);
            t.push((1, 'Y', mark, 11));
        }
        // Sessions carves each slot's lease; each lease carves its sub-agent.
        let slots = [(55u64, 57u64), (65, 67)];
        let mut sessions = 0;
        for (i, (l, _)) in slots.iter().enumerate() {
            let w = 100 - lease * i as u128;
            sessions = push_reweigh(&mut t, 1, 50, sessions, w, w - lease, 0);
            push_reweigh(&mut t, 1, *l, 0, lease, lease - 1, 0);
        }
        let weight = BTreeMap::from([(44u64, 100u128), (55, lease - 1), (57, 1), (65, lease - 1), (67, 1)]);
        // Each queued budget: its pass and rank key, as the oracle ranks them.
        let mut queue: BTreeMap<u64, (u128, (u8, i128))> = BTreeMap::new();
        for &id in weight.keys() {
            t.push((1, 'W', id, 0));
            queue.insert(id, (0, (0, -1)));
        }
        let (start, mut now, mut counted) = (1_000u64, 1_000u64, 0u64);
        let mut gone = None;
        for n in 0..slices {
            let e = n + 2;
            if away > 0 && n == slices / 2 {
                gone = queue.remove(&44).map(|q| q.0);
                t.push((e, 'D', 44, gone.unwrap()));
            }
            if let Some(pass) = gone.filter(|_| n == slices / 2 + away) {
                let floor = queue.values().map(|q| q.0).min().unwrap();
                t.push((e, 'W', 44, pass.max(floor)));
                queue.insert(44, (pass.max(floor), (0, -i128::from(e))));
                gone = None;
            }
            let (&b, &(pass, _)) = queue.iter().min_by_key(|(id, (p, k))| (*p, *k, **id)).unwrap();
            let after = pass + u128::from(SLICE_US * 10) * stride / weight[&b];
            t.extend([(e, 'K', b, pass), (e, 'I', b, u128::from(now)), (e, 'R', b, after), (e, 'O', 0, 0)]);
            queue.insert(b, (after, (1, i128::from(n as u32))));
            counted += if b == 44 { 1_000 } else { 0 };
            now += SLICE_US;
        }
        let end = start + slices * SLICE_US;
        // Each lease ends: its sub-agent, then itself, each lifted into its parent.
        let e = slices + 2;
        let f = queue.values().map(|q| q.0).min().unwrap();
        for (i, (l, sub)) in slots.into_iter().enumerate() {
            let at = u128::from(end) + 100 + 10 * i as u128;
            let (mut lease_pass, sub_pass) = (queue[&l].0, queue[&sub].0);
            t.push((e, 'D', sub, sub_pass));
            t.push((e, 'X', sub, at));
            lease_pass = push_reweigh(&mut t, e, l, lease_pass, lease - 1, lease, f);
            t.push((e, 'P', l, lease_pass));
            lease_pass = push_lift(&mut t, e, (l, lease_pass), (sub, sub_pass), f, 1, lease);
            t.push((e, 'P', l, lease_pass));
            t.push((e, 'Y', sub, at + 1));
            t.push((e, 'D', l, lease_pass));
            t.push((e, 'X', l, at + 2));
            let w = 100 - lease * (2 - i as u128);
            sessions = push_reweigh(&mut t, e, 50, sessions, w, w + lease, f);
            sessions = push_lift(&mut t, e, (50, sessions), (l, lease_pass), f, lease, w + lease);
            t.push((e, 'Y', l, at + 3));
        }
        let log = trace(&t) + &format!("CHARGED-SHARE bystander {start} {end} 50 {marks}\n");
        (log, counted * 1000 / (end - start))
    }

    #[test]
    fn a_charged_share_is_the_kernels_not_the_count() {
        // The gate's steady window: the count reads 694 of the window, under the old floor of 783;
        // the kernel charged the bystander 100 of every 120 parts it charged under users, its
        // weight's share among the budgets it ran beside.
        let (log, gross) = containment(10, 1_200, 0, "2 3");
        assert_eq!(gross, 694);
        let ok = run(&log, "");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains("charged share bystander: 833 of 1000")
                && s.contains("competitors by weight {44: 100, 55: 9, 57: 1, 65: 9, 67: 1}, expected 833;")
                && s.contains("target met (783 <= share <= 883)")),
            "{ok:?}"
        );
        // Out of the queue for a quarter of the window, it did not keep its share.
        let (log, _) = containment(10, 1_200, 300, "2 3");
        let missed = run(&log, "");
        assert!(
            missed
                .as_ref()
                .is_err_and(|e| e.contains("expected 833; 1 wakes") && e.contains("target missed")),
            "{missed:?}"
        );
        // Leases of weight 41 are owed 80 of 182: what is expected follows the competitors.
        let (log, _) = containment(41, 1_820, 0, "2 3");
        let ok = run(&log, "");
        assert!(ok.as_ref().is_ok_and(|s| s.contains("expected 549;") && s.contains("target met")), "{ok:?}");
        // Without the sessions mark, the whole is the bystander alone.
        let (log, _) = containment(10, 1_200, 0, "2");
        assert!(run(&log, "").is_ok_and(|s| s.contains("bystander: 1000 of 1000")
            && s.contains("competitors by weight {44: 100}, expected 1000;")));
        // A mark the trace does not hold fails the check.
        let (log, _) = containment(10, 1_200, 0, "2 5");
        assert!(run(&log, "").is_err_and(|e| e.contains("0 empty budgets of mark weight 5 destroyed")));
        // A lease of a mark's weight is no mark: it ran.
        let (log, _) = containment(3, 360, 0, "2 3");
        assert!(run(&log, "").is_ok());
        // Malformed: no mark, a mark twice, not a number, a tolerance past the whole.
        for bad in [" 50\n", " 50 2 2\n", " 50 x\n", " 1001 2\n"] {
            let (log, _) = containment(10, 12, 0, "");
            let log = log.replace(" 50 \n", bad);
            assert!(run(&log, "").is_err_and(|e| e.contains("malformed")), "{bad:?}");
        }
        assert!(
            run(&(trace(&[(1, 'W', 1, 0), (1, 'K', 1, 0)]) + "CHARGED-SHARE b 5 5 0 2\n"), "")
                .is_err_and(|e| e.contains("malformed"))
        );
    }

    /// Water-filling: a budget whose weight's share of the harts left is more than its threads
    /// gets its threads, the rest share by weight (the brief's four scenarios' wants).
    #[test]
    fn water_filling_caps_a_budget_at_its_threads() {
        let fill = |b: &[(u64, u64, Option<u64>)], h| {
            water_fill(&b.iter().map(|(id, w, k)| (*id, (*w, *k))).collect(), h)
                .into_values()
                .collect::<Vec<_>>()
        };
        assert_eq!(fill(&[(1, 900, Some(1)), (2, 100, Some(1)), (3, 100, Some(1))], 2), [1000, 500, 500]);
        assert_eq!(
            fill(
                &[
                    (1, 1000, Some(1)),
                    (2, 100, Some(1)),
                    (3, 10, Some(5)),
                    (4, 10, Some(5)),
                    (5, 10, Some(1))
                ],
                3
            ),
            [1000, 1000, 333, 333, 333]
        );
        assert_eq!(fill(&[(1, 900, Some(2)), (2, 100, Some(1)), (3, 100, Some(1))], 2), [1636, 181, 181]);
        assert_eq!(fill(&[(1, 100, Some(4)), (2, 100, Some(1))], 4), [3000, 1000]);
        // A mark that names no threads is never capped; at one hart nothing is.
        assert_eq!(fill(&[(1, 900, None), (2, 100, Some(1))], 2), [1800, 200]);
        assert_eq!(fill(&[(1, 900, Some(1)), (2, 100, Some(1))], 1), [900, 100]);
    }

    /// At several harts (`F`) a charged share is judged against water-filling over the marks,
    /// with every want stated, and net of the lock waits of the harts running a marked budget.
    #[test]
    fn a_charged_share_across_harts_is_judged_by_water_filling_net_of_lock_waits() {
        let (log, _) = containment(10, 1_200, 0, "2:1 3:1");
        // The trace with `inner` after its first pick (inside the window) and `F` before its end,
        // renumbered.
        let at = |log: &str, inner: &[(char, u64, u128, u64)]| {
            let mut out = String::new();
            let mut records: Vec<String> = Vec::new();
            let mut first = true;
            for line in log.lines() {
                let Some(rest) = line.strip_prefix("SCHED-TRACE ") else {
                    if !line.starts_with("SCHED-TRACE-END") && !line.starts_with("CHARGED-SHARE") {
                        out += line;
                        out += "\n";
                    }
                    continue;
                };
                let f: Vec<&str> = rest.split_whitespace().collect();
                records.push(format!("{} {} {} {}", f[1], f[2], f[3], f[4]));
                if first && f[2] == "I" {
                    first = false;
                    for (kind, id, pass, hart) in inner {
                        records.push(format!("{} {kind} {id} {pass:x} {hart}", f[1]));
                    }
                }
            }
            let last = records.last().unwrap().split(' ').next().unwrap().to_string();
            records.push(format!("{last} F 1000000 2 0"));
            for (i, r) in records.iter().enumerate() {
                out += &format!("SCHED-TRACE {i} {r}\n");
            }
            let share = log.lines().find(|l| l.starts_with("CHARGED-SHARE")).unwrap();
            out + &format!("SCHED-TRACE-END {} dropped 0\n{share}\n", records.len())
        };
        // At two harts the bystander (100) and the sessions (80 free), one thread each, are each
        // owed a hart: half, where the one-hart schedule charged it 833.
        let missed = run(&at(&log, &[]), "");
        assert!(
            missed.as_ref().is_err_and(|e| e.contains("charged share bystander: 833 of 1000")
                && e.contains("at 2 harts")
                && e.contains("water-filling wants in thousandths of a hart {44: 1000, 50: 1000}")
                && e.contains("expected 500")
                && e.contains("target missed (450 <= share <= 550)")),
            "{missed:?}"
        );
        // Lock waits of the hart running the bystander come out of its charge, but for the audits
        // on other harts they waited through, which were billed to nobody.
        let waits = [('H', 44, 0, 1), ('Q', 0, 10_800, 1)];
        let net = run(&at(&log, &waits), "");
        assert!(
            net.as_ref().is_err_and(|e| e.contains("lock waits taken out, ticks {44: 10800}")),
            "{net:?}"
        );
        let excused = [('H', 44, 0, 1), ('Q', 0, 10_800, 1), ('y', 800, 0, 1)];
        let net = run(&at(&log, &excused), "");
        assert!(
            net.as_ref().is_err_and(|e| e.contains("lock waits taken out, ticks {44: 10000}")),
            "{net:?}"
        );
        // Judged at one hart only (`@1`), the miss at two is reported, not judged.
        let share = |tolerance: &str| {
            let log = at(&log, &[]);
            let line = log.lines().find(|l| l.starts_with("CHARGED-SHARE")).unwrap().to_string();
            let mut f: Vec<&str> = line.split(' ').collect();
            f[4] = tolerance;
            log.replace(&line, &f.join(" "))
        };
        let reported = run(&share("50@1"), "");
        assert!(
            reported.as_ref().is_ok_and(|s| s.contains("target missed (450 <= share <= 550)")
                && s.contains("judged at 1 harts only: not judged")),
            "{reported:?}"
        );
        assert!(run(&share("50@2"), "").is_err_and(|e| !e.contains("not judged")));
        assert!(run(&share("50@0"), "").is_err_and(|e| e.contains("malformed")));
    }

    /// Budget 44 (weight 100) marked 2, then one hart's picks by stride over `budgets` (id,
    /// weight), `slices` slices of 1,000 ticks timed 10 µs apart from 100 µs, each runner's `H`
    /// stating its weight (or not, with `say_weights` false), and after slice `n` of `lift`
    /// (`n`, the budget, the rise) a lift out of the cap set. The records and the window's end.
    fn round_robin(
        budgets: &[(u64, u128)],
        slices: u64,
        say_weights: bool,
        lift: Option<(u64, u64, u128)>,
    ) -> (Vec<Rec>, u64) {
        let stride = u128::from(redoubt_stride::STRIDE);
        let mut t: Vec<Rec> = Vec::new();
        push_reweigh(&mut t, 1, 44, 0, 100, 98, 0);
        t.push((1, 'X', 60, 10));
        push_reweigh(&mut t, 1, 44, 0, 98, 100, 0);
        push_lift(&mut t, 1, (44, 0), (60, 0), 0, 2, 100);
        t.push((1, 'Y', 60, 11));
        let mut queue = BTreeMap::new();
        for (b, _) in budgets {
            t.push((2, 'W', *b, 0));
            queue.insert(*b, (0u128, (0u8, -2i128)));
        }
        let weight: BTreeMap<u64, u128> = budgets.iter().copied().collect();
        let mut now = 100;
        for n in 0..slices {
            let e = n + 3;
            let (&b, &(pass, _)) = queue.iter().min_by_key(|(id, (p, k))| (*p, *k, **id)).unwrap();
            let after = pass + 1_000 * stride / weight[&b];
            let said = if say_weights { weight[&b] } else { 0 };
            t.extend([
                (e, 'K', b, pass),
                (e, 'H', b, said),
                (e, 'I', b, now),
                (e, 'R', b, after),
                (e, 'O', 0, 0),
            ]);
            queue.insert(b, (after, (1, i128::from(n as u32))));
            if let Some((_, l, rise)) = lift.filter(|l| l.0 == n) {
                let pass = queue[&l].0 + rise;
                t.extend([(e, 'u', l, pass), (e, 'z', l, pass), (e, 'P', l, pass)]);
                queue.get_mut(&l).unwrap().0 = pass;
            }
            now += 10;
        }
        (t, now as u64)
    }

    /// Records written on hart 0, with `inner` (kind, id, pass, hart) after the first timer
    /// interrupt, and `F` stating `harts` at the end.
    fn on_harts(t: &[Rec], inner: &[(char, u64, u128, u64)], harts: u128) -> String {
        let mut records: Vec<(u64, char, u64, u128, u64)> = Vec::new();
        for &(e, kind, id, pass) in t {
            records.push((e, kind, id, pass, 0));
            if kind == 'I' && records.iter().filter(|r| r.1 == 'I').count() == 1 {
                records.extend(inner.iter().map(|&(kind, id, pass, hart)| (e, kind, id, pass, hart)));
            }
        }
        let e = records.last().unwrap().0;
        records.push((e, 'F', 1_000_000, harts, 0));
        let mut s = String::from("boot noise\n");
        for (i, (entry, kind, id, pass, hart)) in records.iter().enumerate() {
            s += &format!("SCHED-TRACE {i} {entry} {kind} {id} {pass:x} {hart}\n");
        }
        s + &format!("SCHED-TRACE-END {} dropped 0\n", records.len())
    }

    /// A share across harts is of everything the kernel charged, net of lock waits, against the
    /// water-filling want of the budget marked among the budgets the program names; at one hart
    /// that is its weight's share.
    #[test]
    fn a_hart_share_is_judged_of_every_charge_against_water_filling() {
        let (t, end) = round_robin(&[(44, 100), (9, 300)], 40, true, None);
        // One hart: 100 of 400 is 250, and every want is stated.
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2:1 300:1\n")), "");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains(
                "hart share v: 256 of 1000 of the CPU the kernel charged in [100, 500] µs (records 27..=222)"
            ) && s.contains(
                "at 1 harts: budget 44 10000 ticks of 38999, its lock waits 0 of 0 ticks taken out"
            ) && s
                .contains("[100:1 250, 300:1 750], expected 250; 2 budgets charged")
                && s.contains("target met (220 <= share <= 280)")),
            "{ok:?}"
        );
        // Two harts, each budget one thread: each is owed a hart, and 250 misses 500.
        let two = on_harts(&t, &[], 2);
        let missed = run(&(two.clone() + &format!("HART-SHARE v 100 {end} 30 2:1 300:1\n")), "");
        assert!(
            missed.as_ref().is_err_and(|e| e.contains("[100:1 1000, 300:1 1000], expected 500")
                && e.contains("target missed (470 <= share <= 530)")),
            "{missed:?}"
        );
        // Judged at two harts only, it misses there and is only reported at one.
        let missed = run(&(two.clone() + &format!("HART-SHARE v 100 {end} 30@2 2:1 300:1\n")), "");
        assert!(
            missed.as_ref().is_err_and(
                |e| e.contains("target missed (470 <= share <= 530)") && !e.contains("not judged")
            ),
            "{missed:?}"
        );
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30@2 2:1 900:1\n")), "");
        assert!(
            ok.as_ref().is_ok_and(
                |s| s.contains("target missed (70 <= share <= 130), judged at 2 harts only: not judged")
            ),
            "{ok:?}"
        );
        for bad in ["30@0", "30@", "30@x"] {
            let malformed =
                run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} {bad} 2:1 300:1\n")), "");
            assert!(malformed.is_err_and(|e| e.contains("malformed")), "{bad}");
        }
        // With two threads the heavier budget is not capped: by weight again.
        let ok = run(&(two + &format!("HART-SHARE v 100 {end} 30 2:1 300:2\n")), "");
        assert!(ok.as_ref().is_ok_and(|s| s.contains("[100:1 500, 300:2 1500], expected 250")), "{ok:?}");
        // One side: at least the want less the tolerance.
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30+ 2 100\n")), "");
        assert!(ok.as_ref().is_err_and(|e| e.contains("target missed (share >= 470)")), "{ok:?}");
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30+ 2 900\n")), "");
        assert!(ok.as_ref().is_ok_and(|s| s.contains("target met (share >= 70)")), "{ok:?}");
        // And at most the want and the tolerance.
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30- 2 100\n")), "");
        assert!(ok.as_ref().is_ok_and(|s| s.contains("target met (share <= 530)")), "{ok:?}");
        let ok = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30- 2 900\n")), "");
        assert!(ok.as_ref().is_err_and(|e| e.contains("target missed (share <= 130)")), "{ok:?}");
        // The lock waits of a hart running budget 44 come out of its part and of the whole.
        let waits = [('H', 44, 100, 1), ('Q', 0, 5_000, 1)];
        let net = run(&(on_harts(&t, &waits, 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")), "");
        assert!(
            net.as_ref()
                .is_err_and(|e| e.contains("budget 44 5000 ticks of 33999, its lock waits 5000 of 5000")
                    && e.contains("147 of 1000")),
            "{net:?}"
        );
        // A lift out of the cap set (`u`, then the `P` that sets it) is no charge: the whole is
        // what the slices ran.
        let (t, end) = round_robin(&[(44, 100), (9, 300)], 40, true, Some((20, 9, 1 << 40)));
        let lifted = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")), "");
        assert!(
            lifted.as_ref().is_err_and(|e| e.contains("1 lifted out of the cap set, at most to the floor")
                && e.contains("budget 44 23000 ticks of 38999,")),
            "{lifted:?}"
        );
        // The lift states the floor (`z`) and never passes it: with the floor below the lift, or
        // with no floor, the check fails.
        let at = t.iter().position(|r| r.1 == 'z').expect("the lift's floor");
        let mut above = t.clone();
        above[at].3 -= 1;
        let above = run(&(on_harts(&above, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")), "");
        assert!(
            above
                .as_ref()
                .is_err_and(|e| e.contains("lifted out of the cap set to") && e.contains("above the floor")),
            "{above:?}"
        );
        let mut none = t.clone();
        none.remove(at);
        let none = run(&(on_harts(&none, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")), "");
        assert!(
            none.as_ref().is_err_and(|e| e.contains("lift out of the cap set states no floor")),
            "{none:?}"
        );
        // A budget charged whose weight the trace does not state fails the check.
        let (t, end) = round_robin(&[(44, 100), (9, 300)], 40, false, None);
        let unknown = run(&(on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")), "");
        assert!(
            unknown.as_ref().is_err_and(|e| e.contains(
                "hart share v: budget 9 was charged in the window, but the trace states no weight for it"
            )),
            "{unknown:?}"
        );
        // Malformed: no mark, a thread count of 0, not a number, a tolerance past the whole, an
        // empty window.
        for bad in [
            "v 100 900 30",
            "v 100 900 30 2:0",
            "v 100 900 x 2",
            "v 100 900 1001 2",
            "v 900 900 30 2",
            "v 100 900 30+- 2",
        ] {
            let log = on_harts(&t, &[], 1) + &format!("HART-SHARE {bad}\n");
            assert!(run(&log, "").is_err_and(|e| e.contains("malformed")), "{bad:?}");
        }
    }

    #[test]
    fn a_charged_share_counts_charges_never_the_floors_lift() {
        // Budget 44 (weight 100, marked 2) is charged 2^24 of pass (1,600 ticks), leaves, and
        // wakes 2^28 higher: the floor's lift, not a charge. Budget 9 is not under the mark.
        let mut t: Vec<Rec> = Vec::new();
        push_reweigh(&mut t, 1, 44, 0, 100, 98, 0);
        t.push((1, 'X', 60, 10));
        push_reweigh(&mut t, 1, 44, 0, 98, 100, 0);
        push_lift(&mut t, 1, (44, 0), (60, 0), 0, 2, 100);
        t.push((1, 'Y', 60, 11));
        t.extend([
            (2, 'W', 44, 0),
            (2, 'K', 44, 0),
            (2, 'I', 44, 100),
            (2, 'R', 44, 1 << 24),
            (2, 'O', 0, 0),
            (3, 'K', 44, 1 << 24),
            (3, 'D', 44, 1 << 24),
            (4, 'W', 9, 1 << 29),
            (4, 'P', 44, 1 << 28),
            (4, 'W', 44, 1 << 28),
            (4, 'K', 44, 1 << 28),
            (4, 'I', 44, 200),
            (4, 'O', 0, 0),
        ]);
        let log = trace(&t) + "CHARGED-SHARE b 50 300 0 2\n";
        let ok = run(&log, "");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains(
                "charged share b: 1000 of 1000 of the CPU the kernel charged to the budgets under its marks in [50, 300] µs (records 25..=34): budget 44 1600 ticks of 1600, audits charged to none; competitors by weight {44: 100}, expected 1000; 1 wakes in the window; charged outside its marks: {}: target met (1000 <= share <= 1000)"
            )),
            "{ok:?}"
        );
        // A weight change inside the window: the competitors could change, and the window is
        // refused.
        let mut changed = t.clone();
        let mut carve = Vec::new();
        push_reweigh(&mut carve, 2, 44, 0, 100, 99, 0);
        changed.splice(26..26, carve);
        let log = trace(&changed) + "CHARGED-SHARE b 50 300 0 2\n";
        assert!(
            run(&log, "").is_err_and(|e| e.contains("budget 44 under its marks was reweighed at record 26"))
        );
        // A second empty budget of the mark's weight, under budget 9: the mark names no one.
        let mut forged = t.clone();
        forged.push((5, 'X', 61, 400));
        push_lift(&mut forged, 5, (9, 1 << 29), (61, 0), 1 << 28, 2, 10);
        forged.push((5, 'Y', 61, 401));
        let log = trace(&forged) + "CHARGED-SHARE b 50 300 0 2\n";
        assert!(run(&log, "").is_err_and(|e| e.contains("2 empty budgets of mark weight 2 destroyed")));
    }

    #[test]
    fn an_unmatched_audit_fails() {
        let pick = [(2, 'W', 1, 5), (2, 'K', 1, 5)];
        for (what, head) in [
            ("a begin with no end", vec![(1, 'U', 1, 130)]),
            ("an end with no begin", vec![(1, 'V', 1, 154)]),
            ("an end of another audit", vec![(1, 'U', 1, 130), (1, 'V', 2, 154)]),
            ("an end before its begin", vec![(1, 'U', 1, 154), (1, 'V', 1, 130)]),
            ("nested", vec![(1, 'U', 1, 130), (1, 'U', 2, 131), (1, 'V', 2, 132), (1, 'V', 1, 133)]),
            // R10 subtracts nothing: an audit inside a destruction fails it.
            ("inside R10", vec![(1, 'X', 7, 100), (1, 'U', 2, 110), (1, 'V', 2, 111), (1, 'Y', 7, 130)]),
        ] {
            let v = verdict(&[head, pick.to_vec()].concat());
            assert!(v.as_ref().is_err_and(|e| e.contains("audit")), "{what}: {v:?}");
        }
        assert!(verdict(&[vec![(1, 'U', 1, 130), (1, 'V', 1, 154)], pick.to_vec()].concat()).is_ok());
    }

    #[test]
    fn a_device_interrupt_that_claims_nothing_bills_nobody() {
        let pick = [(2, 'W', 1, 5), (2, 'K', 1, 5)];
        // Budget 1 is interrupted at 100 µs by a device interrupt.
        let good = [
            // It claimed nothing: nobody pays.
            vec![(1, 'x', 1, 100), (1, 'E', 0, 0), (1, 'c', 0, 0), (1, 'O', 0, 1)],
            vec![(1, 'x', 1, 100), (1, 'c', 0, 0), (1, 'B', 1, 0), (1, 'O', 0, 1)],
            // It claimed nothing, after expiring 2's item: the rest is 2's.
            vec![
                (1, 'x', 1, 100),
                (1, 'B', 3, 40),
                (1, 'E', 2, 1),
                (1, 'c', 0, 0),
                (1, 'B', 2, 9),
                (1, 'O', 0, 1),
            ],
            // It claimed source 7: the handling is the owner's (3), the rest the interrupted budget's.
            vec![
                (1, 'x', 1, 100),
                (1, 'E', 0, 0),
                (1, 'c', 7, 0),
                (1, 'B', 3, 20),
                (1, 'B', 1, 9),
                (1, 'O', 0, 1),
            ],
        ];
        for (n, head) in good.iter().enumerate() {
            let v = verdict(&[head.clone(), pick.to_vec()].concat());
            let empty = if n < 3 { "(1 claiming nothing)" } else { "(0 claiming nothing)" };
            assert!(
                v.as_ref()
                    .is_ok_and(|s| s
                        .contains(&format!("1 device interrupts from user mode billed by the rule {empty}"))),
                "{head:?}: {v:?}"
            );
        }
        for (what, head) in [
            // The defect: the interrupted budget pays for a claim another hart won.
            (
                "claimed nothing, billed",
                vec![(1, 'x', 1, 100), (1, 'c', 0, 0), (1, 'B', 1, 30), (1, 'O', 0, 1)],
            ),
            (
                "claimed nothing, the tail not the expiry's",
                vec![(1, 'x', 1, 100), (1, 'E', 2, 1), (1, 'c', 0, 0), (1, 'B', 1, 9), (1, 'O', 0, 1)],
            ),
            ("no claim", vec![(1, 'x', 1, 100), (1, 'O', 0, 1)]),
            ("two claims", vec![(1, 'x', 1, 100), (1, 'c', 0, 0), (1, 'c', 7, 0), (1, 'O', 0, 1)]),
            ("a claim in a timer interrupt", vec![(1, 'I', 1, 100), (1, 'c', 0, 0), (1, 'O', 0, 1)]),
            ("nested", vec![(1, 'x', 1, 100), (1, 'x', 1, 100), (1, 'O', 0, 1), (1, 'O', 0, 1)]),
        ] {
            let v = verdict(&[head, pick.to_vec()].concat());
            assert!(v.as_ref().is_err_and(|e| e.contains("device interrupt")), "{what}: {v:?}");
        }
    }

    #[test]
    fn a_timer_interrupt_bills_the_budgets_whose_items_it_expired() {
        let pick = [(2, 'W', 1, 5), (2, 'K', 1, 5)];
        // Budget 1 is interrupted at 100 µs; budget 2's timeout expires, or its wait ended early.
        let good = [
            // Expired 2's item: its walk and handling, then the rest, all 2's.
            vec![
                (1, 'I', 1, 100),
                (1, 'B', 2, 40),
                (1, 'E', 2, 1),
                (1, 'B', 2, 9),
                (1, 'B', 2, 30),
                (1, 'O', 0, 1),
            ],
            // Expired 1's own item: 1 pays.
            vec![(1, 'I', 1, 100), (1, 'B', 1, 40), (1, 'E', 1, 1), (1, 'B', 1, 30), (1, 'O', 0, 1)],
            // Found 2's wait ended before its timeout: 2 pays for the walk and the rest.
            vec![(1, 'I', 1, 100), (1, 'E', 2, 2), (1, 'B', 2, 9), (1, 'B', 2, 30), (1, 'O', 0, 1)],
            // Found nothing and ended 1's slice: 1 pays, and the CPU goes to `kmain`.
            vec![(1, 'I', 1, 100), (1, 'E', 0, 0), (1, 'B', 1, 30), (1, 'O', 0, 0)],
            // Found nothing, ended no slice: nobody pays.
            vec![(1, 'I', 1, 100), (1, 'E', 0, 0), (1, 'O', 0, 1)],
            vec![(1, 'I', 1, 100), (1, 'B', 1, 0), (1, 'O', 0, 1)],
        ];
        // Each that bills 2 in 1's time is reported, with the ticks 2 was billed.
        let foreign = [Some(79), None, Some(39), None, None, None];
        for (head, foreign) in good.into_iter().zip(foreign) {
            let v = verdict(&[head.clone(), pick.to_vec()].concat());
            let counted = match foreign {
                Some(t) => format!("1 charging another budget than the one interrupted, {t} ticks"),
                None => "0 charging another budget than the one interrupted, 0 ticks".into(),
            };
            assert!(
                v.as_ref().is_ok_and(
                    |s| s.contains("1 timer interrupts billed by the rule") && s.contains(&counted)
                ),
                "{head:?}: {v:?}"
            );
        }
        for (what, head) in [
            // The defect: the rest after 2's item billed to the interrupted 1.
            (
                "the tail to the interrupted budget",
                vec![(1, 'I', 1, 100), (1, 'B', 2, 40), (1, 'E', 2, 1), (1, 'B', 1, 30), (1, 'O', 0, 1)],
            ),
            (
                "a wait ended early, billed to another",
                vec![(1, 'I', 1, 100), (1, 'E', 2, 2), (1, 'B', 1, 30), (1, 'O', 0, 1)],
            ),
            ("found nothing, no slice end", vec![(1, 'I', 1, 100), (1, 'B', 1, 30), (1, 'O', 0, 1)]),
            ("found nothing, another budget", vec![(1, 'I', 1, 100), (1, 'B', 3, 30), (1, 'O', 0, 0)]),
        ] {
            let v = verdict(&[head, pick.to_vec()].concat());
            assert!(v.as_ref().is_err_and(|e| e.contains("a timer interrupt")), "{what}: {v:?}");
        }
        for (what, head) in [
            ("a charge outside one", vec![(1, 'B', 1, 30)]),
            ("never returned", vec![(1, 'I', 1, 100)]),
            ("nested", vec![(1, 'I', 1, 100), (1, 'I', 1, 100), (1, 'O', 0, 1), (1, 'O', 0, 1)]),
            ("two expiries", vec![(1, 'I', 1, 100), (1, 'E', 0, 0), (1, 'E', 0, 0), (1, 'O', 0, 1)]),
            ("an unknown expiry", vec![(1, 'I', 1, 100), (1, 'E', 2, 3), (1, 'O', 0, 1)]),
        ] {
            let v = verdict(&[head, pick.to_vec()].concat());
            assert!(v.as_ref().is_err_and(|e| e.contains("timer interrupt")), "{what}: {v:?}");
        }
        // The ones nobody pays for are counted inside each share's window: `extra` after the
        // round robin's third slice, at 125 µs.
        let (t, end) = round_robin(&[(44, 100), (9, 300)], 40, true, None);
        let with = |extra: &[(char, u64, u128)]| {
            let at = t.iter().enumerate().filter(|(_, r)| r.1 == 'O').nth(2).unwrap().0 + 1;
            let mut t = t.clone();
            let e = t[at - 1].0;
            t.splice(at..at, extra.iter().map(|&(kind, id, pass)| (e, kind, id, pass)));
            on_harts(&t, &[], 1) + &format!("HART-SHARE v 100 {end} 30 2 300\n")
        };
        let v = run(&with(&[('I', 44, 125), ('O', 0, 1)]), "");
        assert!(v.as_ref().is_ok_and(|s| s.contains("1 timer interrupts nobody's, 0 finding")), "{v:?}");
        // A case can require interrupts that found another budget's wait ended early in a share's
        // window: one inside (budget 44 interrupted, 9's wait) meets it; none fails it.
        let log = with(&[('I', 44, 125), ('E', 9, 2), ('B', 9, 9), ('O', 0, 1)]);
        let v = run(&log, "stale_waits_in=v");
        assert!(
            v.as_ref().is_ok_and(|s| s.contains("target met (220 <= share <= 280); 0 timer interrupts nobody's, 1 finding another budget's wait ended early")),
            "{v:?}"
        );
        let log = with(&[('I', 9, 125), ('E', 9, 2), ('B', 9, 9), ('O', 0, 1)]);
        let v = run(&log, "stale_waits_in=v");
        assert!(v.as_ref().is_err_and(|e| e.contains("none, but the case requires some")), "{v:?}");
        assert!(run(&log, "stale_waits_in=nobody").is_err_and(|e| e.contains("no such share")));
    }

    #[test]
    fn a_broken_trace_is_rejected() {
        let one = "SCHED-TRACE 0 1 W 1 5\nSCHED-TRACE 1 1 K 1 5\n";
        let cases = [
            ("no end line", one.to_string()),
            ("a drop", format!("{one}SCHED-TRACE-END 2 dropped 3\n")),
            ("a short count", format!("{one}SCHED-TRACE-END 3 dropped 0\n")),
            (
                "a gap",
                "SCHED-TRACE 0 1 W 1 5\nSCHED-TRACE 2 1 K 1 5\nSCHED-TRACE-END 2 dropped 0\n".to_string(),
            ),
            ("an unknown kind", "SCHED-TRACE 0 1 X 1 5\nSCHED-TRACE-END 1 dropped 0\n".to_string()),
            (
                "the kernel's time before the end",
                "SCHED-TRACE 0 0 C 9 1\nSCHED-TRACE 1 1 W 1 5\nSCHED-TRACE-END 2 dropped 0\n".to_string(),
            ),
            ("a bad pass", "SCHED-TRACE 0 1 W 1 zz\nSCHED-TRACE-END 1 dropped 0\n".to_string()),
            (
                "entries going back",
                "SCHED-TRACE 0 2 W 1 5\nSCHED-TRACE 1 1 K 1 5\nSCHED-TRACE-END 2 dropped 0\n".to_string(),
            ),
            ("no pick", "SCHED-TRACE 0 1 W 1 5\nSCHED-TRACE-END 1 dropped 0\n".to_string()),
            ("a pick of nothing queued", "SCHED-TRACE 0 1 K 1 5\nSCHED-TRACE-END 1 dropped 0\n".to_string()),
            (
                "a pick at another pass",
                "SCHED-TRACE 0 1 W 1 5\nSCHED-TRACE 1 1 K 1 6\nSCHED-TRACE-END 2 dropped 0\n".to_string(),
            ),
        ];
        for (what, log) in cases {
            assert!(run(&log, "").is_err(), "{what} was accepted");
        }
        assert!(run(&format!("{one}SCHED-TRACE-END 2 dropped 0\n"), "").is_ok());
    }
}

/// The oracle against the executable model's scheduler (`redoubt_model::sched`), run as the
/// kernel's trace would record it: its own rank rules must pass, and each of the model's broken
/// tie rules must be caught on some trace.
#[cfg(test)]
mod model {
    use std::collections::{BTreeMap, BTreeSet};

    use redoubt_model::mutation::Mutation;
    use redoubt_model::sched::Scheduler;

    use super::*;

    struct Rng(u64);

    impl Rng {
        fn below(&mut self, n: u64) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0 % n.max(1)
        }
    }

    /// Records in the kernel's order: the model is driven like the kernel drives its queue (equal
    /// weights and whole slices, so ties are the rule), and every change of a budget's queue
    /// membership, pass and pick becomes the record the kernel would print.
    fn trace(seed: u64, mutation: Option<Mutation>) -> Vec<Record> {
        const ROOT: u64 = 1000;
        let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
        let mut s = Scheduler { mutation, ..Scheduler::default() };
        s.add_budget(ROOT, None, 1 << 31);
        let n = 3 + rng.below(4);
        let mut threads: BTreeMap<u64, BTreeSet<(u64, u64)>> = BTreeMap::new();
        for b in 1..=n {
            s.add_budget(b, Some(ROOT), 10);
            threads.insert(b, BTreeSet::new());
        }
        let mut out = Vec::new();
        let mut entry = 0u64;
        let snapshot = |s: &Scheduler| -> BTreeMap<u64, (bool, u128)> {
            s.budgets.iter().map(|(id, e)| (*id, (e.queued, e.pass))).collect()
        };
        let mut push = |out: &mut Vec<Record>, entry: u64, kind: char, id: u64, pass: u128| {
            let seq = out.len() as u64;
            out.push(Record { seq, entry, kind, id, pass, hart: 0 });
        };
        // What changed between two snapshots, as records; `requeued` names the budget a deschedule
        // requeued, if any.
        let diff = |out: &mut Vec<Record>,
                    push: &mut dyn FnMut(&mut Vec<Record>, u64, char, u64, u128),
                    entry: u64,
                    before: &BTreeMap<u64, (bool, u128)>,
                    after: &BTreeMap<u64, (bool, u128)>,
                    requeued: Option<u64>| {
            for (id, (q, p)) in after {
                let (bq, bp) = before.get(id).copied().unwrap_or((false, 0));
                if Some(*id) == requeued && *q {
                    push(out, entry, 'R', *id, *p);
                } else if !bq && *q {
                    push(out, entry, 'W', *id, *p);
                } else if bq && !*q {
                    push(out, entry, 'D', *id, *p);
                } else if bp != *p {
                    push(out, entry, 'P', *id, *p);
                }
            }
        };
        for _ in 0..200 {
            match rng.below(4) {
                0 => {
                    let b = 1 + rng.below(n);
                    let t = (b, rng.below(2));
                    threads.get_mut(&b).unwrap().insert(t);
                    s.thread_runnable(b, t);
                    let before = snapshot(&s);
                    entry += 1;
                    s.reconcile();
                    diff(&mut out, &mut push, entry, &before, &snapshot(&s), None);
                }
                1 => {
                    let b = 1 + rng.below(n);
                    let Some(&t) = threads[&b].iter().next() else { continue };
                    threads.get_mut(&b).unwrap().remove(&t);
                    let running = s.current.is_some_and(|c| c.budget == b && c.thread == t);
                    let before = snapshot(&s);
                    s.thread_blocked(b, t);
                    diff(&mut out, &mut push, entry, &before, &snapshot(&s), running.then_some(b));
                    let before = snapshot(&s);
                    entry += 1;
                    s.reconcile();
                    diff(&mut out, &mut push, entry, &before, &snapshot(&s), None);
                }
                _ => {
                    if s.current.is_none() {
                        let before = snapshot(&s);
                        entry += 1;
                        s.reconcile();
                        diff(&mut out, &mut push, entry, &before, &snapshot(&s), None);
                        let Some(c) = s.pick() else { continue };
                        push(&mut out, entry, 'K', c.budget, s.budgets[&c.budget].pass);
                    }
                    let c = s.current.unwrap();
                    s.run(c.slice_left);
                    let before = snapshot(&s);
                    s.slice_end();
                    diff(&mut out, &mut push, entry, &before, &snapshot(&s), Some(c.budget));
                    let before = snapshot(&s);
                    entry += 1;
                    s.reconcile();
                    diff(&mut out, &mut push, entry, &before, &snapshot(&s), None);
                }
            }
        }
        out
    }

    #[test]
    fn the_models_own_ranks_pass() {
        for seed in 0..500 {
            let t = trace(seed, None);
            if let Err(why) = check(&t) {
                panic!("seed {seed}: {why}");
            }
        }
    }

    #[test]
    fn the_models_broken_ties_are_caught() {
        for m in [Mutation::R12TieQueuedFirst, Mutation::R12RequeueAhead, Mutation::R12RequeueLifo] {
            let caught = (0..500).filter(|seed| check(&trace(*seed, Some(m))).is_err()).count();
            assert!(caught > 0, "{m:?}: no trace was rejected");
            eprintln!("{m:?}: {caught} of 500 traces rejected");
        }
    }
}
