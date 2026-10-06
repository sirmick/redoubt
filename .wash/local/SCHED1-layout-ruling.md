# SCHED1 fixture checkpoint ruling

Applies to `.wash/local/SCHED1-layout.md` and refines `.wash/local/SCHED1-brief.md`.
Evaluation only: no successful measurement or package acceptance is implied. The Architect
read the layout and the needed trace, interrupt, fixture and model-contract sections;
no code, tests or book pages were changed or run for this ruling.

## Authorized scope

Proceed with the uniform 1 ms constant in kernel, model and slice-relative fixture constant,
and the already authorized unchanged pinned latency measurements on both widths first.
Preserve every numerical latency/share target and all pass, rank, charge, inheritance,
audit and preemption rules. The proposed cluster fixture and a bounded host post-check are
authorized subject to the corrections below. No new trace field, larger ring, general
kernel profiler, new scheduling policy or throughput-loss tolerance is authorized.

## W/K phase and sample proof

W and K carry sequence, reconcile entry, budget and pass; they are not timestamps. Exact
fractional position within the slice is **not required** to demonstrate the cluster/debt
failure. Label [100, 300, 600, 850] us as programmed offsets, not observed offsets from a
pick. Keep the same list on the two builds. Use the existing independent replay to prove
the waking rank, floor, lower-ranked spinners ahead and picks before service. Trusted
deadline-to-service sample windows establish measured latency, net of stamped audits.
Neither W-to-K time nor exact switch cost can be recovered from pass increments.

For the timeout no-preempt attack, use existing timer framing: a spinner K precedes an I
that names that spinner, the sleeper W occurs inside the I ... O interval, and O returns
to user (`1`) without an intervening K. This proves a timeout wake during an unended slice
and continuation of the interrupted spinner. In this controlled interval exclude budget
deadlines/destructions; require the later slice-ending timer interval to return to kmain
(`O=0`), with the spinner requeued and the subsequent pick checked by the rank oracle.
This establishes event order without asserting an unobserved pick timestamp. The original
host/model mutations that preempt on wake must still be caught.

Use record **sequence and the I/O brackets**, not equality of entry numbers, for this
association. `trace::entry()` increments in reconcile near kernel exit, after I is emitted;
W inside that interrupt can therefore have a different entry number from its I. Use the
existing timer parser's ordering convention. A driver IRQ does not emit timer I records:
never attach its W to a nearby timer I merely because the timestamps are close.

For the one-thread stand-ins, setup must establish an unambiguous budget-ID mapping and
an observable boundary between setup and measurement. Isolated setup wake order is
acceptable if validated against the trace and the trusted fixture's fixed creation/start
sequence, not inferred by assuming convenient numeric IDs. During measurement, require
one real blocking wait and exactly one W per reported sample in each stand-in's sequence.
No extra setup/report wake may consume a sample index. An immediate completed receive,
lost IRQ, extra W or ambiguous mapping fails the join; do not discard it to get coverage.
Keep original sample order for the join, sorting only copies when computing percentiles.
Trusted RTC/timer samples supply their own deadlines and service times. For driver wakes,
the controlled single IRQ-wait behavior plus the validated W/sample order is sufficient;
no physical IRQ-arrival timestamp is claimed.

`sched-wake-no-preempt.toml` currently has no trace feature or post-check. Explicitly allow
adding `sched-trace`, zero-drop expectation and the bounded ordering post-check there to
make the proposed proof executable. Keep the 300/400 us adaptation as an initial fixture
placement, not a waiver of trace qualification. Model contract changes to 500 and
100/450/450 plus the final one-unit step preserve its mid-slice/next-pick assertion;
the separate absolute equal-expiry contract stays unchanged.

## Release, debt categories and capacity

The proposed common release is an experiment, not a proof that relative receive timeouts
expire together. Conversion and call entry take time. Arrange an already-running spinner
or busy server across release so the queue need not pick the first waker from idle before
the rest wake. Do not stagger the sixteen spinners to manufacture favorable passes.

Retain the proposed proof obligations: all sixteen distinct spinner wakes before the next
pick, still queued there, and the stated <=100 us equivalent pass spread; floor/lead
classified at each W using the independent replay; positive lead is `W.pass` above the
replayed floor. (Amended 2026-10-05 by the Architect's coverage ruling,
`SCHED1-coverage-ruling.md`: the former ">=8 spinners ranked ahead" measured the slice, not
the debt. Under a 1 ms slice a weight-1000 waker is picked within one own slice, 10 * 2^20,
of the lowest spinner, while spinners space about 100 * 2^20 apart after release, so >=8
ahead is unreachable at 1 ms whatever the waker owes; it is the 10 ms attack's signature.
`ahead` stays a recorded witness, reported as min/median/max per category for candidate and
control alike by the same oracle with no new parameter. The old control is non-vacuous only
if at least 25 of its positive-lead wakes per stand-in show `ahead >= 8`.) Retain at least
25 zero-lead and 25 positive-lead samples per stand-in, with >=5
per category at each programmed offset. Offset coverage describes the programmed attempts;
it is not proof of exact sub-slice timing. Failure to obtain these categories is a fixture
failure to report, not evidence for or against the latency target by itself. Do not relabel
positive lead, choose only fast samples or reduce coverage after observing results.

Compute driver/timer p50 and p99 over all 200 valid samples, not only the qualifying ones;
report category summaries in addition. The old 10 ms control must meet the same coverage
and fail a latency target on each width. A missing-category or malformed-log failure is
not that negative control. Keep fixed setup/phase parameters identical between old and
candidate; if construction needs revision, propose it before comparing changed fixtures.

The 2,097,152-record estimate is plausible for evaluation, not a proven bound. Keep 64 MiB
and `dropped 0`. Actual record counts, particularly in worst-walk, decide capacity; stop
on a drop rather than shrinking measurement windows, suppressing records or enlarging RAM.

## Carve-return feasibility

The 9/10-slice target includes the create syscall. Change 9,000 us to `9*SLICE_US/10`
and measure it; the source's “about a millisecond” is a warning, not a current measurement.
If create returns after that target but before the actual slice end, allow immediate
destruction without extra spinning. The trace must still show a positive lead accumulated
at the low weight and the return in the same running turn: identify the parent's K and
down/up weight changes, with no intervening requeue/deschedule/new pick of that parent.
Retain the 999/1000 carve, restored-weight share bound and independent reweigh checks.
Report actual create/return observations rather than claiming exactly 900 us of user work.

If create itself exhausts the slice or preemption makes the return miss that running turn,
**stop that candidate evaluation and report the trace**. Do not hold off interrupts, make
a special longer slice, exclude create's bill, lower the carve ratio, enlarge the victim's
tolerance or count a post-starvation window as equivalent evidence. This would establish
that the old fixture's scheduling assumption is infeasible, not by itself that rescaling
is incorrect. A controller-assisted return could be a separate fixture redesign, but is
not authorized here without its own negative-control argument. Existing host/model tests
alone do not replace the target attack.

## Required correctness versus diagnostics

Required: unchanged pinned gates and later sweeps/whole-bench acceptance; independent rank,
floor/reweigh/lift and timer-billing checks already present; non-vacuous cluster and no-wake-
preempt evidence; retained sleep-gaming, carve/budget/lease attacks; zero dropped records;
paired useful-work throughput and available switching counts reported honestly. A passing
rank trace is not a complete proof of all runtime billing; keep the existing differential,
mutations and real share attacks as specified in the brief.

Total kernel runtime is not in this trace and is **not a new gate**. Timer B records give
only timer billing; R10 spans give destruction duration and can overlap that billing, so
do not sum them as disjoint work. K gives budget picks, not thread identities or timestamped
switches; infer thread changes only in the proven one-thread-per-budget interval. Missing
exact per-pick phase, generic syscall runtime or total switch cost is an observability
limit to state, not grounds to expand instrumentation. Compare useful-work throughput,
picks/budget changes and audits on identical configurations and same seeds. There is no
numeric throughput-loss allowance: report the data for review and stop on an unexplained
material regression rather than inventing acceptance criteria.

On a candidate target miss, fairness/lease regression, oracle failure, coverage failure,
ambiguous join, infeasible carve placement or trace drop, retain head/seed/command/log and
return the specific failure. Read-only diagnosis is allowed; widening policy or mutating
the oracle to pass is not. Known baseline target failures are expected negative evidence.
No first-slice or category success is promised before measurement. The scheduler-only
replay/acceptance/merge first, followed by rebased IPC3 final acceptance, is unchanged.

## Accepted revision: relative waits after positive preparation

Accepted for bounded fixture evaluation only. The complete construction, evidence limits,
allowed paths, controls and stop conditions are in
[SCHED1-relative-wait-ruling.md](SCHED1-relative-wait-ruling.md). This revision supersedes
the original positive-intent deadline placement; every other obligation above stands.

The diagnostic run completed four samples, then positive-intent attempt index 4 resumed
620 us past its fixed target and failed before `receive`. That is a construction failure,
not a measured scheduler wake-latency failure. Keep 80 ms nominal slots
`A_i = release + i * 80000 us`, offsets `P_i = [100, 300, 600, 850][i % 4]`, the intent
sequence, 200 attempts per stand-in and the fixed measurement end. Zero-intent attempts
retain their one wait toward `A_i + P_i`, failing if already late. Every positive-intent
attempt spins until `A_i`, then programs one relative wait of `P_i` from its actual
post-spin arming reference. Remove the `target - 300 us` assumption. This is a uniform
construction, never a fallback that moves an expired armed deadline.

Use the identical revised fixture and oracle for candidate and old 10 ms controls on
both widths. Preserve all 200 samples, category/offset minima, positive-debt/rank proof,
cluster/spread, window, numeric latency, audit and zero-drop requirements. The old
control must qualify and fail a latency gate. Do not retry, replace or exclude samples,
add preparatory waits, extend windows, search phases, or change scheduler policy.

Relative arming does not guarantee blocking across preemption: an already pending RTC
interrupt can return immediately under R5. Every sample still needs exactly one proven
blocking wait/W/service join, with setup and report wakes excluded. W/K do not timestamp
physical slice phases. Report ordered per-attempt slot, arming/deadline and service
metadata after measurement; report preparation slip separately from wake latency.
Version the plan/parser together and retain bounded negative checks for missing,
immediate, duplicate/reordered and report-substituted samples. Actual coverage decides
whether this construction works; no qualifying result is promised.

The implementer may prepare the scoped revision and report its exact checkpoint.
The orchestrator controls run release; MEM1 retains the current QEMU window. Stop at
the first construction, join, coverage, oracle, drop or candidate latency failure and
return source identity, seed, command and log. No new owner tradeoff is decided here.
