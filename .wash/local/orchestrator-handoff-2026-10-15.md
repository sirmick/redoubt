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

## 07:05Z: AGT4 FINAL c90196606 on 0b487325b (renewed OK). TRAIN 37 = AGT4 c90196606 + AGT3 (after its rebase + range-diff renewal). Trailers for AGT4 as recorded at acceptance (gates line) with Reviewed-by: steward-red-3: Merge verdict: OK (renewed at c90196606).

## 07:25Z: AGT3 ACCEPTED (final) at 66c36d3bb on AGT4 c90196606. TRAIN 37 = 0b487325b + AGT4 c90196606 + AGT3 66c36d3bb ON THE BENCH; logs .tmp/train-37. Then publish, remove .worktrees/AGT4 AGT3 + branches, end agt4/agt3 implementers: AGENT0 step complete (set AGENT0 done). In flight: STEWARD3R (ruling), STEWARD3A (on main), B75.

## PUBLISHED 07:50Z: train 37 on origin/main (AGT4, AGT3): 702 pass rc 0. AGENT0 DONE. AGT4/AGT3 removed, implementers ended. In flight: STEWARD3R (ruling thread STEWARD3R-scope), STEWARD3A (threads STEWARD3A-notice, -sponsored-login), B75. STEWARD3S drafted (needs 3R). Re-accept AGT2/ATK2 (renewals left them reported).

## 08:30Z: B75 ACCEPTED at f9a1fa206 (Plan-Node: B75 / Gates: q run -- cargo test -p redoubt-rt (+2); testbench memory (+1); beamlet-redoubt --features fake 81; redoubt-client 43; consrelay 26; sshd 34 0, make -f scripts/jobs.mk prebuilt both widths (every rt bin builds) 0, make -j4 -f scripts/jobs.mk set CASES=thread-stack-guard (x3),beamlet-footprint,<shell set 57 + unsafe-budget: 113> WIDTHS=rv64,rv32 0, docs; cargo +nightly fmt --check; size-budget (libs/rt 3943 -> 3994, Size budget line in the commit) 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes). Rebasing onto 43588e0d6 (compile-footprint helper peak to quote), range-diff renewal, then TRAIN 38 = B75 (+ STEWARD3A/3R if ready).

## 08:50Z: B75 re-accepted at 54972509a (renewed OK). TRAIN 38 = 43588e0d6 + B75 54972509a ON THE BENCH; logs .tmp/train-38. Then publish, remove .worktrees/B75 + wp-B75, end b75-implementer. STEWARD3A/3R pending; 3B/3C/3S queued.

## 13:30Z: STEWARD3R ACCEPTED at dcdb9bce1 on 43588e0d6 (Plan-Node: STEWARD3R / QA: STEWARD3R-scope / Gates: make -f scripts/jobs.mk prebuilt both widths (every rt bin type-checks and builds) 0, host tests: rt, wire, steward, init, client, ipd; the eight servers (243/0/2 pre-existing ignores); beamlet-redoubt --features fake 81; elixir-oracles 0, cases: init-boot, init-restart, init-rollback, ninep-newconn-discard, redoubt-revoke, beamlet-launch, steward-restart, -session-ends, -context-labels, -ssh-two-principals, agent-login 0, size-budget (six ceilings raised with reasons; steward heap 30 -> 32); unsafe-budget; docs; cargo +nightly fmt --check 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes; stage .wash/qa/STEWARD3R-scope.md with the plan commit). TRAIN 39 = STEWARD3R (+3A) after train 38. STEWARD3S launched on dcdb9bce1.

## 14:20Z: steward3a-implementer -> steward3a-implementer-2 (handoff_from). STEWARD3A at 98a166314 (WIP-complete core; Q3/exit line/gates left). Train 38 (B75) bench slow behind the [net] lock (501+ pass, 0 fail at 4 h). STEWARD3S on 3R's head.

## PUBLISHED 15:00Z (10-16): train 38 on origin/main be9c8b6c4 (B75 spawned-stack guard page); 704 pass rc 0 (bench took ~6 h behind the [net] lock shared with STEWARD3A gates). B75 removed, implementer ended.
## TRAIN 39 = be9c8b6c4 + STEWARD3R dcdb9bce1 ON THE BENCH; logs .tmp/train-39. Then publish (stage .wash/qa too), remove .worktrees/STEWARD3R + wp-STEWARD3R, end steward3r-implementer; tell steward3s to rebase onto the new main. In flight: STEWARD3A (implementer-2), STEWARD3S.

## 15:10Z: TRAIN 39 = 42437cdaf (be9c8b6c4 + STEWARD3R dcdb9bce1; size-budget libs/rt conflict -> 4012 = sum) ON THE BENCH; logs .tmp/train-39. TRAP: start the bench only after `git status --short | wc -l` is 0 in train-20 (a runner started on a conflicted tree once; killed). Then publish, remove STEWARD3R worktree+branch, end steward3r-implementer, tell steward3s to rebase.

## 16:00Z: STEWARD3S at c61ee5d5d on dcdb9bce1 (reviewed OK with notes, folded); after train 39 publishes: rebase onto the new main, libs/rt 4013 with reason, prebuilt + its set, range-diff renewal, train 40 (with STEWARD3A if accepted).

## PUBLISHED 18:40Z (10-16): train 39 on origin/main c1eb9aa3c (STEWARD3R: R41 revocation scope); 703 pass + aio-many-reads-two rv64: a flake (fails 1 in 3 alone; also train 35 without 3R): node requested from the Architect (B76?). STEWARD3R worktree+branch removed, implementer ended. STEWARD3S told to rebase onto c1eb9aa3c (rt 4013). NEXT: train 40 = STEWARD3S (+3A).

## 18:55Z: B76 opened (b76-implementer, wp-B76 from c1eb9aa3c). Net lock: measure the next train's wait on [net] from `scripts/q log` before asking for a node. IN FLIGHT: STEWARD3A (impl-2), STEWARD3S (rebasing for train 40), B76. Queued: 3B, 3C after 3A.

## 19:40Z: B76 ACCEPTED at wp-B76 306dc5705 on c1eb9aa3c (two commits: d88f23f91 bench --keep-failed, 306dc5705 the case's print order; Plan-Node: B76 / Gates: aio-many-reads-two alone x50 both widths (before: rv64 48/50; after: 50/50 both at 0.7-0.8 s), transcripts kept under .tmp/B76/ 0, make -f scripts/jobs.mk prebuilt (rv64 324, rv32 312); aio-many-reads + bench-net-peer both widths; client-host-tests 0, q run -- cargo test -p testbench (212 incl. a_failing_boots_logs_are_kept_under_its_jobs_name); docs; formatting 0 / Reviewed-by: steward-red-3: Merge verdict: OK with notes). Closing note for the node: why the race was 1-in-3 on rv64 under load stays unexplained; the fix removes the race. TRAIN 40 = B76 + STEWARD3S (after its renewal) (+3A).

## 19:50Z: STEWARD3S ACCEPTED at eb5cf690a. TRAIN 40 = 9e792c1d5 (c1eb9aa3c + B76 306dc5705 [2 commits] + STEWARD3S eb5cf690a) ON THE BENCH; logs .tmp/train-40. Measure the [net] wait from `scripts/q log` after DONE (Architect drafts the port-range node if > 1 h). Then publish, remove .worktrees/B76 STEWARD3S + branches, end b76/steward3s implementers, save.sh. STEWARD3A (impl-2) then 3B/3C.

## 21:15Z: STEWARD3Q opened (wp-STEWARD3Q from c1eb9aa3c; the per-account admission cap; lands before 3A). STEWARD3A-2 building (2a) the lease quota; its lease cases run on a scratch merge of 3Q. IN FLIGHT: STEWARD3A, STEWARD3Q. Train 40 (B76 + 3S) benching. Then 3B/3C.

## PUBLISHED 22:10Z (10-16): train 40 on origin/main 6713d6ffa (B76 aio case order + bench --keep-failed; STEWARD3S ipd grant scope); 704 pass rc 0, ~5.5 h wall (host 04:42Z start). B76/STEWARD3S removed, implementers ended. IN FLIGHT: STEWARD3A (impl-2; lease quota 2a), STEWARD3Q (admission cap; lands before 3A). Then 3B/3C. save.sh at the next clean moment (3A worktree dirty while it works).

## 22:30Z: B77 opened (wp-B77 from 6713d6ffa): q --ports leases replace --lock net. When B77 publishes: restart the q daemon (`scripts/q stop` then `setsid nohup scripts/q daemon`) at a train boundary with no member jobs running (q ls empty); the new daemon adopts running leases. IN FLIGHT: STEWARD3A (impl-2), STEWARD3Q, B77.

## 01:05Z (10-17): REVIEWER ROTATION kernel-red-2 -> kernel-red-3 (handoff_from). B77 review reassigned to kernel-red-3. Reviewers now: steward-red-3 (userland/shell/rt/servers), kernel-red-3 (kernel/loader/bench/pool). Members: architect, steward-red-3, kernel-red-3, steward3a-implementer-2, steward3q-implementer, b77-implementer.

## 01:40Z (10-17): B77 ACCEPTED at wp-B77 e7b765b91 on 6713d6ffa (Plan-Node: B77 / Gates: scripts/q-test (14 checks on a private daemon: disjoint ranges, held port skipped, full-pool wait, kill frees, restart adopts ports and cores) 0, make -j4 -f scripts/jobs.mk set CASES=<30 net cases> WIDTHS=rv64,rv32: old daemon (lock fallback) 60/60 in 4,892 s; new daemon, two makes side by side, 60/60 in 269 s 0, q run -- cargo test -p testbench (213 incl. leased_ports); make -f scripts/jobs.mk prebuilt; docs; no-cruft; cargo +nightly fmt --check 0 / Reviewed-by: kernel-red-3: Merge verdict: OK). TRAIN 41 = B77 (+ STEWARD3Q when accepted; 3A after). AFTER train 41 publishes: restart the q daemon from the root (`scripts/q stop`; `setsid nohup python3 scripts/q daemon`), it adopts running pins; until then --ports jobs fall back to lock=net.

## 01:50Z (10-17): TRAIN 41 = 9590eab6c (6713d6ffa + B77 e7b765b91) ON THE BENCH; logs .tmp/train-41 (old daemon: --ports falls back to lock=net; expect the slow net tail one last time). Then publish, remove .worktrees/B77 + wp-B77, end b77-implementer, RESTART THE Q DAEMON (root: scripts/q stop; setsid nohup python3 scripts/q daemon; `q ls` shows adopted pins), save.sh.

## 03:45Z (10-17): STEWARD3Q ACCEPTED at wp-STEWARD3Q 794449d21 on 6713d6ffa (Plan-Node: STEWARD3Q / Gates: host tests: walfsd 54+8, littlefsd 32+4 (+the_limits_fit_the_budget each); testbench the_servers_admit_what_a_manifest_lets_one_account_hold (the tree's largest derived sum = 6) 0, make -f scripts/jobs.mk prebuilt both widths (313, 0 failed); size-budget; docs; formatting 0, cases both widths: steward-three-contexts x3 (negative at in_flight 2 recorded), init-boot, image-disk, steward-home-quota, steward-context-cap/-idle/-labels/-login, walfsd-boot/-quota/-confined-labelled/-label-check/-one-volume, pipe-eight 0, beamlet-footprint (5,542/5,357 of 11,220), beamlet-compile-footprint (9,135/8,804), beamlet-files both widths after the entry move; 21 walfsd entries in 13 manifests at 1,280 0 / Reviewed-by: steward-red-3: Merge verdict: OK). TRAIN 42 = STEWARD3Q (+ STEWARD3A when accepted) after train 41 publishes + the q daemon restart. Tell steward3a-implementer-2 that 3Q is at 794449d21 (its lease cases run on a scratch merge of it).

## 04:40Z (10-17): STEWARD3A reported f04dc870e; review (steward-red-3) + rebase onto 794449d21 (3Q) in parallel; the four lease cases must pass for real on that tree before acceptance. TRAIN 42 = 3Q 794449d21 then 3A (rebased). After 3A: 3B (powerbox) and 3C (crash blame) launch (both need 3A).

## PUBLISHED 05:10Z (10-17): train 41 on origin/main e42f63193 (B77: net cases lease ports from q); 704 pass rc 0. B77 removed, implementer ended. Q DAEMON RESTARTED on the merged scripts/q (ports 23000-23999 pool; quiet last four); from here --lock net is gone from the recipe and net tails run in parallel. NEXT: train 42 = STEWARD3Q 794449d21 + STEWARD3A (rebased; four lease cases for real; review + renewal).

## 05:50Z (10-17): STEWARD3A rotation 2 -> 3 (handoff_from). 2d831801b = code + folds complete on 794449d21; implementer-3 runs the gates (four lease cases for real) and reports; steward-red-3 renews by range-diff vs f04dc870e; then TRAIN 42 = 794449d21 (3Q) + 2d831801b (3A, ff on top). Then launch 3B, 3C (need 3A).

## 06:20Z (10-17): STEWARD3A ACCEPTED at 2d831801b. TRAIN 42 = ced062648 (e42f63193 + STEWARD3Q 794449d21 + STEWARD3A 2d831801b) ON THE BENCH (first under the port-leasing daemon: measure its wall time); logs .tmp/train-42; stage .wash/qa/STEWARD3A-*.md with the plan commit. Then publish, remove .worktrees/STEWARD3Q STEWARD3A + branches, end steward3q-implementer and steward3a-implementer-3, save.sh. 3B + 3C launch now (need 3A: accepted).

## 06:30Z (10-17): STEWARD3B + STEWARD3C launched (wp-STEWARD3B, wp-STEWARD3C from 2d831801b; they rebase onto main after train 42 publishes). 3Q/3A implementers ended. IN FLIGHT: 3B, 3C. After train 42: STEWARD3 step complete except 3B/3C; then STEWARD4 (owner's order) via the Architect.

## 06:50Z (10-17): TRAIN 42 FAILED (711 pass; 28 min wall under the new daemon: B77 works): userland-boot both widths (AGT1 expectation of the agent carve line; sponsored agent prints none) + model-mutations (PolicySubAgentOutlivesAgent, PolicyNoContextCap not caught). All STEWARD3A. steward3a-fixer launched on wp-STEWARD3A-fix (2d831801b) to amend; steward-red-3 renews by diff; then reassemble train 42 = 794449d21 + the amended 3A. Logs .tmp/train-42/fail-*.

## 07:45Z (10-17): B78 opened (wp-B78 from e42f63193) and STEWARD3X opened (wp-STEWARD3X from 15cea878a = 3C head). Stack: main e42f63193 < 3Q 794449d21 < 3A 2d831801b (amend pending) < 3C 15cea878a < 3X. 3B on 2d831801b. Train 42 reassembles with the amended 3A; later trains carry 3C, 3B, 3X, B78 after rebases.

## 08:05Z (10-17): STEWARD3C at 09c209d7f (reviewed OK with notes, folded; holds). Order at the next trains: train 42 = 3Q + 3A-amended; then 3C (rebased onto 3A-amended) + B78; then 3B, 3X as they land.

## 08:35Z (10-17): mdbook reinstalled on this host (cargo install mdbook --version 0.5.4 --force -> elasticlunr-rs 3.1.0); other hosts follow scripts/setup.sh once B78 lands. B78 ships: setup.sh (no --locked for mdbook, with the reason), ceiling 7,000,000 in doccheck, level 3 kept.

## 09:20Z (10-17): B78 ACCEPTED at wp-B78 77269612df on e42f63193 (Plan-Node: B78 / Gates: q run -- cargo testbench docs (index 6,093,217 B on the reinstalled mdbook; ceiling 7,000,000; no mdbook WARN); hand search of R41, budget_pages, walfsd at their anchors 0, q run -- cargo test -p redoubt-doccheck (rules 23 incl. search_index_within + elasticlunr_version; docs 2; fixture fails at ceiling 1,000 naming big/mid/small) 0, cargo +nightly fmt --check; git diff --check; bash -n scripts/setup.sh; doccheck binary clean 0 / Reviewed-by: kernel-red-3: Merge verdict: OK). Rides train 42 with 3Q + 3A-amended (it is on main; the docs case needs the reinstalled host: done). NOTE: any other host must reinstall mdbook per setup.sh after B78 lands.

## 10:15Z (10-17): STEWARD3A re-accepted at e825c2defa. TRAIN 42 (reassembled) = 3976bf148 (e42f63193 + STEWARD3Q 794449d21 + STEWARD3A e825c2defa + B78 77269612df) ON THE BENCH; logs .tmp/train-42b. Then publish (stage .wash/qa/STEWARD3A-*.md), remove .worktrees/STEWARD3Q STEWARD3A STEWARD3A-fix B78 + branches wp-STEWARD3Q wp-STEWARD3A wp-STEWARD3A-fix wp-B78, end b78-implementer, save.sh. 3C rebasing onto e825c2defa; 3B, 3X building.

## PUBLISHED 11:10Z (10-17): train 42 on origin/main 6af2d6020 (STEWARD3Q admission cap; STEWARD3A leases and agents; B78 search index headroom); 714 pass rc 0, 32 min wall. Worktrees STEWARD3Q/3A/3A-fix/B78 + branches removed; b78 implementer ended. NEXT: train 43 = STEWARD3C e562a2879 (on e825c2defa: fast-forwards onto main) once renewed, + STEWARD3X/3B as they land. save.sh deferred (3X worktree dirty).

## 11:40Z (10-17): STEWARD3C ACCEPTED at wp-STEWARD3C e562a2879 on e825c2defa (= main 6af2d6020 content; ff) (Plan-Node: STEWARD3C / QA: STEWARD3C-shared-restart / Gates: host tests: steward server 2+4+30, init 21+1+66 (+3 blame.rs: the timeout, Told/Lost/Gone/Refused), the lockout host test (three blames: sessions and leases destroyed, login locked out, vault arm, window) 0, make -f scripts/jobs.mk prebuilt (329/318); steward-blame-lockout both widths (11.0/10.0 s: one blame attributed on the machine) 0, cases: init-restart, walfsd-power-loss, steward-restart, steward-session-ends, steward-lease-ends; elixir-oracles (no untaken row); steward-model-host-tests 0, docs; size-budget (servers/steward 1785, servers/init 2702 with reasons); unsafe-budget; formatting 0 / Reviewed-by: steward-red-3: Merge verdict: OK). TRAIN 43 = 3C (+ 3X/3B when accepted). STEWARD3X is based on 3C's OLD head 15cea878a: it must rebase onto e562a2879 (3C final) before its train.

## 11:50Z (10-17): TRAIN 43 = 6af2d6020 + STEWARD3C e562a2879 ON THE BENCH; logs .tmp/train-43 (stage .wash/qa/STEWARD3C-shared-restart.md with the plan commit). Then publish, remove .worktrees/STEWARD3C + wp-STEWARD3C, end steward3c-implementer. Remaining: 3B (powerbox), 3X (restart gap). Then STEWARD3 done -> STEWARD4 brief from the Architect.

## PUBLISHED 12:40Z (10-17): train 43 on origin/main 377d1770a (STEWARD3C crash blame); 716 pass rc 0, 28 min. 3C worktree+branch removed, implementer ended. IN FLIGHT: STEWARD3B (powerbox), STEWARD3X (restart gap; rebase onto main 377d1770a before its train: 3C is on main now). Then STEWARD3 done; ask the Architect for STEWARD4 (owner order). save.sh deferred (3X dirty).

## 15:30Z (10-17): STEWARD3Y drafted (needs 3X; steward-red-3 despite the body saying -2): end batch replies after DestroyBudget, disconnects deferred and bounded. Launch after 3X is accepted.

## 15:45Z (10-17): STEWARD3B in review (steward-red-3) while rebasing onto 377d1770a; it drops heading-split-level 2 (B78 on main) and keeps wire op 18 (3X renumbers to 19/20/21). Train 44 = 3B (+3X). After 3X: STEWARD3Y (deferred disconnects). Then STEWARD3 step done -> STEWARD4 brief.

## 16:05Z (10-17): REVIEWER ROTATION steward-red-3 -> steward-red-4 (handoff_from). Reviewers: steward-red-4, kernel-red-3. STEWARD3B review reassigned to steward-red-4 (content on 9ddfa0a45; renewal after the rebase onto 377d1770a).

## 17:20Z (10-17): STEWARD3B ACCEPTED at wp-STEWARD3B 3d51c0982 on 377d1770a (QA: STEWARD3B-channel; stage .wash/qa/STEWARD3B-channel.md). TRAIN 44 = 377d1770a + 3B 3d51c0982 ON THE BENCH; logs .tmp/train-44 (the merge message amended for the QA trailer after the bench started: same tree; the head to ff is `git -C .worktrees/train-20 rev-parse HEAD` at publish, not bd1408db2). Then publish, remove .worktrees/STEWARD3B + wp-STEWARD3B, end steward3b-implementer. Remaining: STEWARD3X (then 3Y).

## 18:00Z (10-17): train 44 first bench void (amend during prebuilt -> "tree changed"); TRAIN 44 rerun = fea3cfc42 ON THE BENCH, logs .tmp/train-44b. Never touch git in train-20 after the runner starts.

## Train 44b (STEWARD3B) red, 19:27Z
Train 44c started: head 76d60b116 = 377d1770a + 5d289d0c2 (fold: init-boot 6 keys); .tmp/train-44c.
init-boot both widths: 3B's manifest adds two approval keys; init prints "6 keys", tests/init-boot.toml:18 said 4.
Fold sent to steward3b-implementer (amend, gate init-boot both widths). Next: re-merge on 377d1770a as train 44c
(.tmp/train-44c), whole bench. Lesson: a manifest change (keys, badges, servers) must gate init-boot; add it to
every steward/init package's gate list. STEWARD3X: OK with notes at 5dd079803 (frozen); P2 lease-rebind refusal
folds at the rebase onto main after 44 publishes; STEWARD3Y body rev 343 updated by the Architect.

## Train 44 published, ~20:00Z
Train 44c green 722/722; main 76d60b116 (STEWARD3B 5d289d0c2), plan commit 612204b9e pushed; wp-STEWARD3B pushed,
worktree and local branch removed; steward3b-implementer ended. save.sh deferred (3X worktree dirty mid-rebase).
STEWARD3X: rebasing onto 76d60b116 with folds (lease-badge rebind refused + host test; docs rerun; init-boot gated,
6 keys). Then renewal by steward-red-4 (range-diff from 5dd079803), train 45 (.tmp/train-45 from 44c's run.sh),
then launch STEWARD3Y (body rev 343; steward-red-4), then the Architect's STEWARD4 brief. Check .wash/local for
owner files first.

## Train 45 started (STEWARD3X)
Head 89d012e8a = 76d60b116 + STEWARD3X 86aa77767 (renewal OK, steward-red-4); .tmp/train-45. On green: ff main,
plan commit (stage .wash/qa/STEWARD3X-rebind.md), push main + wp-STEWARD3X, remove .worktrees/STEWARD3X and the
branch, end steward3x-implementer, save.sh. Then launch STEWARD3Y (body rev 343; needs 3X on main; steward-red-4),
then the Architect's STEWARD4 brief.
STEWARD4 brief refreshed: .wash/local/STEWARD4-implementer-2026-10-16.md (Architect; size L, Tier A, reviewer
steward-red-2 named -> use steward-red-4; no owner decision needed; next free opcode 22). Plan: after train 45
publishes, launch STEWARD3Y (small, drive.rs) AND STEWARD4 from the new main (override: needs STEWARD3; the owner
wants parallel work); STEWARD4 rebases onto 3Y when 3Y lands (hotspot drive.rs/Machine). Update STEWARD4's node
body to point at the new brief in the next plan_set batch (file under .wash/local).
