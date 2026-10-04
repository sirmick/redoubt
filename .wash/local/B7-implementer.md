# B7: a bench run packs the kernel it built, and keeps its files to itself

Tier B (the bench), size S, no needs. Every cargo and bench command runs as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## The bug

Cases that build a kernel with a fault feature have, in some runs, booted a kernel built for
another case, then passed 3/3 when run alone. The cases seen:
- `handle-chain-fault` on both widths, and `endpoint-destroy-open-calls` on rv32, in GATE1's whole
  bench. The second hit the `handle.rs` panic the first injects.
- `process-chain-fault`, in the orchestrator's overloaded run: the guest exited before the
  expected `PANIC` line.

## What the code shows (confirm it first)

The bench runs its cases one at a time, so a race needs a second process sharing the worktree's
`target/`: another bench run, or a single-case run beside a whole bench. Then:
- `prepare` (`tools/testbench/src/main.rs`) runs `cargo build` for the kernel with the case's
  features, and packs `builder.artifact(..)`, which is `target/<triple>/<profile>/redoubt-kernel`
  (`build.rs` `out_dir`). That path is the same for every feature set. Cargo's lock serialises
  the builds, but another process's build can overwrite the file between this build and this
  pack.
- The bundle, the console logs, the disks, the captures and the corrupted ELFs all go to fixed
  paths under `target/testbench/` (`main.rs` `logs`, `build.rs` line 106). A second run
  overwrites them too.

Reproduce it before fixing it. In one worktree, a whole-bench run and a loop of
`cargo testbench handle-chain-fault` give the failure. Report how often.

## The fix: isolate, do not lock

A lock held for a whole run would serialise every agent's single-case checks behind a 30-minute
bench. Instead, make runs safe to overlap:
1. **The kernel by cargo's own report.** Build with `--message-format=json`, take the
   `compiler-artifact` message's `executable` for the package, and copy it into the run's
   directory before cargo's lock is released, that is in the same build call. Cargo hashes that
   path per feature set and profile (`deps/redoubt_kernel-<hash>`), and the copy is the run's own.
   Pack the copy. Do the same for the loader and the programs: any `cargo_build` whose output is
   packed.
2. **A directory per run.** Every file a run writes goes under `target/testbench/run-<pid>/`. A
   `target/testbench/last` symlink names the latest run for people, swapped atomically at the
   run's start. Runs older than the last few may be pruned at start.
3. Nothing else changes: the verdicts, the cases and the log content stay the same.

## The test

- **Host tests in `testbench`:**
  - Two builds of a small fixture package with different features, interleaved (build A, build
    B, then pack A), and A's packed bytes are A's.
  - Two `Builder`s with different run directories write no file in common.
- **A self-check if it can be made deterministic.** A case-level reproduction needs two
  processes, so a host test that runs two bench processes on one tiny case may be the honest
  version. If it cannot be made deterministic, say so: the host tests above are the keeper.
- The whole bench is green, plus the reproduction loop above run against the fix with no
  failure in, say, 20 overlaps.

## Page lines

testbench.md "How to use it". Replace "Each boot's console log, SSH transcripts, disk images and
captures are kept in `target/testbench/`." with:
> Each run keeps its boots' console logs, SSH transcripts, disk images and captures in a directory
> of its own, `target/testbench/run-<pid>/`, and `target/testbench/last` names the latest. Runs may
> overlap in one worktree: each packs the kernel cargo reports for its own features, never the
> shared `target/` path another run may have rebuilt.

GETTING-STARTED.md line 85 says `target/testbench/`. Change it to `target/testbench/last/`, with
the rest of the sentence as it is.

## Owned paths

`tools/testbench/src/{main.rs,build.rs}` and the paths they write, the testbench host tests,
testbench.md "How to use it", and GETTING-STARTED.md's one line.

Hotspots: none in the kernel. B6 (seed sweeps in parallel, `--jobs`), if the owner cuts it,
builds on this package's per-run directory and artifact copies.

## Gates

- The whole bench on both widths.
- The testbench host tests.
- `cargo fmt --check`.
- doccheck.
- The size budget: the bench's row, if it has one.

Report each command with its exit code, and the reproduction's numbers before and after.
