# B6: a seed sweep runs its boots in parallel, when asked

Tier B (`tools/testbench` only). Size S. Needs nothing: B7's per-run directories are in; start
from main. Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev
<command>`, from the worktree. Its own proof boots QEMU: **run your machine checks only when the
orchestrator says the host is free**, and never beside a whole bench.

The owner (2026-10-03) cut it: parallel seed sweeps in the bench.

## Context rules (read these first)

- **Don't read whole files.** In `tools/testbench/src/main.rs`: `Args`, the case loop (the
  `filter`/`chosen` lines and the per-arch boot), `qemu_seed`, and where a boot's log and
  run-directory paths are made. In `case.rs`: `qemu_seed`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** No boot-log hex in reports.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/B6-report.md`.

## Reading list

`docs/testbench.md`: "How to use it" (the run directory), the `icount`/`qemu_seed` paragraph
(about line 176), and "whole_run" after it.

## What exists

- A case may pin `qemu_seed`. `TESTBENCH_QEMU_SEED=M` replaces every pinned seed for the whole
  process, to replay or to sweep.
- Sweeps today are shell loops outside the bench: one `cargo testbench <case>` per seed, one at a
  time, logs gathered by hand.
- Each bench process keeps its files in `target/testbench/run-<pid>-<time>/` (B7).

## The settled design

1. **The flags.**
   - `--sweep SEEDS` runs the one case the filter names, once per seed, on each `--arch` it has.
     SEEDS is `A..B` (inclusive) or a comma list.
     - Refused, before anything builds: an empty, reversed or repeated seed; a filter that names
       no case or more than one; a case without `qemu_seed`.
   - `--jobs J` runs up to J of those boots at once. It defaults to 1, is accepted only with
     `--sweep`, and is refused above the host's `available_parallelism`.
   - Nothing parallel is ever the default: a run without `--sweep` is exactly today's.
2. **One build, many boots.** The kernel and the case's programs are built and packed once. Each
   seed's boot gets its seed directly (QEMU `-seed`), not through the environment, which is one
   per process. `TESTBENCH_QEMU_SEED` keeps its replay meaning and is refused together with
   `--sweep`.
3. **Names.** Each boot writes only under `run-<pid>-<time>/seed-<N>-<arch>/`: its console log,
   transcripts, captures, and its own copy of any disk image the case writes. No two boots of a
   sweep share a file, which is B7's rule within one run.
4. **The join.** When every boot has ended:
   - each seed's result is printed exactly as a single run prints it (the seed line included),
     in seed order, then by arch, whatever order they finished in;
   - then one line: `sweep <case> <arch>: N seeds, P passed, F failed: <failed seeds>`;
   - the exit status is non-zero if any seed failed.
5. **What a parallel result means.** Under `icount` a case's guest-time numbers do not depend on
   the host's load, but the bench's own wall-clock timeouts do, and boots share the host. So a
   result under `--jobs` above 1 is a sweep datum, never a merge-gate verdict. A seed that fails
   only by a timeout under `--jobs` is rerun alone before anyone cites it. The merge gate stays one
   serial whole run on this host.

## The cases

1. **Host tests** (`cargo test -p testbench`):
   - SEEDS parsing: ranges, lists, and each refusal;
   - the per-seed directory and log names are unique across seeds and arches;
   - the join prints in seed order, given results that finish out of order;
   - the summary line and the exit status, with one failed seed;
   - `--jobs` without `--sweep` refused, and so is `TESTBENCH_QEMU_SEED` with `--sweep`.
2. **Machine proof** (when the host is free): the shortest case with `qemu_seed` (name it), run
   with `--sweep 1..4 --jobs 2` on rv64. Four results in seed order, the summary line, four
   `seed-<N>-rv64/` directories. Then seed 3 alone with `TESTBENCH_QEMU_SEED=3`: the same verdict,
   and under `icount` the same reported numbers. Report both outputs' result lines and the wall
   time against four serial boots.

## Page lines (exact text in the report)

**testbench.md**, after the `qemu_seed` paragraph's "to replay a run or to sweep.":

> `--sweep SEEDS` runs one case once per seed (`1..20`, or `3,5,9`), from one build, and prints
> each seed's result in seed order and a summary line; each boot keeps its files in
> `seed-<N>-<arch>/` inside the run's directory. `--jobs J` boots up to J seeds at once. It is
> never the default, and a result under `--jobs` above 1 is a sweep datum, not a verdict: guest
> times under `icount` do not move with the host's load, but the bench's timeouts do. A merge
> runs the whole bench serially, and a seed that failed only by timing out under `--jobs` is rerun
> alone.

Add the two flags to "How to use it"'s block:
`cargo testbench sched-ties --sweep 1..20 --jobs 4   # one case, a seed sweep, 4 boots at a time`.

## Owned paths

`tools/testbench/src/**` (the flags, the sweep, the join, the tests), `docs/testbench.md` (the
lines above).

**Not yours:** the cases, the kernel. Delete no sweep script in `.wash/local`.

## Gates

- `cargo test -p testbench`, `cargo fmt --check`, doccheck, no-cruft.
- The machine proof above, when the host is free.
- Not the whole bench: the orchestrator runs it serially at merge.

Report each command with its exit code, the proof's lines and times, and the page lines.
