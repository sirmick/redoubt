# STEWARD2 handoff (steward2-implementer-3 -> next, ~85 %), 2026-10-06

## Branch state FIRST
- wp-STEWARD2 HEAD d9627acc8: 14 logical commits on c8cd27ba9 (the orchestrator's rebase target; main has moved on since: keep folding with `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash c8cd27ba9`, never onto newer main without the orchestrator). No fixups pending, tree clean, nothing pushed. Local ref s2-wip-backup is the pre-rebase WIP head (delete when accepted).
- Rebase hunk log (every resolution + post-rebase adaptations): .wash/local/STEWARD2-rebase.md. Report detail: .wash/local/STEWARD2-report.md (pre-rebase results; needs the post-rebase section).
- Two commit MESSAGES still to reword (replay method: temp worktree at parent, `cherry-pick -n`, `commit -F`, cherry-pick the rest, `git diff --quiet old new`, `reset --hard new`): bootfsd commit (also erofsd 1.5 MiB; retitle) and the image commit (alice 43,528 / sessions 10,881 per BEAM8's 10,880; littlefsd/erofsd names; endpoint=; buckets=5 at littlefsd:data and erofsd:system; erofsd heap 26; main's image cases re-aimed; new peaks; latency 4.5/3.1 s rv64, 2.5/3.5 s rv32; flood via heap lists). Its Size budget lines stay.

## Gate status at d9627acc8 (prebuilt index current; run cases with `make -k -f /home/mcloonan/redoubt/scripts/jobs.mk -C <wt> rv64/<case>`; env /tmp/s2env.sh; after ANY edit run `$MK prebuilt` again)
PASS (rv64 and rv32 unless noted): steward-boot, init-boot, userland-boot, userland-read-only, userland-bad-start, verity-flipped-tree, verity-wrong-root, verity-signed, verity-bad-signature, verity-rollback, image-disk, beamlet-footprint, bench-net-peer, ipc-outcomes (smoke set complete), steward-restart, steward-login-refused, steward-session-ends, steward-vault-session, steward-ssh-two-principals, steward-sub-budget-flood rv64; boot-profile-unverified rv64; elixir-oracles, steward/init/sshd/wire host-tests, docs, unsafe-budget, size-budget, formatting. Host tests (cargo test -p steward-server, steward, init, sshd, sha256, testbench, erofsd): pass.
FAIL:
1. steward-sub-budget-flood rv32: the flooding process's collector asks one 10.4 MB block (about 4x the 680-page process limit) and the session VM (10,880 pages, heap cap 10,862, shell loaded) lacks it -> VM ends (`memory allocation of 10420992 bytes failed`). rv64 passes. Options to put to the orchestrator: raise sizes.session (e.g. 16,384; then alice's top 4 x 16,386 = 65,544, and re-measure), or accept on rv32 with a residual, or BEAM's sizing. Not decided.
2. boot-profile rv64, rv32 and boot-profile-unverified rv32: the 15 s prompt target. Measured rv64 first console read 15.075 s: init's push of beamlet's public entry 0.6->3.0 s, the steward's carve + streamed launch 3.0->7.6 s, the shell 7.6->15.1 s. QUESTION SENT to the orchestrator (msg c4a92806): (a) restate the target to the steward path, (b) keep 15 s + follow-up, (c) both (recommended). Awaiting answer.

## Memory (post-rebase scans, all within 2x; table updated in docs/testbench.md)
keyd stack 7,368; erofsd heap 13 (cap 26); verity 8,264/50; sshd heap 56 (cap 384); steward 13,224/14; rest as table.

## Latency (in sshd.md, image commit): ssh start -> VM's first line 3.4/2.1 s rv64, 1.4/2.4 s rv32; -> prompt 4.5/3.1 s rv64, 2.5/3.5 s rv32 (alice/bob), host clock.

## Next
1. Orchestrator's answers on the flood size and boot-profile target; apply; prebuilt; rerun the failing ones both widths (+ ssh-two-principals rv64 for scan if session size changes, and the memory cases).
2. Reword the two messages; final short gate (docs, formatting, unsafe-budget, size-budget); append post-rebase results to STEWARD2-report.md; report the head to the orchestrator (who launches the panel).

## TRAPS
- Never `git add -A`; stage by path.
- Tree edits stale the prebuilt index (cases then report stale); hold edits while a run is queued.
- q daemon restarts kill waiting jobs (rc=3, 'daemon went away'): rerun those.
- Copy run dirs right after a case (they are cleaned).

## What consumed my context
The rebase and its adaptations, the gate runs, earlier debugging.
