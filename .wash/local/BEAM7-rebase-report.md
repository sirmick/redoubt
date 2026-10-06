# BEAM7 rebase onto main 37ae709ed — report

Member beam7-implementer-5, assignment 8f741232ea20438b12ddc230a2d5a0a1, 2026-10-05.

## Branch

- Before: wp-BEAM7 6a79d0e13 on base 9212f6f60.
- `git rebase --signoff 37ae709ed`: no conflicts.
- After: head **8da0799d6ca158ae4b590bcb94706894ac9d2329**, base **37ae709ed**.
  - 461c91439 testbench: route host cases to bounded workspaces (was d5b2df3ec)
  - c4f0e3aa7 vm: stop verified lookup fallback on refusal (was 46d418875)
  - 8da0799d6 docs: distinguish absent and refused userland lookups (was 6a79d0e13)
- `git range-diff 9212f6f60..6a79d0e13 37ae709ed..HEAD`: all three `=`. Messages are unchanged
  too.
- `diff <(git diff 9212f6f60 6a79d0e13) <(git diff 37ae709ed HEAD)`: the only difference is in
  docs/testbench.md. That file's blob index line changed, and three hunk headers moved up one
  line (128→127, 207→206, 215→214) because the base edited text earlier in the file. **The
  package's own hunks are unchanged.**
- `git diff --stat 6a79d0e13 HEAD`: 23 files, +2689/−723. All of it is base content
  (BENCHENV1, TOOL1 and DOC1: tests/ssh-reference, tools/testbench/src/{ssh,ssh_guest,qemu}.rs,
  setup/install script, docs and so on).

## Gates (native, PATH=~/.cargo/bin first, RUSTSBI_PROTOTYPER{,_RV32} set as instructed)

| command | exit | result |
| --- | --- | --- |
| `./build --arch rv32 --programs` | 0 | built |
| `cargo testbench --arch rv32 build` | 0 | 12 PASS, 0 FAIL, 0 SKIP |
| `cargo testbench` (unfiltered, both widths, no --allow-skip) | 1 | 379 PASS, 21 FAIL, 0 SKIP |
| `cargo testbench docs` | 0 | PASS |

Whole bench: 19:30:33–19:56:44 UTC. Nothing else ran on the machine. The stdout is in
worktree target/beam7-rebase-whole.log, and the raw run is in
target/testbench/run-2122181-1791228633743380106. bench-ssh-loopback-openssh **PASS** (11.6s),
running in the new QEMU guest.

## Failures: none caused by the package; not rerun, as instructed

### A. Erlang/Elixir toolchain not on PATH (20 cases)

The bench sources userland/otp/tools/env.sh. That script resolves `$BEAMLET_TOOLCHAINS`,
defaulting to `<checkout>/toolchains`. A worktree has no `toolchains/` directory: it exists only
at /home/mcloonan/redoubt/toolchains (otp-28.5.0.6, elixir-1.20.4). On the host, `erl`, `erlc`,
`mix` and `elixir` are not on PATH. The old container provided them.

- `erlc: not found`: beamlet-boot, beamlet-budget-flood, beamlet-console, beamlet-heap-flood
  (each on rv64 and rv32).
- `no erl on the path userland/otp/tools/env.sh sets`: bench-elixir-oracles-broken-guard,
  elixir-oracles.
- `mix: not found`: image-disk, init-boot, userland-bad-start, userland-boot, userland-read-only
  (each on rv64 and rv32).

Likely fix for a rerun: export `BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains` (or
symlink the directory into the worktree). I did not do either. Changing the runner environment
is the orchestrator's decision.

### B. host-tests (1 case)

`ssh::tests::the_keeper_finds_what_names_the_case` failed with `left: [] right: [2148648]`, and
the other 88 passed. This test is in tools/testbench/src/ssh.rs, added by the base (BENCHENV1),
and the package does not touch ssh.rs. The test calls `processes_naming("sh", …)` immediately
after `spawn()`, so it can sample /proc before the child has exec'd `sh` and set its cmdline.
That is a timing race, made more likely by the load from 400 cases. It is not a BEAM7 effect.

## Summaries checked

This was integration only, with no new behaviour. The package's doc hunks are byte-identical,
and the docs checker passes on the new head. I re-checked no prose summaries: none of the
package's claims changed, and the base's doc edits (GETTING-STARTED, the testbench
reference-server text, userland/otp/README.md) don't touch the lookup/refusal claims BEAM7
makes.

## State

The tree is clean at 8da0799d6. There is no live qemu or testbench process. Nothing is pushed.

## Rerun (assignment f70463cd71fe91ac3dac6b80b6c08efa), head 8da0799d6

Environment: as above, plus `BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains`
(otp-28.5.0.6 and elixir-1.20.4 present). Cases ran one invocation at a time, with nothing else on
the machine. The per-invocation summary is in worktree target/beam7-rerun.log, and each run's
stdout is in target/beam7-rerun-<case>[-<arch>].log.

`cargo testbench --arch {rv64,rv32} <case>` passed with exit 0 for all 9 cases on both widths: beamlet-boot,
beamlet-budget-flood, beamlet-console, beamlet-heap-flood, image-disk, init-boot,
userland-bad-start, userland-boot and userland-read-only. That is 18/18 PASS.
`cargo testbench elixir-oracles` exited 0. Its filter also matched
bench-elixir-oracles-broken-guard, and both passed.
`cargo testbench bench-elixir-oracles-broken-guard` exited 0 and passed.
The raw run directories are target/testbench/run-22059xx … run-2225323-*; each one's path is
recorded in target/beam7-rerun.log.

`cargo testbench host-tests` was rerun once, alone, and exited 0. Its filter matched all 16
*host-tests cases, and all 16 passed, including host-tests itself (the B9 flake did not recur).
The raw run is target/testbench/run-2225719-1791231038094494086.

**Gate for 8da0799d6:** the whole run gave 379 PASS. The 20 toolchain cases and host-tests, rerun
alone, give 21 PASS. Together that is all 400 cases passing, with 0 SKIP.
Two environment facts are recorded:
(1) A worktree has no toolchains/ directory, so the bench needs
BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains.
(2) host-tests' failure in the whole run was the known flake B9
(ssh::tests::the_keeper_finds_what_names_the_case racing /proc under load). It passed when run
alone.
