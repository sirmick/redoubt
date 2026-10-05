//! An independent check of the kernel's stride ranks (kernel/scheduling.md R12, the four rank
//! clauses of "The current minimum and ties"), over the raw events a `sched-trace` kernel prints
//! at `system_reset`.
//!
//! The kernel's trace says what its queue did, never why: a budget woke (`W`), was requeued (`R`),
//! left the queue (`D`), had its pass changed (`P`), or was picked (`K`); each record carries the
//! kernel entry (reconcile) it belongs to and the budget's pass after the event (its low 64 bits).
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
//! (`deadline_notice_p99_us=40000`), and reports the audit time beside each. A share is a target
//! too: the program prints its window, the CPU its count stands for and its bounds (`SHARE`), and
//! this check judges it of the window net of the audit time inside it. An audit unpaired, or
//! inside a destruction (R10's own window, which then subtracts nothing), fails the check.
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
//! nobody's, inside each share's window, and those that found another budget's wait ended early;
//! a case can require some of the latter in a share's window (`stale_waits_in=<share>`), so the
//! check it runs cannot pass for want of the interrupts it is about.
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
        if f.len() != 5 {
            return Err(bad());
        }
        let kind = f[2].chars().next().ok_or_else(bad)?;
        if f[2].len() != 1 || !"WRDPKLlefrqwAaGgvNnXYZUVTtIBEOMm".contains(kind) {
            return Err(bad());
        }
        records.push(Record {
            seq: f[0].parse().map_err(|_| bad())?,
            entry: f[1].parse().map_err(|_| bad())?,
            kind,
            id: f[3].parse().map_err(|_| bad())?,
            pass: u128::from_str_radix(f[4], 16).map_err(|_| bad())?,
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
        if i > 0 && r.entry < records[i - 1].entry {
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
    /// When each timer interrupt that found neither and ended no slice came, µs, in trace order.
    pub timer_empty: Vec<u64>,
    /// When each timer interrupt that found another budget's wait ended early came, µs.
    pub timer_stale_foreign: Vec<u64>,
    /// Each walk's span, µs (`M` to `m`, a `walk-trace` kernel's): a receive's pump, a timer
    /// interrupt's expiry and a reconcile, in trace order.
    pub walks: [Vec<(u64, u64)>; 3],
    /// Each destruction's pumps: how many, and their time, µs (`M`/`m` of a pump inside its `X`
    /// and `Y`), in trace order.
    pub r10_pumps: Vec<(usize, u64)>,
}

/// A timer interrupt from user mode, from its `I` to its `O`.
struct TimerEntry {
    seq: u64,
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
    let mut sum = Summary::default();
    let mut i = 0;
    while i < records.len() {
        let r = &records[i];
        i += 1;
        // A budget's pass never falls (the records that carry one).
        if "WRDPK".contains(r.kind) {
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
                    interrupted,
                    at: r.pass as u64,
                    expired: None,
                    charges: Vec::new(),
                });
            }
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
            'O' => {
                let e = open_timer.take().expect("checked above");
                check_timer_entry(&e, r.pass != 0, &mut sum)?;
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
                let want = queued.iter().map(|(id, (pass, key))| (*pass, *key, *id)).min().map(|x| x.2);
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
/// `I`. A wake from a device IRQ has no timer `I`, so it cannot satisfy this check.
fn check_wake_no_preempt(records: &[Record], expected: usize) -> Result<String, String> {
    let mut intervals = Vec::new();
    let mut begin = None;
    for (i, r) in records.iter().enumerate() {
        match r.kind {
            'I' => {
                if begin.replace(i).is_some() {
                    return Err(format!("record {}: nested timer interval", r.seq));
                }
            }
            'O' => {
                let b = begin.take().ok_or_else(|| format!("record {}: unmatched timer return", r.seq))?;
                intervals.push((b, i));
            }
            _ => {}
        }
    }
    if begin.is_some() {
        return Err("unended timer interval".into());
    }
    let mut proof = BTreeMap::<(u64, u64), usize>::new();
    for (n, &(b, e)) in intervals.iter().enumerate() {
        let interval = &records[b..=e];
        let spinner = records[b].id;
        if spinner == 0
            || records[e].pass != 1
            || !records[..b].iter().rposition(|r| r.kind == 'K').is_some_and(|k| records[k].id == spinner)
            || interval.iter().any(|r| "KDXY".contains(r.kind))
        {
            continue;
        }
        let wakes: Vec<_> = interval.iter().filter(|r| r.kind == 'W').collect();
        if wakes.len() != 1 || wakes[0].id == spinner {
            continue;
        }
        let sleeper = wakes[0].id;
        if !interval.iter().any(|r| r.kind == 'E' && r.id == sleeper && r.pass == 1) {
            continue;
        }
        let Some(&(next_b, next_e)) = intervals.get(n + 1) else { continue };
        if records[next_b].id != spinner || records[next_e].pass != 0 {
            continue;
        }
        let ending = &records[next_b..=next_e];
        if !ending.iter().any(|r| r.kind == 'R' && r.id == spinner)
            || ending.iter().any(|r| "WDXY".contains(r.kind))
            || !records[e + 1..next_b].iter().all(|r| r.kind != 'K' && r.kind != 'X')
            || !records[next_e + 1..]
                .iter()
                .find(|r| r.kind == 'K' || r.kind == 'I')
                .is_some_and(|r| r.kind == 'K')
        {
            continue;
        }
        *proof.entry((spinner, sleeper)).or_default() += 1;
    }
    let matches: Vec<_> = proof.iter().filter(|(_, count)| **count == expected).collect();
    if matches.len() != 1 {
        return Err(format!(
            "wake-no-preempt: wanted one spinner/sleeper pair with {expected} I...W...O=1, later I...R...O=0 proofs; found {proof:?}"
        ));
    }
    let (&(spinner, sleeper), &count) = matches[0];
    Ok(format!(
        "wake-no-preempt: {count} timeout wakes of budget {sleeper} continued spinner {spinner} to its slice end"
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

/// Metadata is sent in original attempt order after measurement. The clock unit is explicit:
/// goldfish RTC nanoseconds for the driver, `time_now` microseconds for the timer.
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
        let (standin, unit) = match (f[0], f[15]) {
            ("driver_wake", "rtc_ns") => (0, 1_000u64),
            ("timer_wake", "timer_us") => (1, 1u64),
            _ => return Err(bad()),
        };
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
            || before.checked_add(delay_us) != Some(lower)
            || upper != observed.checked_add(1).ok_or_else(bad)?
            || lower < release
            || lower > observed
            || upper > end
            || before > early
            || early > observed
            || deadline != arm.checked_add(delay_us.checked_mul(unit).ok_or_else(bad)?).ok_or_else(bad)?
            || service < deadline
            || (standin == 0 && rtc_gross != (service - deadline) / 1_000)
            || (standin == 1
                && (arm != before
                    || deadline != lower
                    || service != observed
                    || early != before
                    || rtc_gross != 0))
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
                    let qualified = if intended_positive && lead > 0 && ahead >= 8 {
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
                details.push(format!(
                    "{measure} #{sample_index} programmed-offset {} {} prep-slip {} µs B/L/E/P/U {}/{}/{}/{}/{} armed {}/deadline {}/service {} delay {} {} W {} floor {floor:#x} lead {lead:#x} ahead {ahead} earlier-picks {earlier} peer-R10-overlap {overlap} envelope/credit/net {}/{}/{} µs RTC-gross {} µs lower-witness {} µs",
                    [100, 300, 600, 850][sample_index % 4],
                    if positive { "positive" } else { "zero" },
                    meta.prep_us,
                    meta.before,
                    meta.lower,
                    meta.early,
                    meta.observed,
                    meta.upper,
                    meta.arm,
                    meta.deadline,
                    meta.service,
                    meta.delay_us,
                    if standin == 0 { "rtc_ns" } else { "timer_us" },
                    records[w].seq,
                    metric.gross,
                    metric.credit,
                    metric.net,
                    meta.rtc_gross,
                    metric.lower_witness
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
        let (p50, p99) = (percentile(&mut net.clone(), 50), percentile(&mut net, 99));
        let zero = category_net[0].len();
        let positive = category_net[1].len();
        let (z50, z99) = (percentile(&mut category_net[0].clone(), 50), percentile(&mut category_net[0], 99));
        let (p50_cat, p99_cat) =
            (percentile(&mut category_net[1].clone(), 50), percentile(&mut category_net[1], 99));
        let mut lower: Vec<u64> = metrics[standin].iter().map(|m| m.lower_witness).collect();
        let (l50, l99) = (percentile(&mut lower.clone(), 50), percentile(&mut lower, 99));
        lines.push(format!(
            "{measure}: 200 fenced D-W-service/sample envelopes, {} later report wakes, marker {} X {} Y {} duration {} µs, peer R10 overlap {peer_r10_overlap_us} µs retained; {zero} zero and {positive} positive, {picks} earlier picks before service; all envelope net p50/p99 {p50}/{p99} µs; driver lower-witness p50/p99 {l50}/{l99} µs; zero {z50}/{z99}; positive {p50_cat}/{p99_cat}\n      {}",
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
    Ok(ClusterProof { report, metrics })
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
        r.kind == 'K' || (r.id == parent && "RD".contains(r.kind)) || (r.kind == 'O' && r.pass == 0)
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

/// A share a program judges by this check: `SHARE <name> <start> <end> <cpu> <min> <max>`, a
/// window `[start, end]` on `time_now` and the CPU in it that a count stands for, in µs, and the
/// share's bounds in thousandths of the window.
struct Share<'a> {
    name: &'a str,
    window: (u64, u64),
    cpu: u64,
    bounds: (u64, u64),
}

/// The shares the program printed, in order.
fn shares(log: &str) -> Result<Vec<Share<'_>>, String> {
    let mut out = Vec::new();
    for line in log.lines().map(|line| line.trim_end_matches('\r')) {
        let Some(rest) = line.strip_prefix("SHARE ") else { continue };
        let bad = || format!("malformed {line:?}");
        let f: Vec<&str> = rest.split_whitespace().collect();
        let num = |i: usize| f.get(i).and_then(|v| v.parse::<u64>().ok()).ok_or_else(bad);
        let (window, bounds) = ((num(1)?, num(2)?), (num(4)?, num(5)?));
        if f.len() != 6 || window.0 >= window.1 || bounds.0 > bounds.1 || bounds.1 > 1000 {
            return Err(bad());
        }
        out.push(Share { name: f[0], window, cpu: num(3)?, bounds });
    }
    Ok(out)
}

/// The bench's post-check: parse the case's console log and check it. `args` may bound the p99
/// of R10's durations, `r10_p99_us=N`; each measure's p50 and p99 net of audits,
/// `<measure>_p50_us=N` and `<measure>_p99_us=N`, in each group the program printed; and a
/// lease's end from the steward's decision, `lease_end_p99_us=N`: the worst net decision-wake p99
/// (over every group) plus R10's p99 (kernel/scheduling.md, "Responsiveness"). Each share the
/// program printed is judged against its own bounds, and `stale_waits_in=<share>` requires a timer
/// interrupt that found another budget's wait ended early inside that share's window. A
/// `walk-trace` kernel's walks may each be bounded, `pump_max_us=N`, `expiry_max_us=N` and
/// `reconcile_max_us=N`, the longest net of audits, judged before R10's p99. Every window a target
/// or a share judges has the checked build's audit time inside it subtracted; R10's has none.
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
    for arg in args.split_whitespace() {
        if arg == "cluster" {
            if cluster {
                return Err("duplicate cluster check".into());
            }
            cluster = true;
            continue;
        }
        if arg == "cluster_old_control" {
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
        if !(["r10_p99_us", "lease_end_p99_us"].contains(&name)
            || measure.is_some_and(|m| MEASURES.contains(&m))
            || walk.is_some_and(|w| WALKS.contains(&w)))
        {
            return Err(format!("unknown sched_oracle argument {arg:?}"));
        }
        bounds.insert(name, bound);
    }
    let wake_proof = wake_no_preempt.map(|count| check_wake_no_preempt(&records, count)).transpose()?;
    if cluster_old_control && !cluster {
        return Err("cluster_old_control requires cluster".into());
    }
    let cluster_proof = cluster.then(|| check_cluster(log, &records, &samples, &sum.audits)).transpose()?;
    let carve_proof = carve_return.then(|| check_carve_return(log, &records)).transpose()?;
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
    if n == 0 && (bounds.contains_key("r10_p99_us") || bounds.contains_key("lease_end_p99_us")) {
        return Err("an R10 bound is set, but the trace holds no destruction".into());
    }
    if let Some(bound) = bounds.get("r10_p99_us").filter(|b| p99 > **b) {
        return Err(format!("R10's p99 is {p99} µs over {n} destructions, above {bound}"));
    }
    // Each measure in each group, net of the audits inside its windows.
    let (mut lines, mut missed, mut decision_p99) = (Vec::new(), false, None::<u64>);
    for (m, measure) in MEASURES.iter().enumerate() {
        let want = ["p50", "p99"].map(|q| (q, bounds.get(&*format!("{measure}_{q}_us"))));
        let groups: Vec<_> = samples.range((m, "")..(m + 1, "")).filter(|(_, w)| !w.is_empty()).collect();
        if groups.is_empty() && want.iter().any(|(_, b)| b.is_some()) {
            return Err(format!("a {measure} bound is set, but the log holds no {measure} sample"));
        }
        for ((_, group), windows) in groups {
            let cluster_metrics = if *group == "cluster" {
                Some(&cluster_proof.as_ref().ok_or("cluster samples without cluster check")?.metrics[m])
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
                if met {
                    return Err(format!("old control {measure} met its unchanged envelope target"));
                }
            } else {
                missed |= !met;
            }
            if *measure == "decision_wake" {
                decision_p99 = Some(decision_p99.unwrap_or(0).max(n99));
            }
            let target = if judged.is_empty() {
                "no target".to_string()
            } else {
                let texts: Vec<&str> = judged.iter().map(|(text, _)| text.as_str()).collect();
                format!("target {} ({})", if met { "met" } else { "missed" }, texts.join(", "))
            };
            lines.push(format!(
                "{group} {measure} ({}): net p50/p99/max {n50}/{n99}/{nmax} µs, gross {g50}/{g99}/{gmax}, audits {} µs: {target}",
                gross.len(),
                inside.iter().sum::<u64>()
            ));
        }
    }
    // Each share, of its window net of the audits inside it.
    let shares = shares(log)?;
    if let Some(name) = stale_in.iter().find(|n| !shares.iter().any(|s| s.name == **n)) {
        return Err(format!("stale_waits_in names {name}, but the log holds no such share"));
    }
    for s in shares {
        let (start, end) = s.window;
        let stale = sum.timer_stale_foreign.iter().filter(|t| (start..end).contains(*t)).count();
        let stale_missed = stale_in.contains(&s.name) && stale == 0;
        missed |= stale_missed;
        let inside = audit_inside(&sum.audits, start, end);
        let (gross, net) = (s.cpu * 1000 / (end - start), s.cpu * 1000 / (end - start - inside).max(1));
        let (min, max) = s.bounds;
        let met = (min..=max).contains(&net);
        missed |= !met;
        lines.push(format!(
            "share {}: net {net}, gross {gross} of 1000, audits {inside} µs, {} timer interrupts nobody's, {stale} finding another budget's wait ended early{}: target {} ({min} <= share <= {max})",
            s.name,
            sum.timer_empty.iter().filter(|t| (start..end).contains(*t)).count(),
            if stale_missed { " (none, but the case requires some)" } else { "" },
            if met { "met" } else { "missed" }
        ));
    }
    let mut lease_end = String::new();
    if let Some(bound) = bounds.get("lease_end_p99_us") {
        let wake =
            decision_p99.ok_or("lease_end_p99_us is set, but the log holds no decision_wake sample")?;
        missed |= wake + p99 > *bound;
        lease_end = format!(
            "; lease end p99, net decision wake {wake} + R10 {p99} = {} µs: target {} (<= {bound})",
            wake + p99,
            if wake + p99 <= *bound { "met" } else { "missed" }
        );
    }
    let audit_total: u64 = sum.audits.iter().map(|(b, e)| e - b).sum();
    let head = format!(
        "sched_oracle: {} records, {} picks in rank order, every wake at or above the floor, no pass falling but by a weight change; {} lifts by the rule ({} with a leading parent and work to move); {} weight changes by the rule; R10 {n} destructions over up to {} object frames, µs p50/p99/max {p50}/{p99}/{max}, their threads' ending {t50}/{t99}/{tmax}, no audit inside one; {} audits, {audit_total} µs; {} timer interrupts billed by the rule ({} expiring, {} finding a wait ended early, {} ticks after expiry to the budget billed last; {} ending a slice, {} nobody's){lease_end}",
        records.len(),
        sum.picks,
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
        sum.timer_empty.len()
    );
    let mut report = Vec::new();
    if let Some(proof) = wake_proof {
        report.push(proof);
    }
    if let Some(proof) = cluster_proof {
        if cluster_old_control {
            let mut lower: Vec<u64> = proof.metrics[0].iter().map(|m| m.lower_witness).collect();
            let l50 = percentile(&mut lower.clone(), 50);
            let l99 = percentile(&mut lower, 99);
            let b50 = bounds.get("driver_wake_p50_us").ok_or("old control driver p50 target missing")?;
            let b99 = bounds.get("driver_wake_p99_us").ok_or("old control driver p99 target missing")?;
            if l50 <= *b50 && l99 <= *b99 {
                return Err(format!(
                    "old control envelope-only failure: driver physical lower witness {l50}/{l99} within {b50}/{b99}"
                ));
            }
        }
        report.push(proof.report);
    }
    if let Some(proof) = carve_proof {
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
    let out = std::iter::once(head).chain(lines).collect::<Vec<_>>().join("\n      ");
    if missed { Err(out) } else { Ok(out) }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn cluster_fixture() -> Vec<String> {
        let mut lines = vec![
            "CLUSTER-HEADER driver_wake 950000 17000000".to_string(),
            "CLUSTER-HEADER timer_wake 950000 17000000".to_string(),
        ];
        for (measure, unit, clock) in [("driver_wake", 1_000u64, "rtc_ns"), ("timer_wake", 1, "timer_us")] {
            for i in 0..200u64 {
                let phase = [100, 300, 600, 850][i as usize % 4];
                let positive = i / 4 % 2 == 1;
                let at = 1_000_000 + i * 80_000;
                let before = if positive { at + 2 } else { at - 20 };
                let delay = if positive { phase } else { phase + 20 };
                let lower = before + delay;
                let observed = lower + 10;
                let arm = if unit == 1 { before } else { 50_000_000_000 + i * 80_000_000 };
                let deadline = arm + delay * unit;
                let service = if unit == 1 { observed } else { deadline + 10_000 };
                let rtc_gross = if unit == 1 { 0 } else { 10 };
                lines.push(format!(
                    "CLUSTER-SAMPLE {measure} {i} {} {} {} {before} {delay} {lower} {before} {observed} {} {arm} {deadline} {service} {rtc_gross} {clock}",
                    usize::from(positive), i * 80_000, before as i64 - at as i64, observed + 1
                ));
            }
        }
        lines
    }

    #[test]
    fn cluster_metadata_rejects_bad_records_and_bounds() {
        let lines = cluster_fixture();
        let joined = |v: &[String]| v.join("\n");
        let check = |v: &[String]| cluster_metadata(&joined(v), 950_000, 1_000_000, 17_000_000);
        assert!(check(&lines).is_ok());
        let mut missing = lines.clone();
        missing.remove(4);
        assert!(check(&missing).is_err());
        let mut reordered = lines.clone();
        reordered.swap(4, 5);
        assert!(check(&reordered).is_err());
        let mut duplicate = lines.clone();
        duplicate.insert(2, duplicate[2].clone());
        assert!(check(&duplicate).is_err());
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
            (15, "timer_us"),
        ] {
            let mut bad = lines.clone();
            let mut f: Vec<_> = bad[2].split_whitespace().map(str::to_owned).collect();
            f[field] = value.to_string();
            bad[2] = f.join(" ");
            assert!(check(&bad).is_err(), "field {field}");
        }
        let mut header = lines.clone();
        header[0] = "CLUSTER-HEADER driver_wake 950000 17000001".into();
        assert!(check(&header).is_err());
        let mut header = lines.clone();
        header.push(lines[0].clone());
        assert!(check(&header).is_err());
    }

    #[test]
    fn cluster_envelope_and_certified_audits_are_conservative() {
        let meta = cluster_metadata(&cluster_fixture().join("\n"), 950_000, 1_000_000, 17_000_000).unwrap();
        let first = meta[0][0];
        assert_eq!(
            cluster_window_bounds(first.upper, first.upper - first.lower, first),
            Ok((first.lower, first.upper))
        );
        assert!(cluster_window_bounds(first.upper - 1, first.upper - first.lower, first).is_err());
        // The old E minus RTC duration could precede release, but the full v3 envelope qualifies.
        assert!(first.early < first.upper && first.lower >= 1_000_000);
        let m = cluster_metric(&[(999_999, 1_000_004), (1_000_006, 1_000_010)], 1_000_000, 1_000_008, 10)
            .unwrap();
        assert_eq!((m.gross, m.credit, m.net), (8, 5, 3));
        // Adjacent outer uncertainty bins overlap and are counted once.
        let overlap = cluster_metric(&[(10, 13), (13, 16)], 10, 17, 7).unwrap();
        assert_eq!(overlap.lower_witness, 0);
        assert_eq!(cluster_metric(&[(10, 11)], 10, 12, 2).unwrap().credit, 0);
        assert_eq!(cluster_metric(&[(1, 2)], 10, 12, 2).unwrap().credit, 0);
        assert!(cluster_metric(&[(0, 20)], 10, 12, 0).unwrap().credit <= 2);
    }

    #[test]
    fn cluster_go_protocol_distinguishes_readiness_from_window_and_release() {
        fn add(records: &mut Vec<Record>, kind: char, id: u64) -> usize {
            let at = records.len();
            records.push(Record { seq: at as u64, entry: at as u64, kind, id, pass: 1 });
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
        let mut push = |kind: char, records: &mut Vec<Record>| {
            records.push(Record {
                seq: records.len() as u64,
                entry: records.len() as u64,
                kind,
                id: 17,
                pass: 1,
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
        extra.insert(4, Record { seq: 0, entry: 0, kind: 'W', id: 17, pass: 1 });
        assert!(cluster_waits_before(&extra, 17, 0, boundary + 1).is_err());
        let mut reordered = records;
        reordered.swap(1, 2);
        assert!(cluster_waits_before(&reordered, 17, 0, boundary).is_err());
    }

    #[test]
    fn cluster_fences_reject_missing_duplicate_and_wrong_callers() {
        let rec = |kind, id, time| Record { seq: 0, entry: 0, kind, id, pass: time };
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
                .map(|(i, &(kind, id, pass))| Record { seq: i as u64, entry: i as u64, kind, id, pass })
                .collect::<Vec<_>>()
        };
        assert!(check_wake_no_preempt(&records(&events), 1).is_ok());
        let mut preempted = events.to_vec();
        preempted.insert(4, ('K', 8, 10));
        assert!(check_wake_no_preempt(&records(&preempted), 1).is_err());
        let mut unframed = events;
        unframed[3] = ('B', 8, 10);
        assert!(check_wake_no_preempt(&records(&unframed), 1).is_err());
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
                .map(|(i, &(kind, id, pass))| Record { seq: i as u64, entry: i as u64, kind, id, pass })
                .collect::<Vec<_>>()
        };
        assert!(check_carve_return("CARVE-OBS 900 950 12345\n", &records(&events)).is_ok());
        events.insert(7, ('K', 5, 150));
        assert!(check_carve_return("CARVE-OBS 900 950 12345\n", &records(&events)).is_err());
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

    #[test]
    fn shares_are_judged_net_of_audits() {
        // The window [100, 300] holds the first audit whole (24 µs) and none of the second: net
        // 176 µs, of which 88 µs of CPU is half (gross 440).
        let log = audited("SHARE victim 100 300 88 450 1000\n");
        let ok = run(&log, "");
        assert!(
            ok.as_ref().is_ok_and(|s| s.contains(
                "share victim: net 500, gross 440 of 1000, audits 24 µs, 0 timer interrupts nobody's, 0 finding another budget's wait ended early: target met (450 <= share <= 1000)"
            )),
            "{ok:?}"
        );
        // An upper bound too, and a share out of its bounds.
        assert!(run(&audited("SHARE shell 100 300 88 450 499\n"), "").is_err_and(|e| {
            e.contains("share shell: net 500, gross 440 of 1000, audits 24 µs, 0 timer interrupts nobody's, 0 finding another budget's wait ended early: target missed")
        }));
        // Unsubtracted, the same share misses: a stamp the kernel left out is time the oracle
        // never saw.
        let unstamped = trace(&[(2, 'W', 1, 5), (2, 'K', 1, 5)]) + "SHARE victim 100 300 88 450 1000\n";
        assert!(run(&unstamped, "").is_err_and(|e| e.contains("net 440, gross 440")));
        // Malformed: a field short or over, an empty window, bounds reversed or past the whole.
        for bad in [
            "SHARE victim 100 300 88 450\n",
            "SHARE victim 100 300 88 450 1000 7\n",
            "SHARE victim 300 300 88 450 1000\n",
            "SHARE victim 100 300 88 500 450\n",
            "SHARE victim 100 300 88 450 1001\n",
        ] {
            assert!(run(&audited(bad), "").is_err_and(|e| e.contains("malformed")), "{bad:?}");
        }
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
        for head in good {
            let v = verdict(&[head.clone(), pick.to_vec()].concat());
            assert!(
                v.as_ref().is_ok_and(|s| s.contains("1 timer interrupts billed by the rule")),
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
        // The ones nobody pays for are counted inside each share's window.
        let empty = [(1, 'I', 1, 100), (1, 'O', 0, 1), (1, 'I', 1, 300), (1, 'O', 0, 1)];
        let log = trace(&[empty.to_vec(), pick.to_vec()].concat()) + "SHARE victim 50 200 75 450 1000\n";
        let v = run(&log, "");
        assert!(v.as_ref().is_ok_and(|s| s.contains("1 timer interrupts nobody's, 0 finding")), "{v:?}");
        // A case can require interrupts that found another budget's wait ended early in a share's
        // window: one inside (at 100 µs, budget 1 interrupted, 2's wait) meets it; none fails it.
        let stale = [(1, 'I', 1, 100), (1, 'E', 2, 2), (1, 'B', 2, 9), (1, 'O', 0, 1)];
        let log = trace(&[stale.to_vec(), pick.to_vec()].concat()) + "SHARE victim 50 200 75 450 1000\n";
        let v = run(&log, "stale_waits_in=victim");
        assert!(
            v.as_ref().is_ok_and(|s| s.contains("1 finding another budget's wait ended early: target met")),
            "{v:?}"
        );
        let own = [(1, 'I', 2, 100), (1, 'E', 2, 2), (1, 'B', 2, 9), (1, 'O', 0, 1)];
        let log = trace(&[own.to_vec(), pick.to_vec()].concat()) + "SHARE victim 50 200 75 450 1000\n";
        let v = run(&log, "stale_waits_in=victim");
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
            out.push(Record { seq, entry, kind, id, pass });
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
