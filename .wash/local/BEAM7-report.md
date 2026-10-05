# BEAM7 final-head whole-bench checkpoint, 2026-10-04

Branch `wp-BEAM7` remains clean at `6a79d0e1356493f5307b2af19934b4f3d8515c79`,
based on `main` `9212f6f600591abb3ea78ef450e0d6f4d517ab92`. Review range is
`main..HEAD`: signed commits `d5b2df3ec` host routing, `46d418875` VM
refusal, and `6a79d0e13` docs. Package files under `docs`, `tests`, `tools`,
and `userland` are byte-identical to tested pre-rebase head `65b9f9a8f`.
No source or documentation edits, push or merge followed the final head.

The exact-head, unfiltered whole Tier A `cargo testbench` ran in the existing
`redoubt-dev` container with no skip or exclusion and exited **1**:
**399 PASS verdicts, one FAIL**. The sole failure was
`bench-ssh-loopback-openssh` at 0.0 s: `the reference sshd's container:
podman is not installed (--allow-skip to skip)`. This is an unresolved
reference-runner environment gate, not acceptance. Full stdout is
`target/beam7-whole-tier-a.log`; run directory is
`target/testbench/run-1-1791159626245169437`. All other cases passed,
including both widths of BEAM7's machine cases, the lookup host cases,
docs, formatting, size, unsafe, model, and miri. QEMU was released to the
orchestrator when the bench exited.

Final-head rv32 non-QEMU coverage passed: `./build --arch rv32 --programs`
exit 0 (kernel, loader, test programs), and
`cargo testbench --arch rv32 build` exit 0 (12/12 build cases for runtime,
clients and servers). Logs are `target/beam7-rv32-bootchain.log` and
`target/beam7-rv32-build-cases.log`. One existing `unused_mut` warning in a
test program did not fail the build. Earlier final-head docs, formatting,
size and unsafe gates exited 0; unsafe is 107 uses, zero undocumented.

Stable evidence is `/home/mcloonan/redoubt/.wash/local/evidence/BEAM7/`:
399 whole-run log/capture artifacts, all 26 `target/beam7-*.log` gate/stdout
files, `review-evidence.md`, `SOURCE.md` identifying base/head/run, and
verified `SHA256SUMS` (427 entries). Older focused/negative-control raw
`target/testbench/run-*/*.log` files had already been pruned. Their saved
`target/beam7-*.log` stdout remains; the current whole-run raw logs cover
all focused machine cases. The negative-control raw guest log is unavailable.

Affected summaries checked: `README.md`, `GETTING-STARTED.md`,
`docs/plan/m1-separation.md`, and `userland/otp/README.md` correctly
distinguish verified module boot on Redoubt from planned VM file/native
operations; no update was needed. `docs/userland/README.md` was corrected
in the review range to identify the planned session wiring and built UART
shell boot.

Exact-head panel: red reviewer 95840c9363b45345cad51bf95d717aa8 OK
with notes; simplifier reviewer d15276919a890740ecd52add5fcdb69f OK
with notes; editor reviewer 9fefa255d4cbcbf9aef2a1ded70a0612 BLOCK
solely on the known OpenSSH reference-runner environment gate. No reviewer
found a source or documentation issue; earlier host-routing and app_spec
findings are closed.

A supported OpenSSH reference runner, a passing unfiltered whole bench,
the editor's renewed verdict on that gate, and orchestrator acceptance/merge
remain. Preserve this head; do not repeat passed independent checks absent
a content change.

## Earlier checkpoint (superseded by this section)

# BEAM7 checkpoint, 2026-10-04

Branch `wp-BEAM7` is clean at `6a79d0e13`, based on current `main` `9212f6f60`.
Three signed logical commits: `d5b2df3ec` host test routing,
`46d418875` verified lookup refusal and regressions, `6a79d0e13` docs.
No push or merge. Package files under `docs`, `tests`, `tools`, `userland`
are byte-identical to the tested pre-rebase head `65b9f9a8f`; the only
old-to-new tree changes are `main`'s `.wash/PROJECT.md` and
`.wash/workspace.toml`. Both QEMU and docs writer windows were released.
No BEAM7 command or container is running; no tracked path is dirty.

## Evidence

- Focused host `cargo testbench -v beamlet-lookup` exit 0: CLI 3 tests and
  VM/Redoubt 74 tests. Final pre-rebase log `target/beam7-lookup-cleanup.log`.
  A deliberate `Lookup::Refused` to `Absent` negative control failed the
  zero-path assertion (6 operations versus 0), then restored run passed;
  `target/beam7-negative-control.log` and `target/beam7-lookup-final.log`.
- All seven focused machine cases passed rv64 and rv32 (14 width verdicts),
  each command exit 0:
  `beamlet-boot`, `beamlet-console`, `userland-boot`, `userland-bad-start`,
  `userland-read-only`, `beamlet-heap-flood`, `beamlet-budget-flood`.
  Logs `target/beam7-machine-NAME.log`. Four final cases were executed after
  QEMU release from MEM1; their eight width verdicts were inspected.
- Final-head `cargo testbench docs`, `formatting`, `size-budget`, and
  `unsafe-budget` all exited 0. Logs `target/beam7-final-NAME.log`.
  `docs` executes the code-aware docs checker and warning-free mdbook render.
  Unsafe count is 107 uses, zero undocumented. `git diff --check main..HEAD`
  exited 0. Worktree clean.

## Docs and summaries

`docs/userland/beamlet.md`, `docs/kernel/boot.md`, `docs/testbench.md`,
`docs/SECURITY.md`, `docs/userland/README.md`, `docs/SUMMARY.md` updated;
`docs/todo/beamlet-refused-module-falls-through.md` deleted. Every docs
file committed was read in full. README.md, GETTING-STARTED.md,
docs/plan/m1-separation.md, and userland/otp/README.md were checked and
already distinguish verified modules booting from files/native launching
still planned. docs/userland/README.md was corrected because its figure
caption still said beamlet ran only on the host.

## Remaining

The coordinated Tier A whole `cargo testbench` on this final head, explicit
full rv32 build coverage, and exact-head red/simplifier/editor verdicts are
pending. MEM1's whole bench slot follows SCHED1's short diagnostic run;
BEAM7 must await explicit QEMU/whole-bench release. No long machine run
should be started before that. Preserve passing focused evidence and avoid
rerunning it without a relevant change. The orchestrator must accept/merge;
implementer never pushes.


## Earlier checkpoints (historical; superseded by current checkpoint)

# BEAM7 implementer report (2026-10-04 handoff; host executed, machine in progress)

## Current status, 2026-10-04 (supersedes all earlier checkpoints below)

Branch `wp-BEAM7` at `588d45924`; worktree clean after two new logical commits: `5fafdd1e9 vm: verify app lookup outcomes through Erlang caller` and `588d45924 testbench: route named beamlet host regressions`. No push or merge. Earlier source and partial bench commits remain `0d900bb9f` and `d26324470`. The named host cases run from `userland/otp`: the VM/Redoubt case passes `beamlet-redoubt/fake`; the CLI case runs separately to avoid feature unification. `HostTests.workspace/features` parse with root/no-features defaults, validate a requested workspace under the repository before Cargo, and preserve the default command. Verbose bench output now shows host test execution.

Two inline red findings—missing host routing and unexecuted app_spec caller test—are addressed and executed. The VM `application:load/1` regression checks Erlang's error tuple for Absent/Refused, invalid binary parsing for Found, exactly one `Platform::load_app` attempt per name and no app-lookup file operations for Absent/Refused. The first test run failed because Erlang charlists display as byte lists, and the initial zero-path assertion also counted unrelated module loading; the repaired test examines terms and snapshots the path count after spawning `application`. The third red finding, stale owning pages and open todo, remains. The docs window was granted but no docs write began before this context checkpoint.

Exact host execution in the existing `redoubt-dev` image (UID:GID `1447391350:1447391350`, `--network none`, root mounted `/work`, workdir `/work/.worktrees/BEAM7`, project-local `CARGO_HOME=/work/.cargo`, `RUSTUP_HOME=/work/.rustup`, and `/home/mcloonan/redoubt` symlinked to `/work`): `cargo testbench -v beamlet-lookup` exited 0 after restoration. `beamlet-lookup-cli-host` ran 3 tests; `beamlet-lookup-host` ran 74 tests (Redoubt targets 7+7+2+3, VM targets 37+6+12). The log `target/beam7-lookup-final.log` names `vm::tests::a_refused_system_module_never_touches_the_code_path`, `vm::tests::app_spec_uses_one_source_attempt_and_keeps_its_erlang_result`, and both `verified_*_lookup_propagates_found_absent_and_refused` tests as `ok`. The temporary negative control changed `Lookup::Refused` to fall through like Absent and ran `cargo testbench -v beamlet-lookup-host`: exit 1, specifically the zero-file-access assertion observed 6 operations versus 0; log `target/beam7-negative-control.log`. The terminal-refusal branch was restored and the clean cases reran exit 0. Earlier exploratory named runs exited 1 while repairing the app_spec test; these are not passing evidence.

`cargo testbench host-tests` was an optional preliminary broad host run. Its `host-tests` case, which executes testbench crate unit tests including parser and routing tests, passed (20.6s). It also passed blkd, client, fsd, init, ipd, littlefs, model, net, netd, r4 and rt host cases; I stopped my container after those to free resources for the shared QEMU window, so the overall filter exited 137 and is not a suite pass. The final Tier A whole bench remains required. `rustfmt +nightly --edition 2021 tools/testbench/src/build.rs tools/testbench/src/case.rs userland/otp/vm/src/vm.rs` exited 0 in the dev image; `git diff --check` exited 0. All files in the new commits were read in full by this implementer. No raw `cargo test` was invoked.

The orchestrator granted BEAM7 the docs writer and exclusive focused QEMU windows after MEM1 paused. Focused machine results so far: `cargo testbench beamlet-boot` exit 0, rv64 and rv32 PASS, log `target/beam7-machine-beamlet-boot.log`; `cargo testbench beamlet-console` exit 0, rv64 and rv32 PASS, log `target/beam7-machine-beamlet-console.log`. `cargo testbench userland-boot` exited 0, rv64 PASS in 157.1s and rv32 PASS in 149.6s, log `target/beam7-machine-userland-boot.log`. No other machine cases started. QEMU and docs writer windows were released at this checkpoint to MEM1. Remaining focused commands, serially when MEM1/orchestrator next grants QEMU: `cargo testbench userland-bad-start`, `cargo testbench userland-read-only`, `cargo testbench beamlet-heap-flood`, `cargo testbench beamlet-budget-flood` (all declared rv64 and rv32). Docs writer can be released now; no edit began. Owning pages to update: `docs/userland/beamlet.md`, `docs/kernel/boot.md`, `docs/testbench.md`, delete `docs/todo/beamlet-refused-module-falls-through.md`, remove its `docs/SUMMARY.md` entry. Docs files beamlet.md, boot.md, testbench.md, and todo were read in full this session; SUMMARY.md still needs full read. The whole Tier A bench has a later coordinated slot; full rv32 build, docs checker/render, formatting, size and unsafe gates, history fold/rebase, and exact-head red/simplifier/editor reviews remain. Docker command pattern: `docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -e RUSTSBI_PROTOTYPER=/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper -e RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/BEAM7 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench NAME'`. Log to `target/beam7-machine-NAME.log` outside the Docker command and preserve exit status. Affected summaries checked: `README.md`, `GETTING-STARTED.md`, `docs/plan/m1-separation.md`, and `userland/otp/README.md` accurately describe verified userland modules already built and files/native launching still pending, so no summary edit is needed. No new async/file/client/kernel work and no push.

## Earlier checkpoints (historical)

## Implemented, pending bench execution

- `Platform::{load_module,load_app}` now return `Lookup::{Found(Vec<u8>),Absent,Refused}`. Default application lookup is Absent. All direct callers and implementations in owned VM/Redoubt/CLI files adapted.
- `System::locate_module` searches the code path only for Absent; Found wins and Refused returns immediately. Existing explicit `code:load_binary/3` is unchanged. Logger presence counts Found only. `beamlet:app_spec/1` still gives binary on Found and atom `error` on Absent/Refused.
- Redoubt preserves `Unloaded::Absent` versus `Unloaded::Refused` from the checked source through Platform, retaining one existing console diagnostic with its original reason.
- VM attack test `a_refused_system_module_never_touches_the_code_path` plants bytes at `/home/p/refused.beam` and asserts no `Files` operations. Existing positive bundle/absent tests now assert no operations / successful path read. New fake-kernel integration tests `verified_module_lookup_propagates_found_absent_and_refused` and `verified_application_lookup_propagates_found_absent_and_refused` cover good, absent-with-zero-object-reads, hash mismatch, missing indexed object, short and long object, and exact one-line diagnostics.

## Checks so far

- `docker run ... redoubt-dev cargo +nightly fmt --manifest-path userland/otp/Cargo.toml --all`: exit 0 (format action).
- `docker run ... redoubt-dev bash -c 'sudo mkdir ... && sudo ln ... && cargo testbench formatting'`: exit 0, `PASS formatting` (12.1s). An earlier direct invocation without the worktree host-path symlink exited 1 before running the gate because Git could not find `/home/mcloonan/redoubt/.git/worktrees/BEAM7` inside the container.
- `docker run ... redoubt-dev bash -c 'sudo mkdir ... && sudo ln ... && cargo check --manifest-path userland/otp/Cargo.toml -p beamlet-vm -p beamlet-redoubt -p beamlet --tests --features beamlet-redoubt/fake'`: exit 0. This is a compile check, not a test run.
- `git diff --check`: exit 0.

Host test runs, the negative control, machine/QEMU cases, whole bench, rv32 compilation, docs check/render, size/unsafe gates and final-head review have not run. No acceptance claimed.

## Shared-file coordination

MEM1 owns `tools/testbench/src/{case,build}.rs`; no edits made here. Proposed optional host workspace/features routing and command tests are drafted at `/tmp/BEAM7-host-routing.md` and sent to orchestrator. New `tests/beamlet-lookup-host.toml` and `tests/beamlet-lookup-cli-host.toml` wait for routing support. All machine tests wait for orchestrator coordination with SCHED1's QEMU timing window.

Docs are a single-writer hotspot. Proposed `docs/userland/beamlet.md`, `docs/kernel/boot.md`, `docs/testbench.md`, `docs/SUMMARY.md` and todo deletion were sent to orchestrator; no docs touched. Todo deletion waits for tested closure.

Affected summaries checked: `README.md`, `GETTING-STARTED.md`, `docs/plan/m1-separation.md`, and `userland/otp/README.md` already describe verified module loading on Redoubt and pending VM files/native launching accurately, so no change needed. `userland/otp/vm/src/platform.rs` crate-level description incorrectly called the Redoubt adapter future work, so corrected it. `userland/otp/redoubt/src/lib.rs` still accurately states no files/programs.

Next: receive host-routing and docs windows, add bench cases, run named host tests, perform refusal-as-absence negative control then restore, coordinate machine gates, update docs at tested closure, commit logical groups by exact paths, and report final gates/head for review. No helpers and no push.

Update after assignment dispatch: VM refusal fixture now counts entry to `Platform::files()` itself as well as `Files::{open,read,info,close}`, making the zero-access assertion stronger. `cargo check --manifest-path userland/otp/Cargo.toml -p beamlet-redoubt --target riscv32imac-unknown-none-elf` in `redoubt-dev` exited 0; this is a compile check only and does not replace the Tier A full rv32 gate.

Update after `build.rs` grant: `tools/testbench/src/build.rs` now contains bounded host-workspace validation and feature-aware command construction plus tests for root default, OTP routing, forwarding `beamlet-redoubt/fake`, and invalid/missing requested workspace. `case.rs` is still MEM1-owned, so `cargo_test` temporarily supplies None/[] and cannot route a new case yet. `cargo check -p testbench --tests --quiet` exited 0. VM fixture now has `app_spec_uses_one_source_attempt_and_keeps_its_erlang_result` through embedded `application:load/1`, contrasting Absent/Refused error with Found binary's bad-spec parse path while counting one platform source attempt each and zero Files access. `cargo check --manifest-path userland/otp/Cargo.toml -p beamlet-vm -p beamlet-redoubt --tests --features beamlet-redoubt/fake --quiet` exited 0. Neither compile check executes tests.

## Context checkpoint, 2026-10-04 (supersedes earlier status lines)

The orchestrator requested an immediate WIP handoff before full acceptance. Current HEAD is `d26324470` on `wp-BEAM7`; worktree is clean. No push or merge. Commits are `0d900bb9f vm: stop verified lookup fallback on refusal` (exact paths: `userland/otp/cli/src/main.rs`, `userland/otp/redoubt/src/lib.rs`, `userland/otp/redoubt/tests/lookup.rs`, `userland/otp/vm/src/bif/info.rs`, `userland/otp/vm/src/platform.rs`, `userland/otp/vm/src/vm.rs`, `userland/otp/vm/tests/hostile.rs`, `userland/otp/vm/tests/limits.rs`) and `d26324470 testbench: prepare bounded host workspace routing` (exact path: `tools/testbench/src/build.rs`). Every committed file was read in full before committing. Both commits are checkpoint work; neither has full acceptance tests.

The source includes an `app_spec_uses_one_source_attempt_and_keeps_its_erlang_result` VM test, added after the initial handoff description, but it has only been compiled. It is therefore still a real pending acceptance item for the successor to run and repair if needed. `build.rs` route validation/command construction is partial: `case.rs` still has no workspace/features fields and `cargo_test` still passes `None`/`[]`, so the named host case cannot yet run. The updated draft is saved at `/home/mcloonan/redoubt/.wash/local/BEAM7-host-routing.md`; its title says MEM1 owns implementation, which is stale for `build.rs` only. BEAM7 had the `build.rs` window; MEM1 still owns `case.rs` until explicit handoff. No case or docs files were edited.

Exact checks after the latest source additions: `cargo check --manifest-path userland/otp/Cargo.toml -p beamlet-vm -p beamlet-redoubt --tests --features beamlet-redoubt/fake --quiet` exit 0; `cargo check -p testbench --tests --quiet` exit 0; `rustfmt +nightly --edition 2021 tools/testbench/src/build.rs userland/otp/vm/src/vm.rs` exit 0; `git diff --check` exit 0 immediately before commits. Earlier full OTP check including CLI, formatting bench, and rv32 Redoubt check are detailed above. No direct `cargo test` was used. Final formatting gate after the latest small edits is outstanding.

Remaining gates: receive `case.rs` window; wire optional workspace/features into `cargo_test`; add VM/Redoubt host case and separate CLI case; execute named host case through `cargo testbench`; verify each test actually executes; run restored-refusal-as-absence negative control and restore; coordinate all QEMU/machine runs with SCHED1; complete whole bench, rv32 full, docs/render, nightly format, size and unsafe checks; review final HEAD. Docs window must be granted before touching the book; proposed edits and summary checks are above. Delete the todo only after tested closure and reconciled references. No async/files APIs were added or enabled. The orchestrator's latest message mentioned `red findings5c69fb098b4e32d0b261e92380ba7a74`; a targeted Wash QA lookup of `5c69fb098b4e32d0b261e92380ba7a74` returned `unknown QA thread`, so no finding content was available to incorporate here.
