# B6 report

Branch wp-b6, from main 04b23071b. Tip e74ead5c2 `testbench: a seed sweep boots its seeds in
parallel when asked` (one commit: code, tests, page lines in lock step). It replaces f9f47e519;
the only changes are the page's port-race residual sentence and the commit body saying why the
example is sched-latency, not sched-ties (code identical: `git diff f9f47e519 e74ead5c2` touches
docs/testbench.md only). doccheck 0, no-cruft 0 after.

## Step 1: host (done)

### What was delivered (tools/testbench/src/main.rs, docs/testbench.md)

- `--sweep SEEDS` (clap value parser `parse_seeds`: `A..B` inclusive or comma list; empty,
  reversed, repeated refused at parse). `--jobs J` is `requires = "sweep"`, default 1.
- `sweep()` decides before anything builds: TESTBENCH_QEMU_SEED set is refused; J must be
  1..=available_parallelism; the filter must name exactly one case (a case whose whole name is the
  filter wins, so `sched` is not refused because `sched-ties` exists); the case must be a boot
  case with `qemu_seed`.
- `run_case` split into `build_case` (build and pack once; early results for build cases, a host
  that cannot boot, a failed build) and `boot_case` (the smp loop, seed given directly, files under
  the `logs` it is given). A run without `--sweep` goes through `run_case` exactly as before; the
  seed line and the sched_oracle summary go through an `out` sink (println in a normal run).
- `run_sweep`: build per arch, then (seed x arch) boots on a scoped-thread pool of J
  (`in_parallel`, results in finish order), each into `run-<pid>-<time>/seed-<N>-<arch>/`
  (create_dir: a collision is an error, never a share). Log names via `boot_log`; disk, peers,
  pcap, ssh transcripts and keys all derive from the log's path/dir, so all land in the seed dir.
  The bundle is built once per arch and only read (`-initrd`).
- `join`: sorts by (seed, arch order), prints notes then result lines exactly as `report` would,
  then per arch `sweep <case> <arch>: N seeds, P passed, F failed[: s1,s2]`; returns the failure
  count; `run_sweep` bails (non-zero exit) on any. A bench error in one boot becomes that seed's
  FAIL `bench error: ...` rather than aborting the other boots.

### Commands (all via in-dev, from the worktree)

- `cargo test -p testbench`: 0 (79 passed; new: sweep_seeds_are_ranges_or_lists,
  a_sweep_is_refused_before_anything_builds, sweep_boots_have_their_own_files,
  the_join_prints_in_seed_order_and_counts_failures)
- `cargo +nightly fmt -p testbench --check`: 0
- `cargo clippy -p testbench --all-targets`: no warning in main.rs (pre-existing elsewhere)
- `cargo run -q -p redoubt-doccheck`: 0; `cargo testbench docs`: 0; `cargo testbench no-cruft`: 0
- Real binary, nothing boots: `sched-ties --sweep 1..4` -> `sched-ties: --sweep needs a case that
  pins qemu_seed` (1); `--jobs 2` alone -> clap required-argument error; `TESTBENCH_QEMU_SEED=3 ...
  --sweep 1..4` refused; `--jobs 999` -> `from 1 to this host's parallelism, 24`; `--sweep 3,3` ->
  `seed 3 is given twice`.

### Page lines

testbench.md, after "to replay a run or to sweep.": the brief's paragraph verbatim. "How to use
it" block: `cargo testbench sched-latency --sweep 1..20 --jobs 4   # one case, a seed sweep, 4
boots at a time`. DEVIATION: the brief's line names `sched-ties`, which pins no `qemu_seed`, so the
bench refuses that very command; I used `sched-latency` (pins one). Say if you want another case.
The case file's status line also lists the four new host tests.

### Risks

- `qemu::free_ports` binds then releases ports; two boots choosing ports at the same instant
  could race (its comment already names "a second program asking at that instant"). Only cases
  with `[net]` forwards; none of the seeded cases has one today. Stated on testbench.md as a
  residual, in one sentence after the sweep paragraph.
- `unsafe`: none added. rv32: tools/testbench is host-only.

## Step 2: machine proof (done, host given by the orchestrator, alone)

Case: endpoint-destroy-full (the seeded case with the lowest timeout, 900 s; 1.1 s a boot here).
All via in-dev from the worktree, tip 23c3b12ed.

1. `cargo testbench endpoint-destroy-full --arch rv64 --sweep 1..4 --jobs 2`: exit 0, 12 s
   wall including the build. Seed lines 1, 2, 3, 4 in order, each followed by its sched_oracle
   line and `PASS  endpoint-destroy-full [rv64, smp=1]   1.1s`; then
   `sweep endpoint-destroy-full rv64: 4 seeds, 4 passed, 0 failed`. The run's directory has
   seed-1-rv64/ .. seed-4-rv64/, one console log each, and the bundle built once beside them.
   The seeds take effect: the four logs differ (md5 8f2f.., c640.., 878a.., 932a..), and the
   oracle lines take 3 distinct values across the 4 seeds.
2. `TESTBENCH_QEMU_SEED=3 cargo testbench endpoint-destroy-full --arch rv64`: exit 0, 2.3 s;
   `qemu seed 3`, `PASS ... 1.1s`; its sched_oracle line is identical to seed 3's in the sweep,
   and its console log is byte-identical to seed-3-rv64's (md5 878a00dae140 both).
3. Wall time, builds warm: `--sweep 1..4` (jobs 1, four serial boots) 5.2 s, exit 0;
   `--sweep 1..4 --jobs 2` 3.1 s, exit 0; both 4 passed, 0 failed.
   Outputs: .wash/local/B6-{sweep,replay,serial,sweep2}.out in the worktree.

## Review folds (tip 23c3b12ed, replaces e74ead5c2)

`git diff e74ead5c2 23c3b12ed`: main.rs +33/-14, case.rs +11.
- Simplifier (1): the summary says "1 seed" (singular), and its test checks that.
- Simplifier (2): the reversed-range line is gone from a_sweep_is_refused (the parser test covers it).
- Simplifier (3), option (b): `Case::only(cases, filter)` in case.rs beside `chosen`: the
  whole-name match, else chosen()'s set; its doc says why a run of one differs from a serial run.
  sweep() calls it; the serial path keeps chosen().
- Simplifier (4): sweep_boots_have_their_own_files kept.
- Red (1): an `--arch` the case lacks is refused before any build ("--arch x86 is not one of its
  targets (rv64, rv32)"); tested, plus --arch rv32 accepted.
- Red (2): MAX_SEEDS = 10,000, stated in --sweep's help; a range is checked before it is
  collected (0..u64::MAX refused at once); tested at 10,000 accepted, 10,001 and 0..MAX refused.
- Gates: cargo test -p testbench 0 (79); nightly fmt --check 0; clippy clean in the changed lines;
  doccheck 0; no-cruft 0.
