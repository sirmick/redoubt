# Orchestrator handoff, 2026-10-15 ~11:00Z (host clock; context at 94 %)

Workspace `7bce76dc…` (Wash), rebuilt 2026-10-14 from `.wash/` after the old one could not be attached.
Read `.wash/PROJECT.md` set-up, then this. The memory file
`~/.claude/projects/-home-mcloonan-redoubt/memory/parked-2026-10-01.md` has dated STATE lines for every
step of 10-14/15 (times before 07:38Z on 10-15 are ~8 h fast).

## State of main
- `origin/main = 3d5464c41` (train 28). Seven trains published since 10-14: 22 (B55 B58 NSCR2), 23 (B59 K33),
  24 (CONW2 B57), 25 (B48 B54 SCHED4), 26 (B52), 27 (B61 IDLE2 B63 B51), 28 (B62): 16 packages.
- Root checkout: `.wash/plan.toml` + `.wash/qa/*` modified (Wash's; commit as one `plan:` commit at the next
  publish or with `.wash/save.sh`).
- Train worktree `.worktrees/train-20` (detached, reuse, never rename), at 19d158892.

## Train 29: ASSEMBLED and ON THE BENCH (11:20Z)
- Head `142c4b5cb` in `.worktrees/train-20` = main 82b8c9bd9 + B60 94b26298b + B65 43c42bbdc (both accepted,
  trailers in the merge commits). Bench logs `.tmp/train-29/{prebuilt.log,bench.log}`, `DONE` marks the end.
- Then: copy any failing job log aside (`cp target/jobs/<w>-<case>.log .tmp/train-29/`), rerun fails alone
  (`make -k -j1 -f scripts/jobs.mk set CASES=...`), publish (root: `git merge --ff-only 142c4b5cb`, the SWARM
  publishing check, `git add .wash/plan.toml .wash/qa/ && git commit -m "plan: train 29 published (B60, B65)"`,
  `git push origin main`), remove `.worktrees/B60` `.worktrees/B65` and `wp-B60` `wp-B65` local+remote.

## (earlier notes on train 29's parts)
- **B60** ACCEPTED at `94b26298b` (wp-B60 on 3d5464c41; tests+docs: shell-text-profile case, shell.md numbers,
  trap sentence pointing at B65 as 'open'). Trailers: plan_accept B60 was called (gates: prebuilt;
  shell-text-profile + shell-output-rate both widths; docs). Reviewed-by: steward-red-2: OK with notes.
- **B65** reviewed OK with notes at `0cfde6a9d` (P3 folded); b65-implementer is rebasing it onto 94b26298b and
  rewriting shell.md's trap sentence as history; then plan_accept B65 (gates in .wash/local/B65-report.md:
  beamlet-vm+redoubt 190/190, ./test-shell, difftest 527, docs, fmt; the 64 MiB self-compare test 0.13 s,
  negative 40.6 s; machine prompt-walk 69→6.6 ms/step), merge B60 then B65 onto 3d5464c41 in train-20 with
  `Reviewed-by: steward-red-2: Merge verdict: OK with notes`, whole bench (`make -f scripts/jobs.mk prebuilt
  && make -k -j8 -f scripts/jobs.mk all-rv64 all-rv32`, logs under .tmp/train-29, detached via setsid), copy a
  failing job log aside BEFORE rerunning, rerun fails alone, publish (ff main, publishing check per
  SWARM 'Publishing', plan commit, push, remove merged worktrees+branches).
- **B53** `wp-B53 2abeb0728` (save.sh --local-only; reviewed OK twice) waits ONLY on the OWNER's approval,
  asked in prose several times, unanswered. Merge it only on the owner's yes.

## Open with the owner (ask again, in prose, not blocking decision_request)
1. B53 approval. 2. Handle generations (B50; Architect carries .wash/local/architect-questions.md).
3. Unpark SMP5? The kernel track has nothing launchable without 2/3.

## Members (all auto-approved; reviewers launched WITHOUT capability:"reviewer": WASH-R30)
architect (resident, idle), idle2-implementer (resident, idle; kernel track blocked), b60-implementer (resident,
B60 frozen), b65-implementer (resident, rebasing B65), kernel-red-2, steward-red-2 (reviewers; assign reviews as
assignments on the node; cc the implementer). Ephemeral members retire on task completion: use resident.
Members at ~70 % context: checkpoint (member_control checkpoint), end, relaunch with handoff_from.

## Candidates not opened
B64 (shell per-piece prologue), B65 follow-ons none. pipe-sixty-four stays quiet class (B62 report).

## Traps for the orchestrator
- plan_get/plan_set print the whole tree (~115 KB each): read node bodies from .wash/plan.toml; batch plan_set.
- Any package adding a kernel println! gates on beamlet-reduction-rate + beamlet-schedulers-* (CONW2's lesson).
- jobs.mk reruns overwrite target/jobs/<w>-<case>.log: copy it aside first.
- A stale .git/index.lock may block publishing: check /proc for git processes by cwd, then remove.
- Merge conflicts in tests/size-budget.toml between packages raising the same ceiling: resolve to the sum with a
  `Size budget:` line in the merge commit.
- save.sh refuses on dirty scratch worktrees under .tmp (B53 fixes that); check `git worktree list | grep .tmp`.
- Reviewers' turns ending without a report = their Wash calls refused (capability reviewers): never use the flag.

## PUBLISHED (12:10Z): train 29 is on origin/main 47b0d98ea. Nothing is launchable without the owner's decisions (B53, B50 handle generations, SMP5). B64 is a recorded candidate. Members left: architect, idle2-implementer (idle), kernel-red-2, steward-red-2.

## 13:15Z: B64 OPENED on the owner's standing "keep working through M2": b64-implementer (resident, auto) in .worktrees/B64 = wp-B64 from 47b0d98ea; steward-red-2 reviews (Tier A); report expected at .wash/local/B64-report.md. Then: review, plan_accept, train 30 (with B53 if the owner says yes), bench, publish. Owner decisions still open: B53, handle generations (B50), SMP5.

## 13:55Z: TRAIN 30 = dbd97b72b (47b0d98ea + B64 87ba3378e, trailers in the merge) ON THE BENCH; logs .tmp/train-30/{prebuilt,bench}.log, DONE marks the end, failing job logs copied to fail-<w>-<case>.log there. Then rerun fails alone, publish (ff root main, publishing check, plan: commit, push), remove .worktrees/B64 + wp-B64 local/remote, end b64-implementer.
