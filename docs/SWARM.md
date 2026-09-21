# Building milestone 1: how the work runs

Owns: how BUILD-PLAN.md's work packages are executed: one orchestrating session (the
`orchestrator` agent) that runs independent packages in parallel as sub-agents, each in its
own git worktree, and reviews every package before merging it. The roles are project agents in
`.pi/agents/`; the orchestrator launches them through the `subagent` tool.

## Roles
- **Orchestrator** (`orchestrator` agent, one session): owns the claims table below, starts packages whose needs
  are met, reviews results, merges, and keeps BUILD-PLAN.md, STATUS.md and HISTORY.md current. It
  does not implement packages itself.
- **Architect** (a sub-agent the orchestrator consults): knows the frozen design back and
  forth and answers the questions a package cannot be built without. The design is read-only to
  the swarm; the architect opens each question formally in QUESTIONS.md, records the answer in
  ANSWERS.md, backlinks the `Answered` line, applies the accepted answer to the design note, and
  adds the HISTORY.md entry. A genuine owner decision comes back still open, with `Rec` and
  `Alt`, and is not merged until the owner answers. The protocol is
  `.pi/skills/architect-qa/SKILL.md`.
- **Implementer** (a sub-agent per package): works only on its package, in its own worktree and
  branch (`wp-k1`, `wp-m0`, ...), and reports what it built, its test results and anything it found
  wrong in the design. It does not guess at an open design question: it stops and the orchestrator
  asks the architect.
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
   orchestrator asks the **architect**, who records the question in QUESTIONS.md and the answer in
   ANSWERS.md and raises a genuine owner decision with the owner. Nothing merges while its design
   question is open. Changes need a HISTORY.md entry.
4. **Done means:** the package's acceptance tests and attack cases pass in `cargo testbench`; the
   whole bench is still green; no undocumented `unsafe` and the ratchet does not rise; rv32 still
   compiles; the three reviewers' findings are fixed or recorded.
5. **Merging.** The orchestrator rebases the branch on `redoubt`, re-runs the bench, and merges one
   package at a time. One line per package in HISTORY.md.
6. **Other sessions.** Any other session working in this repository finishes or pauses its work
   before the build starts, and follows the same staging rule.
7. **The record is the source of truth.** Every design decision a package depends on is traceable
   to a numbered question and answer (QUESTIONS.md, ANSWERS.md). An implementer is told the
   question and answer number it is building to; an instruction without one is not a design change,
   it is a guess.

## Agents and workflows

The roles above are project agents in `.pi/agents/`, driven by the `orchestrator`:

| Role | Agent |
| --- | --- |
| Orchestrator | `orchestrator` |
| Architect | `architect` (protocol: `.pi/skills/architect-qa/SKILL.md`) |
| Implementer | `implementer` |
| Reviewers | the builtin `reviewer`, one child per angle |

Two workflow scripts drive a package: `.pi/workflows/run-package.js` (implementer, gated on
its acceptance command, then the three reviewers) and `.pi/workflows/review-package.js`
(the three reviewers alone, for a fix pass or a package built outside the swarm). Launch
them with `subagent({ workflowScriptPath: ..., args: {...}, cwd: "<package worktree>" })`.
The `/swarm` prompt template starts the whole queue.

## Waves
Derived from BUILD-PLAN.md "Order". The orchestrator starts each package the moment its needs are
merged; the waves show what can run together.

| Wave | Runs in parallel |
| --- | --- |
| 1 | M0 model, W1 codecs, L1 littlefs, T1 bench extensions, A1 ABI crate |
| 2 | K1 budgets and handles; R1 runtime (after A1, W1); L1 and T1 continue |
| 2b | design review of answers 1-55; A2 ABI update; W2 generator update |
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
| M0 | review | wp-m0 | round 3; also carries WP-M1 (answers 28-55) |
| W1 | merged | wp-w1 | d52896bee |
| L1 | merged | wp-l1 | 25ab39296 |
| T1 | merged | wp-t1 | 987bacbed |
| A1 | merged | wp-a1 | 44f1780a1 |
| K0 | merged | wp-k0 | f7b9fdd16 |
| T1b | merged | wp-t1b | 6cd067a39 |
| R1 | merged | wp-r1 | 8298608af (carried the answers 39-42, 50-53 part of R1b) |
| K1 | merged | wp-k1 | e1d2c6216 |
| A2 | merged | wp-a2 | c98034520 |
| R1b | merged | wp-r1b | 86117e7af |
| W2 | merged | wp-w2 | 3715363a9 |
| K0b | merged | wp-k0b | e30d43304 |
| K2 | merged | wp-k2 | 95788dcd0 |
| K3 | merged | wp-k3 | 12c52c2d7 |
| K4 | building | wp-k4 | kernel track |
| R4 | building | wp-r4 | bootfsd and consoled |
| D3 | building | wp-d3 | netd and ipd |
| S1 | merged | wp-s1 | 14bcc6e9d |
| V1 | merged | wp-v1 | 05955bf86 |
| D1 | building | wp-d1 | blkd; host side first |
| all others | waiting | | see BUILD-PLAN.md "Needs" |

States: `waiting` (needs not merged), `ready`, `building`, `review`, `merged`.
