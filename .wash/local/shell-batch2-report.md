# Shell batch 2 into main: gate report (2026-10-08)

Worktree .worktrees/shell-batch2, detached. Base origin/shell 33a11a2ca.

## The merge
`git merge --no-ff --no-commit origin/main` (8e50b5489): "Already up to date". origin/main
8e50b5489 is an ancestor of origin/shell 33a11a2ca (shell merged main in earlier), so there is
nothing to merge and no "merge main into shell" commit was made; no conflicts.

## The reflow
4562b4e5d "docs: the editor's paragraph on pasted keys, reflowed", on 33a11a2ca: the 156-column
line in docs/userland/shell.md's editor paragraph (the deaf-window sentence) reflowed to 100
columns; no wording changed. Two other lines in that section exceed 100 columns only because of
an unbreakable link (`Redoubt.Editor.Buffer`'s and `Redoubt.Editor.Manager`'s), left as they are.

## Gate on 4562b4e5d
- make prebuilt: rc=0 (rv64 237, rv32 223 cases built, 0 failed).
- set: the worktree's `./scripts/shell-cases origin/main`, now 15 cases (the shell set gained
  steward-context-login): beamlet-footprint beamlet-launch boot-profile boot-profile-unverified
  steward-context-login steward-restart-ssh steward-session-ends steward-ssh-idle
  steward-ssh-two-principals steward-sub-budget-flood steward-vault-launch steward-vault-session
  userland-bad-start userland-boot userland-read-only, on rv64 and rv32: 30/30 PASS; with the
  static checks formatting, no-cruft, size-budget, unsafe-budget: PASS (rv64; rv32 has no
  target for them). set rc=0, 34 targets rc=0.
- docs: PASS, rc=0.
- ./test-shell whole: rc=0, every stage passed (formatting, natives, beamlet, BEAM, entry point,
  fake kernel).
- full difftest (userland/otp tools/difftest under q, 8 cores): 527/527 passed, 0 failed, 21
  skipped by design; rc=0.
- beamlet-footprint: rv64 heap 5,459 of 11,885 pages, stack 35,880 B; rv32 heap 5,277 pages,
  stack 28,920 B.
Not pushed.
