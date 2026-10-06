# SCHED1 steps 1 and 3: replay of the saved rv64 seed-3 console. Result: amended oracle PASS

This followed the Architect's coverage ruling (`.wash/local/SCHED1-coverage-ruling.md`) and the
host grant `e9bf75bf`. Every cargo command was pinned off the bench with
`taskset -c 8-23 nice -n 10`. No QEMU was run.

## Source

HEAD `d4e481887` (the step-2 amendment) on `wp-SCHED1`, with a clean tree before and after.
The step-1 patch `.wash/local/evidence/SCHED1/step1-replay-diagnostic.patch` was applied for
the replay only, then reverted with `git apply -R` and the test file removed;
`git status --porcelain` showed 0 lines afterwards.

The input was the saved console
`.wash/local/evidence/SCHED1/run-v3-run-2344574-1791234070302434531/sched-cluster-rv64-smp1.log`,
SHA256 `c9a25c07050135f894b6f37fa862b50f74775ebe216886ee0850076333ed31bf`, byte-identical to the
run. It came from the candidate built from HEAD `18818cb14`; the kernel, fixture and guest are
unchanged since then, and only the host oracle changed.

## Commands and exit codes

1. `taskset -c 8-23 nice -n 10 cargo +nightly fmt --all -- --check`: exit 0.
2. `taskset -c 8-23 nice -n 10 cargo test -p testbench --test sched_oracle_only`: exit 0, 29
   passed.
3. `git apply step1-replay-diagnostic.patch`, then
   `SCHED1_REPLAY_DIAG=1 SCHED1_REPLAY_LOG=<console> taskset -c 8-23 nice -n 10 cargo test -p testbench --test sched_oracle_replay -- --ignored --nocapture`:
   exit 0, `ORACLE PASS`. The full output, including 200 SCHED1-DIAG lines, is in
   `step1-replay-output.txt` in this directory.
4. `git apply -R` and the test file removed: the tree is clean.

## Step 1 distribution: positive-intent samples (25 per offset per stand-in)

| Stand-in | Offset 100 | 300 | 600 | 850 | Total with lead > 0 |
| --- | --- | --- | --- | --- | --- |
| driver_wake | 23 | 25 | 25 | 25 | 98 of 100 |
| timer_wake | 25 | 25 | 25 | 25 | 100 of 100 |

Over all positive-intent samples:
- driver: `ahead` 0/1/5 (min/median/max); lead/2^20 0/9/15.
- timer: `ahead` 0/1/5; lead/2^20 2/9/17.

Two driver samples at offset 100 had lead 0. They are positive-intent wakes that did not qualify,
and they still count in the 200-sample percentiles. The maximum `ahead` of 5 confirms the ruling's
arithmetic: at 1 ms, 8 or more spinners ahead never happens.

The 25-total and 5-per-offset minima hold for lead > 0 on both stand-ins, so step 3 applies.

## Step 3: the candidate's rv64 verdict under the amended oracle

The verdict is PASS. The oracle's lines, with per-sample detail lines omitted:

- `sched_oracle: 156054 records, 14217 picks in rank order, every wake at or above the floor, no
  pass falling but by a weight change; 2 lifts by the rule; 27 weight changes by the rule; R10 2
  destructions over up to 1218 object frames, µs p50/p99/max 1911/1912/1912, no audit inside
  one; 19753 audits, 2543625 µs; 11436 timer interrupts billed by the rule (..., 0 nobody's)`
- `cluster driver_wake (200): envelope net p50/p99/max 3962/11610/32920 µs, envelope gross
  5044/13939/36481, certified audit credit 287985 µs: target met (p50 <= 15000, p99 <= 50000)`
- `cluster timer_wake (200): envelope net p50/p99/max 5887/15629/31459 µs, envelope gross
  7141/28384/35230, certified audit credit 320178 µs: target met (p50 <= 15000, p99 <= 50000)`
- `cluster: 16 queued spinner W before K 5222, W entries 5792..5792, sequences 5187..5217,
  spread 0x0 <= 0xa00000`
- `driver_wake: 200 fenced D-W-service/sample envelopes, 1204 later report wakes, marker 31 X
  102215 Y 102232 duration 1912 µs, peer R10 overlap 0 µs retained; 100 zero and 98 positive, 293
  earlier picks before service; all envelope net p50/p99 3962/11610 µs; RTC gross less outer audit
  bins (lower witness) p50/p99 3797/10902 µs; zero 3161/3983; positive 6178/13838; spinners
  ranked before the wake, min/median/max: zero 0/0/0, positive 0/1/5`
- `timer_wake: 200 fenced ..., 1205 later report wakes, marker 32 X 102265 Y 102282 duration
  1911 µs, peer R10 overlap 1912 µs retained; 100 zero and 100 positive, 412 earlier picks before
  service; all envelope net p50/p99 5887/15629 µs; zero 4237/24919; positive 7235/14232; spinners
  ranked before the wake, min/median/max: zero 0/0/0, positive 0/1/5`

Coverage passed for both categories, at 25 or more per category and 5 or more per offset. The
zero-lead per-offset counts are not printed, but the gate checks them. Both envelope targets were
met on all 200 samples. Certified credit and the driver lower witness are reported as above.

This is one rv64 candidate datum, re-judged rather than retried. It is not acceptance:
- rv32 is still to run;
- both old controls (`sched-cluster-old-control`, rv64 and rv32, with `slice-10ms`) are still to
  run, and they must also be non-vacuous: at least 25 positive-lead wakes per stand-in with
  `ahead >= 8`;
- the full gates follow, under their own grants.

HOST RELEASED.
