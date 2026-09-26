# How Redoubt is built

Redoubt is built by a small swarm of AI agents under one human owner, run in Wash. Work is cut
into **packages**; each package has one implementer and a panel of read-only reviewers, a resident
Architect answers design questions, and an orchestrator plans, integrates and merges. This page
is the process: the roles, what a package is, how one runs and is accepted, how questions are
decided, and the current claims. The Wash setup itself (models, MCP calls, the QA file) is in
[PROJECT.md](PROJECT.md). The [tenets](TENETS.md) outrank this page, and the owner's instructions
in a session outrank it too.

This page and PROJECT.md are the only ones in the book that carry process material: package IDs,
QA thread names, review rounds. Every other page describes the system, and its provenance lives in
git.

## Roles

| Role | Tier | Job |
| --- | --- | --- |
| Orchestrator | frontier | plans the waves, keeps the claims, launches and ends package members, integrates and merges one package at a time; never implements |
| Architect | frontier | the resident design authority: answers design questions from the pages, records owner decisions, applies accepted rules to the owning page |
| Implementer | workhorse | builds exactly one package in its own worktree and branch |
| Red team | workhorse | reviews a package for a way to break it: a rule or invariant violated, a label or capability bypassed, an attack case whose verdict could be forged |
| Simplifier | light | reviews for what can be deleted or made simpler without losing a rule, and for any growth of the trusted computing base the pages did not require |
| Editor | light | checks that the code and the pages it cites agree, that every `SAFETY:` justification is true of the code, that names and paths are spelled the same everywhere, and that comments and pages keep the book's voice, vocabulary and links with no process leftovers |

**Tiers** name what a member is for, not a model. Cost matters: every call re-sends a member's whole
context, so the cheapest capable tier is used.

| Tier | Model and thinking |
| --- | --- |
| `frontier` | the strongest model the owner pays for (Claude Opus), high thinking |
| `workhorse` | the same model (Claude Opus), low thinking; medium for kernel commits and merge-gate reviews |
| `light` | an efficient everyday model (Claude Sonnet), low thinking |

### The orchestrator

- Owns the [claims](#claims) and the order of work, derived from the [plan](plan/m1-separation.md).
- Starts a package the moment every package it needs is merged, in its own worktree
  (`.worktrees/<package>`) and branch (`wp-<package>`).
- Sizes the review panel to the risk ([running a package](#running-a-package)) and keeps the same
  reviewers through every round of one package.
- Rules on a member's question itself when the pages settle it or a recommendation is clear, and
  flags any answer that would open an insecure or complex door. Asks the Architect when a package
  meets a design decision the pages do not settle, and never lets an implementer guess.
- Escalates to the owner only a genuine owner choice, and stops only the work that depends on it.
- Commits and merges; implementers and reviewers do not.
- Bounds every task it hands out: how many tool calls before the first written result, and what to
  return.
- Keeps a running report for the owner: packages started and their state, questions asked and
  their status, review verdicts, what was fixed or recorded as a follow-up, merges (package and
  commit), what waits on the owner, and the next wave.

### The Architect

- Resident for the whole workspace, one session, reused for every question so its context stays
  warm. Given the package, the page and rule involved, the exact contradiction and the candidate
  options with their consequences, so it re-derives as little as possible.
- Cites a settled rule directly; reopens nothing settled.
- For an open question it can answer from the tenets and the pages, it answers, writes the rule on
  its owning page, and names the follow-up package if code must change.
- For a genuine owner choice, it asks the owner with a recommendation and the real alternative,
  and keeps the dependent work blocked until the owner answers. A recommendation is never an
  approval.
- Edits pages, not code or tests. Source wins for current behaviour: a page that disagrees with the
  code is a finding, reported, not hidden.

### The implementer

Builds exactly one package and nothing else, and does not break these rules:
1. **One package, one worktree, one branch.** No other package's files.
2. **Stage only the paths the package delivers.** Never `git add -A` or `git commit -a`, and never
   run git against the shared checkout. Leave the work staged and uncommitted so the reviewers and
   the orchestrator see the diff, unless the assignment says to commit.
3. **The design is read-only.** A contradiction, a gap, or a decision the pages do not settle is
   reported as a blocking question naming the page, the rule and the options, never improvised.
4. **No undocumented `unsafe`,** and the ratchet only falls
   ([the unsafe budget](testbench.md#the-unsafe-budget)).
5. **rv32 keeps compiling:** the kernel, the loader, `redoubt-sys`, `redoubt-rt` and the servers.
6. **Every behaviour lands with its test.** A security property lands with an attack case whose
   verdict comes from the system, never from the attacker
   ([rule F](testbench.md#rule-f-trusted-verdicts)); a bug fix lands with the test that would have
   caught it.
7. **The real harness.** Run the package's cases and the focused tests with `cargo testbench`, and
   report the exact commands and exit codes.
8. **Rust and formatting,** per [CONTRIBUTING.md](../CONTRIBUTING.md#formatting).

It reports: what was delivered and the paths changed; the tests run, with exit codes, and each new
attack case with why its verdict comes from the system; the `unsafe` count before and after, the
rv32 build and the whole bench; design problems found; open risks; the branch and state; the next
step.

### Reviewers

One angle each, read-only: a reviewer reports findings and edits, creates and stages nothing.

- **Scoped to the diff.** Only concrete, current issues the diff causes or makes reachable, each
  backed by the source, a test or reproduction, or a contradiction with a page. The diff includes
  its untracked files: a reviewer lists them as well as the staged and unstaged changes.
- **Bounded.** The first finding within about 15 tool calls, the verdict within about 30. Read the
  diff or range given, the pages it cites and, for the red team, the rules it claims to keep; not
  the whole book, other branches or history. One question and a few named files per task: a
  compound question gets its first part answered and the rest named out of scope.
- **A finding** is exactly: the file and line, the concrete input, sequence or contradiction that
  triggers it, and what breaks. A claim without a mechanism is not a finding. Say **delete**
  (nothing depends on it, and what was checked), **trim** (keep the rule, cut the text) or
  **keep** (what depends on it). A residual the pages already state is noted and passed over.
- **Severity** P0, P1 or P2 on each finding, and one verdict at the end: `Merge verdict: BLOCK`,
  `Merge verdict: OK` or `Merge verdict: OK with notes`. `No issues found.` is a valid result.
- A reviewer completes its assignment copying the implementer, so the implementer already holds
  every finding.

## Packages

A package is a unit of work one implementer can finish and a panel can review. It is written as:

- **ID and size:** a short ID (letters and a number, such as `K7`, `S2` or `DOC1`) and S (hundreds
  of lines), M (about 1,500) or L (larger). An ID is never a lone R, I or M followed by digits,
  which the book uses for rules, invariants and milestones.
- **Reads:** the pages and rules it implements.
- **Delivers:** the paths it owns, which is its staging boundary, and the tests it adds.
- **Needs:** the packages that must be merged first, and the decisions that must be settled.
- **Accepted when:** the cases that must pass, including every attack case for a security property
  it touches.

The remaining work of each milestone, in order, is on its plan page, starting with
[M1 (separation and containment)](plan/m1-separation.md); the orchestrator cuts it into packages
and records them in the [claims](#claims).

**Hotspots** get one writer at a time: the kernel's page tables, memory, messages, call dispatch and
architecture mapping code, the loader's verification, the bench's bundle builder, and `docs/`.
Runtime IPC and server changes are coordinated with their native callers. Generated wire code
changes only through its tables and the generator.

## Running a package

1. **Worktree.** The orchestrator makes `.worktrees/<package>` on `wp-<package>`, and launches the
   implementer and the reviewers with it as their working directory, so the reviewers see the
   writer's diff.
2. **Design first.** If the package raises an open design question, the Architect settles it (or
   the owner decides) before any code is written.
3. **Implement,** gated on the package's acceptance command, by default the full bench.
4. **Review** in rounds. The panel is sized to the risk:
   - trusted code (the kernel, the loader, the ABI, `unsafe`, anything a rule or an attack case
     governs): the red team, the simplifier and the editor;
   - tests, docs, comments or tooling configuration only: one reviewer, the red team for tests or
     the editor for documentation, adding the others only if the findings show more risk.

   The orchestrator creates a round's assignments together and waits for all of them. It then
   decides which findings to apply and sends one fix assignment that cites them by reviewer and
   number (for example "red 3, editor 1") rather than restating them. Each round refreshes the diff
   and the evidence; retained context is not evidence.
5. **Fix and re-review** until the verdicts are OK, or the remaining findings are recorded as
   follow-ups.
6. **Accept and merge** ([acceptance](#acceptance)).

Review may be batched for small changes; trusted-code work gets its own round with the red team. A
package merged before its review is **merged (review due)**, not done, and that debt is cleared
before the next wave.

## Questions and decisions

Questions are Wash QA threads, one per question, named `<package>-<topic>`. A question opens with
the page and rule involved, the contradiction or gap, a recommendation and the evidence. Answers
carry the thread, so the question, its answers and any owner decision stay together. Only the
orchestrator, or a reviewer of the thread's package, resolves a thread, with evidence; a pending
owner decision prevents that. A thread reopens with a reason when the evidence changes.

The decision itself lives in the book, not in QA:
- An accepted rule is written on the page that owns it, with its reason beside it; a new security
  property takes the next free rule ID.
- An undecided question on a planned section is an item of that section's **Open:** list; deciding
  it removes the item and writes the rule.
- An owner decision that changes a guarantee or a wall is written on the [tenets](TENETS.md).
- Provenance (who decided, when, on which thread) is the commit message and the QA thread. The
  pages carry no decision numbers and no dates.

A package is not accepted with an unresolved blocking thread. A deferred non-blocking question is
recorded on its page's Open list or as a follow-up in `docs/todo/`.

## Acceptance

A package is done only when:
- its acceptance tests and attack cases pass in `cargo testbench`, each verdict from the system;
- the whole bench is green, and the docs checker finds nothing;
- rv32 still compiles;
- no `unsafe` is undocumented and no ratchet rose without a stated reason;
- every reviewer's finding is fixed or recorded as a follow-up;
- the pages it changes say what the code now does, with status lines naming the new tests;
- no blocking question is open.

The orchestrator then rebases the branch, reruns the bench, merges, updates the claims, and ends
the package's members.

## Staging, commits and handoffs

- Stage by path. Never `git add -A` or `git commit -a` in a shared worktree, never stage another
  session's work, never `git stash` (the stash is shared by every worktree; set work aside with a
  WIP commit).
- A commit message says what and why, and ends with a trailer naming the model that wrote it.
- Nobody pushes without the owner's word.
- **Handoff** at about 300K tokens of context: the member finishes and commits its current step,
  writes `<PACKAGE>-HANDOFF.md` in its worktree (state, next steps, traps, open questions), commits
  it, reports and stops. The orchestrator ends it and launches a fresh member from that file. A
  reviewer hands off between rounds, never during one.
- A member never ends a turn without a report or a waiting status, and never polls.

## Cost

- Test output fills context. Filter or tail bench and cargo output to the verdict and the failing
  lines; never read a whole log.
- Repeat a case about five times to confirm a result; twenty only when chasing a flake.
- Expensive tests run sparingly: the `consoled` flood test runs once per round, in loops of at most
  twenty.
- Build artefacts, caches and logs stay on the project root's filesystem.

## Claims

The ledger of package state. `waiting` means an unmet dependency or an unsettled decision; `ready`
still needs a scoped assignment; `building` and `review` describe execution; `merged` is not
accepted until its review is done. Merged packages leave the table; their record is in git.
"Step" below is a step of the remaining work on the plan for
[M1 (separation and containment)](plan/m1-separation.md#remaining-work).

| Package | State | What | Needs |
| --- | --- | --- | --- |
| DOC1 | review | the documentation rewrite, on `wp-doc1` | owner review, then the switch-over and the deletion of the old docs |
| kernel follow-ups | waiting | the kernel items of step 1 | DOC1's switch-over |
| server follow-ups | waiting | the server items of step 1 | DOC1's switch-over |
| beamlet follow-up | waiting | the module search order, step 1 | DOC1's switch-over |
| HIST1 | waiting | the git history rewrite | the old docs deleted |
| IPC1 | review | the IPC completion checker; its remaining gates are model replay on the real kernel, the timer, the serving path and concurrency | step 9 |
| typed parking | waiting | parking a typed call in the serving library | an owner decision; blocks the console's `resize` |
| the kernel containment gate | waiting | step 2 | the follow-ups |
| init and the manifest | waiting | step 3 | the follow-ups |
| beamlet on Redoubt; IEx on the console | waiting | step 4 | `init` |
| the file server | waiting | step 5 | `init` |
| the steward, `keyd` and `sshd` in a boot | waiting | steps 6 and 7 | the file server, beamlet |
| the agent and the attack suite | waiting | step 8 | everything above |

## Review debt

Outstanding, none blocking, each with its page:
[the kernel's print on a panic](todo/print-panic-reentry.md),
[`process_map`'s flag order](todo/process-map-flag-order.md) and
[the write-only mutation](todo/write-only-mutation-split.md).
