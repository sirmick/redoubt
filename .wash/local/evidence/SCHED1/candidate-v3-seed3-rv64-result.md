# SCHED1 v3: the one candidate run, seed 3, rv64. Result: FAIL at the oracle's coverage gate

Granted by orchestrator message `76198717cedaf37cfb94ba06265d5cc1`. I ran it once and stopped at
the first failure. No retry, no edit, no rv32, no control run.

## Before the run

- `cargo +nightly fmt --all -- --check` exited 1. The only diff was the layout of the new
  preparation-slip test (`let before = if ...`). I applied `cargo +nightly fmt --all`, which
  changed that test only, and committed it as `18818cb14` "testbench: format the preparation-slip
  test" (`tools/testbench/src/sched_oracle.rs`). A second check exited 0.
- `cargo test -p testbench cluster_metadata_rejects` exited 0. In both test binaries all three
  passed: `cluster_metadata_rejects_a_preparation_slip_it_cannot_sign`,
  `cluster_metadata_rejects_arming_and_containment_failures` (the relabelled neighbour) and
  `cluster_metadata_rejects_bad_records_and_bounds`.

## Source identity (before and after: identical)

The run was on HEAD `18818cb1448aa5c7d8278b57ea22920980a13a80`, one formatting commit on top of
the planned 234fcd72d. `git status --porcelain` was empty.

| File | SHA256 |
| --- | --- |
| tests/programs/src/sched.rs | 7e23436209f0866cdb0a8a18be17af569043a73fb06e5364b665b3fd6e32e8db |
| tests/programs/src/bin/sched-cluster.rs | 2a49b2dc0975599762aa93147dbd23962e60cc683e01fa95a02ce05d7f90f31a |
| tests/sched-cluster.toml | 1024e5aa225396971f47c33d6c049262a1088619797c8ae25cf9beb8e4ab004e |
| tools/testbench/src/sched_oracle.rs | 2f2ea7bd895b36627a028064ad6b6250108ef8d77a470544858b49bc911d8f08 (only the fmt change since the plan) |
| kernel/src/sched.rs | 818ab27434a28ae94500d938dc8c31552081d5c1ca2a3a279c3a4be8a8d799e4 |

## Command

The environment was native and as in the plan (`TESTBENCH_QEMU_SEED` unset):
`cargo testbench --arch rv64 sched-cluster`. It ran from 21:01:09 to 21:01:32 UTC and exited 1.
One case ran.

```
      qemu seed 3 (TESTBENCH_QEMU_SEED=3 replays it)
FAIL  sched-cluster [rv64, smp=1]       16.8s  sched_oracle: cluster driver_wake positive-lead coverage [0, 0, 0, 0], total 0, needs >=25 and >=5 per offset
```

## Preserved

- Run directory: `target/testbench/run-2344574-1791234070302434531`. It was copied byte for byte
  to `.wash/local/evidence/SCHED1/run-v3-run-2344574-1791234070302434531/` (`diff -qr` exit 0).
- Manifest: `run-v3-run-2344574-1791234070302434531-sha256.txt` (27 files).
- Bench stdout: `run-v3-run-2344574-1791234070302434531-bench-stdout.txt`.
- Console `sched-cluster-rv64-smp1.log`: SHA256
  `c9a25c07050135f894b6f37fa862b50f74775ebe216886ee0850076333ed31bf`, 156,991 lines,
  5,826,001 bytes.

## What the console and the oracle's order establish (greps only)

- `[cluster] calibrated: 10 ticks/us, 19773 iterations/ms`
- `CLUSTER-PLAN v3-kernel-envelope 100 300 600 850 200 80000 50000`
- `CLUSTER-WINDOW 870588 920588 16920588`: H, R and F, all from one reading.
- `CLUSTER-RAW` for both stand-ins: 200 waits each, and no `CLUSTER-FAIL`.
- `CLUSTER-WORK server go..16920588 4651264 spinners 920588..16920588 5747712`
- Each stand-in's `CLUSTER-HEADER` echoes 870588 16920588.
- `SCHED-CLUSTER TEST PASSED`, and `SCHED-TRACE-END 156054 dropped 0`.

The oracle reached the coverage gate, so every check it runs before that gate passed. That covers:
- trace parse and the rank, floor, lift, reweigh and timer checks;
- the v3 plan and window (R = H + 50000, F = R + 16000000);
- metadata for all 200 records of both stand-ins, including R <= L <= P, U <= F, d = a + 1000δ,
  B <= E <= P, s >= d and the header echo;
- the serial readiness/go protocol for all 19 children;
- the 16 spinner releases;
- both X/Y marker fences;
- exactly 200 D-W-service joins per stand-in;
- the common release with 16 queued spinner W and a spread within 100 µs of equivalent pass;
- the busy server running across release;
- all 200 driver envelopes against their LATENCY-SAMPLE, and their certified credit.

Driver zero-lead coverage cannot be read from the error alone. The oracle checks zero then
positive and stops at the first failure, so zero-lead coverage passed. The driver's positive-lead
category is empty: no positive-intent driver wake was queued with a pass lead above the floor and
at least 8 spinners ranked ahead.

The timer's coverage was not reached. No net, credit or lower-witness percentile was computed,
so there is no envelope target verdict.

Gross figures from the console, for orientation only. They are not the oracle's net values and
not a verdict:

| Measure | p50 | p99 | max |
| --- | --- | --- | --- |
| driver envelope U − L (µs) | 5044 | 13939 | 36481 |
| timer envelope U − L (µs) | 7141 | 28384 | 35230 |
| driver RTC lateness floor((s − d)/1000) (µs) | 4909 | 13253 | 36441 |

## Open (not decided here)

Whether a 1 ms slice leaves the positive-lead category qualifiable under this fixture is a
design question for the orchestrator and Architect. Options include the per-W witnesses (lead,
ahead) or a fixture change. I made no tuning, threshold, phase or fixture change. The oracle
could print each stand-in's witness distribution (lead > 0, ahead counts) as a diagnostic before
failing coverage, but that is a source change and needs its own assignment.

MACHINE RELEASED: no cargo, QEMU or bench process of mine is running.
