GATE1, parked 2026-10-01.

BRANCH: wp-gate1 at 1bd502c47, rebased on main a678d8928 (K15). Worktree clean. Six commits:
- the five earlier commits, rebased: 45978db43, cc6871fdd, 2b2777ff3, 5edd18aca, d05a8aebd
- 1bd502c47, which adopts K15's protocol. The driver gets p1=1. The steward stand-in holds Window{end,gross} for timer, decision and notice samples and calls hand_over after CT_VERDICT. The launcher prints gross lines and calls b.samples(d|s, words[..seen], "gate"). The program's own target rows, the 'target missed' forbid, the retired WAKE/DECISION/NOTICE consts and 'victims stay responsive' all go. post_check has sched-latency's bounds.

The WIP CT_SPLIT commit was dropped. origin/wp-gate1 is stale (old base); never push.

PASSED:
- rv64 seed 3: exit 0 in 225 s. Net in µs, p50/p99:
  - driver 8805/10687
  - timer 8045/9651
  - decision 8454/8543
  - deadline notice 21803/23545 (gross 54549 p99)
  - R10 21190/23874
  - lease end 32417
  All 10 rows ok, share 830 against floor 783.
- Sweep, rv64 seeds 1-12: all pass, notice p99 23545-23827, lease end 32329-32884. Table in .wash/local/GATE1-sweep.md, logs in .wash/local/GATE1-sweep-logs/.

LEFT:
- rv32 at seed 3
- the sweep: rv64 13-16, rv32 1-16; then pin the worst seed in tests/kernel-containment.toml (qemu_seed)
- the page: docs/kernel/README.md Containment goes from planned to built, its status line names bench:kernel-containment, and the sweep table goes under The run
- cargo run -q -p redoubt-doccheck
- cargo testbench --allow-skip (one SKIP, bench-ssh-loopback-openssh)
- ./build --arch rv32 --programs
- cargo +nightly fmt --all --check
- fold into logical commits

COMMANDS: run all of them from the worktree through /home/mcloonan/redoubt/.wash/local/in-dev. A seed: in-dev env TESTBENCH_QEMU_SEED=N cargo testbench kernel-containment --arch rv64|rv32. Read the post-check net lines from stdout.

TRAPS:
- Runs share target/, so run seeds serially. A 4-lane parallel plan with copied target dirs was denied.
- TaskStop leaves the testbench and QEMU running inside the container; kill them by PID.
- Each run takes about 4 min, and logs are about 25 MB, so tail them.
- K15-progress.md does not exist; I worked from the K15 merge diff and docs/testbench.md#checked-builds.
