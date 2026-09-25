//! An independent check of the kernel's stride ranks (WP-K5; KERNEL-SPEC.md R12, the owner's four
//! rank clauses), over the raw events a `sched-trace` kernel prints at `system_reset`.
//!
//! The kernel's trace says what its queue did, never why: a budget woke (`W`), was requeued (`R`),
//! left the queue (`D`), had its pass changed (`P`), or was picked (`K`); each record carries the
//! kernel entry (reconcile) it belongs to and the budget's pass after the event (its low 64 bits).
//! It holds no tie key. This module rebuilds the order from those events alone, with its own reading of the rules,
//! and checks every pick against it:
//! - the lowest pass first; at an equal pass,
//! - a budget that woke ranks ahead of one that was requeued;
//! - of two that woke, the later kernel entry's first, and within one entry the lower id;
//! - requeued ones in the order they were requeued.
//!
//! A destruction's lift (a child's work since entry moving to its parent) is recorded as a group
//! of nine records, every operand and the result, and recomputed here from the rule as the spec
//! states it (KERNEL-SPEC.md R12, Inheritance): W = (child pass - max(entry, floor))+ x child weight
//! + child remainder; the parent becomes max(its pass, floor) + W / its weight, keeping its
//! remainder only if it was not below the floor, the remainders carried.
//!
//! It does not only trust the passes it is given. It keeps its own **floor**, the queue's lowest
//! pass, never lowered (so it holds while the queue is empty) and raised after a reconcile's wakes,
//! as the kernel's is, and requires every wake's pass to be at least that floor; and it requires
//! every budget's pass never to fall.
//!
//! Each destruction (R10) is bracketed by `X` and `Y` records carrying the time in µs in the pass
//! field; the check reports their durations, and a case can bound their p99
//! (`post_check = "sched_oracle r10_p99_us=30000"`).
//!
//! A trace that is malformed, incomplete, lost records or holds no pick is rejected: a check that
//! saw nothing proves nothing.

use std::collections::BTreeMap;

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
        if f[2].len() != 1 || !"WRDPKLlefrqwAaXYZ".contains(kind) {
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
    let g = records.get(..LIFT.len()).ok_or_else(|| format!("record {}: a lift group cut short", records[0].seq))?;
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

/// What a check covered.
#[derive(Debug, Default)]
pub struct Summary {
    pub picks: usize,
    pub lifts: usize,
    /// Lifts a `max` rule would have got wrong (a leading parent, work to move).
    pub telling: usize,
    /// Each destruction's duration, µs, in trace order.
    pub r10_us: Vec<u64>,
    /// The most object frames present at a destruction's start (R10 walks them all).
    pub r10_frames: u64,
}

/// Check every pick in `records` against the four clauses, the floor and every pass's
/// monotonicity, and every lift against the rule.
pub fn check(records: &[Record]) -> Result<Summary, String> {
    let mut queued: BTreeMap<u64, (u128, Key)> = BTreeMap::new();
    let mut last_pass: BTreeMap<u64, u128> = BTreeMap::new();
    let mut floor: u128 = 0;
    let mut open_r10: Option<(u64, u128)> = None;
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
            return Err(format!("record {}: budget {} woke at pass {:#x}, below the floor {floor:#x}", r.seq, r.id, r.pass));
        }
        match r.kind {
            'X' => {
                if let Some((id, _)) = open_r10 {
                    return Err(format!("record {}: a destruction began inside budget {id}'s", r.seq));
                }
                open_r10 = Some((r.id, r.pass));
            }
            'Z' => sum.r10_frames = sum.r10_frames.max(r.pass as u64),
            'Y' => match open_r10.take() {
                Some((id, t)) if id == r.id => sum.r10_us.push(r.pass.saturating_sub(t) as u64),
                _ => return Err(format!("record {}: budget {}'s destruction ended without beginning", r.seq, r.id)),
            },
            'L' => {
                let (used, tells) = check_lift(&records[i - 1..])?;
                i += used - 1;
                sum.lifts += 1;
                sum.telling += usize::from(tells);
            }
            'l' | 'e' | 'f' | 'r' | 'q' | 'w' | 'A' | 'a' => {
                return Err(format!("record {}: a lift record outside a lift group", r.seq));
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

/// The bench's post-check: parse the case's console log and check it. `args` may bound the p99
/// of R10's durations: `r10_p99_us=N`.
pub fn run(log: &str, args: &str) -> Result<String, String> {
    let records = parse(log)?;
    let mut sum = check(&records)?;
    let n = sum.r10_us.len();
    let (p50, p99, max) =
        (percentile(&mut sum.r10_us, 50), percentile(&mut sum.r10_us, 99), sum.r10_us.last().copied().unwrap_or(0));
    for arg in args.split_whitespace() {
        let bound = arg
            .strip_prefix("r10_p99_us=")
            .and_then(|v| v.parse::<u64>().ok())
            .ok_or_else(|| format!("unknown sched_oracle argument {arg:?}"))?;
        if n == 0 {
            return Err("r10_p99_us is set, but the trace holds no destruction".into());
        }
        if p99 > bound {
            return Err(format!("R10's p99 is {p99} µs over {n} destructions, above {bound}"));
        }
    }
    Ok(format!(
        "sched_oracle: {} records, {} picks in rank order, every wake at or above the floor, no pass falling; {} lifts by the rule ({} with a leading parent and work to move); R10 {n} destructions over up to {} object frames, µs p50/p99/max {p50}/{p99}/{max}",
        records.len(),
        sum.picks,
        sum.lifts,
        sum.telling,
        sum.r10_frames
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn lift_trace(pb: u128, pr: u128, cp: u128, cr: u128, e: u128, f: u128, wc: u128, wp: u128, pa: u128, par: u128) -> String {
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
        let together = [(1, 'W', 1, 9), (1, 'K', 1, 9), (1, 'D', 1, 9), (2, 'P', 3, 12), (2, 'W', 3, 12), (2, 'P', 2, 9), (2, 'W', 2, 9), (2, 'K', 2, 9)];
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
        let t = trace(&[(1, 'X', 7, 100), (1, 'Y', 7, 130), (1, 'X', 8, 200), (1, 'Y', 8, 250), (1, 'W', 1, 5), (1, 'K', 1, 5)]);
        let ok = run(&t, "r10_p99_us=50");
        assert!(ok.as_ref().is_ok_and(|s| s.contains("R10 2 destructions over up to 0 object frames, µs p50/p99/max 30/50/50")), "{ok:?}");
        assert!(run(&t, "r10_p99_us=49").is_err_and(|e| e.contains("above 49")));
        assert!(run(&t, "r10_p99=49").is_err());
        // Unpaired or nested brackets.
        assert!(verdict(&[(1, 'Y', 7, 1), (1, 'W', 1, 5), (1, 'K', 1, 5)]).is_err());
        assert!(verdict(&[(1, 'X', 7, 1), (1, 'X', 8, 2), (1, 'W', 1, 5), (1, 'K', 1, 5)]).is_err());
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
            (
                "a pick of nothing queued",
                "SCHED-TRACE 0 1 K 1 5\nSCHED-TRACE-END 1 dropped 0\n".to_string(),
            ),
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
