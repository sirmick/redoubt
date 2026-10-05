# MEM1 recovery checkpoint (paused 2026-10-04)

Branch `wp-MEM1`, original recovery base `e9f2fcb958f4229ba90a009999b95fd03897c058`. Source was preserved dirty in place, matched the orchestrator snapshot, and no backup was replayed. HEAD `71b253a88` after four path-scoped commits. No push, stash, or blanket stage. Every committed file was read fully by this member.

## Committed state

- `2ba44be21` launcher/stub paint and launch host tests: `stub/src/lib.rs`, `libs/client/src/launch.rs`, `libs/client/tests/launch.rs`.
- `e903d845f` measured size ceilings: `tests/size-budget.toml` sets exact stub 366, libs/client 987, servers/init 1957 (former 361/966/1952), with reasons in commit.
- `197b6e3c1` init stack preflight, refusal and host fixtures: `image/manifest.json`, `servers/init/src/bin/init.rs`, `servers/init/src/check.rs`, `servers/init/src/fuzz.rs`, `servers/init/src/manifest.rs`, `servers/init/src/refusal.rs`, `servers/init/tests/manifest.rs`, `tests/data/init/bound.json`. Image stack pages 3–5 are provisional placeholders pending machine peaks.
- `71b253a88` machine measurement harness and cases: `Cargo.lock`, `tests/init-boot.toml`, `tests/init-refuses-stack.toml`, `tests/memory-host-tests.toml`, `tools/testbench/Cargo.toml`, `tools/testbench/src/case.rs`, `tools/testbench/src/main.rs`, `tools/testbench/src/qemu.rs`, `tools/testbench/src/memory.rs`.

Only remaining dirty paths at checkpoint: `docs/kernel/budgets.md`, `docs/kernel/memory-layout.md`, `docs/servers/init.md`, `docs/testbench.md`, `docs/userland/native.md`. BEAM7 owns docs writer window, so no docs edits or commits were made by successor. BEAM7 also owns QEMU slot; successor requested release notification. No QEMU process was in flight on takeover. No machine peak measurement was run in this recovery session.

## Exact test invocation and evidence

All bench commands run in existing `redoubt-dev` container as UID 1447391350, with main checkout caches/firmware mounted:
`docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/MEM1 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench <case>'`.
The `sudo: unable to resolve host` warning is benign; commands exited as stated.

- `cargo testbench size-budget`: exit 1 before ceiling edit (stub 366/361, client 987/966, init 1957/1952), retained `target/testbench/run-1-1791154640921777830`; exit 1 after uncommitted edit due ratchet commit rule, retained `run-1-1791154659120783212`; exit 0 after `e903d845f`, exact 366/366, 987/987, 1957/1957.
- `cargo testbench init-host-tests`: exit 0 after fixing stale image-fixture fixed-16 assertions in committed test. `cargo testbench memory-host-tests`: exit 0 (5.0s). `cargo testbench --list`: exit 0 after new cases.
- `cargo testbench formatting`: exit 0 (10.5s) after `71b253a88`. `cargo testbench unsafe-budget`: exit 0 after `71b253a88`, client and init 0 unsafe, stub 7, all undocumented 0. `cargo testbench no-cruft`: exit 0 after `71b253a88`.
- `git diff --check`: exit 0 at snapshot and after source commits.
- Predecessor preserved report `.wash/local/MEM1-recovery/implementer-report.md` reports PASS full `cargo testbench host-tests` including 382.6s model-host-tests, `client-host-tests`, `init-host-tests`, `init-build` and `client-build` both widths, formatting, unsafe-budget, no-cruft. Its earlier failed attempts are distinguished there. These are predecessor-reported passes, not final-head acceptance. Full 382s model suite was intentionally not repeated without relevant model change.

## Pending work

BEAM7 still owns QEMU and docs windows at pause; await release message, then run focused `cargo testbench init-boot` rv64/rv32 with firmware env vars `RUSTSBI_PROTOTYPER=/work/bios/target/riscv64imac-unknown-none-elf/release/rustsbi-prototyper` and `RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper`. Scanner reports `stack <name> <peak bytes> of <declared pages> pages`; final declarations must be twice the larger of rv64/rv32 peaks rounded to pages. Rerun both widths with final declarations and `init-refuses-stack` both widths, then release QEMU promptly. Check final bound <=512 to substantiate 'at least doubles'; `INIT_PAGES=1024` fixed. Update bound fixture and assertions as needed.

In docs window, correct loader-init's 32-page stack as one backed +31 reserved, distinct from client stacks default16, allowed1..128, all selected pages backed, envelope `0x7FF80000..0x80000000`. State init's actual `Launch` use on machine without claiming all APIs boot-tested; public paint is driven-path measurement, not adversarial proof. Editor provisional BLOCK additionally found `docs/plan/m1-separation.md:157` calling client-library coverage host-only despite init-boot `Launch` use, and `docs/userland/native.md:326` says tested(28) but lists 3 bench+26 host =29. Fix both. Provisional scanner bool-table finding withdrawn; red review provisional OK with notes. Final exact committed-head red/simplifier/editor reviews mandatory.

Affected summaries inspected so far: `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/plan/m1-separation.md`, `image/README.md`; no stack-specific claim in first three or image README, so no change seen there. Milestone page needs editor fix above. No crate READMEs at `servers/init/README.md`, `libs/client/README.md`, `stub/README.md`. Complete final affected-summary pass and report each path/reason. Final Tier A whole bench both widths, rv32 build, unsafe, size, docs checker, formatting, no-cruft, book render and clean logical commits still pending; no acceptance claimed.
# MEM1 evidence at 6535a9acb

Branch `wp-MEM1`, HEAD `6535a9acb`, clean worktree. No push, stash, blanket staging or helper agents. Existing source commits `2ba44be21`, `e903d845f`, `197b6e3c1`, `71b253a88` remain as in the saved report. Documentation commits: `24b0fc579` (client launch coverage and stack controls), `fb63dfb3f` (measured stack bounds), `6535a9acb` (affected summaries). Every committed file was read in full by this member.

## Measurement and refusal evidence

All bench runs used existing `redoubt-dev` Docker container as UID 1447391350 with the main checkout mounted at `/work`, `cargo testbench` in `/work/.worktrees/MEM1`, and both firmware variables. Correct paths: rv64 `/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper`, rv32 `/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper`. First rv64 attempt with the saved stale `riscv64imac` firmware path failed before boot (exit 1), evidence in `target/testbench/run-1-1791155853660255389`; no scanner threshold was bypassed. Corrected runs all exited 0:

- `cargo testbench init-boot --arch rv64`: PASS, log `target/testbench/run-1-1791155890542911418/init-boot-rv64-smp1.log`.
- `cargo testbench init-boot --arch rv32`: PASS, log `target/testbench/run-1-1791155918896927773/init-boot-rv32-smp1.log`.
- `cargo testbench init-refuses-stack --arch rv64`, then `--arch rv32`: PASS, both exit 0.
- `cargo testbench init-refuses-bound --arch rv64`, then `--arch rv32`: PASS, both exit 0.

| Server | rv64 peak bytes | rv32 peak bytes | Final declared pages = ceil(2 × larger peak / 4096) |
| --- | ---: | ---: | ---: |
| keyd | 5224 | 4464 | 3 |
| consoled | 8328 | 7064 | 5 |
| bootfsd | 6936 | 5768 | 4 |
| blkd | 4408 | 3744 | 3 |
| netd | 4264 | 3344 | 3 |
| ipd | 8008 | 6624 | 4 |
| fsd:data | 6888 | 5392 | 4 |
| blkd:system | 4408 | 3744 | 3 |
| fsd:system | 5896 | 4128 | 3 |
| beamlet | 4704 | 3624 | 3 |

The provisional `image/manifest.json` declarations already equal these final values, so no source change was needed after measurement. Both boot logs report the image bound as 496 pages; `INIT_PAGES=1024` exceeds twice that bound (992). The bound fixture remains distinct: it uses its own machine/device inputs and passed its prior host tests. QEMU was released immediately after the focused runs, before BEAM7's remaining cases.

## Documentation and summary review

- `docs/kernel/budgets.md`: loader init's 32 reserved pages/one initially backed page distinguished from client stack charge; final measured bound 496 and `init-refuses-bound` evidence.
- `docs/kernel/memory-layout.md`: loader's top 32-page reservation distinct from client 1–128 fully backed pages in the 128-page envelope.
- `docs/servers/init.md`: `stack_pages` default/range/budget refusal, driven boot measurement, launch paint, case list.
- `docs/testbench.md`: `memory = true`, QMP dump and scanner verdict, driven-path and forgeability limits.
- `docs/userland/native.md`: client `Launch` on machine, other operations' host coverage, 29 listed tests, stack controls.
- `docs/plan/m1-separation.md`: progress says client launching is exercised on machine.
- `image/README.md`: corrected manifest server count from six to ten.
- `docs/servers/README.md`: fresh connection boot coverage and current `fsd` placement.
- `docs/kernel/README.md`: distinguishes inclusive line snapshots from enforceable code-line size budget.
- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/userland/README.md`: inspected affected passages; no stack-specific or stale coverage assertion to change. No crate READMEs exist at `servers/init/README.md`, `libs/client/README.md` or `stub/README.md`.

`cargo run -q -p redoubt-doccheck` in container: exit 0 after final docs edits. `mdbook build docs`: exit 0; only installed mdbook-mermaid 0.5.0 versus mdbook 0.5.4 compatibility warning. `cargo testbench docs`: exit 0. `git diff --check`: exit 0.

## Final-head non-QEMU gates

At `6535a9acb`, `cargo testbench formatting`, `size-budget`, `unsafe-budget`, `no-cruft`, `docs`, `init-build --arch rv32`, `client-build --arch rv32` each exited 0. Exact relevant size ceilings: stub 366/366, client 987/987, init 1957/1957. Unsafe counts: client 0, init 0, stub 7, all undocumented counts 0. No full host/model suite was repeated without model changes; predecessor's PASS is preserved in the saved report.

## Remaining gates

Whole `cargo testbench` on both widths, followed by exact-head red/simplifier/editor panel review, remain. BEAM7 owns QEMU after MEM1's focused slot. No acceptance claimed.
## Whole Tier A bench attempt at 6535a9acbd6eea8fcdc7142d291da0d95ed7a656

One unfiltered `cargo testbench` with both firmware variables in the cached Docker environment completed, exit 1, with 396 PASS and exactly 5 FAIL lines. Full stdout: `target/MEM1-whole-bench.log`; run artifacts: `target/testbench/run-1-1791157885812581464`. The source head was clean and verified before the command. No separate host/model reruns, `--allow-skip`, weakened thresholds, or QEMU overlap. QEMU released to BEAM7 after the process exited.

- `bench-ssh-loopback-openssh` failed before its case because Podman is absent from both the dev container and host. This is an environment prerequisite, not a MEM1 source verdict.
- `userland-boot` rv64/rv32 and `userland-read-only` rv64/rv32 failed because beamlet faulted code 15. Guest logs show store page faults at `0x7fffb648` (rv64) and `0x7fffc500` (rv32), below beamlet's then-declared 3-page mapping (`0x7fffd000..0x80000000`). The original `init-boot` measurement drove only the shallow start path. The shell cases reach deeper calls and expose an undersized beamlet declaration. Their logs are `target/testbench/run-1-1791157885812581464/userland-{boot,read-only}-rv{64,32}-smp1.log`.
- All other cases reported PASS, including `init-boot`, `init-refuses-stack`, `init-refuses-bound`, and `kernel-containment` on both widths; `model-host-tests` 392.6s and `rt-miri` 94.6s.

Recovery in progress, uncommitted pending docs/QEMU windows: `image/manifest.json` beamlet stack tentatively restored to 16 pages, `tests/userland-boot.toml` and `tests/userland-read-only.toml` set `memory = true` to measure the deeper shell path, and `servers/init/tests/manifest.rs`'s bound assertion adjusted for the provisional declaration. `cargo testbench init-host-tests` exits 0 with these changes. The 16-page declaration implies bound 507, but neither is final until a focused both-width machine measurement. The owning budget page must be updated in a granted docs window. No panel review or acceptance is claimed.

## Context-warning handoff after whole Tier A attempt (2026-10-04)

Branch `wp-MEM1`, exact HEAD `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`. Worktree is dirty only at `image/manifest.json`, `servers/init/tests/manifest.rs`, `tests/userland-boot.toml`, `tests/userland-read-only.toml`; `git diff --check` passes. These are uncommitted recovery changes after the full-suite finding: provisional beamlet stack_pages 16, corresponding provisional bound assertion, and `memory = true` on both userland cases so the shell path gets measured. All four changed files have been read in full. No docs edited since the whole bench. Do not stamp 16 pages or 507 bound as final until deeper rv64/rv32 peaks are measured. 

The whole unfiltered `cargo testbench` at clean HEAD above exited 1 with 396 PASS and 5 FAIL. Exact stdout `target/MEM1-whole-bench.log`; artifacts `target/testbench/run-1-1791157885812581464`. Four userland cases fail because beamlet's 3-page stack faults below its mapping: `userland-boot` and `userland-read-only`, each rv64 and rv32. The fifth is `bench-ssh-loopback-openssh`, which cannot start because Podman is absent in container and host. There was no `--allow-skip` or softened scanner. The bench's other cases passed, including both-width init boot/refusal/containment, 392.6s model host tests and 94.6s rt-miri. `cargo testbench init-host-tests` on the dirty recovery changes passed; do not repeat whole host/model tests merely for handoff. No `cargo testbench`/QEMU/Docker test process remained at this checkpoint. QEMU was explicitly released to BEAM7 after the suite, and BEAM7 currently owns machine and docs windows. No new runs began on this context-warning turn.

Final-head pre-bench gates on `6535a9acb` all passed: formatting; size-budget stub 366/366, client 987/987, init 1957/1957; unsafe-budget client/init 0, stub 7, all undocumented 0; no-cruft; docs/doccheck/mdbook render; rv32 init-build and client-build. Six focused QEMU init-boot/stack-refusal/bound-refusal cases passed both widths with shallow-path server peaks in the preceding table; that table's beamlet 3-page value is superseded by the whole-suite fault and must be remeasured. Prior docs/summaries including M1 Launch claim and native test count were committed at clean HEAD, but `docs/kernel/budgets.md` still states bound 496 and must be reconciled with the final deeper-path declaration. All prior source and doc commits were read fully before committing.

Next authorized work, after explicit machine/docs release: focused `cargo testbench userland-boot --arch rv64`, then rv32, and `userland-read-only` rv64/rv32 with both firmware variables (rv64 `riscv64gc`, rv32 `riscv32imac`) in existing `redoubt-dev` Docker UID 1447391350. Inspect scanner/QMP logs for each server's peak, derive beamlet pages `ceil(2 * max(rv64,rv32 peak)/4096)`, update image declaration, owning bound fixture and `docs/kernel/budgets.md` from the actual final image; verify bound <= 512 and rerun affected QEMU/refusal cases as appropriate. Then commit exact paths only after reading any newly committed file in full, rerun required Tier A gates on the new clean head, and obtain exact-head red/simplifier/editor panel review. Podman remains an external prerequisite for a fully green whole suite; escalate the real environment failure, do not bypass it. No acceptance or completion claimed. No push, stash, blanket stage, helper agents or process remains.

## Architect ruling on MEM1-runtime-stack at handoff

Architect confirmed the early-boot coverage gap and accepted the dirty calibration preparation as provisional only. Exact dirty paths remain `image/manifest.json`, `servers/init/tests/manifest.rs`, `tests/userland-boot.toml`, and `tests/userland-read-only.toml`; no further edits or commits were made at this checkpoint. The 16-page beamlet declaration is for calibration only, not a final bound. The two userland cases have `memory = true` so their full driven paths can be scanned. `cargo testbench init-host-tests` passed with this dirty preparation, and `git diff --check` passed. The prior clean HEAD remains `6535a9acbd6eea8fcdc7142d291da0d95ed7a656`; the worktree itself is dirty as listed.

After BEAM7 explicitly releases QEMU, successor must measure per-server peaks for all three cases (`init-boot`, `userland-boot`, `userland-read-only`) on rv64 and rv32. Each final declaration is `ceil(2 * maximum observed peak across all three cases and both widths / 4096)` pages. Then rerun the machine measurements with those final declarations, update the exact image bound, owning budget doc and bound fixture, and keep QA `MEM1-runtime-stack` blocking until the evidence supports closure. The five-failure whole bench remains exit 1 and is not overall acceptance. No live QEMU or testbench process at handoff. Machine and docs windows remain BEAM7-owned. No new diagnostic or test was started after the context-warning instruction.
