2026-10-03T04:52:10Z B7 started: read SWARM sections, brief, build.rs/main.rs; warming build for reproduction
2026-10-03T04:54:31Z repro running: bench + handle-chain-fault loop; first wrong-kernel hit (budget-deadline rv32, handle.rs:461). Found: cargo JSON executable is the unhashed uplifted path; container pids are all ~1 so run-<pid> collides. build.build-dir split works on a fixture.
2026-10-03T05:13:24Z checkpoint: bench 13/282 wrong-kernel FAILs, loop 7/559; detail .worktrees/b7/target/b7-repro/checkpoint.md; asked orchestrator on executable path + run-<pid> collision
2026-10-03T05:49:10Z fix written (build-dir via --config, per-run target-dir, run.rs run dirs + flock + last link), host tests 60/60; env var form deadlocked test-programs' nested cargo, switched to --config; timing a warm one-case run
2026-10-03T05:59:12Z committed 013c70cf7 (fix + pages + host tests); fmt 0, doccheck 0, host tests 60/60, warm single case green; waiting for host for overlapped run (bench + loop) and clean gate bench
2026-10-03T08:30:58Z rebased onto caf6f37b5 (main.rs conflicts with recipe/listing resolved; run.rs test made robust to fork-shared lock) -> 8bdca01ce; overlapped run started (/tmp/b7-overlap-*.log)
2026-10-03T08:55:08Z overlap after fix: bench exit 0 (325 PASS, 0 FAIL, 1 SKIP openssh), loop 708 iterations 0 failed; gate bench started (/tmp/b7-gate-bench.log)
2026-10-03T09:16:26Z gate exit 0 (325/0/1); amended ddfdb029c (prune NotFound, KEEP comment, Architect's message edit); reported
2026-10-03T09:57:21Z rebased onto 57f4cf01f -> dd7db6bb4 (no conflicts); host tests 62/62, fmt 0; gate bench 2 running (/tmp/b7-gate-bench-2.log)
