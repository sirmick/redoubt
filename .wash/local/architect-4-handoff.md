# Architect-4 pause handoff

Owner-requested project pause, conveyed by orchestrator message
`bf850cf9d715c4595c577672ae47ea5b`. Stop after this handoff. Do not resume work,
investigate, run tests/QEMU, spawn helpers, commit or push without a resumed scoped
assignment. Preserve all live worktrees and assignments. All Architect-4 assignments
are complete; no partial investigation remains. No pending owner question was issued
by this member.

## Role and recovery

Read `.wash/README.md`, governing PROJECT Questions and SWARM Architect/Questions
sections, and the task's owning pages. TENETS governs design. No broad book/history
reread. Answer tracked QA with `message_send(thread_id,reply_to)`; only actual human
answers authorize owner tradeoffs, requested through `decision_request`. Architect
does not resolve QA, implement, test or push. Book edits require coordinated writer
scope. Report an assignment once, then preserve its files absent renewed instruction.
Set waiting and END TURN; never poll. Earlier background is preserved in
`.wash/local/architect-3-handoff.md`; the current owner rootless decision below
supersedes that older handoff's pending owner proposal.

## SCHED1 accepted v3 proposal and exact readiness

Authoritative proposal: `.wash/local/SCHED1-consistent-window-proposal.md`.
Accepted body SHA256 before checkpoint appendix:
`bb46a4fb13843d2c99cca051206e13a10dc4a28a8183586b784d916782fd3f9e`.
Current file including requested acceptance appendix:
`5fc29bb74118276786e2cac5e6a5f74abe5759d92ce5e4c890523951ba0cafb0`.

Assignment `3382d196e606bedf921456cf2e33b140` completed; tracked delivery
`1d1933c459639482c0088e75f7fef266`. Root acceptance message
`69dc8f439d7fdaf7e340578407f2cfab` reports red review `515bdf82` found proof sound
and accepts **bounded source preparation only**, no machine grant. Root requested
the appendix; its delivery is `9bc6abe5cbda6d4ac3eaaae8f8146ce1`.

Root explicitly rejected the simplifier's suggestion to drop +1: floored P cannot
upper-bound physical fractional time. **Retain U=P+1, U<=F, full certified audit
intersections and lower-witness negative control.** No subsequent simplification
or implicit measurement weakening is authorized. Red readiness is for the design,
not acceptance of an unreviewed implementation or machine evidence.

Construction (read full proposal before implementation/review):

- One successful kernel T defines H=T+200000, R=H+50000 and F=R+16000000; identical
  H/F are sent to all 19 children and printed. Cluster-only kernel-clock checks use
  the existing 256-iteration chunks; general Bench::go and other workloads stay.
  No raw-counter absolute epoch conversion. Paired useful-work counts must be fresh,
  not normalized against the different raw-counter loop calibration.
- All 200 attempts keep 80-ms slots, phases100/300/600/850 and intent sequence.
  Kernel pre-arm B defines delta toward zero-intent target, or the unchanged relative
  phase after positive preparation. L=B+delta, fixed before arming. Driver retains
  RTC a/d/s and E before service/P after; timer checks actual timeout outcome.
  Report the envelope [L,U), U=P+1, require R<=L<=P and U<=F. Physical deadline/service
  is contained under the settled monotonic same-rate assumption; no equal RTC epoch.
- This is an explicit v3 **measurement-definition change**, not repair of v2 data.
  Gross envelope is U-L, driver RTC gross is separate. Certified audit interiors
  [u+1,v) give exact conservative credit inside the envelope. Both general and
  cluster oracle consumers must share the same validated calculation. Unrelated
  workloads keep existing audit calculations. Ordinary observation/preemption/R10
  costs stay included; uncertain audit edge bins remain charged.
- Unchanged p50<=15000/p99<=50000 targets apply over all200 net envelopes. Both
  candidate and old10ms also compute driver physical lower witnesses: RTC gross less
  maximum possible overlapping audit time, using the union of outer [u,v+1) spans.
  Old control must fully qualify, fail an envelope target AND exceed a corresponding
  driver lower-witness target on each width. An uncertainty-only upper-bound miss is
  not the negative control. No success is promised.
- All ranks/debt/categories,16 spinner cluster/spread,exact blocked D/W/service joins,
  two exclusive X/Y marker fences,64MiB/zero drops and original pinned cases remain.
  Metadata is versioned, indexed, reported after own fence with received H/F headers.
  No per-sample IPC/wait, kernel field, policy/tolerance or stack-limit expansion.
- Owning rule updates are coordinated sections in docs/kernel/scheduling.md
  Responsiveness and docs/testbench.md Checked builds. Timer semantics unchanged.
  Full proposal lists owned paths, minimal negatives, proofs and first-run stops.

Current routing snapshot says `sched1-implementer-5` owns assignment
`456a5b16c1f518e831f8fb75bc8eac0a` (implement reviewed v3/host checks).
No source checkpoint from that assignment was reviewed by Architect-4. Owner pause
supersedes ongoing preparation; use that implementer's pause handoff on resume.

## SCHED1 archived diagnosis remains failed

Assignment `2de23c01deea0d26dbe19ece62c06328` completed;
`.wash/local/SCHED1-sample0-clock-ruling.md`, tracked answer
`90a44ff73d5be231f48efe16b3636812`.

Preserved seed3 rv64 run at HEAD7b23f5283 + machine patch63b87f63...5f987 is in
`.wash/local/evidence/SCHED1/run-1-1791164421199359362/`; console SHA
969cfa247ffa83945c1849b0d45baeb9c7aa16a08f46ccb0c59d1b81a3b65a01.
Host-only go correction fe3ae211...91d52e advanced offline analysis to sample0.
E956884, RTC S-D36267192ns, P957012: L920616<release920644 by28us. Real deadline
bracket [920616.808,920745.808) crosses release; no actual early alarm proved.
Separately E-gross920617 is27us early and disqualifies the unchanged audit interval.
Bench::go's raw-counter child anchor vs later kernel printed anchor is a real
identity bug, but its unrecorded separation is not proved to cause this failure.
The E-before-S synthetic audit interval is displaced; P-E128 is not measured E-to-S.
W/K/nearby timer I cannot timestamp the driver deadline. No retroactive v3 repair,
passing archived sample, coverage or latency verdict is authorized.

## MEM1: current ruling and reported evidence pending assessment

Authoritative `.wash/local/MEM1-runtime-stack-ruling.md` remains: full init-boot,
userland-boot and read-only on both widths; final per-server pages are
ceil(2*max_peak_bytes/4096) across all six, then all six rerun with final declarations.
Max128; stop fault, missing/ambiguous paint, or overflow. First-thread exercised-path
evidence is not an adversarial bound on every thread. No tolerance/policy change.

Scanner correction a1187eac3 was red-cleared for analysis/calibration only. Existing
page-offset qualification precedes bounds; mismatched paint earns no unit, the
original touched unit remains missing, congruent out-of-range stays fatal. Details
and original raw evidence are in the prior handoff and MEM1 local rulings.

At this member's arrival, six calibration scans had maxima beamlet31784B=>16pages,
fsd:system12072B=>6pages; final six reruns were underway, with standard root bound507
versus merged521. Those were reported status, not this member's acceptance.
At pause, workspace member status reports **MEM1 measured implementation committed
at cd73175b5; evidence in target/MEM1-final-report.md** in its worktree. Architect-4
has not read/reviewed that final report or independently verified its six results.
Do not treat the earlier 'reruns underway' state as the latest implementation status,
or treat the newly reported commit as accepted. Consult MEM1's final pause handoff,
report and root coordination on resume. QA remains open/blocking. No new MEM1 design
investigation was done for this pause.

## Rootless runner decision and BEAM7 blocker

Actual owner decision `7a1843e9078fcc6ce78c78646a3ad530`: **KEEP ROOTLESS; supply a
runner**. Runner connection/environment details were requested and remain the known
blocker. Docker-rootful exception is rejected/not authorized. No further VM/backend/
permission alternatives or reused exhausted approvals. Earlier owner proposal in
`.wash/local/BENCHENV1-owner-proposal.md` is historical; do not execute its alternative.
BENCHENV1/BEAM7 acceptance requires a proven compatible rootless runner and the real
reference gate, followed by separately coordinated broader validation. No gate waiver.

## Current routing and QA snapshot

Verified with workspace team/QA reads solely to save this handoff. Members are
entering pause; a listed assignment is not permission to continue during it.

| Member | Current ID |
| --- | --- |
| orchestrator | 5e72d6c884926b33a54b579af9bb50de |
| architect-4 (this member) | 82e8a4f3f22a3ed0ec16c21a46807d4f |
| sched1-implementer-5 | 9147d20cea7fde2514158accd2642daf |
| sched1-red-2 | d0ad7a9452b4558648b08b3598718cae |
| sched1-simplifier | 9aa85e726935bcd346e919563dde4fac |
| mem1-implementer-4 | ea414c2272b9e0bed61f9c7740a192e8 |
| mem1-red | 20f9b237c82a8b1ac0a374a40e748976 |
| beam7-implementer-4 | 3e0a58f75da48c9cb9b8b4455fb09717 |
| bench-env-implementer-2 | 6543a1c18d9b22e4ea7ff9c42e9f025f |

Do not route to retired sched1-implementer-4 or original sched1-red. Verify live
routing on resume before sending work. Current relevant threads, all assigned to
orchestrator and **open/blocking**:

- IPC3-wake-latency, revision78, nodeIPC3.
- MEM1-runtime-stack, revision15, nodeMEM1.
- BENCHENV1-container-permissions, revision21, nodeBENCHENV1.

Other open questions (not assigned to this Architect) include RT1-unsafe and
VOL1-verified-volumes; no new work is implied. Do not resolve any of these from a
handoff or a root source-preparation checkpoint.

## What this member changed and what is left

Only local reports were written: SCHED1-sample0-clock-ruling.md, the v3 proposal plus
the explicitly requested root-checkpoint appendix, and this handoff. No code, tests,
book, commits, launches, installs or pushes. No helper agents. No tool cell, shell
session, file handle or process remains running. No further edit to reported
artifacts is authorized by the pause request. Save/report this handoff and stop;
resume only after the owner/root resumes scoped work.
