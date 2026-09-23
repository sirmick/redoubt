# Decision record

The specifications own accepted behavior and the reason for it. This file records approval
provenance; [QUESTIONS.md](QUESTIONS.md) contains only unresolved decisions.

## Accepted decisions through 2026-09-22

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
Approval does not establish implementation: see [STATUS](STATUS.md).

## Recording a new decision

Append the ID, date, decision-maker, decision, short reason, affected specification section
and any residual. Apply the rule to that specification, remove the resolved question from
the open list, and preserve its proposal with the approval when needed to interpret it.
A revision gets a new entry naming the prior decision; do not rewrite approval history.
Do not repeat it in HISTORY or ARCHITECT-NOTES.
