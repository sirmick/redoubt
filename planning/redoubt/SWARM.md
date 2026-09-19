# Building milestone 1: how the work runs

Owns: how BUILD-PLAN.md's work packages are executed: one orchestrating Claude session that runs
independent packages in parallel as sub-agents, each in its own git worktree, and reviews every
package before merging it.

## Roles
- **Orchestrator** (one Claude session): owns the claims table below, starts packages whose needs
  are met, reviews results, merges, and keeps BUILD-PLAN.md, STATUS.md and HISTORY.md current. It
  does not implement packages itself.
- **Implementer** (a sub-agent per package): works only on its package, in its own worktree and
  branch (`wp-k1`, `wp-m0`, ...), and reports what it built, its test results and anything it found
  wrong in the design.
- **Reviewers** (sub-agents per finished package): red team (attack it against the spec and the
  attack suite), simplifier (what can be deleted), editor (code, comments and notes agree). Same
  pattern as the design reviews.
- **Owner** (Mick): approves any change to the frozen design, and anything irreversible.

## Rules
1. **Parallel where the plan allows.** A package starts as soon as every package it needs is merged.
   Independent packages run at the same time; the kernel track runs one package at a time.
2. **Isolation.** Each implementer works in its own worktree and stages only the paths its package
   owns (BUILD-PLAN.md "Delivers"). Never `git add -A` or `git commit -a`.
3. **The design is read-only.** An implementer that finds a spec problem stops and reports it; the
   orchestrator raises it with the owner. Changes need a HISTORY.md entry.
4. **Done means:** the package's acceptance tests and attack cases pass in `cargo testbench`; the
   whole bench is still green; no undocumented `unsafe` and the ratchet does not rise; rv32 still
   compiles; the three reviewers' findings are fixed or recorded.
5. **Merging.** The orchestrator rebases the branch on `redoubt`, re-runs the bench, and merges one
   package at a time. One line per package in HISTORY.md.
6. **Other sessions.** Any other session working in this repository finishes or pauses its work
   before the build starts, and follows the same staging rule.

## Waves
Derived from BUILD-PLAN.md "Order". The orchestrator starts each package the moment its needs are
merged; the waves show what can run together.

| Wave | Runs in parallel |
| --- | --- |
| 1 | M0 model, W1 codecs, L1 littlefs, T1 bench extensions, A1 ABI crate |
| 2 | K1 budgets and handles; R1 runtime (after A1, W1); L1 and T1 continue |
| 3 | K2 endpoints and messages; host-side parts of D1/D2/D3 against R1 |
| 4 | K3 devices and interrupts; R4 bootfsd and consoled (once K3 lands) |
| 5 | K4 process creation; B1 beamlet platform; D1 blkd; D3 netd and ipd; S1 keyd |
| 6 | K5 timer and preemption; R2 loader stub; D2 fsd |
| 7 | R3 init; C1 conformance; B2 IEx on the UART |
| 8 | K6 delete legacy; S2 steward |
| 9 | S3 sshd |
| 10 | E1 the agent and the attack suite: milestone 1 done |

## Claims
| Package | State | Branch | Notes |
| --- | --- | --- | --- |
| M0 | review | wp-m0 | |
| W1 | review | wp-w1 | |
| L1 | review | wp-l1 | |
| T1 | merged | wp-t1 | 987bacbed |
| A1 | review | wp-a1 | fixing to the answered spec |
| K0 | building | wp-k0 | fix: lending an untouched page panics the kernel (found by T1b) |
| T1b | review | wp-t1b | attack cases asserted by the system (answer 26) |
| all others | waiting | | see BUILD-PLAN.md "Needs" |

States: `waiting` (needs not merged), `ready`, `building`, `review`, `merged`.
