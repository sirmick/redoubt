# SCHED1 implementer-6 handoff (2026-10-06)

## Branch state, traps first
- Worktree `/home/mcloonan/redoubt/.worktrees/SCHED1`, branch `wp-SCHED1`, tip **55af8fb8c**. It sits on IPC3's rebased tip `410b19d75`, which is on main `fb1f3a58f`.
- Commits, each signed off:
  1. `1c21c2ce5` testbench: prove the cluster, a waiting wake, a carve's return, a round
  2. `bbfdf13b5` kernel: use a one millisecond scheduling slice
  3. `8250df8dd` tests: the cluster case, its 10 ms control, and the 1 ms share cases
  4. `55af8fb8c` docs: the 1 ms slice and its cost, the cluster's envelope and control
- The worktree is clean. Nothing is pushed. Never push. The safety ref `wp-SCHED1-prefold` (39586d915) is kept until the merge.
- The review panel is complete:
  - red round 6 OK with notes, with its P2s folded;
  - editor OK on 2fae99293, plus a cosmetic rewrap folded in the rebase.
  - The red confirms the rebase delta.
- **Open: B11.**
  - init-boot fails on both widths: "beamlet: no heap record found".
  - The check is main's memory scan (`42bc99847`), which reads heap records from the stopped guest. rt writes beamlet's record at its startup. The guest stops at init's last line, and init starts beamlet last.
  - Release at 10 ms passed; release at 1 ms fails; checked at 1 ms and at 10 ms both pass (scratch, rv64).
  - It is an ordering accident; not SCHED1's code. A B11 implementer fixes it on main.
  - Evidence: `.wash/local/evidence/SCHED1/gate-rebase/REPORT.md` and `initboot-diag/`.

## Next, on the orchestrator's word, after B11 merges
1. Rebase once more onto main's tip: `git rebase --signoff --onto <main> 410b19d75` (or IPC3's new tip if IPC3 is rebased first; ask).
2. Rerun init-boot on both widths, and the smoke set (userland-boot, init-boot, bench-net-peer, ipc-outcomes) on both widths.
3. Report the tip, hunks and gate. SCHED1 then goes into train 3.

## Traps and how to work here
- **Env for every shell:** `. /tmp/s1-env.sh` (jobserver env, PATH, RUSTSBI_PROTOTYPER and _RV32, BEAMLET_TOOLCHAINS, unset TESTBENCH_QEMU_SEED). If /tmp was cleared, rebuild it from the brief.
- **Substring filters:** a case name is a substring filter.
  - `host-tests` runs every `*-host-tests` case. Run host tests as `jobserver bounded cargo test -p <crates>`.
  - `sched-latency` also runs `-tcg`. Use `--sweep 1..1` for one seed.
  - `sched-cluster-old-control` is `whole_run=false`, so it runs only by its exact name.
- **Pool:** it is often at 0 free. Long runs go detached: `setsid nohup <script> &`. Write each step's result to files and watch with Monitor's until-loop.
  - Tool background jobs die at 30 or 60 minutes, and killed makes may leak tokens.
  - Copy consoles out of `target/testbench/run-*` immediately; rotation prunes them. A watcher loop copying `run-*/*.log` every 10 s works.
- **Folding:**
  - Use `amend!` or `fixup!` commits, then `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash <base>`, and check the tree is unchanged.
  - To set WIP aside for a rebase, commit it as WIP and then `git reset HEAD~1`. Never stash.
  - To reword: `GIT_SEQUENCE_EDITOR="sed -i 's/^pick <sha>/reword <sha>/'" GIT_EDITOR="cp <msgfile>"`.
- **Formatting:** the formatting case uses nightly rustfmt (`rustfmt +nightly --edition 2021 --config-path rustfmt.toml <file>`). Plain `cargo fmt` uses stable and gives false diffs.
- **sched_oracle `round`** (debt-lift):
  - The guest destroys an empty marker budget before it makes S.
  - The first W after the marker's Y must be a new budget's (no W/R/D/K record before it; a P just before its W is fine).
  - That budget must be picked before any other budget is picked twice.
  - The one-round bound rests on `check_lift`. LIFT1 (on the plan) builds the construction that exercises it.

## Rulings in force
- **The 1 ms slice stays** (owner's decision). The per-switch cost is a stated residual. RECON1 fixes the queue walks (`raise_floor` runs 9 times per slice end; reconcile finding: `.wash/local/evidence/SCHED1/five-cases/reconcile-finding.md`).
- **Share fixtures:** judged by ratio of counts, with useful work printed beside them.
  - large-weight's server must be within 50 of 1000/1800 (555) either way, and each user at least 900/1000 of the users' mean.
  - server-busy compares the server with B and C, and B with C.
  - carve: the victim against all counts.
- **ties:** two destructions; clause 3 is checked in the guest, clause 2 is left to the oracle.

## Evidence index (.wash/local/evidence/SCHED1/)
- `five-cases/`: RESULTS.md (attribution), reconcile-finding.md, decompose.py, attribute.py
- `impl6-workorder-report.md`
- `r5-debt-lift-finding.md`
- `gate-fixtures/` (r5, r5b, r6)
- `gate-rebase/` (REPORT.md, consoles, initboot-diag)
