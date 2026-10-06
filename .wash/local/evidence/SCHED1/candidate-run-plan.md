# SCHED1 v3: the one focused candidate run (prepared, not run)

The proposal's first machine release is a single coordinated candidate run: rv64, seed 3,
`sched-cluster`, on the branch head. This file is the plan; the run needs its own explicit grant.

## Source identity

Prepared at HEAD `234fcd72d7a2a704fc833cf6061b86e0a681f348` on `wp-SCHED1`, with a clean
`git status --porcelain` (0 lines). SHA256 of the files the run depends on:

| File | SHA256 |
| --- | --- |
| tests/programs/src/sched.rs | 7e23436209f0866cdb0a8a18be17af569043a73fb06e5364b665b3fd6e32e8db |
| tests/programs/src/bin/sched-cluster.rs | 2a49b2dc0975599762aa93147dbd23962e60cc683e01fa95a02ce05d7f90f31a |
| tests/sched-cluster.toml | 1024e5aa225396971f47c33d6c049262a1088619797c8ae25cf9beb8e4ab004e |
| tools/testbench/src/sched_oracle.rs | bcedf6a403780709defcbabbd53a8429052be00e5e477792f7ce55faba4e0133 |
| kernel/src/sched.rs | 818ab27434a28ae94500d938dc8c31552081d5c1ca2a3a279c3a4be8a8d799e4 |

The head can move before the grant. Commit 234fcd72d adds an uncompiled host test: its compile
and fmt check and its one `cargo test` come first at the next grant, and any fix gets a commit
of its own. The run is made only on a clean tree. Its HEAD and the hashes above are recorded
again immediately before the run and after it. If they differ from this table, the report says
so and gives the new values.

## Pre-run checks (no QEMU)

```sh
cd /home/mcloonan/redoubt/.worktrees/SCHED1
git status --porcelain            # must print nothing
git rev-parse HEAD                # recorded
sha256sum tests/programs/src/sched.rs tests/programs/src/bin/sched-cluster.rs \
  tests/sched-cluster.toml tools/testbench/src/sched_oracle.rs kernel/src/sched.rs
pgrep -fa 'qemu-system|cargo testbench'   # nothing else may run (serialised machine)
```

## Command (native, no container)

```sh
cd /home/mcloonan/redoubt/.worktrees/SCHED1
export PATH=$HOME/.cargo/bin:$PATH
export RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper
export RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper
export BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains
unset TESTBENCH_QEMU_SEED         # the case pins qemu_seed = 3 itself; nothing may replace it
cargo testbench --arch rv64 sched-cluster; echo "exit=$?"
```

- Exactly one boot: `sched-cluster`, rv64, smp 1, seed 3 from the case file. The filter also
  appears inside `sched-cluster-old-control`, but that case is `whole_run = false`, and a partial
  filter leaves it out (docs/testbench.md, "The case file"). The bench's selection line must show
  one case; if it shows two, stop before the boot finishes and report.
- The case has `debug_assertions = true` and `kernel_features = ["sched-trace"]`, with
  `timeout_secs = 600`. QEMU is 10.2.1 (`/usr/bin/qemu-system-riscv64`).
- No `--sweep`, no `--jobs`, no rv32, no old control, no second invocation.

## Expected artefacts

All of them are in `target/testbench/run-<pid>-<time>/` (also `target/testbench/last`):
- `sched-cluster-rv64-smp1.log`: the console. It holds the calibration line, 19
  `CLUSTER-READY`, `CLUSTER-PLAN v3-kernel-envelope ...`, `CLUSTER-WINDOW H R F`, `CLUSTER-RAW`
  for both stand-ins, `CLUSTER-WORK server go..F n spinners R..F n`, the `LATENCY-COUNT` and
  `CLUSTER-HEADER` lines and 200 `CLUSTER-SAMPLE` + `LATENCY-SAMPLE` per stand-in, then
  `SCHED-CLUSTER TEST PASSED` and `SCHED-TRACE-END <n> dropped 0`.
- The run's bundle (`sched-cluster-rv64.tar`) and the cargo copy directories
  (`cargo`, `cargo-qemu-virt+sched-trace`).
- The bench's verdict line and the oracle's report (PASS) or its first error (FAIL), on stdout.

## Stop and preserve

- Stop at the first failure of any kind: a guest `FAIL` or halt, a missing expect, a build error,
  a timeout, or the oracle's first construction, join, fence, spinner, rank, category,
  containment or target error. No retry, no other seed, no rv32, no source edit, no threshold
  change.
- Before any later bench invocation, PASS or FAIL:
  - copy the whole run directory byte for byte to
    `.wash/local/evidence/SCHED1/run-v3-<run-dir-name>/`;
  - check it with `diff -qr` (exit 0);
  - write `run-v3-<run-dir-name>-sha256.txt` over every file;
  - record the console's SHA256, line count and size.
- Report, from targeted greps only (never the whole log): the command, the exit code, the bench
  verdict line, the oracle's summary or first error, the plan/window lines, `SCHED-TRACE-END`,
  and the source identity before and after. A PASS here is one candidate datum. It is not
  acceptance: the rv32 run and the two old controls (`sched-cluster-old-control`, rv64 and rv32)
  follow only on their own coordinated releases.
- Release the machine and say so (MACHINE RELEASED) as soon as the run and the copy are done.

---

# Next runs: rv32 candidate, then the two old controls (prepared 2026-10-05, not run)

Each run waits for the orchestrator's "go" for that run, and they go in this order. All of them
use the QEMU slot through the machine wrapper; host work uses `machine host`. The oracle is the
amended one, so each run's own post-check is its verdict and needs no separate re-judging.

## Source identity

HEAD `14fc61bd3fcab5f8384a0190508a01c50aa83aa8` with a clean tree. SHA256:

| File | SHA256 |
| --- | --- |
| tests/programs/src/sched.rs | 7e23436209f0866cdb0a8a18be17af569043a73fb06e5364b665b3fd6e32e8db |
| tests/programs/src/bin/sched-cluster.rs | 2a49b2dc0975599762aa93147dbd23962e60cc683e01fa95a02ce05d7f90f31a |
| tests/sched-cluster.toml | 880c2ef0331d22e8afa435fcffb57e48aec17db95491a9c345d0eb06eef0b91a |
| tests/sched-cluster-old-control.toml | 5656b5c19a152f610a4a505a592ffb22fa2dea27a3b302e9b5c05a7014d82e61 |
| tools/testbench/src/sched_oracle.rs | 7f54585ae0805204628b94a747f122084d1c0874684d5e03ad4801d1e9b01f3c |
| kernel/src/sched.rs | 818ab27434a28ae94500d938dc8c31552081d5c1ca2a3a279c3a4be8a8d799e4 |
| kernel/Cargo.toml | 5dceb09f7742da5df4f1b0b82b1ae497f80dcf95f4b6542f8a65fbeb14d803f6 |

Since the rv64 run, the guest sources and `kernel/src/sched.rs` are unchanged. The oracle, the
toml description and the docs did change. HEAD and the hashes are recorded again before and after
each run, with the same pre-run checks as above plus `machine who`.

## Environment

As for rv64, with these differences:
- `M=/home/mcloonan/redoubt/.wash/local/machine`.
- `TESTBENCH_QEMU_SEED` is unset; each case pins `qemu_seed = 3`.
- `cd /home/mcloonan/redoubt/.worktrees/SCHED1`.

## Run A: rv32 candidate (on "go")

```sh
$M qemu cargo testbench --arch rv32 sched-cluster; echo "exit=$?"
```

- Exactly one boot: `sched-cluster [rv32, smp=1]`.
- Expected console: `sched-cluster-rv32-smp1.log`, with the same lines as rv64 and
  `SCHED-TRACE-END <n> dropped 0`.
- Verdict: the post-check `sched_oracle cluster ...` with the case's four targets.
- On PASS, report from greps of the bench's stdout and the console: the oracle's lines (coverage
  per category, envelope net and gross p50/p99/max per role, certified credit, the driver lower
  witness, `ahead` min/median/max) and the trace line.

## Run B: rv64 old control (on "go", after A is reported)

```sh
$M qemu cargo testbench --arch rv64 sched-cluster-old-control; echo "exit=$?"
```

- The filter is the case's whole name, so this by-name case runs (`whole_run = false`), and the
  filter does not match `sched-cluster` itself.
- Kernel features: `sched-trace` and `slice-10ms`, in a checked build. The post-check is
  `sched_oracle cluster cluster_old_control` with the same four targets.
- PASS means all of these hold:
  - every construction and coverage gate;
  - non-vacuity: at least 25 positive-lead wakes per stand-in with `ahead >= 8`;
  - at least one unchanged envelope target missed;
  - a driver lower-witness percentile over its target.
- The result line also reports the `ahead` distributions. A FAIL on the vacuity, envelope-only
  or lower-witness rule is the control's result, not a fixture failure. It is reported as is,
  and nothing more runs.

## Run C: rv32 old control (on "go", after B is reported)

```sh
$M qemu cargo testbench --arch rv32 sched-cluster-old-control; echo "exit=$?"
```

Same as B, on rv32.

## For every run

- Stop at the first failure of any kind: no retry, no other seed, no edit, no next run without
  its "go".
- Before anything else uses the machine: byte-copy `target/testbench/run-<pid>-<time>/` to
  `.wash/local/evidence/SCHED1/run-v3-<dir>/` (`diff -qr` exit 0), write
  `run-v3-<dir>-sha256.txt`, save the bench's stdout beside it, and record the console's SHA256,
  line count and size.
- Report from targeted greps only, then say MACHINE RELEASED. The slot frees itself when the
  command exits; confirm with `$M who`.
