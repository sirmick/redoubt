# SCHED1 limited 1 ms slice experiment

Scope released: kernel/model/shared-test slice constants only and unchanged pinned `sched-latency` seed 3 on rv64, then rv32. No cluster/oracle/fixture adaptation, docs edit, sweep or alternate quantum. Source base is IPC3 `a0fbbcab12be5c43f217cf94b7f006d3d1f37689` on `wp-SCHED1`; the candidate is the uncommitted three-path diff (binary diff SHA-256 `47662cd390e59e4725bce23b62e0aeca703b92f082aa26ef8d532f53973ca3f2`). It changes `kernel/src/sched.rs::SLICE_US` 10,000 -> 1,000, `model/src/spec.rs::SLICE` 10,000 -> 1,000 (and its 10 ms comment -> 1 ms), and `tests/programs/src/sched.rs::SLICE_US` 10,000 -> 1,000. `git diff --check` exit 0. The worktree contains exactly these three modified files; no commit yet, because this is an early candidate checkpoint.

## Commands, exits, logs

Exact commands (rv64, then rv32):

```sh
docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -e RUSTSBI_PROTOTYPER=/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper -e RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper -e TESTBENCH_QEMU_SEED=3 -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/SCHED1 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench --arch rv64 sched-latency'
docker run --rm --user 1447391350:1447391350 --network none -e HOME=/home/dev -e CARGO_HOME=/work/.cargo -e RUSTUP_HOME=/work/.rustup -e RUSTSBI_PROTOTYPER=/work/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper -e RUSTSBI_PROTOTYPER_RV32=/work/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper -e TESTBENCH_QEMU_SEED=3 -v /home/mcloonan/redoubt:/work:z -w /work/.worktrees/SCHED1 redoubt-dev bash -lc 'sudo mkdir -p /home/mcloonan && sudo ln -sfn /work /home/mcloonan/redoubt && exec cargo testbench --arch rv32 sched-latency'
```

Both corrected commands exited 0. This used the existing `redoubt-dev` image, project-local toolchain caches and RustSBI binaries; no host install. The first attempt used UID/GID 1000:1000 and exited 1 before Cargo (`sudo: you do not exist in the passwd database`). The image's UID/GID labels were checked as 1447391350:1447391350, and the two corrected runs exited 0. Container `sudo` printed a harmless hostname-resolution warning. No candidate test was run in the failed preflight.

The filter also matches the reference `sched-latency-tcg` case: it passed on both widths but has no latency targets. The pinned `sched-latency` gate itself passed on each width; every N=1/4/16 target met, the rank/charge oracle passed, and each trace ended with `dropped 0`.

- rv64 gate log: `/home/mcloonan/redoubt/.worktrees/SCHED1/target/testbench/run-1-1791151707393209491/sched-latency-rv64-smp1.log` (bench reported PASS, 46.9 s).
- rv32 gate log: `/home/mcloonan/redoubt/.worktrees/SCHED1/target/testbench/run-1-1791151878419580757/sched-latency-rv32-smp1.log` (bench reported PASS, 48.7 s).

## Measured gate output

Values are microseconds; each cell is `net p50/p99/max | gross p50/p99/max` as printed by `sched_oracle`. There are 200 driver and timer samples, 50 decision and deadline samples per N.

| Width | N | Driver wake | Timer wake | Decision wake | Deadline notice |
| --- | ---: | --- | --- | --- | --- |
| rv64 | 1 | 1209/1701/1702 \| 1426/1944/1944 | 806/2556/3143 \| 949/2997/3735 | 641/641/641 \| 669/669/669 | 3405/3450/3450 \| 25706/25750/25750 |
| rv64 | 4 | 1629/2410/2428 \| 1978/2788/2806 | 1212/2322/3271 \| 1479/3038/3987 | 1022/1024/1024 \| 1077/1078/1078 | 3830/3832/3832 \| 28213/28214/28214 |
| rv64 | 16 | 4120/18031/21190 \| 5328/21331/23941 | 4614/21150/26850 \| 5768/25352/32618 | 1600/8079/8079 \| 1762/8896/8896 | 4280/5423/5423 \| 30739/38274/38274 |
| rv32 | 1 | 1313/1825/1904 \| 1582/2131/2204 | 884/2210/2619 \| 1061/2770/3187 | 830/832/832 \| 881/884/884 | 3547/3715/3715 \| 30102/30266/30266 |
| rv32 | 4 | 1578/2674/2802 \| 1979/3209/3332 | 1526/3671/4961 \| 1775/4599/6283 | 1368/1378/1378 \| 1447/1457/1457 | 3796/3963/3963 \| 32651/32788/32788 |
| rv32 | 16 | 4829/22525/24036 \| 6527/26250/28124 | 4987/24731/29592 \| 6139/30420/35132 | 9962/16859/16859 \| 11426/19174/19174 | 4413/8556/8556 \| 35433/43727/43727 |

| Width | Trace records / drops | Picks | Audits / total µs | Timer interrupts | Slice-ending timer entries | R10 p99/max µs | Lease-end sum µs | N=16 server share |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| rv64 | 484107 / 0 | 45071 | 49613 / 12685353 | 42313 | 41594 | 4129/4129 | 8079 + 4129 = 12208 | 387/1000 (>=354) |
| rv32 | 473882 / 0 | 43757 | 50133 / 15281276 | 41126 | 40322 | 4214/4215 | 16859 + 4214 = 21073 | 388/1000 (>=354) |

The old IPC3 implementer-5 report records N=16 **net p99 only** at the same pinned workload: rv64 driver/timer 96,212/54,018 and rv32 86,550/84,591 µs, both failures against 50,000. Against those reported figures, candidate deltas are rv64 driver -78,181 µs (-81.26%), timer -32,868 µs (-60.85%); rv32 driver -64,025 µs (-73.97%), timer -59,860 µs (-70.76%). These are early directional comparisons, not the complete required paired baseline: full baseline gross/net distributions, counts and configuration fields are not yet verified from saved logs. The report gives old rv64 audits as 12,210 / about 10.80 s; that rounded value likewise cannot serve as a precise paired cost delta. No throughput-loss tolerance exists; useful-work/switch cost remains unmeasured at this checkpoint.

Stop here for Architect/orchestrator review. The unmodified latency fixture passed, but clustered-wake proof, negative control, slice-relative attack adaptation, broader seed sweeps, containment and final Tier A gates remain pending. No book claim should be updated from this early result alone.
