# Decision record

The specifications own accepted behavior and the reason for it. This file records approval
provenance; [QUESTIONS.md](QUESTIONS.md) contains only unresolved decisions.

## Archived decisions through 2026-09-22

The [dated owner answers](archive/2026-09-22/ANSWERS.md) and
[original numbered questions](archive/2026-09-22/QUESTIONS.md) preserve exact recommendations,
amendments and approval attribution. Accepted IDs: **1–126, 150–160, 162, 167–168**.
**161** is the orchestrator's package split, not a new owner-approved interface.

| Decision area | Current owner |
| --- | --- |
| Kernel objects, accounting, IPC, ABI, errors and scheduling | [KERNEL-SPEC](KERNEL-SPEC.md) |
| Labels, confinement, mediation and admission | [CONTAINMENT](CONTAINMENT.md), [TENETS](TENETS.md) |
| Delegation, leases and approvals | [CAPABILITIES](CAPABILITIES.md) |
| Boot, manifest, startup and keyd | [INIT](INIT.md), [VERIFIED-BOOT](VERIFIED-BOOT.md) |
| 9P, parked calls, console size and resize | [NAMESPACES](NAMESPACES.md), [USERLAND-API](USERLAND-API.md) |
| Wire encoding and generated protocols | [WIRE](WIRE.md) |
| Package split 161 | [BUILD-PLAN](BUILD-PLAN.md) |

Later answers replace earlier wording where stated in the archive: 56 was revised; 57/58
were replaced by 82; 103 removed `first` and priority tiers; 120 added the bundle signature
domain. Answers 167/168 add observable IPC outcomes without changing lend ownership rules.
Answer 166 below revises 103's one-`SLICE` latency claim without changing its mechanism.
Approval does not establish implementation: see [STATUS](STATUS.md).

## Recording a new decision

Append the ID, date, decision-maker, decision, short reason, affected specification section
and any residual. Apply the rule to that specification, remove the resolved question from
the open list, and preserve its proposal with the approval when needed to interpret it.
A revision gets a new entry naming the prior decision; do not rewrite approval history.
Do not repeat it in HISTORY or ARCHITECT-NOTES.

## Recovery decisions (Mick, 2026-09-22)

Mick approved the three recommendations below together: “yes, sounds fine. thanks”.
The preceding proposal distinguished the actual rv32/rv64 context charges from the
creator-paid notice, defined final-thread exit including fault/blame, and retained bundle
readback as unfinished acceptance after the lifecycle implementation lands. These decisions
approve the contracts and recovery sequence; they do not accept an implementation package.

### 127. Charge separately allocated process context storage.

**Decision:** the process/notice object remains one page charged to the creator's budget.
Charge all `PROCESS_IMPL_PAGES` saved-context frames to the execution budget from address-space
creation until teardown: currently one on rv32 and two on rv64. Retain one IPC page per thread
and all actual page-table pages, including the root, charged to the execution budget. Count
each distinct frame once and remove the global per-PID context reservation. Initial loader
processes also pay for their actual context frames; their temporary lack of process/notice
objects is a boot integration limitation, not an extra unbacked process-page charge.

**Reason:** the separately allocated notice frame cannot also back a context frame with a
different payer and lifetime. This resolves the clarified question 127; it does not approve
the historical proposal's `PROCESS_IMPL_PAGES - 1` charge alongside a separate notice frame.
**Owner:** KERNEL-SPEC.md, What objects cost. **Residual:** accounting, rollback and restoration
tests must verify both widths; approval alone does not establish that evidence.

### 169. Retain bundle readback as a K4 acceptance dependency on R2/R3.

**Decision:** independently reviewed K4 lifecycle integration may land with clean guest
`[[file]]` readback explicitly unfinished. Complete that readback through the production
loader-stub/init bundle handoff in R2/R3, retaining it as a K4 acceptance dependency. Correct
the testbench's attribution of the kernel/init-only loader transition to R3. Do not add a
temporary boot ABI or claim full K4 acceptance from the lifecycle integration.

**Reason:** the specified loader transition requires init to launch the remaining programs;
the current loader interprets data entries as ELF. **Owners:** BUILD-PLAN.md, WP-K4/WP-R3;
testbench.md, Files in the bundle. **Residual:** the guest must still compare the injected
bytes after a clean boot. The current expected loader rejection does not satisfy that gate.

### 170. The final thread exits its process.

**Decision:** `thread_exit` with surviving siblings cleans up only the exiting thread and
its IPC, without a process-exit notice. The final thread's `thread_exit` is equivalent to
`process_exit(0)`. Determine open-call status and current-call blame before cleanup: report
`faulted` when the process holds open calls, using the existing blame rule; otherwise report
`exited` with code 0. Preserve normal process teardown, creator-paid notice lifetime and PID
retention.

**Reason:** avoid a started, inert zero-thread process that the current interface cannot
restart. **Owner:** KERNEL-SPEC.md, Process, Messages and System calls. **Residual:** real-kernel
tests must cover final-thread exit both with and without open calls and surviving siblings.

## Wakeup latency decision (Mick, 2026-09-23)

Mick answered the Architect's Wash decision request on QA thread G1-q166 with “go with
recommendation”, after an earlier relayed “answer A”. The recommendation (A) kept answer 103's
mechanism and withdrew its universal one-`SLICE` claim; the alternative (B) kept a hard
one-`SLICE` bound and changed the scheduler to establish it under a proof. A was chosen.

### 166. Withdraw the universal one-slice wakeup claim; specify ties and preemption (revises 103).

**Decision:** keep answer 103's single stride queue, actual-runtime charging and the
`max(own pass, current minimum)` wake rule. Withdraw the claim that a driver or the steward
woken under load runs within about one `SLICE`; nothing in the mechanism establishes it, and
no deadline follows from weight. R12 gains a deterministic wake-first tie rule (a waking budget
is ranked ahead of already-queued budgets with an equal pass) and states that preemption
happens at slice end or at a deadline, never on wake alone. Wakeup is prompt but not bounded.
The attack-test and WP-K5 acceptance bullets that repeated the bound are replaced by a
measured responsiveness target under a named workload: N spinning user budgets at the manifest
user weight, one driver and the steward at their manifest weights, recording weights, runnable
budgets, prior passes, wake latency and lease-termination latency. WP-K5 proposes the numeric
target with that evidence and a package reviewer accepts it.

**Reason:** answer 103's "stated cost: up to one `SLICE`" was a promise, not a consequence of
`max(own pass, current minimum)` with unspecified ties and retained larger passes; a woken
budget can wait for the running slice plus a slice per equal-pass budget ahead of it. Option A
preserves the accepted no-priority design and makes the guarantee honest and testable at no
mechanism cost. **Owners:** KERNEL-SPEC.md, R12; RESOURCES.md, Scheduling and Attack tests;
BUILD-PLAN.md, WP-K5. Answer 103's latency wording is superseded; its removal of `first` and
priority tiers stands. **Residual:** human control (TENETS guarantee 3) rests on a measured
steward lease-termination latency, not a proven bound, until something needs a real-time rule.

## Loader-stub mapping decision (Mick, 2026-09-24)

Mick answered the Architect's Wash decision request on QA thread R2-stub-self-map with "go with
recommendation". The recommendation was a new `map_fixed` call; the alternatives were (A) an
`addr` argument on `map_anon` and (B) static-PIE programs relocated by the stub. Parent-side
segment mapping was rejected because it would make `init` and the steward parse ELFs.

### 172. `map_fixed`: a process maps new pages at an address it names.

**Decision:** add `map_fixed(addr, len, flags)`. It maps zeroed pages at exactly `addr` in the
caller's own address space, charged to the caller's budget like `map_anon`'s. `addr` and `len`
are page-aligned, `len` is not 0, and the range lies in user space and overlaps none of the
caller's mappings; otherwise `InvalidArgument` and nothing is mapped. It never replaces a
mapping (unlike POSIX `MAP_FIXED`). Flags follow `map_anon`'s rules (not 0, not W+X, not W
without R). Errors `InvalidArgument`, `OutOfMemory`. `map_anon`, its kernel-chosen addresses
and its callers are unchanged. The loader stub maps a program's segments with it before mapping
anything else, and exits if a segment overlaps the stub, the startup page or the image.

**Reason:** the stub runs inside the started child, where `map_anon`'s address is the kernel's
and `process_map` is refused, yet native programs are fixed-address static ELFs. One small
kernel call keeps the stub the only ELF parser and keeps one linking convention. **Owners:**
KERNEL-SPEC.md, R11, System calls (appended last so earlier call numbers keep their values),
Errors; PACKAGES.md, Launching a process, step 5. **Residual:** a kernel package for the sole
kernel writer (kernel, `redoubt-sys`, executable model) with rv64 boot and rv32 compile
acceptance and attack cases (occupied range, outside user space, unaligned, len 0, overflow,
W+X, W without R, budget exhausted); MEMORY-LAYOUT.md records the stub's address once WP-R2
fixes it, so program link bases avoid it.
