HOME1 handoff (beam17-implementer, resident beamlet member)

## Branch state
- wp-HOME1 in /home/mcloonan/redoubt/.worktrees/HOME1, off origin/main 44d970ab4. ALL WORK UNCOMMITTED (stage by path when committing; never add -A).
- Changed: servers/init/src/{manifest.rs,check.rs,refusal.rs}, servers/init/tests/manifest.rs, servers/steward/src/{own.rs,bin/steward.rs}, servers/steward/tests/own.rs, libs/fileserver/src/quota_tests.rs, tools/testbench/src/{disk.rs,case.rs,ssh.rs}, image/{manifest.json,disk.toml}, tests/data/{boot-profile/manifest-unverified.json,steward/vault-launch.manifest.json}, tests/steward-restart.toml (description arithmetic), docs/servers/{init.md,steward.md,walfsd.md}, docs/testbench.md, docs/userland/sessions.md. New: tests/data/init/overcommit.json, tests/init-refuses-overcommit.toml, tests/steward-home-quota.toml.

## Rulings (orchestrator)
1. Steward holds ONE home connection per principal (minted with Q at first need, kept for its life; init disconnects at its exit); each session new_connection(held, "", 0) shares Q. Done: How::Carved in own.rs, Machine::carve/fresh_carved, Via enum for release.
2. volumes[].bytes; init refuses a volume whose home quotas sum past it (or no bytes); packer refuses a recipe whose manifest bytes differ from the partition (disk.toml `manifest = "image/manifest.json"`), bench holds the same at case load (case.rs check). Done.
3. Sibling fields: home_quota (required with home), NO vault quota (ruled: drop label_sets[].quota; vault = its labelled volume bounded by room). init refuses a labelled volume two principals' label sets name (Why::SharedVault). Done.
- Plan node wording to report: home/vault lines parsed in servers/steward/src/own.rs and written by init check.rs steward_own_lines (not libs/steward manifest.rs); no vault quota.

## Done/passing
- cargo test -p redoubt-init -p redoubt-steward-server: pass. redoubt-fileserver quota tests pass (new sessions_minted_through_one_carve_share_its_quota_across_a_restart covers restart re-carve: probe's 14 s is too short for an SSH write on the bench). testbench disk:: case:: tests pass (image partition = 33,537,024 bytes).
- Bench: init-refuses-overcommit PASS rv64+rv32. docs check PASS. rustfmt done.
- steward-home-quota: first runs showed the quota works (alice SSH got enospc beside her console session's 5 MiB); IO.binwrite raises, so switched to :file.write; rerun on both widths in progress (log .tmp/HOME1/hq.log).
- Testbench gained session `name` (two sessions of one user); documented in testbench.md.

## Traps
- A principal's console + 2 SSH sessions don't fit its sub-budget (OutOfMemory): the case uses alice's console session (typed via [[input]]) + one SSH session.
- Session heap limit ~3 MB: write in 64 KiB chunks from one reused binary.
- Elixir IO.binwrite raises on error; use :file.write.

## Next
Case on both widths -> read committed files in full (Tier A) -> commit in logical groups (VM-free: init+steward+fileserver test; bench hold+session name; image values+cases+docs) -> gates: init/steward/walfsd host tests, steward-*/sshd-*/init-* cases + beamlet set (scripts/shell-cases origin/main) both widths, size/unsafe budgets, formatting, docs -> report as question to orchestrator with head and gates; detail in .wash/local/HOME1-report.md.
