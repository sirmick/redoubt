# Redoubt pause checkpoint — 2026-10-04

Owner requested a project pause. Do not resume development without an explicit request.
This checkpoint is being finalized: verify the completion section before assuming all workers stopped.
Never push under the current AGENTS.md/session instruction. No package from this development run was merged.

## Workspace and recovery

Root /home/mcloonan/redoubt; Wash workspace 8a005a3cee5b786ad913a8412bb1b22f.
Orchestrator 5e72d6c884926b33a54b579af9bb50de. Reuse this workspace; do not restart or tear it down.
Read .wash/README.md, PROJECT.md and SWARM saving/resuming rules. Inspect git/worktrees and live Wash before changes.
Reviewers have been paused. Pause requests sent to all five remaining residents; confirmations pending at initial write.
No new assignments, implementation, tests, merges, installs, container launches or retries during pause.
Save is local only; .wash/save.sh pushes and must NOT be run under the current restriction.

## Package checkpoints

- BEAM7: wp-BEAM7, .worktrees/BEAM7, clean 6a79d0e1356493f5307b2af19934b4f3d8515c79.
  Implementation/docs and all three final source reviews clear. Whole bench 399 PASS / 1 environment FAIL (OpenSSH reference missing Podman).
  Remaining: supplied supported rootless runner, required exact-content whole bench, renewed gate review and acceptance. No waiver.
  Durable .wash/local/BEAM7-report.md and evidence/BEAM7/ (427 checked files, whole-run plus stdout evidence).
- MEM1: wp-MEM1, .worktrees/MEM1. Observed clean cd73175b5 (image sizing), preceded by a1187eac3 (reviewed scanner correction).
  Six calibration workloads have valid scans, including failed 2x-margin calibration runs; these are not final PASS evidence.
  Measured image pages: keyd3, consoled5, bootfsd4, blkd3, netd3, ipd4, fsd:data4, blkd:system3, fsd:system6, beamlet16.
  Standard image bound507, read-only merged bound521. 1024>=2*507 but not 2*521: do not generalize the doubling claim.
  All SIX FINAL full-workload scans PASS exit0 at clean cd73175b5; reports f9f2d718/86d433a6, target/MEM1-final-report.md and stable raw evidence.
  Focused host, no-cruft, size, unsafe, docs and formatting gates PASS. QEMU released. Docs distinguish standard507 and merged521.
  Full final-head whole bench, supported rootless reference gate, integration/sign-off and exact-head final panel/acceptance remain unfinished.
  QA MEM1-runtime-stack remains blocking through acceptance. Authoritative runtime-stack ruling and paint diagnosis in .wash/local/.
  Evidence .wash/local/evidence/MEM1 includes original failed whole bench (396/5), raw dump and calibration/final scans.
- SCHED1: wp-SCHED1, .worktrees/SCHED1. Original HEAD7b23f52835f7b6141f0ed7405e0ef5eb2a3d2097 is only three slice constants atop IPC3.
  Large fixture/oracle/model dirty checkpoint was fe3ae2110cb0af522e39d25e3dc4bf8821a1a44445cbd643d0b3b5a7dd91d52e before latest implementer.
  Check new implementer-5 pause handoff for any partial v3 edits/commit; do not overwrite it with old snapshot.
  Initial latency seed3 passed both widths, but no cluster acceptance. Saved candidate failed 402/400 join; corrected host protocol passes join then sample0 interval fails.
  Actual clock-bound failure does not prove a physical early deadline; the old E-minus-gross audit window starts27us before release and is disqualified.
  Approved prospective v3 proposal .wash/local/SCHED1-consistent-window-proposal.md; reviewed pre-append hash bb46a4fb13843d2c99cca051206e13a10dc4a28a8183586b784d916782fd3f9e;
  appended acceptance hash5fc29bb74118276786e2cac5e6a5f74abe5759d92ce5e4c890523951ba0cafb0.
  Single kernel H/R/F; B-based deadline lower bound and U=P+1 full envelope, certified audit interiors, physical driver lower witness for old control.
  Simplifier proposal to drop +1 explicitly REJECTED: floored P does not upper-bound physical fractional time. Retain U<=F and full-envelope credit.
  Source preparation only was approved; no v3 machine grant. Fixed200 samples/phases/window, exact joins/fences/debt/categories/targets/64MiB remain.
  Requires fresh identical candidate/old10ms both widths, qualifying physical control miss, costs/sweeps/gates/docs and scheduler-only replay/test/review on main before merge.
- IPC3: wp-ipc3, .worktrees/ipc3, a0fbbcab12be5c43f217cf94b7f006d3d1f37689. Waits for SCHED1 acceptance/merge, then rebase and full renewed acceptance.
  Owner explicitly chose fix scheduler before merge. QA IPC3-wake-latency remains open.

## Reference runner decision

Owner decision7a1843e9078fcc6ce78c78646a3ad530: KEEP ROOTLESS; SUPPLY RUNNER.
Rootful Docker exception NOT approved. Runner connection/environment details requested asynchronously, still missing at initial write.
No more local backend/VM/permission alternatives authorized. Old disposable container redoubt-bench-env-preflight remains STOPPED.
Image sha256:fd04016ea9206bf839f6bdf28f3f30ab88a8ac1f6f262b2eb177a63d820cffc4.
Both direct unshare and actual Podman newuidmap failed uid_map EPERM. Exact denying policy unknown; earlier scoped execution approvals exhausted.
Remote-VM preparation stopped before launch because dynamic paths require forbidden broader exports. No Docker socket exposure, rootful backend, host policy change or gate skip.
QA BENCHENV1-container-permissions stays blocking. Reports/proposal/no-go in .wash/local/BENCHENV1-*.md.

## Members and handoffs

Current Architect architect-4 (82e8a4f3f22a3ed0ec16c21a46807d4f); SCHED1 implementer-5 (9147d20cea7fde2514158accd2642daf), red-2 (d0ad7a9452b4558648b08b3598718cae).
MEM1 implementer-4 (ea414c2272b9e0bed61f9c7740a192e8); BEAM7 implementer-4 (3e0a58f75da48c9cb9b8b4455fb09717); bench-env-implementer-2 (6543a1c18d9b22e4ea7ff9c42e9f025f).
Managed member handoffs under .wash/local/handoffs/. Older members are ended; do not send instructions to retired IDs.
Relevant assignments at pause: MEM1 8315112ad60301343068f21c4b941fe3; SCHED1 456a5b16c1f518e831f8fb75bc8eac0a; BEAM7 7d26c3e0d5c059fbdbcbbcee3bd14ab8.
Open/unfinished assignments must not be mistaken for package acceptance. Prior reviewer verdicts are scoped to exact hashes.

## Evidence and testing traps

Every test through cargo testbench. It prunes old run logs: archive raw evidence under root .wash/local/evidence/NODE before another invocation.
Some historical early logs were already pruned; reports state which hashes/excerpts survive. Do not claim unavailable raw evidence.
QEMU runs serialized; on resume grant new windows only after checking no prior processes.
Existing redoubt-dev UID/GID1447391350, /work project mount, CARGO_HOME=/work/.cargo and RUSTUP_HOME=/work/.rustup.
Firmware root bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper and riscv32imac equivalent.
No toolchain installs by implementers. Preserve live Docker/cache assets; do not prune.
Wash feedback was appended to /home/mcloonan/wash/BUG-REPORT-redoubt-workspace.md; no Wash code change.

## Completion

PENDING: all writer confirmations, member pause verification, final git identities/dirty state, local plan/QA commit and local scratch snapshot.
Remote backup is prohibited by the current no-push instruction. Large evidence remains on this machine and is not covered by the small-file wash-local snapshot.
