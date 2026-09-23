# Package coordination

Owns package state and execution rules. [BUILD-PLAN](BUILD-PLAN.md) owns remaining deliverables
and acceptance; [STATUS](STATUS.md) describes current behavior. User instructions govern the
current session; this document describes the project's swarm workflow.

## Roles and execution

- The orchestrator maintains claims, assigns independent work in isolated worktrees and integrates
  one package at a time. The kernel track has one writer. Reconcile external kernel work before a port.
- The resident architect resolves questions from the specifications and records genuine owner
  decisions through `.pi/skills/architect-qa/SKILL.md`. Implementers do not invent answers.
- Implementers own their package's paths and tests. Reviewers are read-only and cover defensive
  behavior, simplification and code/documentation consistency.
- Stage only owned files; inspect other sessions' changes before staging. Do not use `git add -A`
  or `git commit -a` in a shared worktree.
- Start packages when their dependencies are merged. Stop only dependent work for an unresolved
  design decision. Update the owning contract and approval record when a decision is accepted.
- Acceptance requires the package's tests and attack cases, a green full bench, rv32 compilation,
  no undocumented unsafe and no unexplained ratchet increase. Rebase and retest before integration.
- Review may be batched for small changes; TCB/security work gets its own risk-bounded round and
  defensive reader. A package merged before review is `merged (review due)`, not done. Clear review
  debt before the next wave; record only outstanding debt here. Evidence belongs with the change.

Project agents live in `.pi/agents/` (including `reviewer`). `.pi/workflows/run-package.js`
runs implementation, acceptance and three review angles; `review-package.js` runs reviews alone.
The `/swarm` prompt starts that workflow.

## Claims

This is the source of truth for package state. `waiting` means unmet dependencies; `ready`,
`building`, `review`, `merged`, and `folded` describe execution/integration, not product maturity.
External candidates remain unaccepted until ported and tested against the current contracts.

| Package | State | Branch | Notes |
| --- | --- | --- | --- |
| M0 | review | wp-m0 | Remote candidate `674831cf9`; absent from active workspace; current-spec reconciliation required |
| M1 | review | wp-m1 | Carried by wp-m0; remove obsolete priority tiers and add current IPC outcomes/traces |
| W1 | merged | wp-w1 | d52896bee |
| W2 | merged | wp-w2 | 3715363a9 |
| W3a | merged | wp-w3 | 612a0a599; review R-1 complete |
| A1 | merged | wp-a1 | 44f1780a1 |
| A2 | merged | wp-a2 | c98034520 |
| A3 | folded | | into wp-k2 (answer 103; the `first` flag) |
| L1 | merged | wp-l1 | 25ab39296 |
| T1 | merged | wp-t1 | 987bacbed |
| T1b | merged | wp-t1b | 6cd067a39 |
| T1c | review | wp-t1c | Checker/runtime repair in fe807fc4b; actual configured budgets pass, runtime 9/9. Server coverage omissions remain |
| V1 | merged | wp-v1 | 05955bf86 |
| K0 | merged | wp-k0 | f7b9fdd16 |
| K0b | merged | wp-k0b | e30d43304 |
| K1 | merged | wp-k1 | e1d2c6216 |
| K2 | merged | wp-k2 | 95788dcd0 (carried A3) |
| K3 | merged | wp-k3 | 12c52c2d7 |
| K4 | building | wp-k4 | External candidate 6ddf06786 inspected, not integrated or accepted; selectively port lifecycle work |
| K5 | ready | | needs K2; serialized behind K4 on the kernel Hotspots, not on dependencies |
| K6 | waiting | | needs K1-K5, R1b |
| IPC1 | review | wp-ipc1 | Implementation in fe807fc4b; model, K5 timer, native exit and concurrency acceptance remain open |
| R1 | merged | wp-r1 | 8298608af (carried the answers 39-42, 50-53 part of R1b) |
| R1b | merged | wp-r1b | 86117e7af |
| R1c | merged | wp-r1c | cd65fa610; joined `Parked` to the 9P skeleton (recovery of `5d29d136e`, answers 156-158); reviewed R-R1c |
| R1d | waiting | | typed parking awaits decision 163; needs R1c (merged); blocks `resize` in B2a |
| R2 | waiting | | needs R1b, K4 |
| R3 | waiting | | needs R2, W1, K3, K5; carries the `confined` manifest |
| R4 | merged | wp-r4b | 69466924c; bootfsd and consoled recovered from wp-r4; reviewed R-R4b |
| B1 | waiting | | needs R1b, R4 (both merged) |
| B2 | waiting | | IEx on the UART; needs B1, R3 |
| B2a | waiting | | the console library (`consol` codec, `Redoubt.Console`/`.Key`, answer 162); needs B2, R4 |
| B2b | waiting | | `Redoubt.Ed`, `Shell.top()`; needs B2a |
| D1 | merged | wp-d1b | 8681f2648; blkd recovered from wp-d1, reviewed R-D1 (editor BLOCK fixed; red team 4/4 OK) |
| D2 | waiting | | needs D1, L1, R1b |
| D3 | building | wp-d3 | External claim unverified; no source supplied in recovery review |
| S1 | merged | wp-s1 | 14bcc6e9d |
| S2 | waiting | | needs R3, B1, D2 |
| S3 | waiting | | needs D3, S1, S2 |
| C1 | waiting | | needs M1, K5, T1, IPC1 (answers 167-168 outcomes in traces) |
| E1 | waiting | | needs everything (milestone) |

## Recovery work

The owner prohibited wholesale merges of the supplied remote branches. Fetching them did not
accept any package. Exact evidence is in the
[recovery inventory](archive/2026-09-22/ASTRA.md#11-remote-branch-recovery-review--2026-09-22).

| Source | Required action |
| --- | --- |
| `origin/wp-d1` e4d22b980; `origin/wp-r4` 33a46e010 | Source is already recovered into servers/blkd, bootfsd and consoled. Preserve later fixes. Restore missing bench definitions and server unsafe-budget coverage; see BUILD-PLAN verification follow-up. |
| `origin/wp-k4` 6ddf06786 | Selectively port process lifecycle/tests; preserve current loan protection, outcome ABI and rollback. Complete hostile mapping, exit/lend, PID reuse and bundle-readback acceptance. |
| `origin/wp-m0` 674831cf9 | Move candidate to model/, register it, remove old priority tiers and reconcile outcomes, record validity, ghost checks, mutations and traces. Existing process-lifecycle modeling also needs conformance tests. |
| D3 | External claim remains unverified. |

## Review debt

Completed R-1/R-2/R-3/R-R1c/R-R4b/R-T1 rounds have no remaining debt. IPC1's implementation
reviews do not close its acceptance gaps. New TCB changes, including the budget-record guard,
need their own review before package acceptance; this edit does not claim a three-review round.
The server-verification recovery and future K4/model ports also need their normal acceptance.

## Cross-cutting review records

Use [the dated assessment](archive/2026-09-22/ASTRA.md) for past evidence and
[the active follow-up list](https://github.com/sirmick/redoubt/blob/main/ASTRA.md) for unresolved
findings. Keep completed checkpoint narratives out of this ledger.
