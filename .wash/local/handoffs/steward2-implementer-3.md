# STEWARD2 handoff (steward2-implementer-3, second checkpoint, B18/B19 pause), 2026-10-06

## Branch state FIRST
- wp-STEWARD2 HEAD 9ae9ff6a8 = cbaa5b900 (14 logical commits on main bd6f768f6) + ONE fixup commit "fixup! image: ..." (tests/steward-sub-budget-flood.toml sequencing). Fold it into the image commit (last commit) before review: `git reset --soft HEAD~2 && git commit -C cbaa5b900` works since both are the last two commits. Tree clean, nothing pushed, no runs of mine alive. Local backup ref s2-wip-backup (old WIP head; delete when done).
- Not rebased onto FSN1 (littlefsd); waits for the orchestrator's word.
- Commit list and re-fold method: see .wash/local/handoffs/steward2-implementer-3.md (previous handoff) and .wash/local/STEWARD2-report.md section "steward2-implementer-3".

## TRAPS
- Machine is scheduled by q now (scripts/q, scripts/jobs.mk; RESUME-q.md); the B18/B19 resume note will change how cases run again: read it. /tmp/s2env.sh: MK now points at scripts/jobs.mk, jobserver line removed.
- A malformed tests/*.toml (e.g. a probe waiting on a mark nobody sets) breaks EVERY case run from the worktree. Probe file tests/zz-flood-probe.toml is deleted; don't commit probes.
- Changing image/manifest.json while a case runs fails its memory scan (artefact).
- Run dirs are cleaned by later runs: copy logs right after each case (/tmp/s2-run.sh does this per case; reuse it).
- Kill by PID/process group, never pkill a pattern from your own command line.

## Finding this session: case 4 (flood) was starvation, not a crash
Probe (logs /tmp/s2-keep/probe/): console session + three SSH logins booting VMs at once on one hart: in 700 s alice and bob reached only the shell banner, the vault session only beamlet's first line ("read from" matched 15.4 s after ssh started). The earlier "closed by remote host" was in the same overload. Fix (the fixup commit): logins sequenced with leading waits: bob -> prompt (mark bob-up) -> alice (wait bob-up) -> prompt (mark alice-up) -> vault (wait alice-up) floods, marks flooded -> bob and alice answer Enum.sum. timeout 2400. Not yet run. Worth telling the orchestrator: four VMs booting at once on smp1 get nowhere in 700 s (idle/booting VM CPU cost; beamlet is not ours).

## Next, in order
1. Clean reruns (copy logs per case): rv64 flood, two-principals, session-ends; rv32 flood, two-principals, vault-session. (Already passing: rv64 login-refused, vault-session, restart, image cases, steward-boot, elixir-oracles, host-tests cases, docs; rv32 login-refused, session-ends, restart, image cases, steward-boot.) A case failing beside other work is rerun with `q run --quiet -- cargo testbench --arch W case` before it counts; say so in the report.
2. Latency (orchestrator ask): case 2's session logs carry "[/read from/ matched X s ...]" and "[/\([0-9]+\)> / matched Y s ...]" for alice and bob, both widths: numbers in the report + one sentence (build, width) on sshd.md or steward.md (into the image commit), and reword the testbench commit message to mention the session log's timing note (replay method in previous handoff).
3. Fold the fixup; rerun unsafe-budget, size-budget, doccheck, fmt (all green at cbaa5b900).
4. Report via member_update (<=1900 bytes) with detail in .wash/local/STEWARD2-report.md: add K23 note (owner: K23 replaces the restart case's reboot with a restart that logs every session out), the flood starvation finding, case results with exit codes, latency.
5. FSN1 rebase on the orchestrator's word.

## What consumed my context
Bench waits, three fold passes, the sshd/steward/R25 debugging chain, memory scans, doc fixes, two checkpoints.
