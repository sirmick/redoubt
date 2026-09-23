# Review follow-up

The [dated assessment and recovery inventory](docs/archive/2026-09-22/ASTRA.md) preserve original
findings, reproductions and the owner's branch restrictions. Those checkpoints are historical.
[STATUS](docs/STATUS.md) owns current implementation; [SWARM](docs/SWARM.md#claims) owns package state.

## Outstanding findings

| Finding | Next action |
| --- | --- |
| D1–D3: confinement mediation, authority closure, wakeup bound | Owner decisions [164–166](docs/QUESTIONS.md); retain the qualifications beside the affected guarantees. |
| IPC1 acceptance | Reconcile the executable model and traces, add K5 timer cases and native process-exit cleanup tests. Reconcile concurrency acceptance with PLAN's post-M1 SMP scope without silently waiving the existing gate. |
| A3: consoled unknown-request handles | Close attached handles on rejection and test the actual serving path. |
| Raw syscalls combined with owning runtime views | Audit this inherited API soundness boundary separately; the IPC regression does not certify arbitrary combinations. |
| Missing server verification registration | Restore the five server host/build cases and omitted blkd/bootfsd/consoled unsafe-budget entries listed in the archived recovery inventory; keep the runtime ceiling at 9. |
| Legacy interfaces and speculative APIs (S1/S2/S5) | Finish K6 migration; assess unused flatipc crates and grow the client API from integrated callers. |
| Smaller containment acceptance gate (S4) | Establish a kernel/runtime gate before relying on full-product acceptance. |

C1/C2/C3/A1 have implementation fixes in `fe807fc4b`; C4's checker now scans real configured
sources and rejects missing/empty coverage. These do not complete IPC1 acceptance.
A2's remaining budget-record path now uses the shared RAM/ownership validator: the added
regression reproduced a kernel panic on rv32 before the fix. Verification is recorded in STATUS.

## Recovery constraints

Do not merge the supplied remote branches wholesale. D1/R4 source is already recovered;
K4 and the old model need selective ports onto the current contracts. Preserve the protected
loan mappings, outcome ABI and rollback checks. D3's external implementation remains unverified.
See [the recovery inventory](docs/archive/2026-09-22/ASTRA.md#11-remote-branch-recovery-review--2026-09-22)
for exact tips, path mappings, missing tests and model discrepancies.
