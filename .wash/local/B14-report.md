# B14 report: the share fixtures at the 1 ms slice

Branch wp-B14, worktree /home/mcloonan/redoubt/.worktrees/B14, from main fdafcf2cb.
Head 67746b417 `tests: sched-share judges each spinner's share of what the three counted`.
State: sched-share is fixed and committed. sched-carve-return needs no fixture change: it passes on
K24's kernel on both widths (section 2). sched-exit-churn threads-exit goes to K25 as a kernel
charge, by the orchestrator's decision (A); its clause is unchanged (section 3). Analysis scripts and the rv32 logs are in /home/mcloonan/redoubt/.wash/local/B14/.

## Reproduced on main's tip (boot cases run under icount, so the numbers repeat exactly)

| case | rv64 | rv32 |
| --- | --- | --- |
| sched-share | PASS, weight 300 got 550 (the floor is 550) | FAIL, weight 300 got 543 |
| sched-exit-churn | FAIL, threads-exit net 439 | FAIL, threads-exit net 426 |
| sched-carve-return | PASS (u-after-return net 479) | FAIL, `parent 31 lost its running turn at record 1084 (R)` |

The oracle lines are in target/jobs/{rv64,rv32}-sched-{share,exit-churn,carve-return}.log in the
worktree; full console logs are in .wash/local/B14/.

## 1. sched-share: the bias, and the fix

The calibration (`Bench::new`) measures the counting loop's rate with the launcher running alone
for 100 ms. In the window, the three spinners' counts add up to 912 (rv64) and 899 (rv32) of
1000 of what that rate would fill. The shortfall is what a slice end that switches budgets costs
over a lone spinner's slice end, plus the marks audit. A checked build runs that audit about once
a slice (`Marks::audit_due(now, SLICE_US, ..)`), and it is not subtracted, because the case has no
trace. Every spinner pays these costs once per slice it runs, so the relative shares hold:
543/899 = 603, 178/899 = 198.

Fix (tests/programs/src/bin/sched-share.rs): each share is judged of the CPU the three counted,
not of the alone-rate's window. The window share is printed beside it, along with the sum.
Results:
- rv64: 198 / 198 / 602 counted (181 / 181 / 549 of the window), the three counted 912.
- rv32: 198 / 198 / 603 counted (178 / 178 / 543 of the window), the three counted 899.

The fix needs no oracle change: sched-share has no trace and no post-check.

## 2. sched-carve-return rv32: the lost turn, explained against R12 (fixture unchanged)

R12's preemption points: a slice ends `SLICE_US` after the pick (kernel/src/sched.rs `pick`:
`set_slice_end(now + SLICE_US)`). The fixture assumes that U's wake begins a fresh slice: it spins
until 9/10 of a slice after its wake and then destroys the carve. In the rv32 trace:
- U (budget 31) is picked right after an audit that ends at 237,763 µs (record 1066, V).
- Its next timer entry, the slice end, is at 238,905 µs (record 1081, I 31). That is 140 µs past
  the pick's 1 ms, and it lands just as its carve call returns (CARVE-OBS: the create took
  792 µs after the wake).
- So pick to wake is about 350 µs. That is the leave path's work (settle, switch, reconcile)
  before U's first instruction, inside the slice. Add the 792 µs create and U has spent more than
  1 ms before it can call destroy, at any spin length. The slice ends at weight 1, U is requeued
  (R at 1084), and its pass keeps the destruction out of the window, as the case's comment
  predicts.
- On rv64 the same path is cheaper and the case passes. Its turn proof: K 1099 through both
  weight changes, create 740 µs.

This is the per-switch cost, counted inside the slice. K24 ("a slice starts when the thread
returns to user") removes the pick-to-wake part. After that, U's destroy comes at about 900 µs
into its slice. Nothing in the fixture can make the destroy land inside the pick's slice on rv32
under the current rule. I changed nothing here.

Confirmed on K24's kernel (wp-K24 at 2f9a6ec31, a detached scratch worktree since removed, with
`q run --cores 8 -- cargo testbench --exact --arch <w> sched-carve-return`):
- rv32: rc=0, PASS. Turn proof: `parent 31 K 1031 through 1000->1 and 1->1000`, create 792 µs;
  u-after-return net 480 (gross 466).
- rv64: rc=0, PASS. K 1073 through both changes, create 740 µs; net 480 (gross 471).

So carve-return is fixed by K24 with the fixture as it is. It needs a rerun on main once K24
merges.

## 3. sched-exit-churn threads-exit: not a fixture bias (decided: A, the kernel side goes to K25)

The orchestrator's answer: the cost of the pick and switch belongs to the budget whose action
caused it, not to the descheduled victim and not to nobody. The clause stays as it is, and K25
takes the kernel side on both widths. The numbers below are for K25. The scripts are in
.wash/local/B14/: `charges.py LOG 100` gives the per-budget charges in each SHARE window;
`victim.py LOG 100 32 threads-exit` gives the victim's charges by context; `absorbed.py LOG 100 31
threads-exit` gives the lift's absorption. Each takes the full console log;
.wash/local/B14/sched-exit-churn-rv32-smp1.log is main fdafcf2cb's.

From the rv32 trace (`charges.py`, `victim.py`, `absorbed.py`), in threads-exit's window: 2 s,
57.9 ms of audits, 1942 ms net. Attacker = budget 31, victim = budget 32.
- What the kernel charged: victim 928.8 ms, attacker 868.5 ms, so the victim got 516 of the 1000
  charged to the pair. The charges favour the victim.
- But only 1797 ms of the 1942 ms net window is charged to either budget: 7.5% went to nobody.
  Beside an honest spinner (carve-return's window) the gap is 1.9%.
- The victim's count stands for 827.9 ms, 89% of what it was charged. A plain spinner in
  carve-return counts 97.8%, and processes-exit's victim 99%.
- The wake's floor lift absorbs nothing of the attacker's (0 µs on rv32, 5 ms on rv64), so
  sleeping is not the gain.
- So the attacker's 1 ms thread cycle (start a worker, the worker counts 0.9 ms and exits, the
  main thread polls on a 1 ms timeout) costs about 100-150 µs a cycle. That cost lands on the
  victim (the deschedule rule bills the pick and switch to the budget descheduled) or on nobody.
  At a 10 ms slice it was 1-1.5% of a cycle; at 1 ms it is about 10%. That moves the victim from
  above 450 to 426 (rv32) / 439 (rv64).
- That is exactly what this case exists to catch, so loosening it would hide it. Options are in
  the question: (A) the kernel bills that time to the attacker (K25 has threads-exit on rv64; the
  mechanism looks the same on rv32 since SCHED1); (B) the fixture judges against a control round
  (victim beside an honest spinner), which would pass at about 473 and hide a gain of about 5%. I
  recommend A.
- processes-exit and processes-fault pass on both widths (net 484-489).

## Gates (head 19f847c43 unless noted; the first round ran on dd27086f7)

- `make -f scripts/jobs.mk prebuilt`: rc=0 (rebuilt before each run set).
- sched-share alone: rv64 rc=0, rv32 rc=0 (on dd27086f7).
- All 19 sched-* cases, the two widths run side by side (my reading of "the light set": I found no
  definition of it). Tree = the commit's content before one doc reflow.
  - rv64: every case passes but sched-exit-churn (threads-exit, item 3) and sched-timer-flood.
  - rv32: also failing are sched-carve-return (item 2) and sched-cluster-old-control (K25's).
  - sched-timer-flood fails on both widths only on B17's clause: cancelled-waits `0 finding
    another budget's wait ended early (none, but the case requires some)`. Its share passes:
    net 467 / 458.
- `q run --cores 4 -- cargo test -p testbench`: rc=0, 141 passed.
- docs: rc=0 (on the final text). formatting, no-cruft, size-budget: rc=0 each (before the
  reflow, which touched only .md lines).
- Not run: the unsafe ratchet (no Rust outside a test program changed), the whole bench.

## Documentation

- docs/kernel/scheduling.md, Charging: the measured shortfall and why sched-share judges a ratio
  of counts. R12's attack list: sched-share's share is of the CPU the three counted.
- docs/testbench.md, Checked builds: the claim "a case that judges a share in its program has no
  audit inside its window" was false (the marks audit runs about once a slice). It is replaced by
  what is true: sched-share and sched-server-busy judge ratios, net of both costs; the cases that
  judge a count of the window carry the costs in their tolerance.
- Checked with no change needed: README.md, GETTING-STARTED.md, docs/README.md,
  docs/plan/m1-separation.md (names sched-share against R12, still true), docs/SECURITY.md R12
  row (the same cases), docs/kernel/timer.md (lists sched-share as a test only).
- docs/testbench.md has no per-clause table for shares: the share paragraph is the clause text,
  and that is what I updated.
- The pages are hotspot files. I read the edited sections and their surroundings, not the whole
  files (the context rule forbids whole-file reads of large files).

## Open

- threads-exit: K25's kernel change (decision A).
- Reruns on main after K24 and K25 merge: carve-return (passes on K24 already), exit-churn and
  sched-share.
- The residual in docs/testbench.md (a count running 0.3-0.65% over its window) is untouched.
  The calibration runs alone, so it already includes a lone budget's slice ends.

## Round 2 (orchestrator: keep the efficiency visible)

The changes, folded into the one commit, now head 19f847c43:
- sched-share prints `[share] counted <n> of the calibrated 1000` (912 rv64, 899 rv32) with no
  verdict, and the case's expect list requires that line.
- docs/testbench.md says the share is relative and the efficiency is reported beside it.

Gates on 19f847c43:
- docs rc=0. The first try failed with doccheck C5, because the first citation of R12 must read
  `R12 (scheduling)`; the sentence now names the promise in words.
- prebuilt rc=0.
- sched-share rv64 rc=0 (603 / 198 / 198 counted, counted 912).
- sched-share rv32 rc=0 (603 / 198 / 198, counted 899).
- cargo test -p testbench rc=0, 141 passed, and formatting rc=0, both on 8927bbae6. Only a .md
  file changed after it.

There is no oracle change in this package: tools/testbench/src/sched_oracle.rs is untouched. The
fix is the sched-share program, its case file and the two pages.

## Round 3 (red P2: does the marks audit run in sched-share's window?)

It does. I ran sched-share once with `kernel_features = ["sched-trace"]`, in a scratch worktree at
19f847c43 (since removed). Both widths passed, counting 911 and 896 (912 and 899 untraced).

Over the span from the spinners' first pick to their last (about 2.09 s):
- rv64: 1,595 marks audits (U/V 4), 50.3 ms.
- rv32: 1,517 marks audits, 72.5 ms.

That is about one after every slice end, so a slice end between spinners does leave `visited`
set. The same holds in carve-return's u-after-return window, where neither budget blocks: 784
marks audits against 784 timer entries (rv32).

So the claim is kept, and the time is now in docs/kernel/scheduling.md ("the marks' audit after
about every slice end: 50 ms of the 2 s window on rv64 and 73 ms on rv32, in a traced run"). The
figures are matched to the latest runs: 603 counted on both widths, 550 and 543 of the window.
The commit body is updated to match.

Head 3a5c213dc; only docs/kernel/scheduling.md differs from 19f847c43. docs rc=0.

## Round 4: rebased onto main with K24 (2151b2aa4)

The rebase was clean. Fresh prebuilt rc=0 (206 / 192 cases). All runs are on 9cc004f30, which
has the same code as head 7855c341c; the head differs only in the page's figures.

| | before K24 (fdafcf2cb + B14) | after K24 (2151b2aa4 + B14) |
| --- | --- | --- |
| sched-share rv64: counted N of the calibrated 1000 | 912 | 921 |
| sched-share rv32: counted N of the calibrated 1000 | 899 | 910 |
| rv64 weight 300, of what was counted (of the window) | 603 (550) | 602 (555) |
| rv32 weight 300, of what was counted (of the window) | 603 (543) | 603 (549) |
| marks audits in the window, traced, rv64 | 1,595, 50.3 ms | 1,511, 47.6 ms |
| marks audits in the window, traced, rv32 | 1,517, 72.5 ms | 1,427, 68.2 ms |
| sched-carve-return rv32 | FAIL (lost turn at 1084) | PASS: K 1031 through both changes, net 480 (gross 466) |
| sched-carve-return rv64 | PASS | PASS: K 1073, net 480 (gross 471) |

- Under the old test, sched-share would now pass on rv64 too (555 of the window, floor 550), but
  rv32 would still fail (549).
- K24 recovers about 1 point of the efficiency on each width.
- The page and the commit body now give the after-K24 figures.
- Gates on 7855c341c: docs rc=0. sched-share rv64 and rv32 rc=0, and carve-return rv32 and rv64
  rc=0 (on 9cc004f30).
- The whole sched-* set waits for K25.

## Round 5: rebased onto main f820b6ba3 (B18 merged)

The rebase was clean; head is 34afe5d19. Fresh prebuilt rc=0 (216 / 202 cases). Runs are on
54e501e6e, which has the same code as the head; the head only changes the page's rv32 figure from
910 to 909.

- sched-share rv64 rc=0: weight 300 got 602 of what was counted (555 of the window); counted 921
  of the calibrated 1000.
- sched-share rv32 rc=0: weight 300 got 603 (549 of the window); counted 909 of the calibrated
  1000.
- sched-carve-return rv32 rc=0: K 1033 through both weight changes, u-after-return net 480
  (gross 466).
- docs rc=0 on 34afe5d19.

Still holding for K25 before the whole sched-* set.

## Merge report: rebased onto main ba4aabd8b (K25 merged)

Head 67746b417, one commit: `tests: sched-share judges each spinner's share of what the three
counted`.

The range-diff against 34afe5d19 shows no change to any hunk of mine. The only difference is
context: K25 rewrote the line just before my scheduling.md hunk (the slice end's cost is now
"billed to the budgets picked after each slice's end"). After the gate I amended the figures in
the page and the commit body to this main's measurements. That is the only difference between
the head and 42a411fd7, the commit the gate ran on.

The short gate on 42a411fd7, all through jobs.mk, every exit 0:
- build-rv64 rc=0, build-rv32 rc=0, prebuilt rc=0.
- docs, formatting, size-budget, unsafe-budget and no-cruft: rc=0 each.
- All 19 sched-* cases on rv64 and rv32: rc=0 each, sched-exit-churn and sched-timer-flood
  included.
- The smoke set on rv64 and rv32: userland-boot, init-boot, bench-net-peer, ipc-outcomes,
  sum-clear and lend-untouched-page (smp=1 and smp=4), rc=0 each.
- Host tests: none of the touched crates has any. The test programs and the docs have no host
  tests, and testbench is untouched; its last run, on 8927bbae6, was rc=0 with 141 passed.

Then on 67746b417: docs rc=0.

The efficiency line (sched-share, rerun on 42a411fd7, since the bench keeps only the newest run
directories):

| | fdafcf2cb | + K24 | + K25 (now) |
| --- | --- | --- | --- |
| rv64: counted N of the calibrated 1000 | 912 | 921 | **938** |
| rv32: counted N of the calibrated 1000 | 899 | 909-910 | **929** |
| rv64 weight 300, of what was counted (of the window) | 603 (550) | 602 (555) | 599 (563) |
| rv32 weight 300, of what was counted (of the window) | 603 (543) | 603 (549) | 600 (558) |
| traced marks audits, rv64 / rv32 | 50 / 73 ms | 48 / 68 ms | 49 / 71 ms |

The now column of the efficiency line is from the untraced run; the traced run counted 940 and
931.

Other shares in the set:

| share | rv64 net (gross) | rv32 net (gross) |
| --- | --- | --- |
| exit-churn threads-exit | 492 (484) | 487 (475) |
| exit-churn processes-exit | 494 | 494 |
| exit-churn processes-fault | 494 | 495 |
| carve-return u-after-return | 493 | 495 |

Before K25, threads-exit was 439 on rv64 and 426 on rv32.
