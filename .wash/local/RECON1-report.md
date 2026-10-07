# RECON1: the reconcile's cost bounded by its stated loop bounds

Branch `wp-RECON1`, worktree `/home/mcloonan/redoubt/.worktrees/RECON1`, from `main` f4d3b41d9.

## Design (settled before the kernel changed)

The attribution (`evidence/SCHED1/five-cases/reconcile-finding.md`) stands: the cost is
`Queue::raise_floor`'s unconditional walks, each reading every queued budget's seven-word `State`
through checked `kframe` reads, four to nine walks a slice end. Two changes in `libs/stride`, both
from the finding's sketches, and nothing in the rules:

1. **Ranks beside the slots.** `Queue` gains `ranks: [Rank; N]`, parallel to `slots`: the queued
   budget's `(pass, tie, id)` as its frame holds it. It is written at every place the queue changes
   a queued budget's pass or tie: `fold`'s charge, `deschedule`'s requeue, `reconcile`'s wake,
   `reweigh`'s rescale and `destroy`'s lift of a queued parent (a budget not queued has no slot and
   no rank). `raise_floor` and `pick` read the ranks, never the frames. The frame stays the
   authority; the cache is derived.
2. **The floor is raised only when it can have moved.** The floor is the minimum queued pass and
   never falls, so after every queue operation `floor == min` while the queue is non-empty (every
   pass a queued budget is given is at or above the floor: a wake is `max(own, floor)`, a charge
   only raises, a rescale and a lift start from the floor). The minimum can rise only when a budget
   at it leaves, when a budget at it is charged, rescaled or lifted, or when a wake fills an empty
   queue. `raise_floor` runs at those points and nowhere else: a reconcile with nothing lost and
   nothing gained reads no state at all.

The checked build audits the ranks against `sched_state(frame)` for every queued budget inside
`sched::audit` (the marks' audit, about once a slice after a reconcile that visited a budget, and
before the hart idles), off the exit path (the refresh's lesson from K25: never on the measured
walk). Host tests: the cache agrees with the frames after every operation (the differential
checks it at every step over 3000 seeds, and a random-operation test in `tests.rs`); a slice end's
frame reads are bounded whatever the queue holds (a counting `Budgets`); the stride differential
(`the_crate_and_the_model_agree`) unchanged, the model unchanged.

Two rules the host tests held the design to (the first attempt raised the floor inside the
operations, and both the random marked-against-full test and the differential caught it):
- **A reconcile's leaves are one step.** The floor is their result whatever their order: when they
  empty the queue it holds, where a raise after each leave would have carried it to the last
  leaver's pass. So a removal only marks the floor pending, and the operation raises it once.
- **A weight change alone does not raise the floor** (the model's `reweigh` does not; the next
  fold, deschedule, reconcile or destruction does). So the queue keeps one `pending` flag, set
  when a budget at the floor leaves or its pass rises, or when wakes fill an empty queue, and
  `raise_floor` runs only at the points the model raises and only when the flag is set. The audit
  requires `min <= floor` unless a raise is pending.

## Measurements

Method: SCHED1's (`evidence/SCHED1/five-cases/RESULTS.md`), on this branch before and after the
change, every console in `.wash/local/evidence/RECON1/` (`before-*`, `after-*`). The release runs
are `sched-large-weight-release` (new, committed) and a scratch `diag-recon-10ms` (release, the
10 ms slice by SCHED1's scratch `SLICE_US` edit, reverted: `scratch-slice-edit.diff`,
`slice-10ms` refuses a release build). The checked runs are scratch sched-trace cases of
`sched-large-weight` (both widths), `sched-carve-inflation` and `sched-debt-lift` (rv64) read by
SCHED1's `decompose.py` (gap median between slice-end interrupts, split at the marks' audit), and
a walk-trace run read for the RECONCILE spans. No scratch case is committed.

### Release: useful work and kernel time per slice end (nine runnable budgets, 1 ms slice)

| | rv64 before | rv64 after | rv32 before | rv32 after |
| --- | ---: | ---: | ---: | ---: |
| all nine, of 1000 of the window, at the build's own rate | 873 (rate 22092) | **979** (22497) | 861 (7613) | **964** (7756) |
| the server alone | 483 | 543 | 476 | 534 |
| 10 ms build, all nine / rate (the reference, unchanged tree) | 982 / 24655 | | 979 / 8773 | |
| useful work, 1 ms against 10 ms (share x rate) | 0.797 | **0.910** | 0.763 | **0.870** |
| kernel time per slice end, 1/f - 1 ms (f = share x rate / rate10) | 0.278 ms | **0.120 ms** | 0.338 ms | **0.173 ms** |
| about, in instructions (125 per µs) | 34,800 | 15,000 | 42,300 | 21,600 |

Gate: useful work >= 0.85 both widths (0.91 / 0.87); kernel time per slice end <= 0.19 / 0.22 ms
(0.120 / 0.173). SCHED1's own before-numbers (0.762 / 0.718, 0.357 / 0.449 ms) were taken before
K24 moved the exit work out of the slice; this branch's before-numbers are the like-for-like ones.

### Checked, traced: the slice-end gap, net of the marks' audit (medians, µs)

| Case (L queued) | before: to audit + rest | before net | after: to audit + rest | after net | audit before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| carve-inflation depth 1 (L=3), rv64 | 103 + 124 (SCHED1) | 227 | 69 + 111 | **180** | 30 → 41 |
| carve-inflation depth 4 (L=6), rv64 | 139 + 162 (SCHED1) | 301 | 71 + 116 | **187** | 51 → 72 |
| large-weight (L=9), rv64 | 180 + 255 | 435 | 73 + 122 | **195** | 77 → 105 |
| large-weight (L=9), rv32 | 214 + 300 | 514 | 97 + 164 | **261** | 104 → 137 |
| debt-lift (L=17), rv64 | 259 + 289 (SCHED1) | 548 | 77 + 136 | **213** | 129 → 182 |

Per queued budget per slice end (rv64, L = 3..17): before about 23 µs; after (213 - 180) / 14 =
**2.4 µs**, under the 3 µs gate. Fixed term at L = 0, extrapolated: about 175 µs checked (SCHED1
had about 155; K25's per-hart asserts are in between), unchanged by this package. The marks'
audit grows: it now carries the ranks' audit (one frame read per queued budget), inside
`sched::audit`, stamped and outside every window (K15, K18).

Walk-trace RECONCILE spans (rv64, large-weight): before 4715 spans over 1479 slice ends (3.19 a
slice), median 73 µs, mean 87; after 5331 over 1687 (3.16), median **20 µs**, mean 25.

### The refresh's acceptance lines

| Line | Today (brief) | Measured on this branch | Target |
| --- | ---: | ---: | ---: |
| `sched-share` "the three counted N of 1000", checked | 912 / 899 | **975 / 964** (scratch `diag-recon-share`) | reported |
| the same in release (`sched-share-release`, new) | | **>= 960 both widths** (the case's expectation; a passing console is not kept, so the exact figure is the case's verdict) | >= 960 |
| `sched-large-weight-release` useful work against 10 ms | 0.762 / 0.718 | **0.910 / 0.870** | >= 0.85 |
| `worst-walk` reconcile waking 250 budgets (oracle max, net of audits) | 7.6 / 8.7 ms | **7.1 / 8.1 ms**; bound `reconcile_max_us=8900` (rv32 + a tenth) | <= 3.5 / 4.0 ms: **missed, a finding** |
| `sched-exit-churn` nobody's share | 75 of 1000 | not computed (see residuals) | <= 25 |
| kernel time per slice end, nine budgets, release | 0.357 / 0.449 ms | **0.120 / 0.173 ms** | <= 0.19 / 0.22 |

**The worst-walk finding.** The queue walks were about 0.5 ms of the 7.6 ms (two `raise_floor`
walks of 250 budgets at about 1 µs each per budget in that build, which this change removes);
what remains, 7.1 ms, is about 28 µs per woken budget, and the wake's own work is: `settle`'s
count moves (two frame writes per slot), the wake itself (`live`, `ready`, one `sched_state` read,
`set_sched_state`'s seven writes), and in the traced checked build two more `sched_state` reads
per wake (`set_state`'s PASS check and `woke`'s record) and `check_visited`'s three frame reads per
visited budget. The target assumed the wakes cost far less than they do; none of it is a queue
walk or the fixed term, so per the brief I report it and stop: the trace's and `check_visited`'s
reads are `sched.rs`'s trace path and `marks.rs`, outside this package's owned paths, and the
release build has neither. The toml's bound follows the measured value plus a tenth, as the brief
says bounds move.

**Not done: the oracle's "nobody N of 1000".** The one `sched_oracle.rs` change the refresh allows
needs the per-budget charge reconstruction `check_charged_share` does (pass rises times the weight
the trace states), applied inside each `SHARE` window for every budget, with its host test. I read
that code and judged it a day's careful work in a file not otherwise mine; with the kernel-side
measure of the same cost (the slice end's kernel time, now 0.12 / 0.17 ms, billed to the budget
picked) delivered, I left it for the orchestrator's call rather than rush it. `sched-exit-churn`
passes both widths (threads-exit net 508 / 505, from K25's).

## Commits

- f22ed4e50 `stride: the floor is raised only when it can have moved, from ranks the queue keeps
  beside its slots` (lib, tests, differential; the pages' host-test entries and the floor
  paragraph; `Size budget: libs/stride` 650 → 694).
- 8ea3a61bd `kernel, tests, docs: a slice end's kernel time is measured, and the queue's ranks
  are audited against the frames` (the audit in `audit_marks`; `sched-large-weight-release`,
  `sched-share-release`, worst-walk's `reconcile_max_us=8900`; the measured sentences;
  `Size budget: kernel` 9360 → 9363, three lines).

The scratch cases (`diag-recon-*.toml`) were never committed; they are kept in
`evidence/RECON1/scratch-cases/`. SCHED static grows by 511 × 32 B = 16 KiB (the ranks). No
`unsafe` added (the crate forbids it; the kernel's count unchanged, `unsafe-budget` passes).

## Summaries checked (the pages move with the code)

- `docs/kernel/scheduling.md`: "The current minimum and ties" (floor paragraph gains the bound and
  the ranks; status 10 → 12), "Charging" (the three cost sentences replaced by the measured ones;
  sched-share's figures 938/929 → 975/964 with the window shares), "Responsiveness" (the slice-end
  gap sentence remeasured), R12's status line (the reconcile's 7.6/8.7 → 7.1/8.1 ms; 45 → 49 with
  two host tests and two bench cases), "Residual risks" (K24's "A slice end's kernel time grows
  with the queued budgets" removed: measured and bounded now). The flowchart node "floor =
  max(floor, lowest queued pass)" still states the rule.
- `docs/SECURITY.md` R12 row: the two host tests and two bench cases added; status unchanged.
- `docs/testbench.md` (:598-608, shares and the slice-end cost): still true as written (ratios of
  counts, costs in tolerances); no numbers there. No change.
- `docs/plan/m1-separation.md:53` names `sched-large-weight` for the server-CPU claim: still
  true; the release sibling is a measurement case, not a new claim. No change.
- `docs/kernel/timer.md:308` (the expiry walk of 250 waits, 26.8 / 28.9 ms): the timer's walk,
  not the reconcile's; unchanged by this package. No change.
- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `libs/stride` has no README: nothing names
  the reconcile's cost. No change.
- The model (`model/src/sched.rs`): unchanged; the differential proves the queue still says what
  it says.

## Gates (head 8ea3a61bd; every command through jobs.mk or `q run`, on the pool's rules)

Full short gate on the working tree one docs-sentence and the two size-budget raises before the
commits (`target/recon1/gate-1.log`, make -k over 64 targets; rc=2 for the four main failures and
size-budget before its raise was committed):
- build-rv64 rc=0, build-rv32 rc=0 (`target/recon1/builds-1.log`).
- `q run --cores 4 -- cargo test -p redoubt-stride`: 21 host tests + the differential (3000 seeds)
  + `a_broken_model_disagrees`: rc=0. `rv64/stride-host-tests` PASS, `rv64/host-tests` (the
  kernel's) PASS, `rv64/model-host-tests` PASS, `rv64/model-mutations` (fanned, release) PASS.
- `docs` PASS (three runs, the last on the final text), `formatting` PASS, `unsafe-budget` PASS,
  `no-cruft` PASS; `size-budget`: FAIL before the commits (kernel 9363 > 9360, then libs/stride
  694 > 650), rerun on the committed tree below.
- Every `sched-*` case (19) PASS on rv64 and rv32, `worst-walk` PASS both widths (and again with
  `reconcile_max_us=8900`), `sched-large-weight-release` and `sched-share-release` PASS both
  widths.
- Smoke set: `userland-boot`, `init-boot`, `ipc-outcomes`, `bench-net-peer`, `sum-clear`,
  `lend-untouched-page` PASS on both widths.
- **FAIL, main's, not this branch's:** `kernel-containment` (both widths, seed 13),
  `sched-latency` (both), `sched-latency-tcg` (both): `sched_oracle: an audit inside a
  destruction` (`U 2`, AUDIT_PROCESS_INDEX, between a destruction's `D` and `Y`). Reproduced on
  an untouched export of main f4d3b41d9: `/tmp/recon1-main/sched-latency-rv64.log`. K19's; the
  orchestrator was told.

Final run on the committed tree (head 8ea3a61bd, scratch cases removed; `target/recon1/gate-final.log`,
make -k rc=0): `size-budget` PASS, `docs` PASS, `no-cruft` PASS, `formatting` PASS,
`unsafe-budget` PASS, `stride-host-tests` PASS, `sched-large-weight-release` PASS rv64 and rv32,
`sched-share-release` PASS rv64 and rv32. The boot cases of the first batch were run on the same
code (the commits changed only pages, case files and the two ceilings after it); the kernel and
stride sources are byte-identical between the two runs (`git diff` of the batch's tree against
the head: docs, tomls and size-budget only).

Not run: the whole bench (the train's). The pool's rules held throughout (`q run` / jobs.mk; the
boot cases are icount-pinned and their verdicts hold beside other work; nothing was rerun alone
because nothing of mine failed).

## Residuals and open points

- Main f4d3b41d9 fails `sched-latency`, `sched-latency-tcg` and `kernel-containment` with "an
  audit inside a destruction" (K19's process-index audit). This branch inherits it until main is
  fixed; a rebase onto the fix and a rerun of those three cases is the next step.
- worst-walk's reconcile of 250 wakes is 7.1 / 8.1 ms, over the refresh's 3.5 / 4.0 ms: a finding
  (above), the cost being each wake's own frame traffic, the trace's extra reads and
  `check_visited`, not a queue walk.
- The oracle's "nobody N of 1000" on the `share` line is not done (above).
- Design note for the Architect, not a change: `Wiring::switch` folds and deschedules `cur` with
  two `sched_state` reads and two `set_sched_state` writes of the same frame; a slice end's
  remaining per-switch frame traffic is there and in the trace path, if the fixed 100–175 µs is
  ever this subsystem's to lower.
