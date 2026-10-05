# SCHED1 single cluster diagnostic — 2026-10-04

Assignment `3a65746fa0b153b86f7ab7541575be79`. BEAM7 explicitly released QEMU. One run only: rv64 `sched-cluster`, `TESTBENCH_QEMU_SEED=3`, existing `redoubt-dev` image, UID:GID `1447391350:1447391350`, `--network none`, project-local Cargo/Rustup and RustSBI binaries. Command:

```sh
docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -e RUSTSBI_PROTOTYPER=/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper -e RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper -e TESTBENCH_QEMU_SEED=3 -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/SCHED1 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench --arch rv64 sched-cluster'
```

Exit 1, 4.1 s. Testbench failed on forbidden `[cluster] FAIL` output. Console log: `target/testbench/run-1-1791157775978771536/sched-cluster-rv64-smp1.log`, SHA256 `9a1a3293d2e1a613088b529961733ed64e4361fdd720ef3305619bb97895fcfe`. Exact evidence:

```text
122:CLUSTER-RAW role=17 child=17 seen=1 words=320100 320720 2147746304 1025 tag=1 count=4
123:CLUSTER-FAIL role=17 index=4 branch=2 result=0 target_us=320100 current_us=320720
124:[cluster] FAIL: stand-in 17 took all 200 real waits
```

Branch 2 is zero computed delay before the fifth driver `receive`. The recorded current time is 620 µs past the fixed target. Exactly four driver waits completed. This establishes a fixture construction failure. There is no 200-sample latency or W/rank/category proof from this run, and it does not establish a scheduler latency failure. Architect must rule how to retain one real wait per fixed attempt, positive debt, offsets/categories, identical candidate/old-slice construction, and all 200 samples before any behavior change.

Source identity before and after the run: HEAD `7b23f52835f7b6141f0ed7405e0ef5eb2a3d2097`; tracked `git diff --binary | sha256sum` = `bb3c9b3478540108a8bc46f295ce76d8dc15bf31c9de68d628dda557e63a0c43`; untracked cluster source SHA256 `575157c5b5ff8ff175ed94c0e49d60e8edbd54126b90affc0dd36bdef1aabadb`; TOML SHA256 `3c8f848bcce196ac8c9bf7c41e98cec7b87191df432a4cd93afa2e7ee090e5b9`. Original prediagnostic failure log preserved at `target/testbench/run-1-1791155555700002952/sched-cluster-rv64-smp1.log` with SHA256 `ec5bf8ebaa639a328d19c762d4e6d584e455264fb400da42ba9723c8669a86e4`.

No retry, rv32, other case, source edit or redesign. QEMU released immediately to MEM1; findings sent to orchestrator and architect-2 on QA `IPC3-wake-latency` in messages `606a7b1f6f5c3215706d3f261bc1b703` and `aecb7e53128b1d1ea7bf26f180df50f3`.
