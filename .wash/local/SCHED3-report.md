# SCHED3 report (sched3-implementer-2, 2026-10-09)

Branch `wp-SCHED3` (`.worktrees/SCHED3`), base main `f5316e38c`, three commits, no WIP:

1. `de9c75b31` stride: a budget lifted out of the cap set keeps the lag it was owed (A1 + A2:
   eligibility form, lift target `min(pass, floor)` following the floor, k=0 in neither W nor
   floor, a budget leaving its hart counts one waiting thread as running; model, differential,
   kernel cap audit, oracle `z`/`u <= floor`; docs scheduling.md, model.md, SECURITY.md, testbench.md)
2. `cdf1fee3c` testbench: a timeout's wake is judged on several harts (D: `O` return reason in the
   trace, oracle check with witnesses, `sched-wake-no-preempt-harts` at 2 harts, `SpinCalling`
   role, test-only `wake-preempts`; the one-hart case keeps its proof)
3. `2de48e03e` tests: the churn and flood lines are judged at two harts (un-keeps; oracle's
   report-only count of timer interrupts charging another budget than the one interrupted;
   timer.md residual; scheduling.md table, keep list, containment, residual pointer)

Head: `2de48e03e`.

## Against the design and rulings

- **A1/A2**: done in the eligibility form (architect ruling, thread SCHED3-lift-target-form). The
  requeue-as-running condition is implemented per budget, as the stride queue counts: a budget a
  switch is taking off its hart counts one waiting thread as running. The model states it the same
  way (the crate cannot tell a blocking thread from a requeued one without new plumbing through
  the kernel's switch; per budget it also excuses a block with a sibling thread waiting, transient
  and on the side R12 takes). New: scenario `heavy capped` (catches only R12PriorityById /
  R12IgnoreWeight: capped-already stays eligible, so it cannot see the requeue rule), contract
  and mutation `R12RequeueWaitsForCap` (caught by the contract and by the differential).
  `R12UncapBanksCredit` still caught (model-mutations).
- **B1**: dropped (ruling, thread sched3-b1-evidence). Its code is commit `33eaaf326` (not on the
  branch); patch kept at `.wash/local/SCHED3-B1-33eaaf326.patch`. **Trigger met on the final
  build** (orchestrator filed SCHED4): deadline-flood-billed-traced rv64, 16 deadlines: 87 of 96
  destructions on hart 0 under the VICTIM; 89 timer interrupts charging another budget, 833,543
  ticks, ~17 % of the 4.99M window; victim 482 (passes, ≥ 450). rv32 and 64 deadlines unmoved
  (872/877/880; rv32 3 entries/34,046 ticks). exit-churn: 27/70,232 rv64 (0.23 %), 8/17,035 rv32.
  The placement depends on timing alone: the earlier build (before the trace's `O` reason and the
  requeue refinement) had 91 of 96 on the creator's hart and 870. The report-only count is the
  detector. Note: the "1229 entries / 31.7k" figure in the B1 thread was all timer entries
  interrupting the victim (billed.py), not the count the oracle now keeps.
- **C**: residual restated. Containment's bystander stays `@1`; at two harts after A it read 450
  and 567 on rv64 (two builds) and 557 on rv32, against 450 to 550: both sides of the band.
- **D**: restated property on scheduling.md "Preemption points"; oracle as designed plus the
  `O` reason (a preempting interrupt and a preempting call were invisible to the first oracle:
  the WIP version passed the `wake-preempts` kernel only for want of witnesses). The original case
  could not produce witnesses at 2 harts (0 mid-slice, 2 at calls); the new case's spinners call
  `time_now` every 100 µs and the sleeper takes 60 naps.

## Before / after, two harts (gate configs, rv64 / rv32, of 1000)

| Line | main f5316e38c | SCHED3 |
| --- | --- | --- |
| sched-budget-churn-shell (≥450) | 398 FAIL / 509 | 571 / 539 |
| deadline-flood-billed-traced 16 / 64 (≥450) | 415 / 406 FAIL; 516 / 515 | 482 / 877; 872 / 880 |
| sched-exit-churn threads-exit (≥450) | 506 / 464 (was `@1`) | 554 / 587 |
| kernel-containment bystander (`@1`, 450–550) | 464 / 442 (SMP4) | 450, 567 / 557, reported |
| sched-wake-no-preempt at 2 | kept; 0 mid-slice, 2 at calls | -harts: 21 / 17 witnesses of 63 |
| A only (B1 off) vs A+B1, earlier build | | deadline-flood 868,875/873,879 vs 870,875/871,877 |

Other shares at two harts (final code): sched-share 300: 467/465, each 100: 266/267; idle-gap
333/332; sleep-gaming near-slice 498/483; budget-churn spinning parent 548/592; exit-churn
processes 621/631, fault 616/627; timer-flood 644/552, 539/563; carve-inflation four deep
460/538. Logs `.tmp/SCHED3/logs/final2/` (snapshot of 7d8ec64d9, code identical to the head).

Deadline notice, two harts, net p99 N=1/4/16: B1 off (baseline, ruling) 4.6/4.6/5.1 ms rv64,
6.2/5.2/6.1 rv32; final code 4.6/4.4/5.3 rv64, 5.5/5.1/4.4 rv32 (`.tmp/SCHED3/logs/final2c/`).

## Negative runs recorded

- `wake-preempts` on sched-wake-no-preempt-harts, final code: FAIL both widths, "budget 31's wake
  at record 1462 took hart 0 from budget 34" (rv64), "... took hart 1 from budget 34" (rv32).
  `.tmp/SCHED3/logs/finalneg/`.
- `R12RequeueWaitsForCap`, `R12UncapForfeitsWait`, `R12EmptyHoldsFloor`: caught (model-mutations).
- Without A (main): budget-churn-shell and deadline-flood fail at 2 harts (table).

## Gates

Full set on folded head 5d584fcbf (code identical to 2de48e03e; since then only the size ceilings,
commit messages and doc numbers changed): `make -f scripts/jobs.mk prebuilt` exit 0; `make -k set
CASES="docs formatting size-budget unsafe-budget no-cruft host-tests model-host-tests
stride-host-tests model-mutations userland-boot init-boot bench-net-peer ipc-outcomes sum-clear
lend-untouched-page deadline-flood-billed-traced deadline-flood-billed kernel-containment
sched-*" (all 31 sched cases)`: 89 verdicts, every one PASS but size-budget (kernel over its
ceiling); model-mutations PASS (164 variants); every case on rv64 and rv32 at its configured harts
(1 for the smoke set and the sched cases but sched-capped 2-4, sched-capped-holds-floor 2,
sched-lock-contention 2, -4 at 4, sched-latency 1+2, sched-wake-no-preempt-harts 2,
lend-untouched-page 1+4). Logs `.tmp/SCHED3/gate/gate.out`, `.tmp/SCHED3/gate/jobs-*`.
On 2de48e03e: size-budget PASS (kernel 10048->10072 over A and D, libs/stride 841->877 and model
10825->10910 in A, each with its `Size budget:` line), docs PASS, unsafe-budget PASS (count
unchanged), no-cruft PASS; formatting PASS on 5d584fcbf (identical Rust).
sched-lock-contention-4-mttcg at 4 harts, snapshot of 2de48e03e's code: PASS rv64 and rv32, exit 0 (`.tmp/SCHED3/logs/mttcg/`).
Intermediate commits A and D: stride (25 + differential), testbench oracle (54) and model
scheduler tests pass at each.
The four lines at 2 harts on both widths and the 2-hart wake case: PASS (final2 logs, table above).
One hart unchanged: main at one hart reads shell 632/535 and threads-exit 500/504, the branch
633/535 and 500/504.
Not run: the whole bench (integration train's).

## Affected summaries checked

- docs/kernel/scheduling.md: updated (cap set, preemption points, responsiveness table, residuals,
  keep table, containment, R12 lists).
- docs/kernel/timer.md: R12 for timer work residual with numbers; the hart timer section unchanged
  (B1 dropped, arming unchanged).
- docs/kernel/model.md, docs/SECURITY.md (R12 row), docs/testbench.md (oracle: `z`, `O`, wake
  check, foreign count): updated.
- docs/kernel/budgets.md: no cap/lift text there; no change.
- docs/kernel/README.md containment: its claim (bystander judged at one hart, reported at two) still
  true; no change.
- docs/plan/m2-usable-shell.md several harts: claims unchanged; no change. README.md,
  GETTING-STARTED.md: no scheduler claims; no change.
- .wash/local/SCHED3-design.md: B1 trigger section appended.

## Fix round 1 (kernel-red OK with notes on 2de48e03e), new head 8f10427be

Commits: ea25b6902 stride (A, with the fix), c6171572e testbench (D), 8f10427be tests (L).

- P2-1: `Harts::switch` takes `requeued` (the runner's own thread is still runnable). The kernel
  sets a per-hart `preempted` flag in `sched::preempt` (slice end or deadline) and `leave` passes
  and clears it; a block or an exit passes false. Only a requeued budget counts its own thread
  as running for cap eligibility; a sibling waiting after a block is a wait. The model takes it
  from whether the descheduled thread is still runnable; mutation `R12BlockLeavesAsRequeued`
  (a block counts as a requeue) is caught by a new contract (a heavy budget whose thread blocks
  while its sibling waits is not capped, and the sibling's budget holds the floor) and by the
  differential. Cost: one bool through the switch; no more.
- P3: containment's "measured again once zeroing leaves the lock" dropped; oracle summary and
  docs say "lifted out of the cap set, at most to the floor" (the `u`/`z` docs too).
- Size budgets: kernel 10048 -> 10057 (A) -> 10076 (D), libs/stride 841 -> 879, model
  10825 -> 10914 (A), each with its line.

Gates on 8f10427be: prebuilt 0; `make set` docs, formatting, size-budget, unsafe-budget,
no-cruft, host-tests, model-host-tests, stride-host-tests, smoke set (userland-boot, init-boot,
bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page), sched-wake-no-preempt (1 hart),
sched-wake-no-preempt-harts (2), sched-capped (2-4), both widths: all PASS, exit 0
(`.tmp/SCHED3/gate/gate5.out`). Mutations R12BlockLeavesAsRequeued, R12RequeueWaitsForCap,
R12UncapBanksCredit, R12UncapForfeitsWait, R12EmptyHoldsFloor: caught. Intermediate commits A
and D: stride, oracle and model scheduler tests pass.
Two harts (`.tmp/SCHED3/logs/fix2/`): budget-churn-shell 571/540; deadline-flood 868,876 /
872,880 (high mode again; foreign-timer count 8/74,989 rv64, 3/34,065 rv32); exit-churn threads
559/585 (count 1/4,086, 8/17,065); kernel-containment PASS, bystander 452/557 reported;
wake-no-preempt-harts 20/25 witnesses. Not run this round: full model-mutations, the whole bench.
