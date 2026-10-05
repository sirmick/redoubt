# SCHED1: one kernel window and conservative latency envelopes

Proposal for assignment `3382d196e606bedf921456cf2e33b140`, QA
`IPC3-wake-latency`. This is one prospective construction for root checkpoint review,
not implementation authorization, a machine grant, or an amendment of the archived
run. `.wash/local/SCHED1-sample0-clock-ruling.md` remains final for that run.

## Decision proposed

Use kernel microseconds for the cluster's entire declared workload window. Measure
each attempt with an explicitly reported kernel-clock interval that contains its
physical deadline-to-service interval. Apply the unchanged numerical latency limits
to that interval after subtracting only certified audit time inside it. Keep the RTC
duration as an additional physical measurement, never translate it by pretending
its service read happened at an earlier kernel timestamp.

This requires a **declared cluster measurement-definition correction**. It is not
the previous E/gross rule with one check removed. Candidate and old-10-ms control
must both be rebuilt and measured with this same construction on both widths. No
existing cluster observation qualifies under the new version.

## What is a bug, and what remains uncertain

The exact sources and hashes are in the sample-0 ruling. Relevant preserved source:
`tests/programs/src/sched.rs:2092–2103` computes the child window from `ticks()` and
the printed window from a later `time_now()`. Even with the correct 10 ticks/us,
these are two different anchor instants, and kernel time also subtracts BOOT_TICKS.
Treating these independently formed endpoints as one window is a construction bug.
The size of their separation in the archived run was not recorded; it is not proved
to be the cause of the sample-0 failure.

At `sched.rs:437–444`, kernel E is read before RTC service S. Gross is the RTC
duration S-D. Therefore `[E-floor((S-D)/1000), E]` is displaced from the physical
deadline/service interval by the E-to-S separation, apart from rounding. This is an
interval-definition mismatch, not evidence that the RTC rate is wrong. The old
conservative L proof was sound and the old sample properly failed it; neither L nor
E-gross is the actual deadline. Sample 0's physical deadline remained undecided,
while its defined audit interval indisputably began 27 us before printed release.

Source also confirms a timer distinction worth preserving explicitly: the timer
stand-in's pre-call `time_now()+delay` is a lower bound, not the kernel's exact
timeout deadline. `kernel/src/message.rs:658` adds the delay to its own later
`time_now`. The proposed definition honestly includes that call-entry separation.

## One workload window, without a raw-counter mapping

Add a cluster-only go helper and corresponding cluster interpretation of its one
go message. Do not change general `Bench::go` or other workloads.

1. After the existing isolated readiness setup, obtain one successful kernel
   timestamp T. Compute, with checked arithmetic, start `H=T+200000`, release
   `R=H+50000`, and fixed end `F=R+16000000`, all in microseconds.
2. Send the same H and F, as the existing two 64-bit values, in the existing go
   delivery order to all 19 children. Print H/R/F from those exact values. Every
   child computes R=H+50000. No second clock read defines or adjusts an endpoint.
3. Cluster busy work counts the existing fixed 256-iteration chunks, checking
   successful `time_now()` between chunks against F. Positive preparation similarly
   checks kernel time between chunks until its nominal slot. The cluster uses no
   absolute `rdtime` endpoint. Keep the current calibration output for the oracle's
   tick/pass-unit checks; it is not a conversion between workload clock epochs.
4. Each spinner takes one successful kernel read B and one timeout receive of
   `R-B`, failing if B>=R. The later actual timeout can be late; the existing
   sixteen-W/common-pick/spread proof must establish the cluster. No per-spinner
   delay adjustment or extra preparatory wait is permitted.

These clock checks add ordinary billed kernel work to the spinning workload. They
are deliberate and identical in both builds, counted in useful-work comparison and
rank/debt replay. They are not promised to preserve old throughput or coverage. This
small cluster-specific change avoids calibration brackets, per-child epoch mappings,
new trace fields and a kernel timing API. If its overhead prevents qualification,
stop rather than changing chunk size or clock-check frequency to obtain coverage.
As with the old chunk loop, work can return from its final check after F; no sample
endpoint is thereby extended and no last partial chunk is a new latency sample.
Compare raw useful-work counts for this identical new loop across the two builds;
do not normalize them with `Bench::rate` from the different raw-counter loop.

## Per-attempt construction and records

For i=0..199 keep `A_i=R+i*80000`, phase
`p_i=[100,300,600,850][i%4]`, and positive intent `i/4 % 2 == 1`.

- Zero intent obtains successful kernel B, computes `delta=A_i+p_i-B` with checked
  subtraction, and fails if delta is zero or the target has passed.
- Positive intent does the chunked kernel-clock spin to A_i, then obtains successful
  kernel B, requires B>=A_i, and uses delta=p_i.
- Define the lower envelope endpoint `L=B+delta` with checked addition. It is fixed
  before arming. Zero intent gives L=A_i+p_i; positive intent gives L>=A_i+p_i.
  Preparation slip is B-A_i with a checked signed representation, not a latency
  subtraction. The window and phase are never changed after observing service.

**Driver:** after B and delta computation, read RTC a, set checked
`d=a+1000*delta`, arm it, and take exactly one interrupt receive with the existing
delta+100000 guard timeout. Retain the existing successful pre-service kernel E
read, RTC service s, and successful post-service kernel P read, in that order.
Retain interrupt-result checking and RTC clearing. Require `s>=d`, `a<=d`, and
`B<=E<=P`; all reads/arithmetic must succeed. E is retained as a diagnostic and its
cost remains in the observation sequence; it no longer anchors an RTC-sized window.

**Timer:** use B as the pre-call arming observation, take exactly one timeout receive
of delta, require the timeout outcome, then obtain successful kernel P. Do not print
L as though it were the kernel's observed timeout deadline. It is the explicit lower
envelope endpoint. Immediate returns still fail the trace join.

For both roles define checked `U=P+1`, require `R<=L<=P` and `U<=F`, and report
`LATENCY-SAMPLE cluster <role> U (U-L)`. The extra one microsecond is the unavoidable
upper edge of a floored timestamp's resolution bin, not a selectable safety margin
or a tolerance. It can only make acceptance harder. Report B, delta, L, P, U and
the role's raw observations in indexed metadata. Report driver
`floor((s-d)/1000)` separately as RTC gross; it is no longer the length attached to
U. The oracle independently reconstructs every field and rejects inconsistency.

Use a new exact plan/metadata version, for example `v3-kernel-envelope`, and reject
v2 logs in this path. Maintain bounded arrays of exactly 200 records per role,
reported only after each role's original marker fence. Do not add an IPC exchange
per sample, a marker, a wait, or a kernel trace field. Metadata memory/build sizing
must be checked in source review; no stack or memory-limit widening is implicit.
Each stand-in's post-fence metadata header echoes the H/F it received; the host
requires those headers to match the declared window. Source review verifies that
the sole launch loop sends these same two values to all 19 children. There is no
claim that W/K records reveal message payloads.

## Containment proof

Let k(x) be real kernel-clock microseconds at observation x. Successful kernel
reading B means `B<=k(B)<B+1`. Driver RTC a is read after B. Under the already required
monotonic same-rate VM clocks, physical deadline time is `k(a)+delta`, irrespective
of the RTC epoch. It is at least L. Successful s>=d puts that deadline no later
than the physical service observation s. P is sampled after s, so `k(s)<=k(P)<P+1=U`.
Thus the whole physical driver interval is contained in `[L,U)` within `[R,F]`.
No numerical RTC-to-kernel epoch conversion appears in the proof.

For the timer, the kernel forms deadline `C+delta` at its later floored reading C.
Monotonicity gives C>=B, and the timeout cannot expire before that deadline
(`docs/kernel/timer.md`, Time and Timeouts). The validated timeout return and later
P imply its physical deadline-to-P interval is also contained in `[L,U)`.
This is a bound on time spent after the actual timeout, not a claim to observe C.

R<=L and U<=F prove both physical and reported-interval containment directly.
The old E-minus-RTC-duration L check is replaced for v3 by this theorem, not waived
for a failing sample. The sample-0 ruling and its inclusive proof remain applicable
to the old data. This proposal makes a stronger sub-microsecond upper assertion by
including P's entire resolution bin; it does not infer equality of RTC/kernel epochs.

## Exact audit credit and what the gate measures

Use half-open intervals for arithmetic; endpoints have zero duration. Preserve all
existing trace validation: complete, ordered, non-overlapping U/V audit pairs, correct
IDs, no audit inside R10, zero drops. Let an audit's floored stamps be u and v.
Its real stamped frame begins in `[u,u+1)` and ends in `[v,v+1)`. Therefore its
**certified interior** is `[u+1,v)` if v>u+1, otherwise empty. This deliberately leaves
uncertain boundary fractions charged as ordinary elapsed time.

For each sample compute the exact integer credit

`C = sum max(0, min(U,v) - max(L,u+1))`,

where each intersection is empty when its upper endpoint is not greater than its
lower endpoint. Checked addition and validated disjoint audit pairs prevent double
credit. Require C<=U-L; the gated value is `N=(U-L)-C`, with checked subtraction.
The max here defines interval intersection, not a clamp or repair of sample times.

This gives exact netting of the explicitly defined envelope against the audit time
the existing stamps certify. It does **not** claim exact physical RTC latency or
sub-microsecond exact audit lengths. If J is the actual physical latency interval,
J is a subset of the envelope and the credited interiors are subsets of the actual
stamped audit frames. Consequently non-audit time in J is at most N. This monotonic
set-containment proof also covers an audit overlapping only an envelope edge.
No audit outside the envelope can reduce its net value.

This is a cluster-only conservative correction to the existing integer audit credit,
which currently uses `[u,v)`. Do not change `audit_inside` for unrelated cases.
At most the uncertain edge bins cease to be credited; there is no gate increase.
Keep full U/V stamps and report gross envelope, certified credit, net envelope, and
RTC raw gross separately, with labels that disclose their meanings.
Both current consumers of cluster latency in `sched_oracle.rs` must use this same
validated credit calculation: the general group/percentile loop in `run` and the
cluster category/report path. Validate v3 metadata and boundaries first, then share
one computed per-sample result; do not leave a second cluster verdict using the old
`audit_inside` calculation. Non-cluster groups retain their existing calculation.

Arming/observation overhead in `[L,U)` remains counted unless it is inside a certified
audit interior. This includes pre-service E's syscall/return, RTC service access and
entry to P. Ordinary preemption, scheduling work and overlapping peer R10 remain.
No calibration estimate, fixed syscall cost, preparation slip or envelope excess is
subtracted. P's return and later bookkeeping happen after the observation endpoint;
their ordinary billed work affects the next attempt and remains in workload/rank
evidence. They are not secretly charged to the latency just observed.

## Targets and comparable controls

For both stand-ins apply p50<=15000 us and p99<=50000 us to all 200 N values. Retain
the existing percentile algorithm, category summaries/minima, and all independent
rank/debt/spinner/join/fence gates. A passing conservative upper metric proves the
physical non-audit latency meets that target; an upper-metric miss alone does not
prove physical latency misses it. Report that distinction.

To ensure the old-control failure demonstrates latency rather than merely loose
observation bounds, additionally compute a driver lower witness on **both** builds:
enclose each audit frame by `[u,v+1)` and measure the **union** of those enclosures
inside `[L,U)` (adjacent frames' uncertainty bins may overlap). Call its length Cmax.
The actual physical audit overlap cannot exceed Cmax. A lower bound on physical
driver non-audit latency is `max(0, floor((s-d)/1000)-Cmax)`. Zero is the mathematical
lower bound when credit exceeds raw duration; it neither drops nor changes a sample.
Compute its p50/p99 over all 200 in the same way.

The old 10-ms build must satisfy every construction/coverage gate, fail at least one
of the unchanged envelope latency targets on each width, **and** have a driver
lower-witness percentile exceed its corresponding unchanged target. This last
condition prevents accepting an apparent negative control caused only by arming or
observation uncertainty. Apply and report the same witness calculation for candidate
and control. If the old build only fails an upper bound or fails qualification, stop;
it is not the required demonstrated negative control. No success is predicted.

All paired throughput/pick evidence must be collected afresh because kernel-clock
loop checks and B affect the workload. Prior v2 values, archived sample 0, and prior
old-control observations are not comparable v3 acceptance evidence. Original
`sched-latency` and its pinned gates stay unchanged.

## Owning rules, paths and review boundary

Governing pages are `docs/kernel/timer.md` (Time/Timeouts: clock origins, floor
resolution and relative timeout arming), `docs/kernel/scheduling.md#responsiveness`
(measured latency targets, virtual clock, audits and comparable workloads), and
`docs/testbench.md#checked-builds` plus Rule F (host verdicts, complete samples and
stamped audit subtraction). TENETS 6 requires a meaningful known-bad control.

The owning scheduling page needs an explicit cluster paragraph defining the upper
envelope metric and lower-witness control. The testbench page needs the v3
cluster-only certified-interior audit rule and honest resolution limits. Timer
semantics do not change. These are measurement corrections, not new scheduler policy
or a new security-rule ID. They must be reviewed with the implementation before any
claim of accepted results; this local proposal does not silently change those pages.

Owned implementation paths, if root authorizes preparation:

- `tests/programs/src/sched.rs`: cluster-only go, clock checks, sample/metadata helpers;
  preserve general helpers and non-cluster roles.
- `tests/programs/src/bin/sched-cluster.rs`: new go/version/fields and matching reports.
- `tests/sched-cluster.toml`: exact version/expectations, same seeds/clock/targets.
- `tools/testbench/src/sched_oracle.rs`: v3 parser/construction checks, envelope audit
  credit and lower witness; preserve existing independent oracle and other workloads.
- The two owning documentation sections above, in a coordinated writer window.

No kernel/model/policy, RTC device semantics, ring capacity, timeout tolerance,
phase list, chunk size, marker protocol or unrelated case changes. No helper agents.

## Minimal host negatives and machine stop

Host checks, when authorized, all run through `cargo testbench` and cover:

1. Reject v2/mixed units, mismatched plan/stand-in window headers, missing/duplicate/
   reordered records, failed timestamps, arithmetic overflow, wrong phase/intent,
   zero/late zero-intent target, positive B before slot, and wrong reconstructed L/U.
2. Reject L<R, L>P, U>F even when P==F, s<d, wrong d-a, reversed B/E/P. Accept a
   correctly contained sample whose old E-minus-RTC bound is early, only when its
   **new full envelope** is inside; reject one with an out-of-window envelope.
3. Credit exactly the certified audit intersection at either edge; zero credit for
   empty/sub-microsecond interiors; reject invalid/nested/missing audit pairs. Show
   uncertain leading fractions and audits outside the sample cannot lower N. Check
   union handling for overlapping outer audit bins in Cmax and checked credit sums.
4. Keep the existing protocol/fence/199-plus-report/immediate/extra-wait negatives;
   keep rank, latency and coverage mutations. Show an upper-metric-only old failure
   is insufficient, while a qualified physical lower-witness failure is sufficient.

Root authorizes source preparation, exact-diff review and machine order separately.
First machine release remains one coordinated focused candidate run; MEM1's ownership
is respected. Stop at the first timestamp/construction failure, immediate or wrong
receive, trace loss, join/fence ambiguity, spinner/rank/category failure, containment
failure, or candidate envelope target miss. Preserve raw source/command/seed/log
before any next run. No retry, alternative phase, larger margin, sample replacement,
window extension or wholesale bench follows a failure. Widths and old controls only
follow their coordinated release; a nonqualifying or unproved old-control failure
also stops. All 200 attempts, fixed slots, fixed 16-second window and original
obligations remain.

No unavoidable owner tradeoff is presently identified: this proposal strengthens
qualification and discloses its changed metric rather than weakening a guarantee.
It needs root design/review coordination because it changes the cluster definition.
If it proves infeasible, that is a new finding; reducing coverage, relaxing numerical
targets or accepting a control without a demonstrated latency failure would be a
separate decision, not an implication of this proposal.

## Root checkpoint: accepted for bounded source preparation

Root message `69dc8f439d7fdaf7e340578407f2cfab` on `IPC3-wake-latency`
accepts the proposal at SHA256
`bb46a4fb13843d2c99cca051206e13a10dc4a28a8183586b784d916782fd3f9e`
for **bounded source preparation only**. That hash identifies the proposal before
this checkpoint appendix. Root reports red review `515bdf82` found the proof sound.
Implementation is assigned after the saved implementer handoff; this checkpoint
grants no machine run.

Root declined the simplifier's suggestion to drop the one microsecond from the
metric: a floored P does not upper-bound the physical fractional duration. Retain
`U=P+1`, `U<=F`, the full certified audit intersections, and the lower-witness
control. All original gates and paired-evidence requirements remain unchanged.
This records root's scoped design acceptance, not a new owner decision or a waiver.
