# SCHED1: shorten the slice to drain runnable clusters

Design checkpoint accepted for evaluation only; stop at the implementer's fixture and
trace-capacity checkpoint before code. Tier A, size M. The Architect has run no tests and
changed no kernel, model, test or book files for this assessment.

## Decision and evidence

The owner chose to fix the scheduler before merging IPC3 and retain every latency target
(QA `IPC3-wake-latency`, thread event `9d1354d63f10b0774a745ede130f3de6`, verified in
the thread's Owner decision entry). The orchestrator identifies the delivered human-answer
message as `9e35cd7b11fd13a5f1e4c0d5db0d75b1`; this is a message ID, not that thread event ID.
This authorizes pursuing the fix; it is not approval of this candidate or acceptance evidence.
The existing targets already live in `docs/kernel/scheduling.md`, Responsiveness; they need
no numerical amendment. Keep the QA blocking until the reviewed implementation meets them.

The evidence is the final `ipc3-implementer-5` section of `.wash/local/IPC3-report.md`.
At IPC3 `a0fbbcab1`, N=16 driver/timer net p99 is 96,212/54,018 us on rv64 and
86,550/84,591 us on rv32, against 50,000 us. Sleep-gaming is already repaired.
The old delivery scan accidentally spread the spinners' initial passes. Faster IPC leaves
them close together. A waker with retained lead now sits behind many lower-pass spinners,
each entitled to a 10 ms slice. Wake-first ties cannot fix unequal passes.

## Recommended mechanism

Use a uniform `SLICE_US = 1_000`, for every budget and at every pick. Keep the existing
timer arming and preemption points. A wake still never preempts; a deadline still does.
The pass remains `max(own pass, floor)` on wake. Rank, all four tie clauses, exact runtime
charging and remainder, minimum charge, free weight, rescaling and inherited debt all stay.
No new queue, priority, donation, scheduling call, per-class rule or configurable quantum.

This is the smallest candidate: the kernel's slice constant changes, together with its
model value, the test fixtures that express fractions of a slice, and the owning prose.
One traversal of seventeen spinning budgets then costs about 17 ms of slice time instead
of 170 ms. That illustration excludes kernel work, retained debt and repeated picks; it
is not a latency bound or a prediction that the candidate passes. Only measurements decide.

The fairness argument is unchanged: the minimum pass wins and every actual run raises it
by runtime divided by free weight. Smaller service quanta reduce scheduling granularity;
they neither erase debt nor award free runtime. Finite-window shares, the minimum charge
and additional kernel overhead still require attack testing. The label isolation unit and
the measured nature of human-control responsiveness remain as stated in TENETS.

The cost is more slice interrupts, picks and switches. `sched.rs` also uses `SLICE_US` as
the checked mark-audit interval: keep its existing within-one-slice detection rule. Measure
the resulting audit count/time and trace volume. Do not silently detach audit cadence, hide
production work as audit time, expand trace RAM, or change latency netting. If these costs
prevent acceptance, return evidence to the Architect before expanding the design.

Start with 1 ms only. Stop the candidate experiment and report at the first above-target
latency, new fairness/budget/lease regression, oracle failure, dropped trace, or missing
attack coverage. Preserve that run's head, seed, command, exit and logs; give the violated
rule and the smallest supported explanation. Read-only diagnosis is allowed, but no policy
expansion, search over quanta, threshold relaxation or replacement seed is authorized.
Resume changes only after a revised design checkpoint. The intentional old-slice negative
control's latency failure is expected evidence, not a candidate regression.

## Reading and ownership

Read these sections, not historical reports wholesale:

- `docs/kernel/scheduling.md`: Preemption points, The current minimum and ties, Charging,
  Responsiveness, R12 and R23; `docs/kernel/timer.md`: The hart timer and its slice rationale.
- `kernel/src/sched.rs`: `SLICE_US`, `pick`, `slice_over`, `audit`, mark-audit call sites;
  `libs/stride/src/lib.rs`: charge, wake/rank and the Cpu wiring, only as needed to verify
  that the arithmetic and ordering remain unchanged.
- `model/src/spec.rs` (`SLICE`), `model/src/sched.rs` and slice-dependent scheduler tests;
  `libs/stride/tests/differential.rs` uses the model's slice and must continue to agree.
- `tests/programs/src/sched.rs` and the focused `sched-*` programs below;
  `tests/sched-latency.toml`, `tests/kernel-containment.toml`, and `sched_oracle.rs`'s
  latency, charging and audit checks. Use searches and ranges for the large files.
- `.wash/local/IPC3-layout-ruling.md`: audit-cost ruling only; IPC3's final report section.

Owned implementation paths: the slice constant and directly related comments in
`kernel/src/sched.rs`; `model/src/spec.rs`; scheduler fixture/test files; a new focused
clustered-wake bench case and its trace post-check if needed. Production stride arithmetic,
IPC delivery, timeout collection, budget layout and ABI encodings are outside scope.
The Architect owns the final design wording in scheduling.md and timer.md; coordinate
those edits in the package worktree before final review. A required size-ratchet change
needs its measured delta and reason, not an unrelated allowance.

`model::spec::SLICE` explicitly means microseconds, so change 10,000 to 1,000 there;
this is not an abstract model unit that can stay at the old value. Adjust concrete timings
in model fixtures where they mean a fraction/number of slices. Do not change fairness
contracts, mutation meanings or stride operations. The ABI is unchanged: no public call
lets a program select or inspect the scheduling quantum. Observable timing changes.

The differential gives the model and the stride adapter the same synthetic runtime units;
its whole-slice, random sub-slice and 1..20-unit runs assert identical charge/remainder,
floor, rank and deschedule transitions. `libs/stride` itself is unit-agnostic; the real
kernel supplies timebase ticks, while the model's public duration convention is microseconds.
Agreement is not a test of that conversion, interrupt overhead or hardware wake latency.
Keep short-run coverage at one synthetic unit, and target tests at one timebase tick, where
applicable. Do not scale `STRIDE`, `MIN_CHARGE`, timeout ABI units or accounting to make a
smaller slice fit the tests. Real boots establish latency and the actual tick conversion.

## Fixtures and attack coverage

Preserve the existing latency workload, weights, N=1/4/16, 200 wakes, 50 destructions,
16-second measurement window, pinned seeds, instruction clock and thresholds. Do not add
startup staggering. Its existing failing N=16 measurements are the first regression check.

Make slice-relative tests continue to attack their stated rule:

- Update `tests/programs/src/sched.rs::SLICE_US`. Near-slice sleep/exit churn already uses
  it. Find other duplicated constants by intent, not by blindly replacing every 10,000.
- `sched-wake-no-preempt` currently naps 3 ms and requires at least 4 ms delay in a 10 ms
  slice. Place the timeout inside the new slice (initial candidate 300 us) and require the
  remaining-slice delay (initial candidate 400 us), with trace evidence that the timeout
  really expired mid-slice and that its wake did not pick a thread. Retain the host/model
  preempt-on-wake negative controls. If fixture overhead invalidates this placement,
  report it and use observed entry/pick timing to make the attack non-vacuous.
- Carve-return's hardcoded 9,000 us in the shared role means nine tenths of a slice;
  express it as such, and confirm that the return occurs during the intended first slice.
  Keep the 999/1000 carve and its share assertion.
- Debt-lift and destroy-billing bounds measured in slices must use the new slice, retaining
  their coefficients and measured kernel-work terms. Keep absolute service deadlines and
  20/50 us sleep attacks unchanged where they are absolute workloads.
- Timer-flood must still hit the victim's window with the attacker's early-ended waits;
  a green result with zero qualifying interrupts is a failure of the fixture.

Add a focused clustered-wake attack rather than relying solely on startup cost. Arrange
sixteen equal-weight spinner budgets ready together, a weight-1000 busy server, and the
driver/timer stand-ins at the existing weights. Use a blocking barrier/common timed release
so births and sequential setup do not space the active run. Exercise a zero-lead wake and
a wake retaining positive debt, and several wake phases across a slice. The workload may
cause debt by running; it must not set passes through a new kernel interface.

The implementer's layout/test checkpoint must specify how the existing trace independently
proves the intended cluster and positive lead at the qualifying wakes, how many samples
qualify, and why the old slice fails. Check actual ranks, not just a count of runnable
budgets. Keep this as one bounded fixture with a fixed phase list, not a scheduler framework.
Use the existing independent rank/charge oracle and trusted driver/clock verdicts. Require
driver and timer p50 <= 15 ms and p99 <= 50 ms on both widths, zero dropped trace records,
and non-vacuous cluster/lead coverage. Record a negative run on IPC3 with the old 10 ms
slice: a latency miss must be caught, without changing the oracle or expectations.

The original gate remains mandatory even if this attack passes. No claim is made that
1 ms bounds arbitrary budget populations, arbitrarily large retained debt, or kernel work.

## Exact acceptance

All tests run through `cargo testbench`, using the existing dev image and
`/home/mcloonan/redoubt/.wash/local/in-dev` from the worktree. No host installs.

1. First make the paired baseline/candidate comparison specified below on rv64 and rv32.
   The final combined tree must satisfy every existing target below for every N tested.
   Record gross/net p50, p99 and max, audit totals/counts, pick counts, server share and
   dropped records. Baseline misses are retained as failures caught by the unchanged gate;
   do not mark the old build accepted or rerun its whole bench merely to reproduce them.
2. Run `sched-latency --sweep 1..16` and `kernel-containment --sweep 1..16`, on each width,
   serially (`--jobs 1`), preserving all gate thresholds. Every seed must pass. Record
   the worst seed and measure for every target. Do not select a replacement pinned seed.
3. Run the new cluster attack and its old-slice negative control on both widths. Run the
   scheduler attack family: share, large-weight, sleep-gaming, exit/budget churn, carving,
   carve-return, debt-lift, idle-gap, ties, wake-no-preempt, server-busy, destroy-billing,
   timer-flood and deadline-flood-billed. Include budget-deadline and the existing budget
   destruction/revocation cases: lease deadlines must still end a budget while another
   runs, remove its descendants and deliver valid notices. Existing assertions and
   tolerated shares stand; the shorter quantum must not make those attacks vacuous.
4. Run host stride/model differential, scheduler contracts, all R12 mutations and oracle
   checks through their bench cases. Each retained attack must still catch its negative
   control; any fixture adaptation must explain how it preserves that discrimination.
5. Run the Tier A whole bench serially for both widths on each final acceptance head;
   include `worst-walk` by name if it is still excluded from the whole run. Include docs,
   formatting, size/unsafe budgets and rv32 compilation. No fail or unsupported skip is
   acceptance. Final review covers exact heads, all fixture edits and owning-page edits.

| Metric | Unchanged threshold |
| --- | --- |
| Driver and steward timer wake | p50 <= 15,000 us; p99 <= 50,000 us |
| Steward decision wake | p50 <= 25,000 us; p99 <= 95,000 us |
| Deadline notice | p99 <= 40,000 us |
| R10 destruction kernel work | p99 <= 30,000 us |
| Decision wake p99 + R10 p99 | <= 125,000 us, also checked as a sum |
| N=16 busy server's spinning CPU share | >= 354/1000 |

## Required paired experiment

Baseline is IPC3 `a0fbbcab1` with the 10 ms slice; candidate is that exact tree plus the
isolated SCHED1 range with the 1 ms slice. Run serially in the same dev image and QEMU
configuration, with the same build features, RAM, widths, instruction clock and RTC clock.
Use `sched-latency` seed 3 and `kernel-containment` seed 13 on each build and width. Use
seed 3 and the identical fixed phase list for the new clustered fixture on both builds;
record any test-only overlay needed to give the old tree that same fixture and oracle.
Do not backport the candidate quantum to the baseline. Existing saved logs suffice only
if their seed, exact tree/configuration and all comparison fields can be verified; otherwise
collect the missing paired runs. No implementation or run is authorized by this brief alone.

For each pair, provide a table with baseline, candidate, absolute difference and percentage
difference where meaningful:

- Startup pass spread of the sixteen spinner budgets; per qualifying wake, the waker's
  retained lead over the floor and count of lower-ranked budgets ahead; intervening picks
  and execution time until the waker runs. The host trace oracle must establish these.
- Driver, timer, decision and deadline-notice p50/p99/max, destruction p99 and the lease-end
  sum, with net/gross values and all unchanged verdicts.
- Useful spinner work counters per audit-net guest second at the same N/window, alongside
  the busy server's share; also show gross work per guest second so audit cost is visible.
- Picks, budget changes between successive picks, repeated picks of the same budget,
  slice-ending timer entries, timer work billed in the window, and audits' count/time.
  Report counts per audit-net guest second and zero trace drops. A pick records budget
  and pass, not thread ID; call it a thread switch only where the fixture establishes one
  thread per budget. The existing charge records cover timer billing, not all kernel work;
  do not label their sum total switching cost. Useful-work throughput measures the aggregate
  cost that these counters cannot isolate. Add host-side summaries of existing records as
  needed; do not add a production instrumentation channel.

A fixed seed holds the initial randomness constant, not the later schedule: changing the
quantum changes phases. The rank/cluster evidence explains those differences. Use the
unchanged gate plus the explicit clustered fixture and full seed sweep, not the paired
seed alone, to judge robustness. Keep both fixture binaries/configurations identifiable.
For slice-relative tests, document each old/new duration and the invariant it attacks;
absolute sleep-gaming naps, lease deadlines and latency workloads keep their units/values.

There is no settled numeric throughput-loss allowance. Return the measured switching and
useful-work cost at the checkpoint even if latency passes; any unexpected material loss is
an unresolved tradeoff for the orchestrator/owner, not something the implementer may waive.
Do not invent a tolerance. The recommendation is to evaluate the smallest uniform-quantum
change first; if its cost is unacceptable, seek a new bounded design before implementing
an adaptive quantum or wake policy.

## Staging without a dependency cycle

The scheduler parameter fix has no semantic dependency on IPC3's lists. Initially build
and test it on a separate `wp-SCHED1` worktree/branch based on the recorded IPC3 head
`a0fbbcab1`; only SCHED1's commits belong to this implementer. Leave `wp-ipc3` untouched.
Record that integration base and the scheduler-only commit range in the report.

Before final acceptance, replay only the scheduler range onto then-current main. Validate
and review this scheduler-only final head under the same applicable Tier A gates. The
orchestrator can then accept and merge SCHED1 first, with no failing IPC3 intermediate on
main. Next rebase IPC3 onto that main and renew IPC3's complete combined-tree gates and
review; only then accept and merge IPC3. Any conflicts/delta need review under SWARM.
Early stacked-tree test results diagnose the fix; they do not substitute for either final
head's evidence. If separation proves impossible, return the exact coupling to the
orchestrator rather than silently folding IPC3 into SCHED1's acceptance.

Suggested plan node: SCHED1, parent M1, needs K22 and ASID1 (both done). State todo until
the design checkpoint. The IPC3 integration base belongs in its body, not a `needs IPC3`
edge. Do not add the reciprocal edge or change IPC3's state. The orchestrator holds IPC3
at the existing blocking QA until both sets of acceptance obligations are satisfied.

## Book and checkpoint

After the design is accepted and implemented, update scheduling.md's slice rule, diagram,
instruction conversion (1 ms = 125,000 instructions), test/status references and current
measurements. Preserve clearly labelled historical sweeps; replace residual wording that
describes the old 10 ms schedule as current. Update timer.md's slice number and the SBI
arming rationale using measured overhead. Other numerical targets stay. Do not put package
IDs, dates or decision IDs in the book; provenance belongs in the QA and merge trailers.

No genuine owner choice is required to evaluate this mechanism: it pursues the approved
fix under the existing guarantees. An unacceptable measured throughput tradeoff, a proposal
to forgive wake debt, introduce priority/wake preemption, weaken an audit rule, or relax a
target would need a new ruling and, where it changes a guarantee or an owner preference,
an explicit owner decision. Recommend preserving the current rules if that happens; present
measured alternatives through `decision_request`, never infer acceptance from silence.

The orchestrator accepted this experiment design for evaluation only. The next implementer
checkpoint specifies the clustered fixture, slice-dependent edits and
trace capacity estimate. It stops before code so the Architect can verify the attack and
scope. No implementer or helper has been launched by the Architect.
