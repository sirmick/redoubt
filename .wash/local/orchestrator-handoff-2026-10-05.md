# Orchestrator handoff, 2026-10-05 (graceful stop)

Owner's word (2026-10-04 ~22:40): "finish up tool1, push and bring the project to a graceful
stop." Done. Do not start packages until the owner asks for development.

## Published

- `main` = `37ae709ed` = `origin/main` (DOC1 `37ae709ed`, docs only: GETTING-STARTED's
  "On a machine without apt" block, editor OK, doccheck run natively on the pinned 1.99.0).
  Before it, `34a2d9981`. Today's package merges, all pushed:
  - **BENCHENV1** `b15ecfba6`: OpenSSH's reference server runs in a per-session riscv64 Linux
    guest under `qemu-system-riscv64` (recipe `tests/ssh-reference/guest.toml`, snapshot-pinned);
    no container runtime anywhere. Whole bench rv64 226/0/0, rv32 211/0/0, no `--allow-skip`.
    QA `BENCHENV1-container-permissions` resolved. Report: `.wash/local/BENCHENV1-report.md`.
  - plan snapshot `551b29943` (the day's QA threads).
  - **TOOL1** `34a2d9981`: `scripts/setup.sh` (any apt system; `--with-beam`), root
    `rust-toolchain.toml` 1.99.0, Dockerfile 44 lines (debian:trixie + user + the script, no
    backports), dev.sh 91 lines (mounts the checkout at /work and at its host path), agent
    tooling and `scripts/{pi-ensure,ssh-key-ensure}.sh` gone, bench runs on QEMU 8.2 / OpenSSH
    9.6 (parent-death signal instead of `-run-with`; `WarnWeakCrypto` only when `ssh -G` takes
    it), `openssh-server` no longer a host prerequisite. Owner ran the script on this host
    (Ubuntu 26.04, exit 0, "lgtm"). Report: `.wash/local/TOOL1-report.md`.
- Remote branches: `main`, `wash-local`, `wp-BEAM7`, `wp-MEM1`, `wp-SCHED1`, `wp-ipc3`. Stale
  `wp-gate1/init1/k13/steward0/SHELL1` deleted with the owner's yes.

## Parked work (no member running)

- **SCHED1** `wp-SCHED1`: the implementer's dirty snapshot committed as a WIP by the orchestrator
  (top commit "WIP (paused 2026-10-05)"); fold or drop on resume. Design: the v3 consistent
  kernel window (`.wash/local/SCHED1-consistent-window-proposal.md`, accepted for source
  preparation only). QA `IPC3-wake-latency` open, blocking; owner: fix the scheduler before IPC3
  merges, targets unchanged. **IPC3** `wp-ipc3 a0fbbcab1` unmerged, needs SCHED1.
- **MEM1** `wp-MEM1 cd73175b5` (reported): six final scans PASS; needs its final-head whole bench
  (the reference case now runs), panel renewal, acceptance. QA `MEM1-runtime-stack` open.
- **BEAM7** `wp-BEAM7 6a79d0e13`: panel done, needs its whole bench rerun (only the reference
  case failed before) and acceptance.
- **B8** (aio-many-reads-two rv32 flake, pre-existing), **B9** (keeper host test races /proc under
  load): todo, small.
- Ready and briefed but not started: MEM2, VOL1, BEAM3, K19, SMP1, BEAM6.

## Environment notes

- Workspace `c7d0423bb3c3a1a44703a0a409b0e0c6`, catalog `anthropic-pro`. The catalog's `coding`
  slot resolves to `claude-opus-5-5`, which the adapter rejects: launch implementers/red with
  `model: "opus"` until the Wash catalog is fixed (owner said "let's discuss"). Reviewer
  capability is unsupported on claude-agent-acp 0.85.1 (verified 0.81.x only): reviewers are
  instruction-restricted; say so.
- `.wash/local/in-dev` updated for the new dev.sh (no sudo). The shared `redoubt-dev` image is
  still rev 4: the next `./dev.sh` rebuilds it (OTP from source, minutes). Native runs now work
  on this host after `scripts/setup.sh`; `scripts/build-bios.sh` at the root is still needed
  (the owner built firmware only in the TOOL1 worktree, since removed).
- `.worktrees/redoubt-config/` is a stray config dir made by the OLD dev.sh when run from a
  worktree (pi settings, a generated SSH identity `ssh/id_ed25519`, gitconfig). Not Redoubt's;
  the owner decides whether to delete it.
- Rules learned today: one whole bench at a time, nothing else building (the owner's setup.sh
  run overlapped reruns and inflated a flake rate); the whole bench must run on the final head
  when the bench's own code changed in a fold.
