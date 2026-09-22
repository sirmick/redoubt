# Redoubt: three-reviewer assessment

Date: 2026-09-22. Initial base commit: `d9ed817ec5686b3797bc7d43c4c0f2ef2adc90d3`, plus the existing working-tree changes. The renewed adversarial review began after the owner's RustSBI changes landed as `61fd197f93a37485c9bd6f505e0c6ccfe25e4d0f`.

Three reviewers examined (1) design/code consistency, (2) adversarial security concerns, and (3) simplification of code, features, and documentation. The coordinating reviewer checked the principal source paths, ran host tests, reran two isolated reproductions, and consolidated the findings below. This is an AI-assisted review, not a security certification.

**Development context:** Redoubt is an OS under construction. This review challenges the intended design and the prototype components already present; it does not describe a deployed system or an attack on an external target. Severity indicates what should be resolved before integration. Explicitly planned but unbuilt features are kept separate from defects in implemented contracts.

The owner identified RustSBI changes in flight. Firmware and related boot/build edits were left untouched, and transient firmware discrepancies are not findings here. This was a live-tree review, not a review of an immutable snapshot. No implementation changes were made as part of this review.

**Review completion and limits:** the adversarial reviewer completed a subsequent **design-only pass**, at the owner's request, producing D1–D4 below. That pass read specifications only: no implementation inspection, tests, or exploit construction. Its earlier code-focused passes were interrupted by an Astra/Daybreak service restriction and remain partial; A1–A3 preserve their completed observations, without attributing any reproduction result to that reviewer. The consistency and simplification reviews completed. The host reproductions below came from the consistency reviewer and were independently rerun by the coordinator.

## Assessment

The design has a coherent containment model, and the code contains meaningful implementations of its difficult parts: scoped revocation, accounting, label checks against endpoint ownership, admission control, and handling of messages already in flight. The most urgent problems found are where individually reasonable components disagree about the meaning of an IPC result.

In particular, a taken call can consume its lender's memory on abandonment, but the safe runtime continues to represent that memory as an owned buffer. Replies can deliver some handles while returning an error, but the runtime discards their identities. Servers also infer delivery from a successful reply syscall, although success includes discarding an abandoned reply. These contracts need to be settled together before more clients depend on them.

The verification machinery also needs attention: the unsafe-code ratchet passes while silently skipping several nonexistent configured directories. A passing check currently provides less evidence than its label suggests.

The completed design-only pass identifies four areas to settle before integration: the trusted mediation boundary within confinement, the scope of capability closure under delegation, the scheduler's actual wakeup guarantee, and explicit IPC ownership/completion outcomes. D4 is the specification-level root of earlier IPC findings, not another independent defect. The architect round in section 4 routed these to questions 164–168 and separated already-settled implementation obligations. The owner subsequently approved 167–168; section 5 records that follow-up. Questions 164–166 remain open.

### Findings at a glance

Severity describes the identified defect; simplification priorities are recommendations, not vulnerability ratings. “Host reproduction” means the real runtime was exercised against a small substitute kernel implementing the relevant documented behavior, not a booted kernel.

D1–D4 are design clarification priorities for the intended OS, not deployed vulnerability ratings.

| ID | Priority/severity | Finding | Evidence |
| --- | --- | --- | --- |
| C1 | High | Abandoned lend leaves a stale, safely accessible `Buffer` | Source trace + host reproduction |
| C2 | Medium | Partially delivered error replies lose surviving handle identities | Source trace + host reproduction |
| C3 | Medium | Initially read-only call record is rejected only after server effects | Source trace; boot test needed |
| C4 | High assurance gap | Unsafe ratchet passes while skipping missing source roots | Executed checker + source inspection |
| A1 | Medium | Abandoned reply success defeats grant/connection rollback | Static contract mismatch; integration test needed |
| A2 | Investigate; potentially high impact | MMIO records reach RAM-only record accessors | Static validation gap; no boot reproduction |
| A3 | Medium | Console rejects unknown requests without closing attached handles | Renewed review + independent source trace; regression test pending |
| D1 | High design priority | Confinement does not define its trusted mediation exception | Completed design-only review |
| D2 | High design priority | Per-agent capability closure conflicts with permitted delegation | Completed design-only review |
| D3 | Medium design priority | Stride scheduling does not imply the stated wakeup bound | Specification reasoning; no timing measurement |
| D4 | High design priority | IPC contract does not expose ownership/completion distinctions | Design-level root of C1/A1; not an independent defect |
| S1 | High priority | Finish migration and retire the legacy syscall/IPC surface | Two dispatch paths remain active |
| S2 | Medium priority | Remove unused `flatipc` crates | Dependency graph and source search |
| S3 | High priority | Reconcile progress claims with the existing SWARM ledger | Contradictory progress claims |
| S4 | High priority | Add a smaller kernel-containment acceptance gate | Current acceptance depends on the full product |
| S5 | Medium priority | Grow one client API from integrated callers | Existing runtime plus speculative facade |

## 1. Design/code consistency

### C1 — High: abandoned lends invalidate the safe Buffer abstraction

**Contract.** [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), lines 182–189 (R3), says a taken call abandoned by timeout, revocation, or caller death leaves its lend with the server. The caller's ownership/charge ends.

**Implementation.** [kernel/src/message.rs](kernel/src/message.rs), lines 1272–1297, implements abandonment by removing the caller's mapping and moving frames to the server. [kernel/src/arch/riscv/mem.rs](kernel/src/arch/riscv/mem.rs), lines 586–596, implements `drop_lent` with `Pte::EMPTY`. However, [libs/rt/src/ipc.rs](libs/rt/src/ipc.rs), lines 118–132, accepts `Option<&mut Buffer>` and returns on syscall error with the original object unchanged. Its safe `Deref`/`DerefMut` implementations at lines 61–76 assume the mapping remains valid until drop; its destructor at lines 79–83 unmaps the old address.

**Consequence.** After a server takes a lend and the call times out, the caller retains an object that claims to own absent memory. Access can fault. If the virtual address is reused, the stale object's destructor can unmap a new allocation. Two safe objects can also refer to the same reused address. This breaks the runtime's memory-safety argument; no cross-budget memory escape was demonstrated.

**Validation.** An isolated `HostKernel` implementation simulated the documented post-delivery timeout, then reused the freed virtual address for another `Buffer`. Dropping the first buffer issued `Unmap` against the replacement. Both the reviewer and coordinator ran this successfully; no invalid pointer was dereferenced by the reproduction. This establishes the runtime mismatch, not a complete real-kernel exploit.

**Resolution.** Represent lend disposition explicitly. A consuming call API with a returned-versus-consumed buffer result is one option, but the kernel/ABI must supply enough information to implement it. The same `Timeout` or `Dead` can currently mean either a queued call whose buffer is restored or a taken call whose buffer is consumed. Invalidating on every error would therefore lose ownership in some cases; retaining on every error is unsafe.

**Acceptance test.** On a real kernel, cover timeout and revocation both before and after receipt, normal reply, server death, subsequent address reuse, and buffer destruction. Verify mapping ownership and charges as well as the returned error.

### C2 — Medium: partial replies are delivered by the kernel but lost by the runtime

**Contract.** [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), lines 199–203 (R4), permits a reply to arrive with some handles missing: successful slots and words are delivered, missing slots are zero, and `call` returns `OutOfMemory`.

**Implementation.** [kernel/src/message.rs](kernel/src/message.rs), lines 1219–1227, installs the handles that fit and writes the reply with that status. [libs/rt/src/ipc.rs](libs/rt/src/ipc.rs), line 130, uses `nothing(syscall(&call))?`, returning before the reply record is decoded.

**Consequence.** If one of two reply handles fits, that handle is installed but its index is neither exposed nor closed. Repetition leaks caller handle slots. Losing reply words can also lose the identifier required to release server-side state. This is a resource/contract defect, not evidence of privilege escalation.

**Validation.** A host seam supplied words `[42, 0, 0, 0]`, surviving handle `77`, one empty handle slot, and `OutOfMemory`. The actual runtime returned only the error and did not close `77`. Independently rerun successfully.

**Resolution.** Separate “a reply was delivered” from “all requested handles were delivered.” Preserve the reply on the partial-delivery path. If a higher-level API deliberately discards it, close every surviving handle first. Do not decode arbitrary output records for errors that occurred before delivery.

**Acceptance test.** Exercise a real two-handle reply with capacity for one, verify the words and both slot dispositions, then show that handling or discarding the result leaves no untracked handles.

### C3 — Medium: call output-record validation occurs too late

**Contract.** [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), lines 341–360, requires output-record writability to be checked during decoding, before handle, permission, resource, or delivery effects.

**Implementation.** The shared call/send path in [kernel/src/message.rs](kernel/src/message.rs), line 625, only uses `read_record`. In [kernel/src/redoubt.rs](kernel/src/redoubt.rs), lines 192–195, that validates with `write = false`. Calls later write their reply into the same record: `message.rs`, lines 1223–1226. Writability is checked by `write_record` only at that point (`redoubt.rs`, lines 204–208).

**Consequence.** A valid call body placed in initially read-only, backed RAM can reach the server and produce effects before returning `InvalidArgument` when the reply cannot be written. Handles may already have been installed. With an additional bad endpoint, the implementation can also report the wrong first error. This differs from the documented concurrent-unmapping race: the record was invalid as an output from the outset.

**Validation.** Confirmed by tracing source; no QEMU reproduction performed.

**Resolution and test.** Validate call records as both input and output during stage 1, while leaving send records input-only. Retain the later recheck for concurrent changes. Add a test proving the receiver sees no request when the call's record is initially read-only, plus an error-precedence case.

### C4 — High assurance gap: the unsafe ratchet silently counts nonexistent paths as zero

**Evidence.** [tests/unsafe-budget.toml](tests/unsafe-budget.toml), lines 11, 25, 70, 76, and 82, names:

| Configured root, absent in this tree | Actual source root |
| --- | --- |
| `redoubt/paging/src` | `libs/paging/src` |
| `redoubt/signing/src` | `libs/signing/src` |
| `redoubt/sys/src` | `libs/sys/src` |
| `redoubt/rt/src` | `libs/rt/src` |
| `redoubt/keyd/src` | `servers/keyd/src` |

[tools/testbench/src/budget.rs](tools/testbench/src/budget.rs), lines 46–54, returns success for a missing extensionless path because it is neither a directory nor an `.rs` file.

**Observed result.** Running the checker returned `PASS`, reporting zero unsafe uses for paging, sys, and rt and even suggesting their budgets could be lowered to zero. The runtime source alone visibly contradicts that count. The loader/kernel counts are not invalidated by this particular missing-path defect, and crates with `forbid(unsafe_code)` retain that separate compiler check.

**Consequence.** Several advertised coverage areas are not scanned. This is a demonstrated false-green check, not merely stale prose. It does not by itself establish that their real counts exceed the intended budgets.

**Resolution.** Correct the paths; fail on missing configured roots and unexpected empty source sets; test the checker against a deliberately missing root. Consider deriving roots from crate manifests and checking coverage of the declared TCB. Adding new unlisted kernel modules should not silently bypass the ratchet either.

### Explicitly unfinished work is not a newly discovered defect

The new process/thread syscalls are documented as pending WP-K4, budget deadlines and stride preemption as pending WP-K5, and the legacy interface as pending removal in WP-K6. The executable security model is listed as in review; model/kernel replay follows it. Questions 138–146 already track several interim behavior differences. These should be visible in a conformance ledger, but should not be presented as surprise vulnerabilities or as guarantees already delivered.

The host test double is also candid about its limitations: [libs/rt/tests/common/mod.rs](libs/rt/tests/common/mod.rs), lines 3–11, omits budgets, charging, label enforcement, fair waiting, and lender unmapping. Passing those tests cannot validate the missing behaviors. C1 and C2 show why tests at the real kernel/runtime boundary are necessary alongside host tests.

## 2. Adversarial review

The design-only pass below completed successfully. Earlier, partial prototype-code observations follow it and remain clearly separated. These are findings to resolve while building the OS, not claims about a deployed system.

### D1 — High design priority: define trusted mediation within confinement

**Evidence.** [INIT.md](docs/INIT.md), lines 76–108, makes `confined` a boot-wide prohibition on differing label sets sharing servers, endpoints, devices, or cores. It explicitly treats the unlabelled steward as a domain of its own. Yet [CONTAINMENT.md](docs/CONTAINMENT.md), lines 58–67 and 78–89, requires the steward to communicate with labelled reader/writer helpers for declassification and push. [CAPABILITIES.md](docs/CAPABILITIES.md), lines 177–185, also requires labelled requests and an owner approval path.

**Design question.** Which trusted mediation edges are permitted across the boundaries that the blanket placement rule forbids sharing? The documents do not specify a scoped exception or a per-domain control-plane topology reconciling these requirements. They also explicitly put post-boot handoffs outside the static manifest check without defining the full policy obligation that preserves confinement afterward.

**Consequence for implementation.** A strict reading can reject the intended workflow; an ad hoc exception can permit more sharing than the stated guarantee intends. This is an underspecified trust-boundary topology, not proof of an information leak.

**Recommended clarification.** Specify a permitted confinement graph: ordinary per-domain services, named trusted mediators, the operations each may carry across boundaries, and the component responsible for preserving that graph during dynamic grants and helper creation. Give one valid worked confined configuration supporting approval, push, declassification, and lease termination.

**Limit.** The acknowledged milestone-one shared-SSH residual is not a new finding. This question concerns the stronger `confined` profile and does not assume it is already implemented.

### D2 — High design priority: scope capability closure to authorized delegation

**Evidence.** [TENETS.md](docs/TENETS.md), lines 24–26, says an individual agent's authority cannot exceed its initial grants plus human-approved additions. [CAPABILITIES.md](docs/CAPABILITIES.md), lines 19–25, permits copying every handle and narrowing delegation. Meanwhile, `TENETS.md`, lines 30–32, and [ANSWERS.md](docs/ANSWERS.md), lines 233–240, deliberately treat equal-label budgets as one trust domain.

**Design question.** A permitted same-label peer can delegate a handle absent from the recipient's initial set. That delegation narrows relative to the donor while increasing the recipient's authority. The handle rules do not require another human approval for that transfer. What set is the closure guarantee actually over?

**Consequence for implementation.** Intended, correct delegation can violate the literal per-agent guarantee and the authority-expansion verdict in [GAME.md](docs/GAME.md), lines 27–29. The specification and its proposed evaluation would disagree about permitted behavior.

**Recommended clarification.** Define closure over the initial authority of the trust domain, or over the transitive closure of explicitly authorized delegation. State which interpretation is intended; they need not authorize the same graph. Distinguish a recipient acquiring a capability from the system creating authority outside the approved delegation graph, and align the game's win condition with that definition.

**Limit.** Preserve the owner's explicit equal-label trust-domain decision. This is not a proposal to forbid legitimate delegation or introduce nontransferable handles, which would not prevent proxying.

### D3 — Medium design priority: derive the wakeup bound from the scheduling rule

**Evidence.** [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), lines 253–259, selects the lowest pass and wakes a budget at `max(own pass, current minimum)`. [RESOURCES.md](docs/RESOURCES.md), lines 40–51, claims drivers and the steward wait at most about one `SLICE`.

**Design question.** Joining the minimum does not imply selection next when several runnable budgets share that minimum; tie ordering is unspecified. The `max` rule can also retain a waking budget's larger previous pass. Large weight governs future pass increments, not an unconditional next-turn right. Therefore the stated universal wakeup bound does not follow from R12 alone.

**Consequence for implementation.** A scheduler can implement the prescribed algorithm and still fail the responsiveness acceptance criterion. The guarantee supporting responsive human control is stronger than the specified mechanism.

**Recommended clarification.** State tie handling, retained-pass treatment, assumptions about runnable budgets, and a derived latency bound. Either make “about one slice” a measured target under named conditions or add the rule needed to guarantee it. Keep weighted fairness distinct from worst-case response time.

**Limit.** This is specification reasoning, not a timing measurement or an implementation defect. Deferred time donation and strict priority are not themselves findings.

### D4 — High design priority: specify ownership and completion as distinct IPC outcomes

**Evidence.** [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), lines 182–189 (R3), leaves a post-receipt abandoned lend with the server. Lines 212–215 (R4b) return a waiting caller's lend on server death while returning `Dead`. The `call` error row at line 382 exposes `Timeout`/`Dead` without a lend disposition. The `reply` contract at lines 283 and 385 permits discarding an abandoned reply without exposing a delivered-versus-discarded result.

**Design question.** The same call error can accompany different ownership states. Likewise, successful completion of a reply operation does not establish that its caller received the result. What information or protocol lets a client determine whether it still owns memory, and lets a server reclaim newly created state whose identifier was never received?

**Consequence for implementation.** Components can implement plausible but incompatible lifetime rules even while following their respective parts of the specification. This is the design-level root of C1/A1, with partial delivery also relevant to C2; it is not an unrelated discovery.

**Recommended clarification.** Add a normative lifecycle table specifying call status, lend ownership, reply disposition, and server-state cleanup responsibility. These existing cases show the distinctions it must cover:

| Lifecycle event | Status/disposition to distinguish | Lend consequence |
| --- | --- | --- |
| Queued call cancelled before receipt | Timeout or revocation, never delivered | Caller retains or regains its buffer |
| Taken call abandoned | Timeout or revocation after delivery | Lend remains with server until its reply frees it |
| Server dies while caller still waits | `Dead`, distinct from abandonment | Lend returns to caller |
| Normal reply | Reply delivered | Lend returns to caller |
| Partial reply-handle delivery | `OutOfMemory` plus delivered words/surviving slots | Lend returns to caller |
| Reply to an already abandoned call | Operation completed, reply discarded | Server frees abandoned lend; provisional server state needs a defined cleanup policy |

An explicit disposition result is one solution; another protocol is acceptable if it supplies equivalent information. Any resulting ABI or policy change should be recorded through the project's existing design-change process.

### Earlier prototype-code observations — partial

The following observations were completed before the service interrupted the code-focused passes. Their previous source checks remain relevant, but no new code inspection or tests were performed for the design-only pass. C1 is recorded once in section 1 rather than counted again here.

### A1 — Medium: abandoned replies leave unreachable grant/connection state

**Prerequisite.** A caller already authorized to obtain a grant or connection abandons a taken call after the server creates the result but before its reply is delivered.

**Evidence.** [kernel/src/message.rs](kernel/src/message.rs), lines 1208–1217, discards an abandoned reply and returns `Ok(())`. [libs/rt/src/server/typed.rs](libs/rt/src/server/typed.rs), lines 185–194, forwards that reply result. But [servers/keyd/src/server.rs](servers/keyd/src/server.rs), lines 125–135, and [libs/rt/src/server/ninep.rs](libs/rt/src/server/ninep.rs), lines 495–505, undo a newly minted record only if replying returns an error.

**Consequence.** The record and its admission charge survive although their identifying reply never reached the caller. Repetition can exhaust the affected share/bucket. No cross-account denial of service was established. `keyd` has requester-wide release behavior, so describing these entries as necessarily permanent until reboot would overstate the result.

**Test gap.** [servers/keyd/src/server_tests.rs](servers/keyd/src/server_tests.rs), lines 724–735, tests rollback by calling `forget_badge` directly. It proves cleanup works when called, but not that actual abandoned IPC invokes it.

**Resolution.** Make reply delivery/discard disposition observable to server transactions, or give provisional server state a cancellation-aware lifetime and reliable reclamation. Any ABI change must be reconciled with the frozen spec. Test abandonment during the actual serving path, checking admission and grant/connection counts afterward. Include partial reply delivery from C2 in the same contract review.

**Confidence.** High in the source-level mismatch; its live-kernel occurrence and operational consequences remain untested here.

**Excluded claim.** An early suspicion about reply-encoding failure was withdrawn: `NineServer::answer_common` already rolls back that failure. It is not an additional finding.

### A2 — Investigation: MMIO can pass record validation intended for owned RAM

**Prerequisite.** A process has an MMIO device mapping and supplies an address in that mapping as a syscall record. This is not a demonstrated route available to an agent without device authority.

**Evidence.** [kernel/src/arch/riscv/mem.rs](kernel/src/arch/riscv/mem.rs), lines 694–705, checks address range, user permissions, validity, and the lent-out bit in `user_frame`; it does not classify the physical page as RAM or establish physical ownership. [kernel/src/redoubt.rs](kernel/src/redoubt.rs), lines 178–208, uses it to obtain record frames and passes them to [kernel/src/kframe.rs](kernel/src/kframe.rs), lines 17–48, whose accessors assume RAM/physmap frames.

**Reasoned outcome.** On RV32, typical low-address MMIO is below `PHYSMAP_PHYS_BASE = 0x80000000` and reaches `kframe::at`'s assertion. This is a plausible device-holder-triggered kernel panic, not a boot-reproduced result. On RV64, the current loader maps the low physical range into the physmap ([loader/src/paging.rs](loader/src/paging.rs), lines 44–51); the access can therefore reach MMIO rather than being rejected as an invalid record. The result depends on device access width and behavior. No specific RV64 fault or escape is claimed. Firmware work in flight is a further reason to avoid claiming a final platform outcome.

**Resolution and test.** Validate records against the intended RAM and ownership rules before using RAM-only accessors. Use a non-DMA device holder to exercise this boundary, so an existing DMA trust exception does not obscure the result. Confirm that MMIO-backed input and output records return `InvalidArgument` without privileged faults or device side effects. RV32's deferred support status should remain explicit.

**Confidence.** High in the missing RAM classification; impact severity is provisional until a real boot test establishes the reachable outcome.

### A3 — Medium: console rejection leaves attached handles open

**Prerequisite.** A caller has authority to call the console endpoint and includes valid handles with a request whose opcode is neither 9P nor a supported `ninep_common` operation. It need not possess any authority over another process's handle table.

**Evidence.** [servers/consoled/src/bin/consoled.rs](servers/consoled/src/bin/consoled.rs), lines 120–123, supplies an unsupported-operation callback that calls `request.reply(MALFORMED, &[])` directly. [libs/rt/src/server/ninep.rs](libs/rt/src/server/ninep.rs), lines 488–490, dispatches unknown opcodes to that callback, bypassing the normal `finish` path that closes unused request handles. [libs/rt/src/ipc.rs](libs/rt/src/ipc.rs), lines 267–275, sends the reply but does not close received handles automatically. The handles are plain entries, with no request destructor that reclaims them.

**Consequence.** The request is rejected, but handles already installed in the console process remain open without application owners. Repeated rejected requests can consume console handle-table capacity and its memory budget outside the server's normal admission accounting. The kernel's receiver limits bound the growth; this is not an unbounded allocation claim. Its eventual effect on other console clients needs an integration test, but the retained-handle path is present in the prototype.

**Validation.** Found by the restarted adversarial reviewer and independently traced by the coordinator. The reviewer announced a bounded host regression check, but the service stopped it before it returned a result. No executed reproduction is claimed for A3.

**Resolution.** Send unsupported-operation replies through a shared rejection helper that closes every present, unexpected request handle, including partial handle lists. Keep closure policy explicit for extension callbacks that legitimately accept handles. Audit other direct rejection callbacks for the same omission rather than changing all `Request::reply` calls to close handles indiscriminately.

**Acceptance test.** Deliver an unsupported request with attached handles through the actual console serving path; verify the malformed reply and that its handle count returns to baseline. Include a partially revoked list and repeated rejection, then verify a normal client still makes progress.

### Controls worth preserving

The sampled code enforces user-to-user label equality against endpoint ownership, preserves mint stamps, preflights receiver costs, sweeps revoked handles inside queued messages, closes unexpected request handles, and addresses badge reuse after server restart. These checks should survive simplification. None of the findings above establishes a general escape from the kernel's capability boundary.

## 3. Simplification review

### S1 — High priority: retire the legacy syscall/IPC surface after migration

**Evidence.** [kernel/src/arch/riscv/irq.rs](kernel/src/arch/riscv/irq.rs), lines 220 and 234, dispatches both new and legacy calls. [kernel/src/redoubt.rs](kernel/src/redoubt.rs) explicitly describes coexistence. The legacy syscall/server implementation and ABI syscall file together contain more than 4,000 lines; this is a scale indicator, not a promised deletion count.

**Proposal.** Follow K4 → K5 → K6, and make K6 a gate for the first completed containment claim. Migrate supporting programs and attack cases, then delete the obsolete dispatcher, SID/CID machinery, callback interrupt interface, and unused ABI surfaces. Preserve shared internal types still needed.

**Tradeoff.** Large migration with substantial regression risk; legacy binaries intentionally cease working. Do not remove the working bootstrap before replacements exist. The benefit is having one public path through each security mechanism rather than auditing interactions between two.

### S2 — Medium priority: remove the unused flatipc workspace crates

**Evidence.** [Cargo.toml](Cargo.toml), lines 18–19, includes `libs/flatipc` and `libs/flatipc-derive`. The reviewer checked `cargo metadata --no-deps` and searched source/manifests: neither has another in-repository consumer, except `flatipc` using its derive crate. Together they contain roughly 1,232 Rust lines including tests.

**Proposal.** Remove those workspace members and unused sources, update the lockfile normally, and verify the remaining workspace. Check whether any external compatibility commitment is intended before deleting a published API.

**Tradeoff.** Small, low-risk cleanup with no identified application behavior change. This reduces navigation and maintenance cost; it does not shrink the present TCB by that line count, since unused workspace crates need not be linked. Other crates may still require the same proc-macro dependencies.

### S3 — High priority: reconcile status with the existing authoritative ledger

**Evidence.** [BUILD-PLAN.md](docs/BUILD-PLAN.md), line 594, says R4/D1 are building, while lines 597 and 599 call them merged. [ARCHITECT-NOTES.md](docs/ARCHITECT-NOTES.md), line 24, calls question 160 open, while [ANSWERS.md](docs/ANSWERS.md), line 414, records the owner's decision. [STATUS.md](docs/STATUS.md), line 18, says no virtio driver is built, although [servers/blkd/src/lib.rs](servers/blkd/src/lib.rs) contains a substantial implementation with integration explicitly pending in its binary.

**Process correction after reading SWARM.md.** [SWARM.md, Claims](docs/SWARM.md#claims) already declares its table the source of truth for package state; BUILD-PLAN.md's Order derives from it. The finding is drift from that existing ledger, not the absence of a ledger. Reconcile the conflicting summaries against Claims, distinguishing a merged component from its pending boot/integration acceptance. Link to the existing record rather than introducing a competing status table. Keep normative behavior in the owning specifications and historical rationale in QUESTIONS/ANSWERS/HISTORY; use warm-start notes as pointers rather than mutable progress copies.

**Tradeoff.** Low runtime risk and moderate editorial effort. Preserve the design and decision history; remove repeated progress narratives. Protocol tables used by generation require generator checks if edited. Firmware-specific status remains provisional while the owner's changes land.

### S4 — High priority: add a smaller containment acceptance gate

**Evidence.** [PLAN.md](docs/PLAN.md), lines 10–28, describes a thin slice that nevertheless includes beamlet, IEx, init, storage, networking, steward/key service, SSH, and the leased agent. Final acceptance in [BUILD-PLAN.md](docs/BUILD-PLAN.md), lines 583–587, depends on the whole stack.

**Proposal.** Add an intermediate real-QEMU gate using native programs: start hostile code in one budget, enforce preemption/deadlines, revoke a subtree with messages and lends in flight, and prove a separate victim remains responsive. Add model conformance when its implementation and K4/K5 make that possible. Then layer the existing SSH/BEAM product milestone on it.

**Tradeoff.** Medium test/integration work, little architectural risk. It preserves the desired product scope. The smaller gate proves kernel primitives only; it must not be presented as validation of the future steward, approvals, networking, or complete user experience.

### S5 — Medium priority: grow one client API from real callers

**Evidence.** [OS-API.md](docs/OS-API.md), lines 37–54, proposes a nine-module `redoubt-os` facade. Existing client/runtime mechanisms live in [libs/rt/src/client.rs](libs/rt/src/client.rs) and [libs/rt/src/handle.rs](libs/rt/src/handle.rs). The draft API's rejection of a `std::fs` shim also needs reconciliation with [PLAN.md](docs/PLAN.md), lines 131–137, which still describes a future Rust `std` backend.

**Proposal.** Keep the full facade draft future-facing. Add only the operations needed by the next integrated clients to the existing capability-explicit layer, preserving one implementation of reply validation, handle lifetime rules, and error translation. Extract a broader facade once multiple working callers establish its shape. Resolve the `std` direction in one owning document.

**Tradeoff.** Low-risk sequencing change. Preserve ergonomic APIs as a goal; defer speculative wrappers rather than needed behavior. C1/C2 make stabilizing the underlying ownership/error contract especially important first.

### Simplifications not recommended

Retain bounded linear scans where they keep invariants readable, the one-outstanding-request virtio queue, shared admission/label checking, and the shared parked-call mechanism. Do not remove console resize as generic scope trimming: the owner explicitly decided to keep it in answer 160. Do not weaken hostile-input checks to meet a line-count target.

## Validation performed

The coordinator ran:

```sh
env CARGO_HOME=/data/redoubt/.cargo RUSTUP_HOME=/data/redoubt/.rustup \
  /data/redoubt/.cargo/bin/cargo test --offline --locked \
  -p redoubt-sys -p redoubt-rt -p redoubt-wire -p redoubt-signing \
  -p redoubt-bootfsd -p redoubt-consoled -p redoubt-blkd -p redoubt-keyd --quiet

env CARGO_HOME=/data/redoubt/.cargo RUSTUP_HOME=/data/redoubt/.rustup \
  /data/redoubt/.cargo/bin/cargo run --offline --locked --quiet \
  -p testbench -- unsafe-budget
```

The host suite reported **254 passed, zero failed, three ignored** across the selected packages and their test targets. Two ignored cases require the optimized keyd timing build; the third is a wire-vector printing helper. These results do not cover the whole workspace, beamlet's differential suite, fuzz campaigns, or the real-boot matrix. The unsafe check returned `PASS`, but C4 explains why that result is not valid evidence for all configured components.

Two temporary reproduction programs were created outside the repository and inspected/rerun by the coordinator:

```sh
env CARGO_HOME=/data/redoubt/.cargo RUSTUP_HOME=/data/redoubt/.rustup \
  /data/redoubt/.cargo/bin/cargo run --offline --quiet \
  --manifest-path /tmp/redoubt-consistency-OmtXwr/Cargo.toml \
  --bin redoubt-consistency-repro

env CARGO_HOME=/data/redoubt/.cargo RUSTUP_HOME=/data/redoubt/.rustup \
  /data/redoubt/.cargo/bin/cargo run --offline --quiet \
  --manifest-path /tmp/redoubt-consistency-OmtXwr/Cargo.toml --bin lend
```

The first reports that the delivered words/surviving handle were discarded; the second reports that a stale buffer unmapped a replacement allocation. These scratch paths are session-local evidence, not committed regression tests. Their scenarios are specified under C1/C2 so maintained tests can replace them. No QEMU reproduction was run for this review.

The code-focused restart added source checks for A3 only; the earlier test totals above were not rerun and should not be read as fresh coverage of that finding. The subsequent design-only pass completed the confinement/control-plane examination and the other specification questions in D1–D4. It performed no implementation inspection or tests.

## 4. Architect round — 2026-09-22

The resident architect checked all 16 findings against the owning notes and prior decisions,
following [the architect Q&A protocol](.pi/skills/architect-qa/SKILL.md). The round opened
[questions 164–168](docs/QUESTIONS.md#from-the-astra-architect-round-2026-09-22), each with a
recommendation and alternative. **At the close of that triage, all five awaited the owner.** No
answer was fabricated or normative change applied by triage, and no code or tests were changed.
The subsequent owner approval of 167–168 is recorded in section 5; other recommendations above
remain proposals, not accepted design decisions.

### Disposition of every finding

| Finding | Architect disposition | Next step / governing record |
| --- | --- | --- |
| C1 | Implementation symptom of D4; outcome contract now approved, implementation pending | Answer 167 and WP-IPC1; preserve R3/R4b and answers 49/70/81 |
| C2 | Existing contract violation, not a new policy choice | R4 and answers 107/116; preserve or close surviving reply handles; coordinate with Q167 |
| C3 | Implementation follow-up under existing validation order, now owned by WP-IPC1 | Validate output before delivery and retain the later recheck; no separate overlapping kernel package |
| C4 | Verification-tool follow-up, no design choice | Repair configured roots and fail closed on missing sources; add negative tests |
| A1 | Implementation symptom of D4's separate server-side gap; approved fix contract, implementation pending | Answer 168 and WP-IPC1; delivery/discard outcome and provisional-state cleanup |
| A2 | Bounded validation needed; impact remains provisional | Establish record-backing behavior on the real kernel before assigning final severity; no speculative ABI change |
| A3 | Implementation follow-up under an existing cleanup requirement | Answer 85 and WP-R1b; close unexpected handles through the actual rejection path |
| D1 | Genuine owner decision, not a reopening of push approval | Q164; reconcile answers 152/153 with the trusted mediation topology |
| D2 | Genuine owner decision about the claim's scope | Q165; answer 150's equal-label trust domain and legal delegation remain binding |
| D3 | Genuine owner decision revising a prior latency promise | Q166; answer 103 itself includes the one-slice claim, so weakening it is not an editorial correction |
| D4 | Split into two owner decisions, both subsequently approved; do not double-count C1/A1 | Answer 167 caller ownership/output validity; answer 168 server delivery/cleanup |
| S1 | Already planned, not a new architecture decision | Complete K4/K5 and the existing WP-K6 legacy-removal package |
| S2 | Proposed cleanup, not a frozen-design question | Recheck consumers and external commitments before a scoped removal package |
| S3 | Editorial reconciliation with an existing authority | SWARM Claims stays canonical; architect notes now append the correction that Q160 is answered |
| S4 | Proposed orchestration/test sequencing, not a product-scope change | Define an intermediate gate from existing kernel acceptance cases; keep WP-E1's full milestone |
| S5 | Draft/API sequencing work, not approval to rewrite the frozen interface | OS-API is unfrozen and PLAN's `std` backend is future work; reconcile scope without cancelling either, and retain existing userland questions and owner decisions |

### Owner choices

The full Rec/Alt text is in QUESTIONS.md; these are summaries, not substitute decisions.
167–168 were subsequently accepted as recommended (section 5); only 164–166 await the owner:

- **164 — Confinement:** explicitly bound trusted mediation, or separate control planes and use an external owner-mediated crossing.
- **165 — Authority closure:** state the bound over the label trust domain, or define a narrower transitive delegation/proxy graph per agent.
- **166 — Scheduling:** retain stride scheduling with a measured, workload-qualified latency target, or require a hard bound and design the mechanism to prove it.
- **167 — Caller IPC:** expose lend disposition and valid-reply presence independently of errors, preserving existing ownership rules, or change those ownership rules. The recommendation also defines failed-output cleanup.
- **168 — Server IPC:** expose delivery/discard and installed-handle outcomes for transaction cleanup, or retain the reply ABI and add bounded acknowledgement/expiry protocols.

### Follow-up routing — proposed, not dispatched

Repair C4 in a focused testbench package. Keep A2's bounded kernel validation separate from any
unproven fix; route A3 through a runtime/console cleanup package against answer 85. Following
approval of 167/168, WP-IPC1 owns C1/C2/C3/A1 together: C3's initial writable-output validation
is part of the same approved contract, with focused rejection/error-order regressions. C2's R4
policy was already settled. Give this ABI/kernel/model/runtime/server follow-up its risk-bounded
reviews; do not silently revise already-merged packages.

Q164 gates the affected confined-placement and steward paths in WP-R3/WP-S2; Q166 gates the
disputed WP-K5 latency acceptance. Q165 governs the authority claim and game verdict, not a ban
on same-label delegation. Independent, already-specified work need not wait for these choices.
S2/S4/S5 remain proposals for subsequent scheduling, not work started by this review.

The round is recorded in [SWARM.md](docs/SWARM.md#cross-cutting-review-records) and
[HISTORY.md](docs/HISTORY.md). It does not clear existing package review debt or certify any
implementation as fixed. No new tests were run in this documentation-only architect round;
the earlier validation section retains its original scope.

## 5. Owner-approved IPC follow-up — 2026-09-22

The owner approved **167 and 168 as recommended**, after being offered those two decisions
explicitly. This does not approve 164–166, dispatch implementation, or mark C1/C2/A1 fixed.
The resident architect recorded the answers and applied them to KERNEL-SPEC.md (outcomes,
lifecycle, ABI and error/output validity), CAPABILITIES.md (native runtime buffer ownership),
and CONTAINMENT.md (server transaction cleanup).

[WP-IPC1](docs/BUILD-PLAN.md) scopes ABI/kernel/model/runtime/server implementation and its
regressions. SWARM Claims records it as **ready, not dispatched**: merged dependencies permit
initial ABI/runtime work, kernel integration serializes behind K4, and model integration,
real timer tests after K5, and meaningful unsafe coverage are completion gates. Approval of
the design is not evidence that any implementation now conforms to it.

### R-IPC1-design: three-review findings

Scope: the approved specification application, Q&A/history links, and WP-IPC1 plan/status. The
reviewers were independent and read-only; the adversarial review examined design documents only.
This is a review of the uncommitted documentation change, not implementation acceptance.

| Angle | Finding | Disposition |
| --- | --- | --- |
| Consistency/editor | PASS; no substantive inconsistency in outcomes, encoding, owner records or package gates | No required edit; final clarification recheck also PASS |
| Design-only adversarial | R1, nonblocking: explicitly protect output mappings during completion, not just handle-table state | Architect clarified validation/copy/handle changes/publication against mapping changes and teardown/abandonment; pinning also needs completion arbitration. Race tests added to IPC1. Reviewer rechecked: resolved, PASS |
| Simplifier | S1, nonblocking: C3 already falls within the approved IPC contract; avoid a second implementation owner | IPC1 now explicitly owns C3 and its initial read-only/error-precedence regressions; ASTRA routing corrected. Reviewer rechecked: resolved |
| Simplifier | S2, nonblocking: make the unsafe-coverage completion gate finite | Separate C4 repair checks corrected roots and missing-root failure; IPC1 verifies touched on-target production coverage, without inventing a repository-wide policy. Reviewer rechecked: resolved, PASS |

**Round complete: all three angles PASS, with every finding resolved and rechecked.** This closes
the design-document round only, not the IPC implementation findings or prior package review debt.

The documentation/code generation check (`cargo test --offline --locked -p redoubt-wire-gen`,
using the local toolchain) passed all **16 tests**. This is not testing the unimplemented IPC
change; no new IPC or boot results are claimed. WP-IPC1 still requires implementation, its full
acceptance gates, and its own three-review TCB round.

## 6. IPC1 implementation checkpoint — 2026-09-22

This section preserves the earlier host checkpoint and intermediate updates. **Section 7 is
the latest checkout/validation state** and supersedes its earlier outstanding-work descriptions.

**Kernel review blocker:** R-IPC1-kernel's defensive reader found a same-process received-lend
alias can be unmapped/freed while the original lender mapping still records that frame. This is
inherited code but directly undermines returned ownership; it is being reproduced and fixed under
existing R3/R4/I9, as confirmed by the resident architect. Editor/simplifier passed the completion
delta, not this lifetime dependency. Parent independently passed both new outcome boots and ran
the full bench; its only observed case failure was an obsolete fixture path in `bench-bundle-file`.
Neither those passing cases nor the earlier host round imply acceptance of this new finding.

**Resumed work:** the owner requested the remaining correctness/documentation fixes. IPC1's
implementer has resumed kernel completion and real-boot regressions; T1c's implementer is
resolving the runtime violation without raising budget 9. The architect is reconciling accepted
protocol and future-scope decisions. No local K4/model branch or worktree was found; external
claims remain unverified. This is work in progress, not new validation or delivery in main.
The results below describe the completed host checkpoint, not the final remediation state.

Latest provisional evidence: the T1c runtime change centralizes adoption/release of mapped
bytes, reducing documented unsafe uses from 11 to the unchanged budget 9. Coordinator rerun:
122 sys/runtime/checker host/doc tests pass, and every configured unsafe budget passes. The
new reduction has now passed all three review angles. `kernel/src/message.rs` is now included
in the core coverage without raising its limit. This is not a claim that all repository sources
are budgeted: `blkd`'s existing four device/DMA unsafe blocks remain outside the configured roots;
its IPC-touched server code has no unsafe. Bootfsd/consoled production entry points also have no
unsafe, independently of their libraries' forbid attributes.

The IPC kernel implementer reports existing rv64 `redoubt-ipc` and `redoubt-ipc-attack` boots
passing in Docker. New outcome/rollback regressions also passed both widths; coordinator reruns
and the kernel review are still pending. The architect's documentation reconciliation restored accepted error 114,
separated implemented Devs/map-device encoding from open 143/146, and retained open 164-166.

After the owner authorized implementation, the swarm built and reviewed the **ABI/runtime host
slice**, not the whole IPC1 package. At that checkpoint primary-checkout code was unchanged:
reviewed IPC source was isolated, uncommitted, and could not be combined with the old kernel.
The separate documentation/protocol slice has since been applied to the primary checkout:
three-review PASS, with the opcode and rv32-wording nits fixed. Parent reruns there passed
53 wire/generator tests (1 ignored), the actual-server status regression and the publication-link
test. It does not deliver the still-isolated IPC kernel/runtime changes.

| Work | Location / state |
| --- | --- |
| IPC1 ABI/runtime/server/client slice | Branch `wp-ipc1`, `/tmp/redoubt-ipc1-LJ2zHm/worktree`; 26 owned code/test files staged, uncommitted |
| T1c checker repair (C4) | Branch `wp-t1c`, `/tmp/redoubt-ipc1-LJ2zHm/ratchet`; two source/config files changed, uncommitted |
| Approved design and coordination records | `/data/redoubt`; uncommitted documentation changes |

The IPC worktree also holds a read-only copy of the approved design and the reviewed two-file
checker repair as unstaged prerequisites. Those files are not part of the staged IPC slice.
T1c's staging attempt encountered a read-only worktree index; its source changes are intact.

### Delivered and reviewed

- `redoubt-sys`: explicit call status/lend/reply outcomes and server delivery/installed-slot
  outcomes, strict decoding, and explicit expected lifecycle rows in tests.
- `redoubt-rt`: consuming `Buffer` calls, disarming consumed buffers, returned-buffer ownership,
  and closing unclaimed partial-reply handles. `Reply` is non-Copy so extracting an owned reply
  cannot leave a copied identity that the outcome's destructor unexpectedly closes.
- Shared server paths and `keyd`: delivery/required-slot-aware grant and connection rollback.
  Native clients, server dispatch, raw test helpers and host doubles use the new outcome shape.
- Stateful host regressions: retained/consumed buffers, address reuse, partial replies,
  ownership extraction/drop, and actual KeyServer/NineServer serving-path resource cleanup.
- T1c: correct five moved roots; reject missing, empty or source-free configured coverage;
  retain valid zero-unsafe sources. Seven new regression tests; no budget increases.

### Three-review findings and disposition

| Round / angle | Finding | Final disposition |
| --- | --- | --- |
| R-T1c: editor, defensive failure-path reader, simplifier | No required edits | All PASS for the checker diff; its honest production result remains red |
| R-IPC1-host: editor E1 / adversarial R1 | Failed `reply` discarded a still-open request while serving loops continued; bookkeeping-only tests missed its open-call/lend resources | Fixed: preserve request, close temporary handles, send a handle-free `MALFORMED` fallback; retain original error for provisional rollback. If fallback fails, exit under R4b. Stateful serving-path tests cover both paths. Both reviewers rechecked PASS |
| R-IPC1-host: simplifier S1 | Expected ABI validity copied the decoder predicate | Replaced with explicit allowed lifecycle rows; rechecked PASS |
| R-IPC1-host: editor E2 | Stale keyd comment equated failed reply with caller death | Corrected; rechecked PASS |

**All three implementation-review angles passed the host slice.** This is neither a review of
the future kernel changes nor acceptance of the complete IPC1 package.

### Verification and outstanding gates

The coordinator independently reran, in the IPC worktree, with the repository-local Cargo and
Rustup environment:

```sh
cargo test --offline --locked -p redoubt-sys -p redoubt-rt -p redoubt-keyd \
  -p redoubt-bootfsd -p redoubt-consoled -p redoubt-blkd --quiet
cargo check --offline --locked --target riscv32imac-unknown-none-elf \
  -p redoubt-sys -p redoubt-rt -p redoubt-keyd -p redoubt-bootfsd \
  -p redoubt-consoled -p redoubt-blkd -p test-programs --quiet
cargo check --offline --locked --target riscv64imac-unknown-none-elf \
  -p redoubt-sys -p redoubt-rt -p redoubt-keyd -p redoubt-bootfsd \
  -p redoubt-consoled -p redoubt-blkd -p test-programs --quiet
cargo run --offline --locked --quiet -p testbench -- unsafe-budget
```

Host/doc tests: **219 passed, 0 failed, 2 ignored** (optimized timing tests). Both-width checks:
**PASS**, with an existing unrelated `unused_mut` warning in `move-borrowed.rs`. These are affected
crate checks, not kernel/loader builds or boot tests. The T1c worktree's host tests separately
passed **9/9**, independently rerun by the coordinator. Staged whitespace checks pass.

The repaired unsafe check **fails correctly**: runtime **11 documented uses, budget 9**. All other
configured components pass, and production counts are unchanged by IPC1. The architect traced the
two extra runtime sites to existing checked MMIO helpers in commit `71ae71109`; this is baseline
bookkeeping debt, not a new IPC unsafe addition. Existing policy permits a separately justified,
reviewed correction, but neither T1c nor IPC1 silently changes the budget. No new owner design
answer or budget exception was fabricated.

Remaining gates are concrete:

1. **Kernel coordination and implementation:** K4 is still claimed as building, but its branch
   and worktree are absent here. The owner was asked whether that work is active elsewhere. Kernel
   producer/completion, rollback and initial output-validation changes are not implemented here.
2. **Model:** no `model/` or M0/M1 branches exist in this checkout; its location was requested.
   No replacement model was invented.
3. **Real-kernel verification:** the initial host-only QEMU check was insufficient. After the
   owner pointed to Docker, both `qemu-system-riscv64 --version` and `qemu-system-riscv32 --version`
   ran successfully in the existing `redoubt-dev:latest` image (`79d2210ab327`), reporting
   **10.0.13**. Dockerfile explicitly supplies both widths; `dev.sh` is the documented container
   entry point. QEMU availability is **not a blocker**. Existing RustSBI images and both Rust
   targets are also present. K5 timer-dependent cases, completion races, real server cleanup and
   the whole boot bench still have not been run for IPC1; emulator verification is not boot acceptance.
4. **Unsafe gate:** resolve the separately identified 11-versus-9 baseline debt without hiding
   coverage or treating a failed check as success.

IPC1 is therefore **waiting, not accepted or merged**. No commits, budget raises, kernel edits,
firmware edits or full-bench success are claimed by this checkpoint. The later kernel/model work
needs its own risk-bounded review and the package's full acceptance before integration.

## 7. Remediation handoff — 2026-09-22

### Applied to the primary checkout, uncommitted

- Fail-closed unsafe checker and corrected source roots, including `kernel/src/message.rs`.
  No limit changed. Nine checker tests pass; the primary production gate now correctly **fails**
  runtime **11 versus 9**, rather than silently skipping it.
- Accepted `ninep_common not_yours = 2`, regenerated Rust/Elixir codecs, removed obsolete pending
  comments, and a real shared-server-path regression that recognizes the wrong-owner response
  and verifies the owner's connection remains usable. The test derives Disconnect's opcode.
- Accurate implemented/host-tested/boot-integrated/planned status; Devs/Ctrl and current
  `map_device` encoding; OTP/Elixir prerequisites; CLI/default/build/debug instructions; corrected
  source/publication links in the tour generator and regenerated output; stale paths, driver and
  `copy_file` references; accepted future scope. No broad verbosity pass or owner decision change.
- The full bench exposed one stale fixture path, `redoubt/tests/data/bundle-file.txt`. It now
  names `tests/data/bundle-file.txt`; its focused boot passes and all three reviewers pass the fix.

### Isolated IPC work, not delivered in primary

`/tmp/redoubt-ipc1-LJ2zHm/worktree` contains the reviewed ABI/runtime/client/server work plus the
kernel completion implementation and real-kernel tests. The separately three-reviewed Mapping
reduction reaches runtime **9/9** there, with every configured budget passing. Parent reran
229 affected host/doc tests (2 ignored) after combining it with IPC, before the final loan test
draft. These changes require coherent kernel/ABI/runtime integration, which is withheld below.

The kernel round found **R1: incoming loan aliases are insufficiently protected when caller and
receiver share a PID**. The existing PID ownership test permits an alias to be unmapped/freed;
the original lender PTE can then restore a stale frame. This is source-confirmed, not a demonstrated
cross-allocation exploit. The architect confirmed that existing R3/R4/I9 require the fix, with
no new question. A regression draft is saved as unstaged additions to the staged `ipc-outcomes`
test; **the draft was not run and the lifetime correction is not implemented**. The implementation
agent hit a platform restriction twice, including an ordinary-safeguards retry. It was not routed
around. Continuation needs a working supported agent configuration. Editor/simplifier passed the
completion logic, but the overall kernel round remains **BLOCK** until R1 is fixed and re-reviewed.

### Exact validation and limits

With `CARGO_HOME=/data/redoubt/.cargo`, `RUSTUP_HOME=/data/redoubt/.rustup` and the local Cargo:

```sh
cargo test --offline --locked -p redoubt-wire -p redoubt-wire-gen --quiet
cargo test --offline --locked -p redoubt-rt --test ninep_status --quiet
cargo test --offline --locked -p testbench --quiet
cargo run --offline --locked --quiet -p testbench -- unsafe-budget
python3 -m unittest discover -s tools -p test_readme_links.py
git diff --check
```

Primary results: **53 pass/1 ignored**, **1 pass**, **9 pass**, **expected failure runtime11/9**,
**1 pass**, **pass**, respectively. Docker regeneration of `tools/gen_readme.py` in primary
reproduced the same SHA-256 (`a41a996210d95b7b1597ffce5105b32fbd4294bc6d859715d2656f98f41445ab`).
The documented release command with `CARGO_PROFILE_RELEASE_DEBUG=2` built successfully and
`readelf --debug-dump=info` showed the kernel's own `kernel/src/main.rs` compilation unit.

In Docker `redoubt-dev:latest`, mounting the IPC tree at `/work`, local Cargo/Rustup at
`/opt/cargo` and `/opt/rustup`, and primary `bios/target` read-only at `/firmware`, with both
`RUSTSBI_PROTOTYPER` width-specific overrides: parent `cargo testbench ipc-outcomes` passed
rv64 and rv32 checked boots. The implementer also passed five existing `redoubt-*` cases on
both widths. Parent `cargo testbench` then ran the whole matrix: only the stale fixture path
failed; its separate correction subsequently passed its focused boot. These runs precede the
new, unexecuted R1 regression. **No final whole-bench PASS or kernel ownership acceptance.**

Mapping tests cover controlled transitions before reply, not simultaneous multi-hart user
execution; the death case covers serving-thread exit, not every process-teardown case. Model
code is absent and K5 real timer support remains unimplemented. Elixir output was regenerated,
not executed (OTP/Elixir absent). No live-site check. Broader raw-syscall/owning-runtime API
composition remains an inherited soundness boundary, not certified by these reviews.

No commits, pushes, firmware changes, budget increases or unrelated worktree resets occurred.

### Continuation attempt — 2026-09-22

The owner explicitly approved borrowed-mapping protection for same-process loans, then asked
to continue. The existing IPC implementer was resumed without changing models or safeguards;
it again failed with the same platform restriction before returning implementation work. No
R1 kernel fix or new boot result is claimed. The three-review follow-up remains outstanding.

Parent independently reran the primary wire/generator/checker host suites: **62 passed,
1 ignored**; the wrong-owner server regression: **1 passed**; publication-link unittest:
**1 passed**; `git diff --check`: **passed**. These checks do not close the isolated IPC
lifetime defect or the primary runtime's 11/9 unsafe-budget violation. Implementation remains
paused pending a supported agent configuration; no model/configuration change was authorized.

## 8. Sol continuation — 2026-09-22

The owner explicitly authorized Sol. A Sol implementer has resumed the existing isolated
IPC1 worktree under normal safeguards, with no firmware, budget or design changes authorized.
It owns the same-process borrowed-mapping lifetime correction and regression validation.
Section 7 remains the prior handoff, not a claim that work is still platform-blocked.
The saved regression first needed a missing `unsafe` annotation on its legacy `MemoryRange`
construction. With that test-only correction, `cargo testbench ipc-outcomes` exited 1 on
both rv64 and rv32: the assertion rejecting `unmap` of a live received-loan alias failed.
This establishes the pre-fix regression; it does not establish the correction. Three independent
reviews are required before working-tree integration. Model and K5 completion gates remain
outstanding.

The Sol candidate now marks incoming borrower aliases `VALID | S`, leaving outgoing lender
reservations invalid with `S`. Mapping ownership APIs reject both, including same-PID loans;
transfer mappings remain ordinary owned mappings. Return validates both markers and matching
physical frames before restoring the lender. Abandonment retains borrower protection until
kernel cleanup. Cleanup invariant failures no longer silently publish success.
`ipc-outcomes` passes rv64/rv32 after the fix; existing `return-lent-unmapped` passes both
widths at 1 and 4 harts. The source is frozen for the three-review follow-up while remaining
legacy cases and budget validation run. This candidate is still isolated, not integrated.

## 9. Reviewed integration — 2026-09-22

The coherent IPC ABI/kernel/runtime/client/server changes, R1 borrowed-alias correction and
reviewed runtime unsafe reduction are **applied uncommitted in `/data/redoubt`**. Sections 6–8
are earlier checkpoints; their isolated/blocked statements are not the current delivery state.
No RustSBI source or firmware artifact was changed, and unrelated primary edits were preserved.

Three independent Sol correction reviews passed: editor and defensive reviewer OK with notes,
simplifier OK. The editor found missing invariant enforcement in abandoned frame release:
`free_frame_of` swallowed errors. It now returns the result, cleanup checks it, and the kernel
regression asserts that the consumed page's charge is released. This was a missing check,
not a demonstrated reachable ownership mismatch. Source integration matched the reviewed
candidate byte-for-byte except preserved protocol-comment corrections; the existing primary
wrong-owner server regression received its mechanical outcome-API migration.

Parent verification: 229 isolated host/doc tests passed, 2 ignored; the complete isolated
Docker bench passed **99 executions across 56 case files**, exit 0. In primary, the combined
ABI/runtime/server/wire/generator/checker host suites passed **283 tests, 3 ignored**. The tour
was regenerated through its generator and its publication-link unittest passed; whitespace
checks passed. Final primary full-bench validation also passed **99 executions across 56 cases**,
exit 0 (192 seconds). The affected ABI/runtime/server crates compile on both widths. All
three final integration rechecks passed. Every configured primary unsafe budget passes with
zero undocumented uses; runtime is **9/9**, with no limit increases. Touched kernel seams
(`arch/riscv/mem.rs`, `mem.rs`, `message.rs`, `redoubt.rs`) are covered by the configured roots;
no new uncovered production module was introduced.

### Exact final validation

Host commands run in `/data/redoubt` with `CARGO_HOME=/data/redoubt/.cargo`,
`RUSTUP_HOME=/data/redoubt/.rustup` and `/data/redoubt/.cargo/bin` on PATH:

```sh
cargo test --offline --locked -p redoubt-sys -p redoubt-rt -p redoubt-keyd -p redoubt-bootfsd -p redoubt-consoled -p redoubt-blkd -p redoubt-wire -p redoubt-wire-gen -p testbench --quiet
cargo check --offline --locked --target riscv32imac-unknown-none-elf -p redoubt-sys -p redoubt-rt -p redoubt-keyd -p redoubt-bootfsd -p redoubt-consoled -p redoubt-blkd
cargo check --offline --locked --target riscv64imac-unknown-none-elf -p redoubt-sys -p redoubt-rt -p redoubt-keyd -p redoubt-bootfsd -p redoubt-consoled -p redoubt-blkd
python3 -m unittest discover -s tools -p test_readme_links.py
rustfmt +nightly-2026-05-11 --check --config skip_children=true libs/rt/tests/ninep_status.rs
git diff --check
```

The full boot/build/checker bench, including both kernel/loader widths, ran as:

```sh
docker run --rm --network none --user 158649:30 \
  -v /data/redoubt:/work -v /data/redoubt/.cargo:/opt/cargo \
  -v /data/redoubt/.rustup:/opt/rustup -v /data/redoubt/bios/target:/firmware:ro \
  -w /work -e CARGO_HOME=/opt/cargo -e RUSTUP_HOME=/opt/rustup \
  -e CARGO_NET_OFFLINE=true \
  -e RUSTSBI_PROTOTYPER=/firmware/riscv64gc-unknown-none-elf/release/rustsbi-prototyper \
  -e RUSTSBI_PROTOTYPER_RV32=/firmware/riscv32imac-unknown-none-elf/release/rustsbi-prototyper \
  redoubt-dev:latest /opt/cargo/bin/cargo testbench
docker run --rm --network none --user 158649:30 \
  -v /data/redoubt:/work -w /work redoubt-dev:latest python3 tools/gen_readme.py
```

All final commands above exited 0. Logs are under `target/testbench/`. The implementer's
earlier featureless kernel `cargo check` was not a valid platform build and failed for missing
platform modules; the supported testbench built and booted both widths successfully.

Not package acceptance: model code/traces are absent, K5 real timer cases and simultaneous
multi-hart completion-race coverage remain outstanding. The shared server's last-resort
`process_exit` path is host-tested but cannot yet perform native R4b cleanup: the new kernel
process-exit syscall is unimplemented. Native server startup/exit integration remains a K4/R3
dependency. Existing raw-syscall/owning-runtime API composition is a broader inherited soundness
boundary, not certified by this round. OTP/Elixir execution and live-site validation remain
unperformed. Questions 164–166 and other genuine open decisions remain open. No commits or pushes.

## 10. Owner-authorized commit checkpoint — 2026-09-22

The owner subsequently authorized committing the reviewed remediation in the primary checkout.
The uncommitted/no-commit statements in earlier sections describe their respective checkpoints.
This commit records the tested correctness slice, not full WP-IPC1 acceptance; section 9's
remaining gates and validation limits are unchanged. No push was requested.

## Original recommended order (historical review proposals)

The list below preserves the initial review's priorities, not the current task queue. Section 9
records the completed correctness fixes and the remaining implementation/acceptance limits.

1. Fix C4 so the verification report stops silently omitting sources.
2. Resolve D1/D2 with a worked confined session and authority graph; state the scheduler guarantee precisely under D3.
3. Implement the approved IPC lifecycle outcomes through WP-IPC1; address C1, C2, and A1 together, with separate focused regressions and the required reviews.
4. Fix A3 with focused regressions; validate A2 on the real kernel before assigning A2 a final severity. C3 belongs to WP-IPC1 above.
5. Add the smaller containment gate and complete K4/K5/K6 against it.
6. Consolidate implementation status and remove unused crates; evolve the API facade from integrated use.

The highest-value next design artifact is one worked confined session: its authority graph and IPC ownership/completion table. Together they make the central security claims concrete enough for independent implementers and reviewers to interpret consistently.
