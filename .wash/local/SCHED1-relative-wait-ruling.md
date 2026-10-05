# SCHED1: relative waits after positive-debt preparation

Design checkpoint for assignment `e600c5971bf9594ad8ab9a3b817c88f5`, QA
`IPC3-wake-latency`. Records the two-part ruling already sent to the orchestrator and
`sched1-implementer-3`. Complements `SCHED1-layout-ruling.md`; evaluation is not acceptance.

## Finding

The diagnostic log at
`.worktrees/SCHED1/target/testbench/run-1-1791157775978771536/sched-cluster-rv64-smp1.log`,
lines 122–124, reports driver index 4, branch 2, target 320100 us and current time
320720 us, with four samples completed. This is the first positive-intent attempt.
The fixture spun toward `target - 300 us`, resumed after the target and rejected zero
delay before calling `receive`. The assumption that preparation leaves 300 us available
is false. There was no fifth measured wait: this is not a wake-latency verdict or proof
of a scheduler rank/billing defect. The log does not identify every contributor to the
620 us overrun and contains no completed trace proof for the first four reports.

## Allowed construction

Keep the common spinner release, weights, isolated setup, 80 ms nominal slots, 200
attempts per stand-in, intent sequence, and existing fixed measurement end. For attempt
`i`, define `A_i = release + i * 80000 us` and
`P_i = [100, 300, 600, 850][i % 4]`.

- Zero intent: keep one wait toward `A_i + P_i`; a target already passed still fails.
- Positive intent: spin until `A_i`, then program one relative wait of `P_i` from the
  actual post-spin arming reference. Apply this rule to every positive attempt. Remove
  the old `target - 300 us` preparation and absolute-target subtraction on this path.
- Driver: arm a fresh RTC deadline from that reference; retain the interrupt outcome
  check, timeout failure and clearing behavior. Timer: issue the relative timeout and
  retain its timeout outcome check and trusted deadline/service accounting.
- Do not add a preparatory wait, conditional fallback, retry, replacement sample,
  staggered spinner release, larger margin, phase search or extended window.

This changes how positive attempts choose their deadline before arming. It does not
move an already armed deadline after observing lateness. Positive debt still comes
from ordinary billed execution and is accepted only when independent replay witnesses
it. Zero-intent construction remains unchanged. Identical revised fixture source,
phase list, parameters and oracle must run with the candidate and old 10 ms slice.
The original `sched-latency` workload and gates remain unchanged.

## Evidence and limits

Relative programming removes the demonstrated zero-delay subtraction failure. It
cannot guarantee a future RTC alarm is still in the future when `receive` begins:
preemption can occur between arming and entering the kernel. R5 in `devices.md` also
allows an already pending interrupt to return immediately. Such a return is not a
qualifying blocking wait. Do not disable preemption or change interrupt semantics.

Store bounded per-attempt metadata and report it after measurement: index, intent,
nominal slot, actual arming reference/deadline and service time, with clock units made
explicit. Preserve order. Report preparation slip separately; do not add it to wake
latency or subtract it from lateness after an armed deadline. Update the fixture plan
version and parser together so old logs cannot qualify as the corrected construction.
Do not add kernel trace fields. W/K are ordering/rank evidence, not timestamps.

The offsets are programmed durations after positive preparation and nominal-slot
offsets for zero intent. They are not observed phases relative to a scheduler pick.
Trusted deadline/service windows measure latency; existing trace replay establishes
the distinct, real blocked-wait/W/service sequence and debt/rank at each wake.
Require an unambiguous setup/measurement/report boundary. Exactly one measurement W
must join each of 200 ordered windows per stand-in; extra reporting wakes cannot fill
a missing measurement wake. Taking the first 200 W without proving that boundary is
insufficient. Immediate waits, missing/extra wakes and ambiguous joins fail.

Keep all prior empirical requirements: 16 distinct queued spinner wakes before the
next pick; at most 100 us equivalent pass spread; at least 25 zero-lead and 25
positive-lead samples per stand-in, at least five per category per offset; positive
lead with at least eight spinners ahead. Judge latency over all 200 samples, not a
qualifying subset. Preserve every numerical latency gate, audit netting, the 64 MiB
trace ring and zero drops. Both sample deadlines and service windows must remain
within the declared measurement window; do not silently enlarge it.

## Controls and release

The old 10 ms build must satisfy the same construction, wait joins and coverage and
then fail a latency gate on each width. A malformed or under-covered old run is not
a negative control. No category or old-control success is promised by this design.
Retain independent rank/charge checks and existing scheduler mutations. Bound the
oracle checks to this adaptation: reject an old plan version, a missing or immediate
wait, a duplicate/reordered sample, and a report wake substituted for a missing
measurement W. Preserve existing negative checks for rank, coverage and latency.

Allowed implementation paths are the cluster role/helper, cluster fixture and its
manifest, and the bounded cluster parser/join checks. No scheduler policy, kernel
instrumentation, quantum search, other workload, trace capacity or tolerance change.
The implementer reports the exact diff/checkpoint; the orchestrator controls test
release and ordering. MEM1 currently owns QEMU. Stop on the first construction,
join, coverage, oracle, trace-drop or candidate latency failure; retain source
identity, seed, command and log and return the specific evidence. Do not retry or
tune around it. Architect performed no source/test edits or test runs.

## Accepted end fence: existing destruction records

Accepted for bounded evaluation with the following five conditions. This supplies the
measurement/report boundary required above; it does not change sample qualification.

1. Create exactly two empty marker budgets during setup, with `FOREVER` deadlines and
   zero pages, processes and weight. Prefer a non-measured parent. A zero-limit budget
   still costs object storage and setup work: report those costs, not zero overhead.
   No marker threads, descendants, expiring deadlines or nonzero weight carve; preserve
   the measured budgets' free weights. Do not use `Bench::budget`'s oversized allocation.
2. Each stand-in destroys only its own marker after storing sample 200 and its service
   metadata, before any stats, report or `hand_over` operation. Check destruction success.
   Its existing R10 `X` is the exclusive end fence; require the unique matching `Y`.
   A partial or failure-path run must never qualify.
3. `X/Y` name the destroyed budget, not its caller. Ordered setup alone does not expose
   an empty marker's ID because it has no W. Never equate handles with IDs or assign
   markers by destruction order. A bounded attribution proof uses the most recent K
   at X to identify the still-running one-thread stand-in, together with the trusted
   exclusive marker-destroy call sites, FOREVER lifetimes, no other destruction in
   this interval, distinct marker IDs and exactly one X/Y pair per stand-in. Reject an
   intervening deschedule invalidating attribution or any ambiguity. An alternative
   needs an equally explicit proof from existing trace records.
4. Between the validated go/setup boundary and that stand-in's X, require exactly 200
   blocked D-W-service cycles and 200 ordered samples. Exclude later W from sample
   matching. Keep reliable existing stats sends: blocking after X cannot contaminate
   measurement joins, and a nonblocking report can be lost unnecessarily. Negative
   checks must reject absent, duplicate or wrong-caller fences and 199 real waits plus
   a reporting W substituted as sample 200.
5. A fence follows its own stand-in's samples, but can overlap the peer's tail. Report
   X/Y durations and retain overlapping R10 cost in the peer's latency. Never subtract
   that ordinary kernel work as audit or claim the fences are globally outside the
   measurement. Use identical fences on candidate and old-slice control builds.

All previous gates and stop conditions remain. The implementer's source checkpoint
and red review precede machine release; the orchestrator controls that release.

## Accepted clock-boundary proof

Accepted for a bounded source fix, followed by red re-review. Driver RTC metadata
uses nanoseconds; the declared window and existing latency sample end use kernel
microseconds. Keep the existing latency sample end, gross latency, audit netting and
numeric gates unchanged. Add one successful `time_now()` read immediately after the
RTC service read, stored per sample and reported in order after measurement.

Let `E` be the existing kernel timestamp sampled before RTC service, `S` the RTC
service timestamp, `D` the RTC deadline, and `P` the new kernel timestamp sampled
after service. Require `S >= D`, `E <= P` and `P <= window_end`. This upper-bound
check uses the existing integer-microsecond resolution, not a sub-microsecond claim.
Preemption before `P` may conservatively reject a sample; it is not grounds for a waiver.

For the lower boundary compute, with checked subtraction:

`L = E - ceil((S - D) / 1000)`

Require `L >= window_release`. The existing gross latency floors that nanosecond
duration; `E - gross` can consequently be almost one microsecond too high to be a
conservative real-time deadline bound. Use ceiling only for this containment check,
not to change latency statistics or audit subtraction.

The proof assumes the existing monotonic, same-rate QEMU clocks, not equal epochs.
If kernel-clock time at RTC service is `K_s`, kernel-clock time at its deadline is
`K_d = K_s - (S - D) / 1000`. Since `E <= K_s`, the computed `L <= K_d`; the RTC
epoch cancels in its duration. `L < window_release` means containment is unproved
and qualification fails, not that the actual deadline was proved early. A changed
clock-rate assumption requires a new mapping proof.

An explicit kernel timestamp before RTC arming plus the relative delay would also
provide a lower bound, but is unnecessary for this construction and does not remove
the shared-rate assumption. Use the existing rounded lower bound and new post-service
timestamp consistently on candidate and control builds. Timer samples already use
kernel microseconds: check their deadline against release and service against end
directly. Timestamp containment never replaces the blocked-wait and fence proof.

Reject missing, duplicate or mismatched post-service metadata and failed timestamp
reads. Negative checks include `E <= window_end` with `P > window_end`, fractional-
microsecond lateness at release, invalid timestamp order and a missing `P`. The
orchestrator controls test release after red re-review; this ruling authorizes no run.
