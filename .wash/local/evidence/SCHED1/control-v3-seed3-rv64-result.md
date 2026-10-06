# SCHED1 v3 run B: rv64 old control (slice-10ms), seed 3. Result: FAIL in construction

The orchestrator's "go" was message `17bc3cd1`. Run once. Run C (rv32) was not run, because it
follows only if B qualifies.

## Source identity (before and after: identical)

HEAD `14fc61bd3fcab5f8384a0190508a01c50aa83aa8`, with a clean tree.

## Command

`make -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C /home/mcloonan/redoubt/.worktrees/SCHED1 rv64/sched-cluster-old-control`,
through the jobserver with `TESTBENCH_QEMU_SEED` unset. It ran from 22:11:50 to 22:11:56 UTC;
make exited 2 and the bench rc was 1.

```
FAIL  sched-cluster-old-control [rv64, smp=1]   5.0s  forbidden output /\[cluster\] FAIL/: [cluster] FAIL: stand-in 17 took all 200 real waits
```

The run did not end by `timeout_secs`, so under the shared-host rule this is a verdict.

## Preserved

- Run directory: `target/testbench/run-2543441-1791238310923470939`, copied to
  `run-v3-run-2543441-1791238310923470939/` (`diff -qr` exit 0).
- Manifest: `run-v3-run-2543441-1791238310923470939-sha256.txt` (27 files).
- Bench stdout: `...-bench-stdout.txt`.
- Console `sched-cluster-old-control-rv64-smp1.log`: SHA256
  `a252716b1db5da56559e13e2172c1d53086daefd4e7c4d08108c49166da010d6`, 124 lines, 6,953 bytes.

## The console (all of its relevant lines)

```
[cluster] calibrated: 10 ticks/us, 24389 iterations/ms
CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000
CLUSTER-WINDOW 825495 875495 16875495
CLUSTER-RAW role=17 child=17 seen=1 words=80300 184488 2147549696 257 tag=1 count=1
CLUSTER-FAIL role=17 index=1 branch=2 result=0 target_us=80300 current_us=184488
[cluster] FAIL: stand-in 17 took all 200 real waits
```

Role 17 is the driver stand-in. It took attempt 0 and then failed attempt 1 on branch 2, the
zero-intent arming check. Attempt 1 is zero-intent at offset 300 µs, so its target is R + 80,300
µs. B, the kernel reading before arming, was R + 184,488 µs: 104 ms after its target.

Under the proposal, a zero-intent target already passed is a construction failure: "fails if
delta is zero or the target has passed", with no retry and no re-phase. The guest stops measuring
that stand-in at its first failed attempt. The console therefore holds no samples, no
`SCHED-CLUSTER TEST PASSED` and no trace dump, so the host oracle never ran: the bench failed on
the forbidden `[cluster] FAIL` line first.

## Which condition failed

**Construction.** It is not vacuity, not "met every target" and not "lower witness under target":
none of those was reached. The control never produced a joinable 200-attempt sample set, so it
is not the required demonstrated negative control.

The likely mechanism, which this console alone cannot prove: with a 10 ms slice, the driver,
after its first wake, waits behind the busy server and about 16 spinners for whole slices. It
comes back about 184 ms after release, past its next 80 ms slot. The fixed-slot construction
cannot follow a waker that late, and the proposal does not allow skipping an attempt.

## Standing

The candidate passes on both widths. The rv64 control does not qualify under the v3
construction. Under the proposal's stop rule ("a nonqualifying or unproved old-control failure
also stops"), this goes back to the QA thread. Options, for the Architect and orchestrator, not
taken here:

- a construction rule for late zero-intent attempts in the control;
- wider slots;
- a different known-bad control.

No retry, no edit, no rv32 control was run.

MACHINE RELEASED.
