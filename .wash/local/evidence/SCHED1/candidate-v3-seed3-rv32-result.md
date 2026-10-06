# SCHED1 v3 run A: rv32 candidate, seed 3. Result: PASS

The orchestrator's "go" was message `199e3be4`. The run went through the job scheduler, under the
shared-host rule: a guest case's own result is a verdict. Run once.

## Source identity (before and after: identical)

HEAD `14fc61bd3fcab5f8384a0190508a01c50aa83aa8`, with a clean tree. The file hashes match the plan
(`candidate-run-plan.md`, "Next runs").

## Command

`eval "$(/home/mcloonan/redoubt/.wash/local/jobserver env)"`, then
`make -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C /home/mcloonan/redoubt/.worktrees/SCHED1 rv32/sched-cluster`,
with `TESTBENCH_QEMU_SEED` unset. It ran from 22:10:03 to 22:10:32 UTC and exited 0:

```
PASS  sched-cluster [rv32, smp=1]       14.8s
rv32/sched-cluster rc=0 (shared)
```

The pass was not by `timeout_secs`, so it is not void under the shared-host rule.

## Preserved

- Run directory: `target/testbench/run-2538889-1791238209677817427`, copied byte for byte to
  `run-v3-run-2538889-1791238209677817427/` here (`diff -qr` exit 0).
- Manifest: `run-v3-run-2538889-1791238209677817427-sha256.txt` (27 files).
- Bench stdout: `...-bench-stdout.txt`.
- Console `sched-cluster-rv32-smp1.log`: SHA256
  `9d19b1a271d9f96ee35255e2ce0baa5323ceb5295f9eef3a2645619517696338`, 148,566 lines, 5,502,495
  bytes.

Console lines:
- `[cluster] calibrated: 10 ticks/us, 6566 iterations/ms`
- `CLUSTER-PLAN v3-kernel-envelope ...`
- `CLUSTER-WINDOW 903146 953146 16953146`
- `CLUSTER-WORK server go..16953146 2864640 spinners 953146..16953146 3722752`
- `SCHED-CLUSTER TEST PASSED`
- `SCHED-TRACE-END 147629 dropped 0`

## The oracle's lines

On a pass the bench prints only PASS. To get the report text, the same oracle source (HEAD
`14fc61bd3`) was run on the preserved console through the replay harness, with
`step1-replay-diagnostic.patch` applied for the run, then reverted; the tree was clean afterwards.
It gave `ORACLE PASS`, the same verdict as the bench's post-check. The full text is in
`run-v3-run-2538889-1791238209677817427-oracle-report.txt`.

- Trace: 147629 records and 13405 picks in rank order; every wake at or above the floor; 2 lifts
  and 27 weight changes by the rule; R10 2110 µs (p99); 19130 audits, 3093375 µs; 10633 timer
  interrupts billed by the rule, 0 nobody's.
- `cluster driver_wake (200): envelope net p50/p99/max 4200/17727/22780 µs, envelope gross
  5506/21359/27016, certified audit credit 384454 µs: target met (p50 <= 15000, p99 <= 50000)`
- `cluster timer_wake (200): envelope net p50/p99/max 5311/16364/28536 µs, envelope gross
  6941/20911/43030, certified audit credit 375561 µs: target met (p50 <= 15000, p99 <= 50000)`
- `cluster: 16 queued spinner W before K 5253, ... spread 0x0 <= 0xa00000`
- Driver: 100 zero and 97 positive; net p50/p99 4200/17727; lower witness p50/p99 4028/17195;
  zero 3484/4292; positive 6758/22780; `ahead` zero 0/0/0, positive 0/1/10.
- Timer: 100 zero and 99 positive; net p50/p99 5311/16364; zero 2292/6429; positive
  8378/28536; `ahead` zero 0/0/0, positive 0/1/10.

Coverage of both categories held on both stand-ins, at 25 or more per category and 5 or more per
offset.

## Standing

Both candidate widths now pass: rv64 by re-judging the saved log, rv32 by this run. The old
controls, B (rv64) and C (rv32), each wait for their own "go".
