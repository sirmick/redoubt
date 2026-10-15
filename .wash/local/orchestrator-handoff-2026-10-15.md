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

## PUBLISHED 14:40Z: train 30 on origin/main c8422f745 (B64). Bench 649 pass; steward-model-host-tests (steward_noninterference, 35 s host deadline) failed under load, passed alone. .worktrees/B64 and wp-B64 removed; save.sh pushed wp-* and wash-local. Members: architect, idle2-implementer, kernel-red-2, steward-red-2 (b64-implementer ended). Nothing launchable without the owner: B53, handle generations (B50), SMP5.

## 15:20Z: IN FLIGHT B50, B66, B67, B68 (worktrees .worktrees/<id>, branches wp-<id> from c8422f745; reviewers in the nodes). B69, B70 drafted by the Architect, next to launch. Trains: merge accepted heads in train-20, one whole bench per train (<=3 packages), publish, save.sh. B66 may hold port 2222 with a launch QEMU: kill stale ones by pid.

## 15:45Z: B67 ACCEPTED at wp-B67 850b57044 (trailers: Plan-Node: B67 / Gates: q run -- cargo test -p testbench (197 pass) 0, make -j4 -f scripts/jobs.mk set CASES=launch-system,steward-ssh-two-principals,bench-ssh-loopback WIDTHS=rv64,rv32 0, make -f scripts/jobs.mk docs 0, cargo +nightly fmt --check 0 / Reviewed-by: kernel-red-2: Merge verdict: OK with notes). Waits for train 31 companions (<=3 packages). B70 opened.

## 16:05Z: TRAIN 31 = 70148c97b (c8422f745 + B67 850b57044 + B66 2855ee3eb, trailers in the merges) ON THE BENCH; logs .tmp/train-31, DONE marks the end, fail logs copied to fail-<w>-<case>.log. Then rerun fails alone, publish (ff root main, check, plan: commit, push, save.sh), remove .worktrees/B66 .worktrees/B67 + wp-B66 wp-B67, end b66/b67 implementers. Note: other members run gates during this bench; timing flakes rerun alone.

## 16:25Z: B68 ACCEPTED at wp-B68 08355c35d (Plan-Node: B68 / Gates: q run -- cargo test (userland/otp/crypto host tests 11/11: 8 MiB and one-bit-over operands refused <10 ms; BEAM values for in-bound calls) 0, q run -- tools/difftest cryptotests (6/6 vs BEAM) 0, make -f scripts/jobs.mk prebuilt (rv32 284 cases, 0 failed; at ec81dd0ec, fold changed no interface) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check; git diff --check 0 / Reviewed-by: steward-red-2: Merge verdict: OK (B68 renewed at 08355c35d)). Train 32 candidate with B50 (folding doc notes).

## 16:50Z: B70 ACCEPTED at wp-B70 089a17c37 (Plan-Node: B70 / Gates: q run -- cargo test -p redoubt-rt; 20-run loaded loop 0/20 (load 24-27), 20 alone 0/20 at 0.04 s 0, q run -- cargo test for the fake's other users (fileserver, client, littlefsd, bootfsd, erofsd, keyd); rt fuzz crate nightly check 0, q run --cores 4 -- cargo test -p beamlet-redoubt --features fake --test system (26 pass) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check 0 / Reviewed-by: steward-red-2: Merge verdict: OK with notes). Train 32 = B68 + B70 + B50 (when folded).

## PUBLISHED 17:00Z: train 31 on origin/main 3b5e5e975 (B67, B66); bench 650 pass, rc 0. B66/B67 worktrees+branches removed, implementers ended. NEXT: train 32 = B68 08355c35d + B70 089a17c37 + B50 (after its fold) onto 3b5e5e975 in train-20; B69 may join or ride train 33.

## 17:15Z: B71 opened (b71-implementer, wp-B71 from 3b5e5e975). When B69 merges: tell b71 to rebase onto main and take the shell rates (3 runs alone, both widths), then review.

## 17:40Z: B50 ACCEPTED at 6d92ddb3e. TRAIN 32 = 41d499af3 (3b5e5e975 + B68 08355c35d + B70 089a17c37 [SUMMARY.md conflict: both todo lines removed] + B50 6d92ddb3e) ON THE BENCH; logs .tmp/train-32, DONE marks the end. Then publish, remove .worktrees/B50 B68 B70 + branches, end b50/b68/b70 implementers. B69 folding+rebasing onto 3b5e5e975 (train 33 with B71 when ready).

## 17:45Z: B69 ACCEPTED at wp-B69 efdeb40ed (Plan-Node: B69 / Gates: q run -- ./test-shell both VMs (beamlet 366/0; every stage); cells host tests (vectors 34->51); beamlet-screen 25+10 0, make -j4 -f scripts/jobs.mk set CASES=nscr-hostile-text,nscr-beyond-cells,shell-output-rate,shell-text-profile WIDTHS=rv64,rv32 (rates x3 alone, every figure at or above B64's, floors unmoved) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check; mix format --check-formatted 0 / Reviewed-by: steward-red-2: Merge verdict: OK with notes). Train 33 = B69 + B71 (b71 told to rebase onto efdeb40ed for its rates), after train 32 publishes.

## PUBLISHED 18:10Z: train 32 on origin/main 985ec0047 (B68 B70 B50); 652 pass rc 0. B50/B68/B70 worktrees+branches removed, implementers ended. NEXT: train 33 = B69 efdeb40ed + B71 (rebasing onto B69; rates now that the machine is free; then steward-red-2 review). Architect drafting beamlet-budget-from-startup, qmp-socket-private-dir, kernel-attack-gaps per page. save.sh at the next publish.

## 18:25Z: IN FLIGHT B71 (train 33 with B69 efdeb40ed), B72, B73, ATK2, ATK3 (worktrees .worktrees/<id>, wp-<id> from 985ec0047). ATK4 drafted, not launched (max_active 5). kernel-red-2 reviews B73/ATK2/ATK3/ATK4; steward-red-2 B71/B72.

## 18:50Z (host clock reads 16:47Z in START: the date -u stamp, the host is ~2 h behind my notes; use file mtimes): B71 ACCEPTED 1c2828ae9. TRAIN 33 = 8cd6a7516 (985ec0047 + B69 efdeb40ed [SUMMARY.md todo-list conflict resolved: all three lines go] + B71 1c2828ae9) ON THE BENCH; logs .tmp/train-33. Then publish, remove .worktrees/B69 B71 + branches, end b69/b71 implementers. TRAP: `cmd && setsid nohup x &` backgrounds the whole chain; wrap the setsid in ( ... &) instead.

## 19:05Z: B73 in review (kernel-red-2); ATK4 opened (atk4-implementer). ATK2/3/4 all edit docs/todo/kernel-attack-gaps.md (own sections): expect a text conflict at the train; resolve by keeping all three sections' edits.

## 19:10Z: B73 ACCEPTED at wp-B73 18969e24b (Plan-Node: B73 / Gates: q run -- cargo test -p testbench (199 pass incl. a_qmp_socket_is_private_and_binds) 0, make -f scripts/jobs.mk prebuilt (rv64 299, rv32 285, 0 failed) 0, make -j4 -f scripts/jobs.mk set CASES=consrelay-footprint,init-boot,beamlet-footprint,steward-ssh-two-principals,steward-session-ends,userland-read-only,userland-boot WIDTHS=rv64,rv32 (14/14) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check; git diff --check 0 / Reviewed-by: kernel-red-2: Merge verdict: OK). Train 34 candidate (with B72 / ATK*).

## 19:45Z: ATK3 ACCEPTED at wp-ATK3 0e650b847 (Plan-Node: ATK3 / Gates: make -f scripts/jobs.mk prebuilt (rv64 302, rv32 288) 0, make -j4 -f scripts/jobs.mk set CASES=device-object-page,dma-max-runs,device-owner-destroy WIDTHS=rv64,rv32 (x3: 18/18) 0, make -j4 -f scripts/jobs.mk set CASES=<device/DMA/IRQ + init-*/loader-*/rustsbi-boot, 41 cases> WIDTHS=rv64,rv32 (79/79) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check (three plants each caught, reverted) 0 / Reviewed-by: kernel-red-2: Merge verdict: OK). Train 34 = B73 + ATK3 (+ B72 after renewal), after train 33 publishes.

## 20:20Z: ATK2 ACCEPTED at wp-ATK2 877a78c4a (Plan-Node: ATK2 / Gates: make -f scripts/jobs.mk prebuilt 0, make -j4 -f scripts/jobs.mk set CASES=<7 new: sched-round-robin,sched-irq-wake,sched-deadline-preempt,call-bounds,bench-no-trace,bench-trace-scan-catches,bench-test-only-refused> x3 0, make -j4 -f scripts/jobs.mk set CASES=<sched-latency(+tcg),sched-carve-return,sched-destroy-billing,sched-wake-no-preempt,sched-ties> both widths (destroy_call p99 51 ms met) 0, q run -- cargo test -p testbench (203 pass); docs; fmt (plant found at 0x37ba1/0x3b69e; three mutations each caught) 0 / Reviewed-by: kernel-red-2: Merge verdict: OK). TRAIN 34 = B73 18969e24b + ATK3 0e650b847 + ATK2 877a78c4a (merge ATK3 first, then ATK2; devices.md/SECURITY.md R5 rows + kernel-attack-gaps.md conflict: take ATK2's text, which was written as the merge of both). B72 rides train 35 after its renewal.

## 20:35Z: B72 ACCEPTED at wp-B72 5123ddfce (Plan-Node: B72 / Gates: consumer sweep: q run -- cargo test for every bin linking redoubt_rt (28 root crates, beamlet-redoubt, beamlet-vm); build-rv64 and build-rv32; rt fuzz nightly check 0, make -f scripts/jobs.mk prebuilt 0, make -j4 -f scripts/jobs.mk set CASES=init-boot,launch-system,beamlet-footprint,steward-home-quota WIDTHS=rv64,rv32 0, make -f scripts/jobs.mk docs; formatting; size-budget (libs/client 1118, libs/rt 3943, libs/wire 3908, servers/init 2559) 0 / Reviewed-by: steward-red-2: Merge verdict: OK with notes (B72 renewed at 6e33ab324)). Train 35 = B72 (+ ATK4 when accepted); size-budget conflicts with other packages: sum.

## PUBLISHED 20:50Z: train 33 on origin/main 3a80e5af0 (B69, B71); 652 pass. B69/B71 worktrees+branches removed, implementers ended.
## TRAIN 34 = dfa6d8875 (3a80e5af0 + B73 18969e24b + ATK3 0e650b847 + ATK2 877a78c4a; R5 rows resolved: ATK2 text + ATK3 bullets) ON THE BENCH; logs .tmp/train-34. Then publish, remove .worktrees/B73 ATK3 ATK2 + branches, end b73/atk3/atk2 implementers. Train 35 = B72 5123ddfce (+ ATK4).

## 21:20Z: IN FLIGHT ATK6 ATK7 ATK8 ATK9 (wp-* from 3a80e5af0; all edit own sections of docs/todo/kernel-attack-gaps.md: expect adjacent-line conflicts at trains, keep every section's edits). ATK4 OK conditional on prebuilt. ATK5 drafted (needs ATK4): launch on wp-ATK4's head after acceptance. kernel-red-2 is the single reviewer for all: serialize reviews, one at a time.

## 21:30Z: ATK4 ACCEPTED at wp-ATK4 d7cfe6225 (Plan-Node: ATK4 / Gates: make -f scripts/jobs.mk prebuilt (rv64 307, rv32 296, 0 failed) 0, make -j4 -f scripts/jobs.mk set CASES=<11 new boot cases> WIDTHS=rv64,rv32 (x3: 66/66; 11 negatives shown once on rv64) 0, make -j4 -f scripts/jobs.mk set CASES=<init-boot,init-reboot,rustsbi-boot,rng,device,bundle-mapped,6 verity> WIDTHS=rv64,rv32 (23/23) 0, q run -- cargo test -p testbench (host-tests, firmware fallback); doccheck; formatting; git diff --check 0 / Reviewed-by: kernel-red-2: Merge verdict: OK (conditional prebuilt gate met: rv64 307, rv32 296); Wash truncated the trailer with an ellipsis: write it as that). TRAIN 35 = B72 5123ddfce + ATK4 d7cfe6225 (+ ATK5 if accepted in time; ATK5 is based ON d7cfe6225, merge ATK4 first). ATK5 opened on wp-ATK5 from d7cfe6225.

## 21:40Z: TRAIN 34 FAILS rv64 kernel-containment on ATK2's destroy_call gate (log .tmp/train-34/fail-rv64-kernel-containment.log). Fix in flight on wp-ATK2 (amend of 877a78c4a); renewal; reassemble train 34 = B73 + ATK3 + ATK2-new onto 3a80e5af0 (R5 conflict as before). Bench left to finish for any other failure.

## 21:55Z: OWNER PLAN CHANGE APPLIED (.wash/local/plan-2026-10-15-agent0.toml, rev 193): M2 done; SMP5 parked; ATK2-4 + B72 land as they are; NO further kernel-attack-gaps package (ATK5-9 withdrawn: members ended, worktrees/branches removed, drafts in .wash/local/atk5-9-drafts-withdrawn-2026-10-15.toml); AGENT0 step under AGENTS with AGT1 (Tier A, steward-red-2), AGT2 (Tier B, steward-red-2), AGT3 (needs AGT1). Briefs .wash/local/AGT{1,2,3}-implementer.md are the owner's: launch texts point at them. AGT1 + AGT2 launched from 3a80e5af0; AGT3 after AGT1 lands. Train 34 (B73 ATK3 ATK2) awaiting ATK2's destroy_call fix; train 35 = B72 + ATK4. CHECK .wash/local for owner files each wake (ls -t).

## 22:20Z: ATK2 RENEWED OK at wp-ATK2 0396a6edd (DESTROY_CALL tag 7->10; the train 34 failure). Trailer for the merge: Reviewed-by: kernel-red-2: Merge verdict: OK (renewed at 0396a6edd); gates as before + kernel-containment both widths. REASSEMBLE train 34 = 3a80e5af0 + B73 18969e24b + ATK3 0e650b847 + ATK2 0396a6edd when .tmp/train-34/DONE appears (R5 rows: ATK2 text + ATK3 three bullets, as in dfa6d8875: `git checkout dfa6d8875 -- docs/SECURITY.md docs/kernel/devices.md` during the conflict is NOT right if ATK2 changed them again; re-resolve by hand). Logs .tmp/train-34b.

## 22:35Z: TRAIN 34 REASSEMBLED = a43b72b13 (3a80e5af0 + B73 + ATK3 + ATK2 0396a6edd; R5 rows from dfa6d8875) ON THE BENCH; logs .tmp/train-34b. First bench (dfa6d8875) had only the two kernel-containment failures (tag collision, fixed). Then publish, remove .worktrees/B73 ATK3 ATK2 + branches, end b73/atk3/atk2; then train 35 = B72 5123ddfce + ATK4 d7cfe6225.

## 23:00Z: AGT2 in review (4af218700). BLOCKER for AGT3/AGENT0: `defmodule` on the machine faults the session VM (AGT2 report; logs .worktrees/AGT2/.tmp/AGT2/run*); Architect drafting the node (under AGENT0), AGT3 needs it. Launch it as soon as drafted (steward-red-2 if a limit moves).

## 23:10Z: train 34b: rt-miri failed once (B70 test under Miri, STUCK guard; log .tmp/train-34b/fail-rv64-rt-miri.log). Rerun alone on a43b72b13 when DONE (`make -k -j1 -f scripts/jobs.mk set CASES=rt-miri WIDTHS=rv64`); alone-pass = publish train 34, bug node open (Architect drafting).

## 23:20Z: AGT4 opened (agt4-implementer, wp-AGT4 from 3a80e5af0; brief .wash/local/AGT4-brief.md). AGT3 needs AGT1 + AGT4. rt-miri node still being drafted by the Architect.

## 23:35Z: B74 opened (b74-implementer, wp-B74). Train 34 may publish with rt-miri passing alone (B74 open covers the race). IN FLIGHT: AGT1, AGT2 (renewal pending), AGT4, B74. AGT3 after AGT1 + AGT4.

## PUBLISHED 00:20Z (10-16): train 34 on origin/main 9695a51af (B73, ATK3, ATK2); 671 pass + rt-miri alone pass (B74 open on its race). B73/ATK3/ATK2 removed, implementers ended.
## TRAIN 35 = 71279703e (9695a51af + B72 5123ddfce [SUMMARY.md todo conflict: all lines go] + ATK4 d7cfe6225) ON THE BENCH; logs .tmp/train-35. Then publish, remove .worktrees/B72 ATK4 + branches, end b72/atk4 implementers. IN FLIGHT: AGT1 (gates), AGT2 (reference-auth fold -> renewal), AGT4 (per ruling), B74. AGT3 after AGT1 + AGT4.

## 00:35Z: AGT2 ACCEPTED at wp-AGT2 565b9cd37 (Plan-Node: AGT2 / Gates: q run -- ./test-shell both VMs (beamlet 377/0, BEAM 308; every stage; 10 machine-mode tests incl. the forgery test) 0, make -f scripts/jobs.mk prebuilt (rv64 300, rv32 285, 0 failed) 0, make -j4 -f scripts/jobs.mk set CASES=shell-machine WIDTHS=rv64 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check; mix format 0 / Reviewed-by: steward-red-2: Merge verdict: OK (AGT2 renewed at 565b9cd37)). Train 36 candidate with AGT1 (+ B74 / AGT4). agt2-implementer kept resident until merged (AGT3 may want the case switched to agent).

## 00:50Z: REVIEWER ROTATION: steward-red-2 -> steward-red-3 (handoff_from; read-only; results via assignments only). AGT1 in review with steward-red-3. Members now: architect, kernel-red-2, steward-red-3, agt1/agt2/agt4/b74 implementers.

## 01:00Z: AGT1 ACCEPTED at wp-AGT1 0771a430f (Plan-Node: AGT1 / Gates: make -f scripts/jobs.mk prebuilt (rv64 300, rv32 286, 0 failed); build-rv64; build-rv32 0, make -j4 -f scripts/jobs.mk set CASES=<agent-login (new), image-disk, launch-system, steward-ssh-two-principals, shell-commands, userland-boot, init-boot, +12> both widths 0, q run -- cargo test (walfsd, fileserver, host, init, steward); docs; formatting; no-cruft; size-budget; unsafe-budget (counts unchanged) 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes). TRAIN 36 = AGT1 0771a430f + AGT2 565b9cd37 (+ B74 if accepted) after train 35 publishes. AGT3 launches when AGT4 is accepted (brief .wash/local/AGT3-implementer.md; branch from the train carrying AGT1+AGT4 or main after).

## PUBLISHED 01:20Z: train 35 on origin/main 32a448875 (B72 startup-block budget, ATK4 boot cases + device_tree knob); 690 pass + aio-many-reads-two alone 0.8 s both widths (load flake). B72/ATK4 removed, implementers ended.
## TRAIN 36 = 32a448875 + AGT1 0771a430f + AGT2 565b9cd37 ON THE BENCH; logs .tmp/train-36. Then publish, remove .worktrees/AGT1 AGT2 + branches, end agt1/agt2 implementers. AGT3 launches after AGT4 accepted (from main carrying AGT1).

## 02:20Z: TRAIN 36 FAILS beamlet-footprint rv64 on AGT2 (+64 pages at the prompt; log .tmp/train-36/fail-rv64-beamlet-footprint.log). Fix on wp-AGT2 (amend, rebased onto 32a448875), renewal by steward-red-3, reassemble train 36 = 32a448875 + AGT1 0771a430f + AGT2-new. Lesson: every shell-track package runs beamlet-footprint both widths before acceptance (add to every shell launch text).

## 02:40Z: B74 ACCEPTED at wp-B74 c7e51670e (Plan-Node: B74 / Gates: rt-miri alone 20/20 and beside load 20/20 (incl. the new held_clock test); B70's loaded loop 0/20 + 40/40 0, q run -- cargo test -p redoubt-rt; the fake's 12 dependents (fileserver, client, littlefsd, bootfsd, erofsd, keyd, consoled, consrelay, ipd, piped, sshd, walfsd) 0, q run --cores 4 -- cargo test -p beamlet-redoubt --features fake --test system (27 pass) 0, make -f scripts/jobs.mk docs; cargo +nightly fmt --check; clippy (fake) clean; unsafe libs/rt/src 18 unchanged 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes). TRAIN 36 (reassembled) = 32a448875 + AGT1 0771a430f + AGT2-new + B74 c7e51670e, once AGT2's footprint fix is renewed.

## 03:10Z: train 36 also failed job-interrupt-native rv64 (450 s; AGT2 driver suspect; log .tmp/train-36/fail-rv64-job-interrupt-native.log). agt2 told to run the interrupt cases alone and fix in the same amend if real. Reassemble train 36 only after both are renewed.

## 04:10Z: AGT4 ACCEPTED at wp-AGT4 9dfc1c743 (Plan-Node: AGT4 / Gates: make -f scripts/jobs.mk prebuilt (rv64 320, rv32 309, 0 failed); redoubt-vendor-check 0, make -j4 -f scripts/jobs.mk set CASES=<shell set (60) + size-budget> both widths 119/119; shell-compile-module x5 both widths 0, beamlet-compile-footprint (heap 9,120/8,779 of 11,092; stack 34,280/25,832 B), beamlet-compile-flood, beamlet-footprint (5,522/5,343) both widths 0, q run -- cargo test -p beamlet-vm; -p testbench; docs; cargo +nightly fmt --check; git diff --check 0 / Reviewed-by: steward-red-3: Merge verdict: OK). Rides train 37 after a rebase onto train 36's main (then deletes AGT2's shell.md residual paragraph). AGT3 launched on a scratch base (AGT4 + AGT1); rebases onto main after trains 36/37.

## 04:45Z: TRAIN 36 REASSEMBLED = e19e31528 (32a448875 + AGT1 0771a430f + AGT2 a1c21d0f3 [on AGT1; session 11,392] + B74 c7e51670e), first bench (8adb92667) stopped at 630 with the two explained fails (footprint cap, interrupt load flake). ON THE BENCH; logs .tmp/train-36b. Then publish, remove .worktrees/AGT1 AGT2 B74 + branches, end agt1/agt2/b74 implementers; tell agt4 to rebase onto the new main and delete AGT2's shell.md residual; tell agt3 to rebase once train 37 publishes. TRAP: pkill -f with a string in your own command line kills your shell: kill by /proc cwd with explicit pids, excluding $$ and $PPID.

## 05:45Z: train 36b: beamlet-lookup-host failed once (console suite; alone 14/14; log .tmp/train-36b/fail-rv64-beamlet-lookup-host.log); b74 looping it under load; rerun alone on e19e31528 after DONE, publish if clean. B74 gate gap: the fake's users include beamlet-redoubt --test console (add to any future fake change's gates).

## 06:00Z: AGT3 ACCEPTED at wp-AGT3 df6003eec on scratch base 326b6dca1 (Plan-Node: AGT3 / Gates: make -f scripts/jobs.mk prebuilt 0, make -j4 -f scripts/jobs.mk set CASES=shell-compile WIDTHS=rv64,rv32 (15 printed values identical both widths) 0, make -f scripts/jobs.mk docs 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes). ALL FOUR AGENT0 PACKAGES ACCEPTED. TRAIN 37 = AGT4 (rebased onto train 36 main, deleting AGT2 shell.md residual) + AGT3 (rebased content-only; range-diff renewal). Then AGENT0 step done; AGENTS/STEWARD3 is next on the plan (owner's M3 order).

## 06:15Z: STEWARD3 wave launched: steward3r-implementer (wp-STEWARD3R from 32a448875; Architect rules R41 on QA thread STEWARD3R-scope first), steward3a-implementer (wp-STEWARD3A from e19e31528 = train 36 head), b75-implementer (wp-B75 from 32a448875). 3B/3C launch when 3A is accepted. The Architect wrote steward-red-2 as reviewer: it is steward-red-3.

## PUBLISHED 06:30Z: train 36 on origin/main 0b487325b (AGT1 agent principal, AGT2 machine mode + session 11,392, B74 quiescent tick); 693 pass + beamlet-lookup-host alone. AGT1/AGT2/B74 worktrees+branches removed, implementers ended. NEXT: train 37 = AGT4 (rebasing onto 0b487325b; deletes AGT2 residual; recomputes cap 11,220) + AGT3 (rebases onto AGT4 head). In flight: STEWARD3R (ruling first), STEWARD3A (on e19e31528 = main), B75.
