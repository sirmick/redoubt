# BEAM7 review evidence — 2026-10-04

Branch `wp-BEAM7` is clean. Base `9212f6f600591abb3ea78ef450e0d6f4d517ab92`
(`main`); final head `6a79d0e1356493f5307b2af19934b4f3d8515c79`.
Review range: `main..HEAD` (`d5b2df3ec` host routing,
`46d418875` verified lookup refusal, `6a79d0e13` docs). All three commits
carry sign-off. `git diff --check main..HEAD` exits 0. The worktree is clean.

The focused tests were executed on pre-rebase head `65b9f9a8f`. The current
head has no changes from that tree under `docs`, `tests`, `tools`, or `userland`
(`git diff --name-only 65b9f9a8f..HEAD -- docs tests tools userland` is empty).
Only main's `.wash/PROJECT.md` and `.wash/workspace.toml` changed between those
trees. Thus the package content and its focused-test inputs are identical.

| Gate / command | Exit | Evidence |
| --- | ---: | --- |
| `cargo testbench -v beamlet-lookup` | 0 | `target/beam7-lookup-cleanup.log`; CLI 3 tests, VM/Redoubt 74 tests, both cases PASS |
| Deliberate Refused→Absent mutation, `cargo testbench -v beamlet-lookup-host` | 1 as expected | `target/beam7-negative-control.log`; system-side zero-path assertion fails, 6 file operations rather than 0 |
| Restored `cargo testbench -v beamlet-lookup` | 0 | `target/beam7-lookup-final.log`; both cases PASS |
| Seven focused machine `cargo testbench NAME` cases | 0 each | `target/beam7-machine-NAME.log`; `beamlet-boot`, `beamlet-console`, `userland-boot`, `userland-bad-start`, `userland-read-only`, `beamlet-heap-flood`, `beamlet-budget-flood`; each PASS on rv64 and rv32 (14 width verdicts) |
| `cargo testbench docs` | 0 | `target/beam7-final-docs.log`; docs checker and warning-free book render |
| `cargo testbench formatting` | 0 | `target/beam7-final-formatting.log` |
| `cargo testbench size-budget` | 0 | `target/beam7-final-size-budget.log` |
| `cargo testbench unsafe-budget` | 0 | `target/beam7-final-unsafe-budget.log`; 107 uses, zero undocumented |
| `./build --arch rv32 --programs` | 0 | `target/beam7-rv32-bootchain.log`; kernel, loader, test programs release builds; one existing `unused_mut` warning in `tests/programs/src/bin/kernel-half-attack.rs` |
| `cargo testbench --arch rv32 build` | 0 | `target/beam7-rv32-build-cases.log`; 12/12 rv32 build cases PASS for runtime, clients and servers |
| Unfiltered whole Tier A `cargo testbench` on exact head | 1 | `target/beam7-whole-tier-a.log`; 399 PASS verdicts, one FAIL: `bench-ssh-loopback-openssh` because the required reference runner has no `podman` executable. No skip or case exclusion. Harness run directory `target/testbench/run-1-1791159626245169437` |

All commands above ran in the existing `redoubt-dev` image, root mounted at
`/work`, workdir `/work/.worktrees/BEAM7`, UID:GID `1447391350:1447391350`,
network disabled, project-local Cargo/Rustup caches, the worktree host-path
symlink, and the two prebuilt RustSBI image environment variables. The two
rv32 commands and the whole bench were run on the final head. No QEMU was used
for the separate rv32 builds. The QEMU window for the whole bench was released
to the orchestrator when the process exited.

Affected summaries checked on this head: `README.md`, `GETTING-STARTED.md`,
`docs/plan/m1-separation.md`, and `userland/otp/README.md` accurately say
that verified modules and the UART shell run on Redoubt while VM file
operations and native launching remain planned, so no update is needed.
`docs/userland/README.md` was updated in the range; its figure caption now
says the session wiring is planned and verified module boot on UART is built.
Owning pages updated in the range: `docs/userland/beamlet.md`,
`docs/kernel/boot.md`, `docs/testbench.md`, `docs/SECURITY.md`; the resolved
todo was removed from `docs/todo/` and `docs/SUMMARY.md`.

Still required: a supported reference runner to pass the OpenSSH case in an
unfiltered whole Tier A `cargo testbench`; exact-head red, simplifier, and
editor verdicts; orchestrator acceptance and merge. All 399 other whole-bench
verdicts passed, including both widths of BEAM7's machine cases, the lookup
host tests, docs, formatting, size, unsafe, model, and miri. No source or
documentation change has been made since the final head.
