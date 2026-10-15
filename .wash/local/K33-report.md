# K33 report (k33-implementer)

Design checkpoint: .wash/local/K33-design.md. Base db6575a3f (main).

## Branch wp-K33

Rebased 2026-10-14 (k33-implementer-2) onto B58's accepted head 2cb94d714 (`git rebase --onto
2cb94d714 cde8c6691 wp-K33`, no conflicts); the three commits are now 59f46c9d1, eba068549,
904354363 (code identical to the hashes below, which were the pre-rebase ones).

1. 59f46c9d1 stride: a slice end picks where it is, and leaves what the round trip through kmain
   leaves (libs/stride/src/lib.rs, libs/stride/tests/differential.rs).
2. eba068549 stride: every pick in the differential switches a hart on in the kernel's order
   (libs/stride/tests/differential.rs).
3. 904354363 kernel, testbench, docs: a slice's end that picks its own budget again runs its
   thread on (kernel/src/{sched,time}.rs, kernel/src/arch/riscv/irq.rs, libs/stride/src/lib.rs
   `Queue::waiting_of`, tools/testbench/src/sched_oracle.rs, docs/kernel/scheduling.md,
   docs/testbench.md).

## Commit 2 gates (all rc 0)

- `cargo test --release -p redoubt-stride`: 25 unit + 2 differential (agreement at 1/2/4 harts x
  3000 seeds with every outcome of repick > 100 each; a_broken_model_disagrees all caught).
- `make set CASES="model-host-tests model-mutations"`: rc 0, model-mutations PASS 30.8 s,
  model-host-tests PASS 156.5 s. R12 set (35): AllCappedHoldsFloor BlockLeavesAsRequeued CapOnce
  CappedHoldsFloor CreateAtFloorOnly DeadlineWorkUnbilled DestroyDropsDebt DropRemainder
  EmptyHoldsFloor ExitRunsFree FoldAtNewWeight IgnoreWeight LiftByMax LiftCountsEntryWait
  NoFloorWhenIdle NoMinimumCharge OneRunnerPerBudget PreemptOnWake PriorityById RequeueAhead
  RequeueLifo RequeueWaitsForCap RescaleOnlyOnReturn ShortRunsFree SliceCountsExitWork
  SpreadChargesOnce StrideWeightIsLimit SwitchBilledToPrevious TieQueuedFirst TimeoutWakePreempts
  TimerWorkUnbilled UncapBanksCredit UncapForfeitsWait UnnormalizedLift WakeBanksCredit.

## Commit 3: measured (release, smp 1, icount shift=3, exec log; .tmp/K33/i64, i32)

| per timer entry, compute loop alone on its hart | rv64 | rv32 |
| --- | ---: | ---: |
| main db6575a3f (.tmp/K33/m64, m32) | 8,987 | 12,984 |
| in place | 4,245 (-53 %) | 6,736 (-48 %) |

Rest of the in-place entry (rv64): stride queue + marks 1,682 (three reconciles, the cap pass),
trap asm/handler 1,040, billing 553, rearm+SBI 342.

## Commit 3 gates: STOPPED (condition 5)

`make prebuilt` rc 0; `make -k set CASES="<30 sched-*> irq-boot-hart-only beamlet-reduction-rate
kernel-containment userland-boot init-boot bench-net-peer ipc-outcomes sum-clear
lend-untouched-page no-cruft launch-idle docs size-budget unsafe-budget formatting
model-host-tests model-mutations"` (log .tmp/K33/gate-ip.log): 78 targets, 9 rc 1, the rest rc 0.

1. rv32 sched-wake-no-preempt-harts: "0 mid-slice, 3 at another entry, 59 at a slice's end, 1 in
   kmain; fewer than 5". Under 10: the stop condition. The phase artefact K31 found, deeper: a
   slice end now ~half the cost, so wakes are answered at slice ends before the spinners' calls.
   rv64 passed. Per the ruling, B58 (stepped naps, the trace's slice decision) lands first.
2. "hart H re-picked budget N in place in an interrupt that ended no slice" (rv64 sched-latency
   smp 1 and 2, rv64 sched-latency-tcg smp 2, rv32 sched-latency smp 1 and 2, rv32
   sched-latency-tcg smp 2, rv32 sched-capped-mttcg smp 2). This is B58's trace race on the new
   path: `trace::timer_entry` samples slice_over at the entry's start; irq.rs decides after the
   expiry; when the slice ends during the entry, the kernel ends it (and here re-picks in place)
   while the trace's O says 2 ("to kmain, not over"). The new oracle clause catches it. The
   round trip has the same mislabel (the "took hart" false positives K31 recorded). B58's trace
   fix (record the decision's now and slice_end, the oracle judges 'over') resolves both.
3. rv32 sched-share-release: "[share] counted 949 of the calibrated 1000" (want >= 960; rerun alone
   the same, console .tmp/K33/ssr/). rv32 sched-large-weight-release fails the same 'useful work'
   expect (not rerun; same shape assumed). Cause: the calibration runs one spinner alone, now
   re-picked in place (cheaper: 8,481 iterations/ms), while the measured phase alternates budgets,
   still the round trip; the fraction falls. Expected to recover with the direct switch (commit
   4), which makes the switch path cheaper too. rv64 passed both.
4. rv64 size-budget: kernel 10,712 lines, ceiling 10,642: needs a `Size budget: kernel:` line and
   the ceiling raised (+70), or trimming.

Passed (rc 0, both widths, at their own hart counts): every other sched-* incl. sched-timer-entry
1+2, sched-share, sched-exit-churn, sched-capped, sched-wake-no-preempt, sched-lock-contention(-4),
sched-lock-contention-4-mttcg, rv64 sched-capped-mttcg, rv64 wake-no-preempt-harts;
irq-boot-hart-only, beamlet-reduction-rate, kernel-containment (1 hart), the smoke set, launch-idle,
docs, unsafe-budget, formatting, model-host-tests, model-mutations. Not run yet: the 2-hart sweep of
the one-hart sched-* and kernel-containment @2; reduction-rate's number.

## Gates, rebased (2026-10-14, head 904354363 on 2cb94d714)

Script .tmp/K33/g4.sh; logs .tmp/K33/g4-prebuilt.log, g4.log, g4s2.log (+ g4s2-<w>-<case>.out),
g4-twin.log; per-case logs target/jobs/<w>-<case>.log in the K33 worktree. Loopback TCP works
again: launch-idle and bench-net-peer are verdicts.

- `make -f scripts/jobs.mk prebuilt`: rc 0.
- `make -k -j 12 -f scripts/jobs.mk set CASES="<30 sched-*> irq-boot-hart-only
  beamlet-reduction-rate kernel-containment userland-boot init-boot bench-net-peer ipc-outcomes
  sum-clear lend-untouched-page no-cruft launch-idle docs size-budget unsafe-budget formatting
  model-host-tests model-mutations"`: set rc 2; 7 targets rc 1, the other 89 rc 0 (PASS at their
  own hart counts, both widths; rv32 has no target for the host-only ones).
- `make -k -j 2 -f scripts/jobs.mk set CASES="sched-wake-no-preempt-harts-mttcg"` (B58's twin,
  missing from the predecessor's sched list): rc 0, PASS rv64 and rv32.
- `--smp 2` sweep, `target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> --smp 2
  <case>` for the 15 one-hart cases without keep_smp (sched-budget-churn-shell sched-budget-churn
  sched-carve-inflation sched-debt-lift sched-destroy-billing sched-exit-churn sched-idle-gap
  sched-large-weight sched-lift-delay sched-server-busy sched-share sched-sleep-gaming sched-ties
  sched-timer-flood kernel-containment), both widths: 30 of 30 rc 0.

The seven rc 1:

1. rv32 sched-share-release: `[share] counted` below 960 of 1000 (ruled: until commit 4).
2. rv32 sched-large-weight-release: `useful work` below 942 (ruled: until commit 4).
3. rv64 size-budget: `libs/stride: 929 lines, ceiling is 879`. Commit 1 adds 50 code lines to
   libs/stride/src (Harts::repick) with no `Size budget: libs/stride:` line; the kernel's 70 are
   budgeted in commit 3. The predecessor's run reported only the kernel (the gate names the first
   crate over). Fix: raise tests/size-budget.toml libs/stride to 929 in commit 1 with its line.
4. rv64 docs: `docs/kernel/scheduling.md:70: malformed status` (and :68 `no status line covers
   Preemption points`): the status says `tested (10)`; the list has 12 (commit 3 adds
   host:redoubt-stride::the_crate_and_the_model_agree and
   host:testbench::a_pick_in_place_is_a_slice_end_that_requeued_its_runner). Fix: `tested (12)`.
5. rv64 formatting: tools/testbench/src/sched_oracle.rs module doc around line 111 over width (the
   'Several harts' paragraph amended last). Fix: `rustfmt +nightly` rewrap, two lines.
6. rv32 sched-wake-no-preempt-harts (icount, smp 2): `budget 31's 123 wakes: 0 mid-slice, 17 at
   another entry, 34 at a slice's end, 72 in kmain; fewer than 20 could have preempted`. Three
   reruns alone (.tmp/K33/g4rr-wnph-{1,2,3}.out): 17/34/72, 17/34/71 (1 mid-slice), 17/35/71:
   deterministic. B58 recorded 40-43 at a call on rv32 at this gate (wake_witnesses=20); rv64
   passes. Not the condition-5 stop (witnesses >= 10) but a failing gate, so STOPPED here. Reading:
   the in-place slice end is half the cost, so the armed hart's wake is answered at a slice's end
   (34, was fewer) before the other hart's call reaches it; the same shift K31 and B58 ruled a
   turn-order artefact under icount. The mttcg twin (harts at once) passes on both widths. Options
   for the ruling: (a) carry rv32's bound to commit 4 and re-measure there (the direct switch
   moves the slice end again); (b) lower wake_witnesses for rv32 with a ruling; (c) treat as a
   regression. I recommend (a): the twin is the judge of mid-slice, and the icount case's call
   witnesses are what commit 4's re-measurement will set anyway.
   RULED (kernel-red-2, 2026-10-14, QA thread K33-rv32-witnesses, binding): option (a)/(i): red by
   this ruling at commit 3, rv32's wake_witnesses bound carried to commit 4's re-measurement; not
   a regression (a vigour floor, not a rule clause; the clause passes on all 123 rv32 wakes; 17 >
   the stop 10 and > the twin's 5; timing-only, held on the twin). No toml edit in commit 3.
7. rv32 sched-capped-mttcg (smp 2): `hart share late-a: 448 of 1000 ... expected 500; target
   missed (450 <= share <= 550)`, every other share met. Rerun alone (--quiet,
   .tmp/K33/g4rr-capped.out): PASS, late-a 485. Timing flake of the mttcg case run beside the set
   (12 jobs at once); rv64 passed both times. Listed, not a finding.

Not changed: no edits to the worktree after the rebase; items 3-5 are the fixes I would amend into
commits 1 and 3 when told (each is one line or two).

## Amends (b)-(d) and their gates (2026-10-14, k33-implementer-3)

Heads after the amends, on 2cb94d714, tree clean:

1. 830009e86 (was 59f46c9d1): tests/size-budget.toml libs/stride 879 -> 929; message gains
   `Size budget: libs/stride: a slice end that picks where it is (Harts::repick), 50 lines`.
2. 374a9d0cc (was eba068549): replayed unchanged.
3. baaadd3ae (was 904354363): docs/kernel/scheduling.md:70 `tested (10)` -> `tested (12)`;
   tools/testbench/src/sched_oracle.rs module doc lines 108-111 rewrapped by hand to <= 110
   columns (rustfmt's own rewrap would have left a 110/104 pair; `rustfmt +nightly --check` on
   the result: clean). Message unchanged.

Done as `git checkout -b k33-amend 59f46c9d1`, amend, `git rebase --onto k33-amend 59f46c9d1
wp-K33` (no conflicts), amend HEAD, branch deleted.

`make -k -f scripts/jobs.mk set CASES="size-budget docs formatting"` (.tmp/K33/g5.log; per-case
target/jobs/rv64-<case>.log): set rc 0. size-budget PASS (`kernel: 10722 of 10722`, `libs/stride:
929 of 929`), docs PASS 1.5 s, formatting PASS 18.1 s. rv32 has no target for the three.

## sched_oracle.rs read in full (2026-10-14)

Read lines 1000-6344 (the file is 6,344 lines at baaadd3ae; the predecessor's 6,195 is the base's
count, before commit 3's 145 added lines) after the predecessor's 1-999. Nothing demanded a fix.
What the read covered and how it bears on commit 3:

- `check_wake_no_preempt_harts` (1030-1136): a wake inside a timer interrupt returning `O` 0 is
  "at a slice's end: not judged", `O` 1 mid-slice, `O` 2 took the hart unless an `X` is in it. The
  in-place exit writes `O` 0 (as the host test's fixture does), so its wakes land in `at_end`:
  this is the mechanism behind finding (e)'s 34 at a slice's end. The call branch stops at the
  first `I x K H R` on the hart; an in-place `K` without `H` ends the search as a witness, as any
  pick does. The `kmain` branch is reached only when the hart's last `H` is 0, which an in-place
  pick never writes, so it is unaffected.
- `charges` (2477-2570) and `Weights` (2402-2444): an in-place `R` then `K` at the same pass is
  a rise of 0, no charge; a weight stated by `H` persists across in-place picks (no `H` written).
  The `round_robin` fixture (5676-5721) writes `H 0` after the `R` as the kernel's round trip does,
  as commit 3's message says.
- The cluster, control, carve-return, round, lift-delay, charged-share, hart-share, lock-wait,
  section and walk checks, `run` (3067-3511; its head line carries commit 3's `({} in place)`),
  `fence`, `inflight`, the tests (3636-6208) and the model module (6210-6344): read, no dependence
  on the slice end's path beyond the `=`/`O` agreement B58 set and the in-place clause's test
  `a_pick_in_place_is_a_slice_end_that_requeued_its_runner` (4657-4739), whose refusals name the
  five conditions the module doc states.

## Commit 4: the direct switch, code and host-side proof (2026-10-14, k33-implementer-3)

First head a68135377 on baaadd3ae; light gates on it (.tmp/K33/c4-gates-light.log): size-budget
PASS (kernel 10750 of 10750), docs PASS 1.4 s, formatting PASS 17.9 s.

kernel-red-2's review of commits 1-3 (OK to build on), its P2s and P3 folded in (2026-10-14):
commit 3 amended to 3dd4fd6b2 (branch at baaadd3ae, amend, `git rebase --onto` of commit 4, one
comment conflict in sched.rs resolved); commit 4 replayed as 65e4082ce. (a) the in-place exit's
record is `trace::slice_ended()` (records `O` 0; debug-asserts it is inside a timer interrupt
that decided the slice over), `returned(to_user)` stays for `leave`; `O` 0 defined as "the slice
ended at this entry: to `kmain`, or in place to user mode" in sched.rs's RETURN doc,
docs/testbench.md (the RETURN paragraph, :768) and the oracle's module doc (:76); oracle code
unchanged. P3: the module doc says `R` and `K` of an in-place slice end share the entry numbered
before them and no clause reads an `R`'s entry (:111-113). (b) stride-host-tests run at head with
the light gates below (its result there). (c) sched-timer-entry: binding for the machine step:
commit 4's 1-hart p99 must come under B58's (1,096 rv64 / 1,597 rv32; commit 3 measured 1,254 /
1,869), else a finding; the head's docs and toml quote measured numbers (1,073/1,597 are stale).
Commit 4 uses `slice_ended()` for `Own` and `Other` alike. `slice_ended()` is 7 code lines, so
commit 3 was amended once more to take them in its own budget: ceiling 10652 -> 10729, line
`Size budget: kernel: a slice's end that picks where it is (sched::repick, time::due,
trace::slice_ended), 77 lines`; commit 4's ceiling 10729 -> 10757, its line unchanged (28).
FINAL HEADS: 830009e86, 374a9d0cc, 23e0a8bfe (commit 3), 0ba4bd338 (commit 4), on 2cb94d714;
tree clean. At 0ba4bd338 / its predecessor 65e4082ce (same code, ceiling only):
kernel release and checked+sched-trace builds rv64 and rv32 rc 0 (.tmp/K33/c4b-build.log);
light gates (.tmp/K33/c4b-gates-light.log): docs PASS 1.4 s, formatting PASS 22.2 s,
stride-host-tests PASS 16.6 s; size-budget PASS at 0ba4bd338 (kernel 10757 of 10757). Files: kernel/src/sched.rs,
kernel/src/arch/riscv/irq.rs, docs/kernel/scheduling.md, docs/testbench.md, tests/size-budget.toml
(kernel 10722 -> 10750; `Size budget: kernel: a slice's end that switches where it is
(sched::switch_here, Picked), 28 lines`).

Design, against kernel-red's conditions:
- (2) `sched::repick` now returns `Picked::{Own, Other(pid, tid), Kmain}`. For `Other` it records
  the pick (`K`, cursor) and bills `Payer::Next` from `now`, as the round trip did; the trap
  (`irq.rs`) then calls `sched::switch_here(ss, tid, pid, next)` =
  `ss.activate_process_thread(tid, pid, next, true)` + `arch::mem::sync_icache()`: exactly kmain's
  switch (`sched::switch`) with the leaving side exactly `preempt`'s (`leave_previous` makes the
  thread ready; `can_resume`), no copy: `mapping.activate` with the stale mask/ASID flush and
  `set_pid`/`left` are inside `activate_process_thread`. `hart::shot_down()` is re-checked after
  the pick, before the switch, and handled as the entry handles it (`switch_to_thread(KERNEL_PID)`);
  under the lock it cannot fire (a shootdown is served outside it). An `Err` from the activation
  (ruled out under the lock) falls back to `preempt()`, kmain's route. The return (`sched::leave`)
  does the rest unchanged: switch-on (`H n weight`), owed billed to `n`, reconcile, new slice,
  rearm. `set_slice_end(NEVER)` and `trace::returned(false)` (the `O 0`) happen in `repick` for
  `Own` and `Other` alike, as the round trip's return to kmain wrote them.
- (3) Stride and differential: unchanged in code. `Repick::Other` (commit 1) is already this switch
  ("the switch into the pick is the caller's"), and `Kernel::repick`'s `Other` arm already models
  it (mark b, enter n); the equivalence `repick == round_trip` at 1/2/4 harts is the existing test.
  Host proof: `cargo test -p redoubt-stride` 25 + 2 (differential, 17.2 s) pass (.tmp/K33/c4-stride.log).
  Checked audits: for `Own` the marks/cap audit runs in `repick` after the switch-on (as before);
  for `Other` it runs at `leave`, after `leave_previous` + settle count the thread switched from as
  waiting, as the queue does (the counted-not-marked state in between is never audited).
- (4) No synthetic records; no oracle change: a pick of another budget without `H 0` is judged as
  any pick (the in-place clause needs `runs[hart] == picked`); `H n` comes at the return as every
  switch-on's does. Trace of a switching slice end: `I = [B] R b K n O 0 J b 1 H n` (was
  `I = R b H 0 O 0 K n J b 1 H n`). `cargo test -p testbench sched_oracle`: 57 pass
  (.tmp/K33/c4-oracle.log). ONE POINT FOR KERNEL-RED (an oracle reading, not a change made):
  `check_wake_no_preempt` (one hart) counts a proof only if, after the slice end's `O 0`, the
  hart's first `K`-or-`I` is a `K` (sched_oracle.rs:1009-1011); with the direct switch the `K` is
  inside the interval, before the `O`. Today's traces should still satisfy it (the picked sleeper
  blocks, so kmain's next `K` precedes any `I`), to be confirmed by the machine gate; if the gate
  wants it, the amendment is to accept the `K` after the `R` inside the interval, and it goes to
  kernel-red first.
- Builds: kernel release rv64 and rv32 (`--features qemu-virt`) rc 0; `--profile checked
  --features qemu-virt,sched-trace` rv64 and rv32 rc 0 (.tmp/K33/c4-build.log,
  c4-build-checked.log; the plain `debug` profile overflows FLASH before and after, not used by
  the bench). 11 warnings are pre-existing: feature-gated trace consts (`lock-trace`,
  `hold-trace`, `walk-trace`). `cargo +nightly fmt --check -p redoubt-kernel` clean. No bin linking
  redoubt_rt is touched (libs/rt unchanged); the prebuilt build of every program is the machine step.
- Docs: scheduling.md Preemption points (the direct switch, what still goes through kmain, the
  trace), the figure (the pick at a slice's end is the trap's), Charging and Responsiveness
  (B58's one SBI call; the cost figures replaced by placeholders `TBD-RV64 TBD-RV32 TBD-MS-RV64
  TBD-MS-RV32 TBD-RV64-KMAIN TBD-RV32-KMAIN`, Preemption points :141-146, Charging :337-339,
  Responsiveness :715-717, to be filled from the measurement and amended into this commit);
  testbench.md oracle paragraph (a switching slice end is judged as any pick). Summaries checked:
  scheduling.md Preemption points status unchanged (tested (12): no new test); testbench.md oracle
  status unchanged (tested (29): no new test).

## Commit 4 correctness gates (2026-10-14, heads 23e0a8bfe / 0ba4bd338, machine not yet quiet)

Through q, general cores, train 22's bench running beside (no timing-judged case run). Logs
.tmp/K33/g6-prebuilt.log, g6.log, g6s.log, g6s-<w>-<case>-smp<n>.out; per case
target/jobs/<w>-<case>.log.

- `make -f scripts/jobs.mk prebuilt`: rc 0.
- `make -k -j 4 -f scripts/jobs.mk set CASES="smp-shootdown smp-evict smp-fence asid-reuse-stale
  irq-boot-hart-only model-host-tests model-mutations size-budget docs formatting unsafe-budget
  lend-untouched-page kernel-containment"`: set rc 0; all 21 targets rc 0 (rv64 13, rv32 8: the
  host-only ones have no rv32 target). PASS, both widths: smp-shootdown, smp-evict, smp-fence
  (smp 2), asid-reuse-stale (the case; its recorded negative was not run), irq-boot-hart-only
  (smp 2), lend-untouched-page (smp 1 and its own smp 4), kernel-containment (smp 1: 275 s rv64,
  456 s rv32); rv64: model-host-tests 136.7 s, model-mutations 25.9 s, size-budget (10757 of
  10757), docs, formatting 22.3 s, unsafe-budget.
- `target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> --smp N <case>`:
  lend-untouched-page --smp 4 PASS rv64 and rv32; kernel-containment --smp 2 PASS rv64 (382 s) and
  rv32 (492 s). All four rc 0.

Still owed when the machine is quiet (condition (6) and the ruling): the sched-* set,
sched-timer-entry five runs a width alone (1-hart p99 under B58's 1,096/1,597 or a finding),
sched-wake-no-preempt-harts five runs a width alone (rv32 floor), wake-preempts on rv32 under
icount, the twin, beamlet-reduction-rate, the -release share cases, the exec-log instruction
counts for the docs' TBD figures.

## Commit 4 timing gates (2026-10-14, machine quiet, heads 23e0a8bfe / 0ba4bd338)

Sched set: `make -k -j 4 set CASES="<30 sched-*> (all tests/sched-*.toml but
sched-cluster-old-control)"` (.tmp/K33/g7.log, g7-cases.txt). Findings while it ran:

1. rv64 sched-carve-return FAIL 'carve-return: parent 31 lost its running turn at record 927 (O)',
   deterministic (rerun alone .tmp/K33/g7rr-carve.out, run dir kept g7rr-carve-run). Trace
   914-935: `I 32`, `E 31` (31's timeout expired; `W 31` at the pass 32 is requeued to), `=`
   over, `R 32`, `K 31` (a waker ranks ahead of a requeue: the right pick), `O 0`, `J`, `H 31`,
   then `G 31` (the carve). check_carve_return (sched_oracle.rs:2107-2124) takes the last `K`
   before the down-weight as the parent's pick and refuses an `O` != 1 after it; the `O 0` at 927
   is the interrupt's return that switched into the parent. Oracle reading of the new trace shape
   (`K` inside the interrupt before its `O`; the round trip wrote `O 0` then `K` in kmain).
2. rv64 sched-wake-no-preempt FAIL: 10 of 20 proofs ({31: {34: 3, 37: 3, 40: 2, 43: 2}}): the
   proof's last condition (:1009-1011) wants a `K` before any `I` after the slice end's `O`; the
   `K` is now inside the interval. The reading flagged in the commit-4 report.

Both proposed to kernel-red through the orchestrator (question, 2026-10-14) before any oracle
change: (1) the running turn begins after the `O` of the interrupt the pick is in; (2) a `K` after
the `R` inside the interval counts as the pick. Neither touches a rank/floor/wake/lift clause.

Set result: 56 targets rc 0, 4 rc 1 (the two cases above, both widths; rv32 messages 'record 916'
and '10 of 20: {31: {34: 3, 37: 2, 40: 1, 43: 1}}'), set rc 2. The -release share cases PASS both
widths (sched-share-release, sched-large-weight-release): condition met, no stop. Every other
sched case PASS, the twin and sched-capped-mttcg included.

Alone runs (.tmp/K33/alone/*.out, summary.log, twin-more.log, twin8.log; q --cores 2 unless said):

- sched-timer-entry, five a width, 1 hart (p50/p99/max ticks net of audits): rv64 927-928 /
  929-930 / 3,458-3,461 (379 sections); rv32 1,387-1,389 / 1,413-1,415 / 4,261-4,265 (366).
  Under B58's p99 1,096 / 1,597 (commit 3: 1,254 / 1,869): condition (6) and kernel-red-2's (c)
  met. 2 harts, recorded: rv64 2,923-2,927 / 5,430-5,433 / 5,658-6,833; rv32 4,809-4,810 /
  7,352-7,354 / 9,698-9,702. Bounds kept (p99 2,000: 1.41x the slower p99; max 5,000: 1.17x the
  slower max); docs and toml to quote these.
- sched-wake-no-preempt-harts, five a width (mid-slice / at a call / slice's end / kmain): rv64
  28/69/19/7, 30/72/15/6, 26/78/13/6, 27/67/24/5, 26/81/9/7 (witnesses 94-107); rv32 3/66/35/19,
  4/66/35/18, 2/67/36/18, 0/68/36/19, 0/65/38/20 (witnesses 65-70). The 20 floor holds on rv32
  with 3.2x margin (least 65); per the ruling the floor stays 20 (never below 10), the toml
  comment and scheduling.md ranges to quote these. Finding (e) of commit 3 (17 at a call) is gone:
  the switch answers more wakes at a call.
- The -mttcg twin (host clock), q --cores 2, ten a width: rv64 mid-slice 11, 18, 14, 15, 16, 10,
  20, 25, 16, 16; rv32 18, 6, 10, 7, 3 (FAIL: 3 < wake_mid_slice=5, run #4), 14, 13, 12, 13, 16.
  B58 recorded 52-78 / 57-71 (ten runs, under load); the loaded set run here gave 70 (rv64) and
  72 (rv32). At the B58 base in a scratch worktree (.tmp/K33/meas/b58, `cargo testbench`, q
  --cores 8), three a width: rv64 44, 46, 44; rv32 43, 39, 41. Commit 4 with q --cores 8, three a
  width: rv64 52, 55, 49; rv32 48, 57, 52 (twin8.log). So the low counts are the 2-core lease
  (two vCPU threads and QEMU's I/O thread sharing two cores change the race), not the switch:
  at equal lease commit 4 has MORE mid-slice than the base. The gate's quiet recipe leases the
  quiet core set (4), where the set run passed with 72. The twin's bound is unmoved (ruling 3);
  the 2-core alone FAIL is a run-method artefact, listed here, not a finding against commit 4.
- beamlet-reduction-rate, alone, both at this machine's quiet: commit 4 rv64 57,529, rv32 52,150
  (.tmp/K33/meas/rates/k33-*.log); B58 base rv64 56,006, rv32 49,869 (rates/b58-*.log, scratch
  worktree, `cargo testbench`). Gain +2.7 % rv64, +4.6 % rv32. (beamlet.md's table: 55,674 /
  49,928 at the base.)
- Spinner counts of sched-timer-entry's release program (two spinners, one hart, 300 ms window,
  exec-log measurement boots, .tmp/K33/meas/m*/console.log): commit 4 3,549,184 + 3,524,864 =
  7,074,048 (rv64), 1,233,920 + 1,226,496 = 2,460,416 (rv32); commit 3 3,468,800 + 3,450,624 =
  6,919,424 and 1,196,800 + 1,191,168 = 2,387,968: +2.2 % and +3.0 % useful work from the switch;
  B58 (main's base) 3,509,760 + 3,500,032 = 7,009,792 and 1,217,536 + 1,217,536 = 2,435,072:
  commit 4 is +0.9 % and +1.0 % over main (commit 3 alone cost 1.3 % / 1.9 %: its Other path
  paid repick's steps and then kmain's).
- Exec-log instruction counts per timer entry (QEMU `log exec,nochain,int`, release bundles of
  `./launch --program sched-timer-entry --smp 1`, icount shift=3, one hart, two spinners, every
  entry a switching slice end; .tmp/K33/meas/{m,c3m,b58m}{64,32}/entries.txt, tools in
  .tmp/K33/meas: boot2.sh, drive2.py, measure.sh, grab.sh): commit 4 7,050 rv64 (101 entries,
  median 7,057) and 10,577 rv32 (263, median 10,587), firmware 52/57 (one SBI call); commit 3
  10,243 / 14,940; B58 8,482 / 12,184. Docs quote 7,050 / 10,600 and "through kmain 8,500 /
  12,200" (B58, main's); 0.056 / 0.085 ms at 125 instructions a µs.
- wake-preempts negative on rv32 under icount, scratch worktree .tmp/K33/meas/c4 at 0ba4bd338
  with a temporary tests/sched-wake-preempts-neg.toml (sched-wake-no-preempt-harts with
  `kernel_features = ["sched-trace", "wake-preempts"]`, rv32 only; never in the K33 tree),
  `cargo testbench --exact --arch rv32 sched-wake-preempts-neg`: FAIL as required, 'record 1196:
  budget 31's wake at record 1193 took hart 0 from budget 37' (.tmp/K33/meas/neg-wake-preempts-rv32.out).

Docs and toml comments filled from these and amended into commit 4 (2026-10-14): scheduling.md
Preemption points (ranges, the twin's host race, the rv32 icount negative, timer-entry's 930 /
1,415 and 3,461 / 4,265, the switching slice end's 7,050 / 10,600 and kmain's 8,500 / 12,200),
Charging (0.056 / 0.085 ms, 7,050 / 10,600, two spinners), Responsiveness (1.06 / 1.09 ms with two
runnable budgets); tests/sched-timer-entry.toml, sched-wake-no-preempt-harts.toml and the twin's
comments. Bounds unchanged: timer-entry p99 2,000 (1.41x the slower p99) and max 5,000 (1.17x);
wake_witnesses 20 (a third of rv32's least 65); the twin's wake_mid_slice 5 unmoved. HEAD after
the amend: 088f10812 (commit 4) on 23e0a8bfe; light gates on it: size-budget PASS, docs PASS,
formatting PASS.

kernel-red-2's ruling (QA thread K33-oracle-direct-switch, 2026-10-14): yes to both; proof-shape
readers, not clauses. Bindings honoured in sched_oracle.rs (amended into commit 4):
- check_carve_return: a `K` inside a timer interrupt on its own hart (an `I` there with no `O`
  between) begins the turn after that interrupt's `O`, which must be the same hart's and pass 0
  (else refused); `K..O` is still watched (another `K`, the parent's `R`/`D`), only the `O` itself
  excused; the turn from that `O` to the up-weight as before; the round trip's shape as before.
- check_wake_no_preempt: the pick after the slice's end is the one `K` inside the second interval
  after the spinner's `R` and before its `O` 0, or, as before, the first `K`-or-`I` after the `O`
  being a `K`; a `K` before the `R`, or two `K`s, refuses the proof; WDXY guard and `O` 0 kept;
  the `K`'s budget unchecked, as before.
- Host tests: carve_return_proof_requires_the_same_running_turn gains the direct shape (ok) and
  negatives (an interrupt returning 1 or 2 around the pick; the parent's `R` or another `K`
  between the pick and its `O`; a later `O` 0 in the turn; an `O` 1 in the turn stays fine);
  timeout_wake_proof_rejects_preemption_and_an_unframed_wake gains the direct shape (the `K`
  inside, the sleeper's `I` next: ok), a `K` before the `R` (refused), two `K`s (refused), and the
  round trip with an `I` before its `K` (refused as before). Module doc states both shapes.
  `cargo test -p testbench sched_oracle`: 57 pass (.tmp/K33/c4-oracle-3.log); rustfmt clean.
  (First attempt panicked on the empty range `pick + 1..from` for the round trip's pick; fixed.)
- Commit 4's message reworded for it (the rank clauses judge the pick as any pick; the two
  readers take the new shape) and given the measured figures.
Then prebuilt and `make -k -j 4 set CASES="sched-carve-return sched-wake-no-preempt
sched-wake-no-preempt-harts sched-timer-entry size-budget docs formatting unsafe-budget"`
(.tmp/K33/g8-prebuilt.log, g8.log): prebuilt rc 0; set rc 0, every target rc 0: sched-carve-return
PASS rv64 and rv32, sched-wake-no-preempt PASS rv64 and rv32, sched-wake-no-preempt-harts PASS both,
sched-timer-entry PASS both (smp 1 and 2), size-budget PASS, docs PASS, formatting PASS 19.2 s,
unsafe-budget PASS.

FINAL HEADS on 2cb94d714: 830009e86, 374a9d0cc, 23e0a8bfe (commit 3), ec41f5286 (commit 4); tree
clean. Scratch worktrees .tmp/K33/meas/{c3,b58,c4} removed after the measurements (bundles, logs
and entries.txt kept under .tmp/K33/meas). Everything the orchestrator's instruction of
2026-10-14 listed has run; nothing left open on commit 4 but review.

## Final P3s and freeze (2026-10-14)

kernel-red-2's merge verdict on ec41f5286: OK with notes; three docs-only P3s, all commit 4's,
amended into it: (a) scheduling.md: the twin's eight-core ranges stated per width (49 to 55 rv64,
48 to 57 rv32); (b) docs/userland/beamlet.md's rate table: 57,529 / 52,150 measured here, the
base's 55,674 / 49,928 kept as the row "before the kernel's slice end picked and switched where
the trap is", the page's convention; (c) the oracle's module doc: between `K n` and `H n` the
hart's runner still reads `b`, harmless since `b` was just requeued behind `n`. Gates and the
final head below; then wp-K33 is FROZEN for train 23 (merges whole with B59).

`make -f scripts/jobs.mk set CASES="docs formatting size-budget"` at the amended head: docs PASS
1.6 s, formatting PASS 22.5 s, size-budget PASS (10757 of 10757); rustfmt clean.

FROZEN HEADS on 2cb94d714: 830009e86, 374a9d0cc, 23e0a8bfe (commit 3), 9989e2d7f (commit 4); tree
clean. No further changes to wp-K33.

Machine step (when the machine is quiet, per the orchestrator and the rulings): prebuilt; the
sched set + irq-boot-hart-only beamlet-reduction-rate kernel-containment smp-shootdown smp-evict
smp-fence asid-reuse-stale (negative still fails) lend-untouched-page@4 kernel-containment@2 and
the -release share cases (must be green or STOP); sched-timer-entry five runs a width alone, bounds
with stated margin; sched-wake-no-preempt-harts five runs a width alone (rv32 floor >= 10, toml
comment + scheduling.md:97-100 ranges); wake-preempts on rv32 under icount (quote the failing
wake); twin unmoved; beamlet-reduction-rate gain; QEMU exec-log instruction counts for the
switching slice end (two spinners) release both widths, and the same through kmain at baaadd3ae
for the "was" figures.

## Open

- Branch state: baaadd3ae 374a9d0cc 830009e86 on 2cb94d714, tree clean; commit 3's gates as in
  'Gates, rebased' with items 3-5 fixed above; the rv32 wake-no-preempt-harts ruling (QA thread
  K33-rv32-witnesses) ruled 2026-10-14: item 6 red by ruling (above). Commit 3 is final pending
  review.
- Commit 4, binding from the ruling (besides kernel-red's six conditions in the handoff), when
  told to start: (1) sched-wake-no-preempt-harts five runs a width ALONE (not under -j 12);
  rv32's wake_witnesses floor set from them with a stated margin, never below 10; the toml
  comment and docs/kernel/scheduling.md:97-100 give the new ranges and the reason (the slice
  ends move QEMU's turns, not the rule); (2) re-run wake-preempts (the negative, temp toml) on
  rv32 under icount at commit 4 and quote the failing wake, so the floor is non-vacuous there;
  (3) the twin's wake_mid_slice=5 stays the judge of mid-slice, unmoved; (4) K33 merges only
  whole, with the gate green.
- Reading in full: sched.rs, stride lib.rs, differential.rs (predecessor); sched_oracle.rs 1-999
  (predecessor) and 1000-6344 (above): done. scheduling.md and testbench.md read at the edited
  sections (predecessor, and :60-80 here).
