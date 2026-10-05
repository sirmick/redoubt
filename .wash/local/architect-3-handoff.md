# Architect-3 durable handoff

Replacement checkpoint requested by orchestrator message d379d02c5ef2e56f59044d51a7e9eb29. All assigned work is COMPLETE; no partial investigation remains. Do not start new investigation until assigned. No helpers, source/test implementation, test runs, launches or pushes. Stay resident; set waiting and END TURN, never poll.

## Role and routing

Read .wash/README.md, current PROJECT and SWARM Architect/Questions sections, TENETS as needed; do not broadly reread history. Answer tracked QA using message_send(thread_id,reply_to). Cite settled rules. Only actual human decisions authorize owner tradeoffs; decision_request is the owner channel. Architect owns design pages only in coordinated writer windows, may edit unstarted plan nodes, never resolves QA itself. Report assignments once, then do not modify their files absent new authorization.

Verified live routing at this checkpoint:
- orchestrator: 5e72d6c884926b33a54b579af9bb50de.
- this member architect-3: b88629ea4637442d50884134b3b5b325.
- sched1-implementer-4: 6c7d71ce9dc2f567873967ea4d95cdee; sched1-red: eb41be93fc724743bf06639b1ef68a4b.
- mem1-implementer-4: ea414c2272b9e0bed61f9c7740a192e8; mem1-red: 20f9b237c82a8b1ac0a374a40e748976.
- beam7-implementer-4: 3e0a58f75da48c9cb9b8b4455fb09717; remaining acceptance assignment blocked on reference gate.
- bench-env-implementer-2: 6543a1c18d9b22e4ea7ff9c42e9f025f. Do not route to retired original bench-env member87d5....
Verify recipients still live before sends. Some member status strings lag QA instructions.

## Owner proposal: complete, not approved

Assignment 8d3a624180f194a6028b7609fdc9a591 was completed despite the later lifecycle instruction calling it active. Final proposal: .wash/local/BENCHENV1-owner-proposal.md. Successful tracked delivery afbf16aa8da7c553d0f326bd4b9fbd6a replies to59b0b8dd6718f1612c00272ac31733ab on BENCHENV1-container-permissions. Earlier oversized delivery failed; the successful shorter reply is authoritative. assignment_results completed once. No decision_request was issued by me: assignment was preparation for orchestrator presentation. No owner selection or execution approval may be inferred.

Recommendation: retain rootless Podman and supply a compatible native runner; none identified, so BEAM7 acceptance remains blocked. Alternative explicitly opts into docker-rootful reference backend while retaining Podman default/no fallback. Source/owning-rule/launcher topology, residual authority and gates are concrete in proposal:
- ssh.rs selector TESTBENCH_SSH_REFERENCE_BACKEND=podman|docker-rootful, fixed local daemon Unix endpoint, image identity recorded, preserved pins/four mounts/networkless per-session sshd/version and all verdicts.
- docs/testbench.md gets an explicit identity/launcher-authority exception; GETTING-STARTED documents it.
- dedicated scripts/testbench-reference-docker.sh and tests/ssh-reference/Dockerfile.bench-client; dev.sh remains socket-free.
- fresh dedicated dev-based bench container mounts project at identical canonical host path /home/mcloonan/redoubt, cwd .worktrees/BEAM7, cache/firmware absolute underneath. This avoids dynamic mount-path translation. Socket plus numeric socket group granted only to this invocation; no /config/agent/host SSH mounts.
- reference containers are daemon-created siblings, four case files only, no socket/worktree inside them.
DECISIVE LIMIT: direct rootful Docker socket gives ENTIRE bench invocation root-equivalent host authority; cannot technically enforce pinned-test-only operations. Read-only socket mount does not restrict API. Reference UID0 no longer maps to bench UID. If owner requires enforced narrow daemon authority, alternative is unsuitable; do not invent a broker project.
Compatible supplemental CLI image, default-confinement socket access, mounts/labels/ownership/cleanup are unproved gates. Host /usr/bin/docker dynamically links libc; do not assume bind-mounted client works. Source/red review precedes runtime gate; focused real reference first, coordinated wholebench afterward. Stop first failure, no pin/security/path widening; cleanup only exact invocation containers. No gate waiver.

## BENCHENV1 preceding evidence

QA BENCHENV1-container-permissions OPEN/BLOCKING.
.wash/local/BENCHENV1-preflight.md records:
1. Human25b87c4842eeee70bffa85d9c8fdd6f2 authorized exact named one-container seccomp/AppArmor exception. Direct unshare -Ur failed UID-map write; stop honored.
2. My ruling43708c6746e5d6fcc8b0f42f8b799611 distinguished direct single-map unshare from Podman helper path.
3. Human7994b31c264f2bb7cd3d295e71031886 approved exact continuation. Actual Podman invoked newuidmap with expected maps; write failed EPERM, exit125. .wash/local/BENCHENV1-mapping.stderr preserves full diagnostic. No exact AppArmor cause proven. Container stopped again. Both approvals exhausted.
4. .wash/local/BENCHENV1-remote-vm-ruling.md conditionally admitted a default-confined unprivileged TCG guest/rootless service only with exact path/identity/mount/network proof and bounded exports.
5. .wash/local/BENCHENV1-vm-preparation-no-go.md stops before ANY VM preparation/build/launch: unchanged bench creates unpredictable run-.../ssh case paths after start; prelaunched fixed narrow writable export cannot provide them. Broader run-tree export or mirror would exceed scope. Orchestrator explicitly forbids further environment variants. The new owner proposal replaces further VM exploration.

## SCHED1 settled clock, waits and fences

Authoritative earlier ruling: .wash/local/SCHED1-relative-wait-ruling.md; retain original layout ruling/brief obligations.
- Fixed80ms slots,200 attempts per stand-in, phases100/300/600/850; zero intent one wait toward nominal target, positive spin to slot then one relative phase wait. No extra prep wait/retry/replacement/window extension.
- INCLUSIVE driver boundary: S>=D, E<=P<=fixed_end; checked L=E-ceil((S-D)/1000)>=release. E before RTC service, P successful immediately after; RTC ns vs kernel us, same-rate monotonic clocks, NOT epoch equality. Ceiling only containment, existing gross/latency/audit unchanged. No waiver for delayed P.
- Exactly2 empty zero-page/process/weight FOREVER markers in setup; each stand-in destroys its own after sample200 metadata before reporting. X exclusive fence, unique Y, IDs are destroyed budgets not handles/callers. Attribute latest still-running stand-in K plus exclusive call sites, reject ambiguity/deschedule; retain overlapping R10 ordinary cost.
- Exactly200 blocked D/W/service cycles and samples before ownX; postX reportingW never fills missing waits. All old coverage/debt/rank targets,16 spinner common wakes/spread<=100us equivalent,64MiB/zero drops and numeric latency targets unchanged. Old10ms control must qualify AND fail latency on both widths.

Latest bounded diagnosis assignment beae021f277c201ff478ee38fe0be020 COMPLETE:
.wash/local/SCHED1-go-boundary-diagnosis.md, tracked answer bbfecf6e4fe2edf9e7461e6e3211506e.
Preserved seed3 rv64 at HEAD7b23f5283 + diff63b87f63...5f987 failed402 vs400 D/W, not a latency verdict. Evidence .wash/local/evidence/SCHED1/candidate-seed3-rv64-result.md and full run-1-1791164421199359362 (148482records,0drops).
Oracle secondW=go is wrong because report(0) readiness send blocks. Driver84 readinessD3108/W3115, actual windowD3130/goW3789. Extra counted pair is actual go3130/3789; timer87 analogue3242/3822. Markers delivered through process_start handle slots, no extra IPC. Actual go collectively anchored by server33W3249 after last spawnW3205; all19 go deliveries ordered, launcher1 latestK; readiness completions precede anchor.
After truego exactly400 alternating D/W with serviceK per stand-in before ownX; driverX104350/Y104367 marker31, timerX104400/Y104417 marker32. Spinner same ordinal bug: thirdW actualgo, real16 timeoutW5199..5229 entry1868 beforeK5234.
Proposed state-machine correction validates source-consistent immediate/blocking readiness prefix, windowD and ordered launchergo for ALL roles; cannot hardcode thirdW/truncate201 or drop arbitrary pair. Preserve gates.
CURRENT MEMBER STATUS (not independently reviewed): SCHED1 host correction diff fe3ae2110cb0af522e39d25e3dc4bf8821a1a44445cbd643d0b3b5a7dd91d52e; focused oracle PASS; offline archived replay now stops sample0 lower clock bound28us before release. Detail .wash/local/evidence/SCHED1/go-boundary-host-correction.md. No new QEMU reported. Do not weaken inclusive clock rule or investigate this new failure until assigned. QA IPC3-wake-latency remains OPEN/BLOCKING.

## MEM1 scanner clearance and six scans

Authoritative .wash/local/MEM1-runtime-stack-ruling.md: original3page beamlet stack insufficient for full runtime; early init-boot peaks do not bound shell. Retain init-boot and FULL userland-boot/read-only on rv32+rv64, all existing verdicts/attacks. Historical16pages calibration only. Each server final pages=ceil((2*max_peak_bytes)/4096) across ALL6, then rerun all6 with final declarations, recompute standard image/root bound and docs. Max128; stop fault, ambiguous/missing paint or >128. Valid calibration2x shortfall can inform sizing but is failed run. First-thread exercised paths, not all thread stacks/adversarial bound.

Assignment595383cfa562da670b3fb65cbaa5bade COMPLETE, .wash/local/MEM1-paint-diagnosis.md. Preserved raw1GiB dump under .wash/local/evidence/MEM1; SHA256 e6915f3f36c9155029283ba75a62548f3e790c5a7d818c5f44f20f478181406b. Single apparent tag10/index21332 at PA0x80946c98 fails page slot403 vs index%512340. Unaligned valid paint pattern overlaps index; coherent Sv32 mappings/startup args attribute beamlet. Exact writer/legitimacy not proved; no runtime-corruption exoneration or final peak derived.
Accepted bounded correction: existing page-offset qualification BEFORE bounds; mismatch credits no unit, original touched slot stays missing. Page-congruent out-of-range still fatal; duplicates/missing/fault/restart guards preserved.
Implementer commit a1187eac30e52595a896ebccaf0833d8aa6d4f04, scanner-only; patch dd35ddea05785ac908d47fa6665c77fa7aafc60a0b78de85be45beb0e57084ed; .wash/local/evidence/MEM1/MEM1-paint-checkpoint.md. Host tests/format/docs PASS reported; original4-file calibration patch07090083... unchanged.
CURRENT authoritative QA instruction a46de6de20feef167f17df15a15e8bec: red review74b7e93a assignment0a68a949 CLEARED scanner for analysis/calibration ONLY. MEM1 now owns QEMU. Resume six-case calibration and final6 remeasurement. May reuse exact-identity qualified prior peaks and corrected offline rv32 dump ONLY via existing cargo testbench route, no invented entry/mixed evidence. Otherwise rerun required calibration. Finish both read-only workloads, preserve raw evidence before every next run; no unrelated fullbench until focused final evidence.
Earlier partial calibration: init both PASS; userland rv64 valid peaks but fsd:system12072B requires6vs3 pages; userland rv32 failed old scanner; read-only not yet run at that stop. No final sizes/bound accepted. QA MEM1-runtime-stack OPEN/BLOCKING.

## Handoff state

All my assignments reported complete once; files are now review artifacts, do not edit without new instruction. Only .wash/local reports were created by this member. No source/test/book edits, tests, commits, launches, installs, pushes or live processes. No outstanding tool cell, shell session, file handle or process. No pending owner decision issued by this member; prepared proposal awaits orchestrator presentation. Ready for replacement; no further work.

