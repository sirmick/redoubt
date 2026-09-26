# DOC1 inventory: specs and notes

Extracted per `todo/DOC1-docs-rewrite.md` Migration step 3, before `docs/legacy/` is deleted.
Covers: docs/legacy/*.md except ANSWERS.md/QUESTIONS.md (reader A) and WORKSPACE-QA (moved);
HISTORY.md, ARCHITECT-NOTES.md, STATUS.md, BUILD-PLAN.md, DEVICE-GRANTS.md; root ASTRA.md; the
brief's "Carry into todo/" list; and rule-ID citations under kernel/ libs/ loader/ stub/ model/
servers/ tests/ tools/ (source files only).

Legacy milestone numbers (STATUS/BUILD-PLAN/PLAN/HISTORY) are **legacy numbering**: "milestone 1"
there means "separation and containment", not the brief's M1 name necessarily aligned — check the
named capability, not the bare number, when mapping to the brief's M1-M5.

## Part 1: rule-ID map

All IDs are defined in `docs/legacy/KERNEL-SPEC.md`. No ID found cited by code but undefined, and
no ID found defined twice with conflicting text. `R4` has sub-clauses `R4a` (open calls) and `R4b`
(a server dies) also defined in KERNEL-SPEC.md but not separately cited in code (code cites `R4`).

| ID | Type | Source | Statement | Cited in code/tests |
|---|---|---|---|---|
| R1 | rule-id | KERNEL-SPEC.md#Invariants (rule text ~line 203) | Flow. Information flows from budget A to budget B only if B is class `system` or the two share a label set. | kernel/, model/ (invariants.rs, ghost.rs), tests/ |
| R2 | rule-id | KERNEL-SPEC.md ~line 212 | Fair waiting. Blocked senders on an endpoint are grouped by sender budget's (account, ...) and served round-robin by group. | kernel/, model/ |
| R3 | rule-id | KERNEL-SPEC.md ~line 221 | Lends and abandoned calls. A lent page stays charged to the caller; taking the call moves responsibility for its cleanup. | kernel/, model/, loader/, stub/ |
| R4 | rule-id | KERNEL-SPEC.md ~line 232 (R4a/R4b sub-clauses at 248, 255) | Delivery. A message is delivered only if the receiving process's budget can pay for it; sub-rules cover open calls and server death. | kernel/, model/ |
| R5 | rule-id | KERNEL-SPEC.md ~line 311 | Interrupts. On IRQ, the kernel masks the source and sets `fired`; `receive` on the IRQ handle clears/rearms. | kernel/, model/ |
| R6 | rule-id | KERNEL-SPEC.md ~line 315 | Charging. Every kernel object is charged to a budget. | kernel/, model/, docs cross-refs in CONTAINMENT.md |
| R7 | rule-id | KERNEL-SPEC.md ~line 321 | Carving. A child's page/process/weight limits come out of the parent's free limits. | kernel/, model/, BUILD-PLAN.md (K5 carve-lead rescale follow-up) |
| R8 | rule-id | KERNEL-SPEC.md ~line 328 | Accounts. A new budget's account equals its parent's unless the parent's is 0. | kernel/, model/ |
| R9 | rule-id | KERNEL-SPEC.md ~line 331 | Stamps. `endpoint_create`/`budget_create`/`process_create` stamp the new handle with its source's stamp. | kernel/, model/ |
| R10 | rule-id | KERNEL-SPEC.md ~line 334 | Destruction. Destroying budget B destroys descendants first and kills their processes; has a latency target (see S-9). | kernel/, model/, BUILD-PLAN.md (destruction latency), todo (R10 cost near 30ms target) |
| R11 | rule-id | KERNEL-SPEC.md ~line 348 | Memory. No mapping is ever writable and executable, and none is writable without being readable; freed page tables reclaim (partially open — see QUESTIONS 145). | kernel/, model/ (invariants.rs I9 cross-ref) |
| R12 | rule-id | KERNEL-SPEC.md ~line 364 | Scheduling. One flat stride queue over every runnable budget of either class, no distinct system-class fast path. | kernel/, model/ |
| I1 | rule-id (model invariant) | KERNEL-SPEC.md#Invariants item 1 | A process can use only indices into its own handle table (<= MAX_HANDLES); every live handle names a live object. | model/src/invariants.rs |
| I2 | rule-id | KERNEL-SPEC.md item 2 | After budget B is destroyed, no handle stamped with B or a descendant exists anywhere. | model/src/invariants.rs |
| I3 | rule-id | KERNEL-SPEC.md item 3 | A minted handle's badge is non-zero; its stamp is its source's default stamp or a descendant. | model/src/invariants.rs, KERNEL-SPEC.md line 481 (cited beside R11) |
| I4 | rule-id | KERNEL-SPEC.md item 4 | Only badge-0 endpoint handles receive; every one is `endpoint_create`'s result or a copy. | model/src/invariants.rs |
| I5 | rule-id | KERNEL-SPEC.md item 5 | For every budget: usage <= limit; children's limits and own pages plus own objects fit in limits. | model/src/invariants.rs, ANSWERS.md |
| I6 | rule-id | KERNEL-SPEC.md item 6 | Labels never change; labels(child) ⊇ labels(parent); only a system-class creator adds labels. | model/src/ghost.rs |
| I7 | rule-id | KERNEL-SPEC.md item 7 | Every flow obeys R1; delegation vs. cross-label-set receive-right handoff (CONTAINMENT.md). | model/src/invariants.rs, CONTAINMENT.md |
| I8 | rule-id | KERNEL-SPEC.md item 8 | class(child) = class(parent); account(child) = account(parent) unless parent's is 0. | model/src/invariants.rs, BUILD-PLAN.md |
| I9 | rule-id | KERNEL-SPEC.md item 9 | No page mapped W+X; every page zeroed before first use; a lent page unmapped from lender until call ends. | model/src/invariants.rs (heavily) |
| I10 | rule-id | KERNEL-SPEC.md item 10 | Creating then destroying a budget leaves parent's usage/free limits unchanged once exit notices are received/dropped. | model/src/invariants.rs (check::budget_lifecycle), RESOURCES.md |
| I11 | rule-id | KERNEL-SPEC.md item 11 | With k groups (R2) blocked on an endpoint and receiver below MAX_OPEN_CALLS, each group's oldest message is taken within k receives. | model/src/ghost.rs |
| I12 | rule-id | KERNEL-SPEC.md item 12 | Budget ids never reused; a message id is never 0 and never reused within its receiving process. | model/src/ghost.rs |
| I13 | rule-id | KERNEL-SPEC.md item 13 | Every blocking call returns by its timeout, bounded by timer latency; when the thread then runs is R12's concern. | model/src/mutation.rs (TimeoutIgnoredWhileOthersRun), BOOT.md, BUILD-PLAN.md |
| I14 | rule-id | KERNEL-SPEC.md item 14 | No sequence of system calls, with any arguments, panics the kernel. | model/src/gen.rs, model/src/lib.rs (I14's tests run in release builds) |
| I15 | rule-id | KERNEL-SPEC.md item 15 | Every abandoned call is reported to its holder exactly once and stays open until reply; the reply reaches nobody (or `discarded` result if holder replies first). | model/src/invariants.rs, model/src/mutation.rs |

No sub-rule R4a/R4b conflict: they extend R4 in the same source paragraph, not separate IDs
cited elsewhere. `model/src/lib.rs` states the R1-R12 / I1-I15 range explicitly, confirming
no gaps and no IDs beyond I15/R12 currently exist in code.

## Part 2: residuals, caveats, gaps, follow-ups, non-goals, decisions

### S-1: Residual risks named by the tenets
- type: residual-risk
- source: todo/DOC1-docs-rewrite.md#What Redoubt is (brief's restatement of TENETS.md)
- statement: "Residual risks are stated, never hidden": the approval human; host virtio emulation; allowed egress channels (a model API can carry data out); side channels. Claims are "enumerated walls, each attack-tested", never "impossible".
- status: planned
- destination (proposed): TENETS

### S-2: Non-goals (must appear in TENETS)
- type: non-goal
- source: todo/DOC1-docs-rewrite.md#What Redoubt is
- statement: No display/keyboard/mouse/GUI; no Unix signals, fork, setuid or root; no POSIX shell; no symlinks or hard links (namespace binds instead); no swap and no IPv6; no inbound services except SSH (and SFTP/SCP inside it).
- status: planned
- destination (proposed): TENETS

### S-3: Beyond-M5 non-goals
- type: non-goal
- source: todo/DOC1-docs-rewrite.md#Milestones
- statement: The softcore/FPGA platform, SMP and full rv32; Python and Java runtimes ported to Rust (slow accepted, same audit rules apply); the web stack; unattended operation (backup, crash records, field updates, monitoring, a rescue console); a Rust OS facade, swap-like ideas, and others.
- status: open
- destination (proposed): beyond

### S-4: R3 user-space device alias residual
- type: residual-risk
- source: docs/legacy/STATUS.md#Memory and devices
- statement: "User code retains a writable kernel-only physmap alias." Also: DMA drivers remain trusted while they live; non-virtio DMA devices are never reset, so their drivers get no restart.
- status: open
- destination (proposed): kernel (memory/devices page)

### S-5: Device-policy questions open
- type: open-question
- source: docs/legacy/STATUS.md#Memory and devices
- statement: "Device-policy questions 142-146 are open" (device object cost/owner, loader device authority, two holders of one MMIO handle, R11 page-table-free not implemented, device mapping result/named handles).
- status: open
- destination (proposed): kernel (devices) / plan/M4 or M5

### S-6: IPC1 not accepted
- type: acceptance-gap
- source: docs/legacy/STATUS.md#Acceptance gaps
- statement: "IPC1 is not accepted." Host executable model recovered (flat weighted scheduling, independent IPC outcomes, call-output rollback) but host trace checks are not real-kernel replay (WP-C1 work); K5 real-timer cases pending; native exit/loan cleanup has real-kernel coverage; shared server's terminal fallback serving path remains host-tested pending server boot integration; concurrent completion coverage remains open, to be reconciled with PLAN's post-M1 SMP scope.
- status: open
- destination (proposed): plan/M1 (separation and containment)

### S-7: Question 171 (late-invalid receive output)
- type: open-question
- source: docs/legacy/STATUS.md#Acceptance gaps; docs/legacy/QUESTIONS.md#171
- statement: "A receive output record becomes invalid while its thread waits" — unresolved; the model rejects dependent scenarios explicitly, but initial receive validation and call/reply completion only are covered.
- status: open
- destination (proposed): kernel (IPC) / plan/M1

### S-8: Questions 164-165 (mediation and authority closure)
- type: open-question
- source: docs/legacy/STATUS.md#Acceptance gaps; ASTRA.md D1/D2
- statement: Confined placement vs. approved steward mediation paths (164/ASTRA D1), and what authority set is closed under permitted same-label delegation (165/ASTRA D2) are unresolved. Answer 166 made wakeup latency a measured target (RESOURCES.md), not a bound; WP-K5 implements the tie rule and bench case. These qualify the security/latency claims, not just an implementation schedule.
- status: open
- destination (proposed): SECURITY / TENETS (threat-model caveat) / plan/M1

### S-9: consoled unknown-request handle cleanup
- type: acceptance-gap
- source: docs/legacy/STATUS.md#Acceptance gaps; root ASTRA.md "Outstanding findings" (A3)
- statement: "consoled unknown-request handle cleanup and the broader raw-syscall/owning-runtime composition need follow-up." ASTRA A3: close attached handles on rejection and test the actual serving path; the raw-syscalls-combined-with-owning-runtime-views item requires a separate audit of this inherited API soundness boundary — the IPC regression does not certify arbitrary combinations.
- status: open
- destination (proposed): servers/consoled ; todo

### S-10: K4 not fully accepted (bundle readback gate)
- type: acceptance-gap
- source: docs/legacy/STATUS.md#Acceptance gaps; BUILD-PLAN.md (K4 section, ~line 104, 121-123)
- statement: "K4 is not fully accepted." Native lifecycle integration and kernel-notice-based `wx` verdicts are implemented; clean guest bundle-file readback remains an explicit gate for the R2/R3 startup handoff (answer 169); the current bundle-file case still tests loader refusal, not clean readback.
- status: open
- destination (proposed): plan/M1 ; kernel (boot/loader)

### S-11: Hosted-kernel test target does not compile
- type: acceptance-gap
- source: docs/legacy/STATUS.md#Acceptance gaps
- statement: "The hosted-kernel test target fails to compile: both the recovery baseline and final candidate report 112 compiler errors." It is not part of the registered bench; native QEMU results do not establish that hosted target's compatibility.
- status: open
- destination (proposed): todo

### S-12: Unsafe ratchet does not prove full TCB configuration
- type: caveat
- source: docs/legacy/STATUS.md#Verification
- statement: "The unsafe ratchet rejects missing or empty configured source roots; it does not prove that all TCB components were configured." Runtime ceiling 9; blkd 4; bootfsd/consoled together 0; all require zero undocumented uses.
- status: built
- destination (proposed): SECURITY

### S-13: Server host/build registration is not boot integration
- type: caveat
- source: docs/legacy/STATUS.md#Verification
- statement: "Server host/build registrations do not establish boot integration." Applies throughout the recovery evidence entries (2026-09-22 regressions): they add permanent verification coverage, not acceptance of outstanding packages, nor boot integration, nor closure of bundle-file readback/timer-preemption/concurrent-completion/real-kernel-model-replay acceptance.
- status: built (partial)
- destination (proposed): SECURITY / plan pages, as a standing caveat on "built" claims

### S-14: Acceptance gates and decision ownership table
- type: acceptance-gap
- source: docs/legacy/BUILD-PLAN.md#Acceptance gates and decision ownership (~lines 57-67)
- statement: Five named gates, each with owner/enabling work and closure rule: IPC1 model replay (owner: C1, after integrated IPC1/K5 and R3 bundle handoff; native replay evidence feeds acceptance, C1 does not wait for final acceptance); IPC1 serving-path cleanup (owner: native server startup through R3; actual serving paths must exercise terminal fallback/rollback, host coverage is partial evidence only); IPC1 concurrent completion (owner: Architect scopes against PLAN's post-M1 SMP section, owner decides; gate preserved until resolved, single-hart cases are not simultaneous multi-hart evidence); R3 full-server boot and blame (owner: later D2/D3/S2/S3 integration; close with the real milestone manifest and steward, infrastructure integration is not acceptance).
- status: open
- destination (proposed): plan/M1

### S-15: G1 launch gates cleared
- type: decision
- source: docs/legacy/BUILD-PLAN.md (~line 22)
- statement: "G1 cleared the launch gates on 2026-09-23 (review debt and the K5 contract; SWARM records the [decision])."
- status: resolved
- destination (proposed): todo (historical decision, not carried forward as open work)

### S-16: Native replay / IPC1 contract gate (question 171 dependency)
- type: acceptance-gap
- source: docs/legacy/BUILD-PLAN.md (~lines 257-275)
- statement: "The WP-M1 host model is integrated; real-kernel trace replay remains WP-C1 work." Completion gates: WP-M1 integration (complete), WP-K5 real timer support, and repair of ASTRA C4's verification gap. Contract gate: question 171 and relevant open model/kernel questions must have an explicit resolution before native acceptance runs (needs integrated WP-M1, WP-K1 to WP-K5, WP-T1 and integrated work for trace files); C1 supplies replay evidence, no temporary boot-data ABI is implied.
- status: open
- destination (proposed): plan/M1

### S-17: Device/startup and confinement contract gates
- type: acceptance-gap
- source: docs/legacy/BUILD-PLAN.md (~lines 340, 439, 455)
- statement: Contract gates require resolving device/startup questions 142-149 and a confinement question before certain packages close; "Restarting netd, or any driver [...]" is its own gate; admission-chain question 141 affects shared-server production startup needed for boot acceptance; R3's gate needs WP-K5b's DMA reset (already in).
- status: open
- destination (proposed): kernel (devices), servers/netd, plan/M1

### S-18: sshd milestone-1 residual
- type: residual-risk
- source: docs/legacy/BUILD-PLAN.md (~line 514)
- statement: "this sshd in milestone 1, a stated residual" — sshd's milestone-1 scope carries a named residual (console/session isolation boundary not fully closed at that stage; see surrounding S3 acceptance text on approval-channel isolation and logout/VM-death cleanup).
- status: planned
- destination (proposed): servers/sshd ; plan/M1

### S-19: S3 console acceptance partial
- type: acceptance-gap
- source: docs/legacy/BUILD-PLAN.md (~lines 524-535)
- statement: Full console acceptance additionally needs B2a's `consol` codec and R1d's typed parking; approval-channel isolation and logout/VM-death cleanup must hold while the other session remains usable; "the other channel's waiter remains parked and learns no geometry"; basic byte-stream SSH bring-up is not full S3 acceptance.
- status: open
- destination (proposed): servers/sshd, servers/consoled ; plan/M1

### S-20: Device grants retired (historical, not a residual)
- type: decision
- source: docs/legacy/DEVICE-GRANTS.md
- statement: Grants (per-process MMIO/IRQ claim lists in the signed bundle) are retired (WP-K6). Devices now reach a process only as handles to kernel device objects. INTERIM: the bundle's first program holds every device object until `init` places each driver's handles in its startup block per the boot manifest (INIT.md). Enforcement: loader refuses a `grants` bundle entry (`loader-rejects-grants`); kernel refuses a `Grnt` boot argument; bench refuses `[[grant]]`; old claim calls are `InvalidArgument` (`legacy-gone`).
- status: built
- destination (proposed): kernel (devices) — cite as historical rationale, not a live caveat

### S-21: Device object interim authority holder
- type: residual-risk
- source: docs/legacy/DEVICE-GRANTS.md
- statement: "For now the bundle's first program holds every device object (INTERIM...)" — a single early process holds broad device authority until per-driver placement lands.
- status: open
- destination (proposed): kernel (devices) ; plan/M1 or M4

### S-22: ASTRA D1/D2 owner decisions
- type: open-question
- source: root ASTRA.md#Outstanding findings
- statement: "D1-D2: confinement mediation, authority closure" — owner decisions pending at QUESTIONS 164-165; retain the qualifications beside the affected guarantees.
- status: open
- destination (proposed): SECURITY / TENETS

### S-23: ASTRA D3 wakeup bound settled
- type: decision
- source: root ASTRA.md#Outstanding findings
- statement: "D3: wakeup bound" settled by answer 166: a measured responsiveness target in RESOURCES.md, not a hard bound. WP-K5 implements the tie rule and records the measurement.
- status: resolved
- destination (proposed): kernel (scheduling) — cite as the current rule, not an open item

### S-24: ASTRA legacy interfaces / speculative APIs
- type: follow-up
- source: root ASTRA.md#Outstanding findings
- statement: "Legacy interfaces and speculative APIs (S1/S2/S5): Finish K6 migration; assess unused flatipc crates and grow the client API from integrated callers."
- status: open
- destination (proposed): todo

### S-25: ASTRA smaller containment acceptance gate (S4)
- type: acceptance-gap
- source: root ASTRA.md#Outstanding findings
- statement: "Smaller containment acceptance gate (S4): Establish a kernel/runtime gate before relying on full-product acceptance."
- status: open
- destination (proposed): plan/M1

### S-26: ASTRA recovery constraints
- type: security-caveat
- source: root ASTRA.md#Recovery constraints
- statement: "Do not merge the supplied remote branches wholesale." D1/R4 source already recovered; K4 lifecycle selectively recovered, its bundle-readback gate remains for R2/R3. The recovered host model still needs real-kernel replay; late-invalid receive output awaits decision 171. Preserve the protected loan mappings, outcome ABI and rollback checks. D3's external implementation remains unverified.
- status: open
- destination (proposed): todo (process caveat — do not carry the branch-recovery narrative itself into new docs, only the still-open technical items already listed as S-6/S-7)

### S-27: Milestone capability table (legacy numbering)
- type: decision
- source: docs/legacy/HISTORY.md
- statement: Implemented: RustSBI/QEMU boots rv32/rv64; signed bundles, budgets, handles, IPC, device interfaces; host-tested native server components. Next (legacy "milestone 1"): integrated SSH sessions and a leased, contained agent, backed by attack tests. Legacy "milestone 2": signed packages, projects, persistent policy, A/B updates. Legacy "milestone 3": self-hosted development.
- status: built (partial, see STATUS)
- destination (proposed): plan/ (map legacy milestone numbers to brief's M1-M5 explicitly; they do not correspond 1:1 — legacy M2/M3 fold into brief M4/M5)

### S-28: Documentation-map ownership rule
- type: decision
- source: docs/legacy/ARCHITECT-NOTES.md
- statement: Keep rules and rationale in the owning specification; do not maintain a separate decision/progress log elsewhere. Changing a wire table requires the generator drift check.
- status: built
- destination (proposed): SWARM (absorbed as a process rule, not a page of its own)

### S-29 through S-36: Carry into todo/ (brief's explicit list, verbatim follow-ups)
- type: follow-up
- source: todo/DOC1-docs-rewrite.md#Carry into todo/
- statement (each is its own todo item, listed together here to avoid renumbering the brief):
  1. "The sched-latency steward decision-wake target miss on rv64: re-pin the target or change the test. This is an owner decision."
  2. "The R10 (budget destruction) cost near its 30 ms target: fix before the steward work."
  3. "A masked IRQ lost before the first receive (level-latch)."
  4. "The loader stub's test-coverage follow-ups."
  5. "The kernel print! panic re-entry."
  6. "process_map backing its source before refusing bad flags."
  7. "K5's carve-lead rescale follow-up."
  8. "The host-side ssh-loopback bench cases fail on this host."
- status: open
- destination (proposed): todo (one file each, per the brief's layout)

### S-37: KERNEL-SPEC milestone-2 addition
- type: follow-up
- source: docs/legacy/KERNEL-SPEC.md#Added in milestone 2
- statement: `budget_children(h) -> [h]` is added so a restarted steward can enumerate and destroy what it created (CAPABILITIES.md); nothing else in the spec changes for milestone 2.
- status: planned
- destination (proposed): kernel (budgets) ; plan/M5 (persist, install, share) — legacy "milestone 2" maps toward the brief's M5 steward-persistence scope, not M2 (usable shell)

## Notes for the red team / lead

- ANSWERS.md and QUESTIONS.md are reader A's territory; only referenced here where STATUS/BUILD-PLAN/ASTRA
  point at a specific question number, to keep this file's citations checkable against reader A's map.
- The WORKSPACE-QA content was already moved to `.wash/QA.md` per commit c1b1fb681 and is not a
  documentation source.
- BUILD-PLAN.md is 553 lines with a per-package (WP-*) acceptance narrative; S-14 through S-19 pull
  the cross-cutting gates and named residuals. Package-level narrative detail (dependency chains,
  wave numbers) is process material per Page rule 6 and is deliberately not carried forward.
