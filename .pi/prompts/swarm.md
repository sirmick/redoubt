---
description: Run the Redoubt build swarm (SWARM.md) for ready work packages
argument-hint: "[package ...]"
---

Act as the Redoubt swarm **orchestrator**, using the `orchestrator` agent behavior described
in `.pi/agents/orchestrator.md` and the process in `docs/SWARM.md`. Do not implement packages
yourself.

Read first, in this order: `docs/SWARM.md`, `docs/BUILD-PLAN.md` (Order and Hotspots),
`docs/STATUS.md`, `docs/TENETS.md`. Then work the queue.

Requested packages: ${@:-none given; derive the ready set from the build plan}.

## Loop

1. **Pick the ready set.** A package is ready when every package it `Needs` is merged and no
   package conflicting with it on a Hotspot is in progress. The kernel track (`WP-K2`..`WP-K6`)
   is serialized; everything off the critical path may run in parallel. State which packages
   you are starting and why.

2. **One worktree per package.** Allocate an isolated worktree and a `wp-<id>` ref for each.
   Never let two packages write the same hotspot file (`kernel/src/syscall.rs`,
   `kernel/src/services.rs`, `kernel/src/mem.rs`, `kernel/src/arch/riscv/process.rs`,
   `loader/src/verify.rs`, the bench bundle builder, `docs/`).

3. **Run each package.** Launch `.pi/workflows/run-package.js` (via
   `subagent({ workflowScriptPath: ".pi/workflows/run-package.js", args: { package, reads,
   delivers, acceptanceCommand, designQuestion, notes }, cwd: "<package worktree>" })`) as one
   async workflow call per package, so the implementer and its three reviewers share the
   package's checkout. Set `args.designQuestion` when the package raises an open design
   decision.

4. **Honor the architect.** When a package hits a design question, the architect decides
   whether it is settled (cite and continue), open and answerable (recorded in QUESTIONS.md and
   ANSWERS.md, with a follow-up WP filed), or a genuine owner decision. If it is the owner's,
   surface it through `contact_supervisor` with `reason: "need_decision"` and **do not merge or
   start dependent packages until it is answered and applied**. Never decide the design
   yourself, and never edit `QUESTIONS.md`/`ANSWERS.md`.

5. **Accept and merge.** A package is done only when its acceptance tests and attack cases pass
   in `cargo testbench`, the whole bench is green, no undocumented `unsafe` exists and the
   ratchet did not rise, rv32 still compiles, and the three reviewers' findings are fixed or
   recorded. Then rebase the branch on `redoubt`, re-run the bench, and merge one package at a
   time. Keep the claims table in `docs/SWARM.md`, `docs/STATUS.md` current; add HISTORY entries only for milestones.

6. **Continue the waves.** After a merge, start whatever became ready. Stop when the ready set
   is empty or something needs the owner.

## Reporting

Keep a running summary: packages started, their state, questions asked (number and status),
review verdicts, what was fixed or recorded, merges (package and commit), what is blocked on
the owner, and the next wave.