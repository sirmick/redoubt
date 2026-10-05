# SCHED1 cluster qualification diagnostics — 2026-10-04

Assignment 9ccab95dc909814455cb64ad612af853. Architect ruling `IPC3-wake-latency` event `23bafd89c498925f3b6a53e6f0b57e8e` authorizes source-only diagnostics. QEMU belongs to MEM1; no machine run after the original failure.

HEAD remains `7b23f52835f7b6141f0ed7405e0ef5eb2a3d2097`. Original failing log preserved at `target/testbench/run-1-1791155555700002952/sched-cluster-rv64-smp1.log`, SHA256 `ec5bf8ebaa639a328d19c762d4e6d584e455264fb400da42ba9723c8669a86e4`. Original source identity is in `/home/mcloonan/redoubt/.wash/local/SCHED1-report.md`: tracked diff SHA256 `f006d227bee3e94cdcfeb9a476320eccfe3de8d253f9c7160aaf15c03ec588f8`; untracked cluster source SHA256 `ab0d6e1d20299e951fa607725ee47e16260ab7036c0590c536929bd711783305`.

Current source identity: `git diff --binary | sha256sum` = `bb3c9b3478540108a8bc46f295ce76d8dc15bf31c9de68d628dda557e63a0c43`; untracked `tests/programs/src/bin/sched-cluster.rs` SHA256 `575157c5b5ff8ff175ed94c0e49d60e8edbd54126b90affc0dd36bdef1aabadb`; untracked `tests/sched-cluster.toml` SHA256 unchanged `3c8f848bcce196ac8c9bf7c41e98cec7b87191df432a4cd93afa2e7ee090e5b9`.

Changed only `tests/programs/src/sched.rs` and `tests/programs/src/bin/sched-cluster.rs` from the handed-off source. On failure, a child sends one diagnostic report in the usual report slot: original tag and completed count, signed target/current microseconds relative to common release, failing attempt index, branch and result. Branch codes: 1 map error, 2 zero delay, 3 driver receive mismatch, 4 timer receive mismatch. Results: map error ABI code, zero for delay, received variants 1..4 or `0x80 | Error` ABI code. It then answers the parent sample request with its partial windows so the failed run terminates. Successful 200-sample behavior is unchanged. Parent waits for all distinct child reports, counts duplicates, prints raw report/tag/count and decoded failure before the fatal check. No deadline, margin, retry, dropped sample, category, policy, trace capacity or threshold change.

Validation: temporary `tests/sched-source-build-local.toml` (`kind="build"`, `package="test-programs"`, both widths) through `cargo testbench sched-source-build-local` in existing `redoubt-dev` Docker image, UID:GID 1447391350:1447391350, network none, project-local `.cargo` and `.rustup`: rv64 PASS 2.0 s, rv32 PASS 2.0 s, overall exit 0. Path-specific nightly rustfmt exit 0. `git diff --check` exit 0. Temporary manifest removed. Passed host oracle/model checks from the prior report were not repeated; host oracle source was not changed. No QEMU run.

Affected summaries checked: `README.md`, `GETTING-STARTED.md`, `kernel/README.md`, `model/README.md`, `docs/plan/m1-separation.md`, `docs/kernel/README.md`, `docs/kernel/scheduling.md`, `docs/kernel/timer.md`, `docs/kernel/model.md`, `docs/testbench.md`. No diagnostic-only text change is needed while the fixture has not qualified. Existing stale 10 ms behavior/status summaries still require reconciliation at final accepted code and docs window; Architect/BEAM7 hold the page-writing coordination per the prior handoff.

Next reserved diagnostic command after explicit QEMU release:

```sh
docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -e RUSTSBI_PROTOTYPER=/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper -e RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper -e TESTBENCH_QEMU_SEED=3 -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/SCHED1 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench --arch rv64 sched-cluster'
```

Stop on the first result and report its exact `CLUSTER-RAW` / `CLUSTER-FAIL` branch evidence. Do not treat this diagnostic source build as fixture qualification or scheduler acceptance. If branch 2 confirms delay zero, return the Architect redesign question before any behavior change.
