# How Redoubt is built

Redoubt is built by a small swarm of AI agents under one human owner, run in Wash. Work is cut
into **packages**; each package has one implementer and a panel of read-only reviewers, a resident
Architect answers design questions, and an orchestrator plans, integrates and merges. This page
is the process: the roles, what a package is, how one runs and is accepted, and how questions are
decided. The Wash setup itself is in [PROJECT.md](PROJECT.md). The [tenets](../docs/TENETS.md)
outrank this page, and the owner's instructions in a session outrank it too.

Process material lives in `.wash/` and nowhere else: package IDs, thread names and review rounds
never appear in [the book](../docs/README.md), which describes the system; its provenance is in
git and in this directory.

## What is recorded where

One definition of each thing. The book states the rules, the plan and what is built; Wash records
only what exists nowhere else, and links to the book for the rest.

| What | Where | Written by |
| --- | --- | --- |
| the rules, each milestone's goal and remaining work, what is built | [the book](../docs/README.md) | the implementer, the Architect |
| the order of the work, and its state | the plan graph, `.wash/plan.toml` | Wash, through `plan_set` and `plan_accept` |
| the discussion behind a decision | `.wash/qa/<thread>.md`, one file per thread, kept for good | Wash |
| a package's acceptance evidence | the trailers on its merge commit, from `plan_accept` | Wash, committed by the orchestrator |
| handoffs, plans, scratch | `.wash/local/`, never committed | members |

A plan node's body is a link to its plan step or its `docs/todo/` pages, never a restatement of
them, and a resolved thread ends with a link to the page where its rule was written.

## Roles

| Role | Tier | Job |
| --- | --- | --- |
| Orchestrator | frontier | plans the work in the plan graph, launches and ends package members, integrates and merges one package at a time; never implements |
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

- Owns the plan graph: the packages, their order (`needs`) and their state, cut from the
  milestone's remaining work on its plan page, starting with
  [M1 (separation and containment)](../docs/plan/m1-separation.md#remaining-work).
- Starts a package the moment every node it needs is done, in its own worktree
  (`.worktrees/<package>`) and branch (`wp-<package>`). Wash refuses an earlier start unless the
  call gives an override, which it records on the node.
- Sizes the review panel to the risk ([running a package](#running-a-package)) and keeps the same
  reviewers through every round of one package.
- Rules on a member's question itself when the pages settle it or a recommendation is clear, and
  flags any answer that would open an insecure or complex door. Asks the Architect when a package
  meets a design decision the pages do not settle, and never lets an implementer guess.
- Escalates to the owner only a genuine owner choice, and stops only the work that depends on it.
- Merges; implementers commit on their own package branch and reviewers commit nothing.
- Bounds every task it hands out: what to read (a reading list, not "the book"), how many tool
  calls before the first written result, and what to return. Context is the cost: a member that
  reads broadly before writing hands off before it has written anything.
- Answers the owner's status questions from `plan_get`; if the plan does not explain what is
  happening, the plan is wrong, and it fixes the plan first.

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
- May add and edit plan nodes that have not started; only the orchestrator moves a started one.

### The implementer

Builds exactly one package and nothing else, and does not break these rules:
1. **One package, one worktree, one branch.** No other package's files.
2. **Stage only the paths the package delivers,** and commit them on the package branch in small
   groups: never more than about 50K tokens of uncommitted work, never `git add -A` or
   `git commit -a`, never git against the shared checkout. Reviewers read the branch's commits.
   Every file committed was read in full by the member that commits it; helper sub-agents draft
   nothing that is reviewed.
3. **The design is read-only.** A contradiction, a gap, or a decision the pages do not settle is
   reported as a blocking question naming the page, the rule and the options, never improvised.
4. **No undocumented `unsafe`,** and the ratchet only falls
   ([the unsafe budget](../docs/testbench.md#the-unsafe-budget)).
5. **rv32 keeps compiling:** the kernel, the loader, `redoubt-sys`, `redoubt-rt` and the servers.
6. **Every behaviour lands with its test.** A security property lands with an attack case whose
   verdict comes from the system, never from the attacker
   ([rule F](../docs/testbench.md#rule-f-trusted-verdicts)); a bug fix lands with the test that
   would have caught it.
7. **The real harness.** Run the package's cases and the focused tests with `cargo testbench`, and
   report the exact commands and exit codes.
8. **Rust and formatting,** per [CONTRIBUTING.md](../CONTRIBUTING.md#formatting).
9. **The pages move with the code.** A section the package builds goes from planned to built in
   the same commit as the tests its status line names; an Open item the package decides leaves
   the page in that commit; a rule the code departs from is written as the rule, with the
   departure as a residual and a follow-up ([lock step](#the-pages-move-with-the-code)).
10. **A clean branch at acceptance.** Before the merge the implementer rebuilds its branch into
    logical commits, each with a clean message ([commits](#staging-commits-and-handoffs)): no
    WIP, fix-round or review-label commits reach the main history.

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
- **Severity** P0, P1 or P2 on each finding, and one verdict as the **first line** of the result
  (Wash copies it into the merge commit's `Reviewed-by` trailer): `Merge verdict: BLOCK`,
  `Merge verdict: OK` or `Merge verdict: OK with notes`. `No issues found.` is a valid result.
- A reviewer completes its assignment copying the implementer, so the implementer already holds
  every finding.

## Packages

A package is a unit of work one implementer can finish and a panel can review. It is a `package`
node in the plan graph, under its milestone, and is written as:

- **ID and size:** a short ID (letters and a number, such as `K7`, `S2` or `DOC1`) and S (hundreds
  of lines), M (about 1,500) or L (larger). An ID is never a lone R, I or M followed by digits,
  which the book uses for rules, invariants and milestones.
- **Tier:** A or B ([two tiers](#two-tiers)).
- **Body:** links only: the plan step it builds and the `docs/todo/` pages it closes. Its reading
  list, owned paths, tests and acceptance cases go in the implementer's instructions, from those
  pages.
- **Needs:** the nodes that must be done first, and the QA threads that must be resolved.

### Two tiers

The tier is decided by what the code can reach, not by its language. A package is **Tier A** if
any answer is yes:
- does it hold, mint or forward a capability, or change what a budget may reach;
- does it parse input from another label set or from outside the box (a device, the network, a
  file another principal wrote);
- does it render or carry an approval;
- does it run outside one session's own budget: the kernel, the loader, the stub, the runtime
  library, the wire formats, the model, the drivers, `init`, the steward, `keyd`, `sshd`, `ipd`,
  the resolver, `gatewayd`, the bench and the checker, and the beamlet VM itself, which runs
  hostile code.

Everything else is **Tier B**: code that runs inside one session's budget with that session's
authority, where a bug hurts one principal and the walls below hold. Most Elixir is Tier B: the
shell, the editor, helpers and client bindings. The agent harness, approval rendering and the
transfer server are Elixir or Rust and Tier A, because they answer yes above.

| | Tier A | Tier B |
| --- | --- | --- |
| Design questions | the Architect, before code | the page's Open list; the Architect only if the package decides one |
| Panel | red team, simplifier, editor; red at medium for the merge gate | one reviewer: the red team at low if the diff touches any question above, else the editor |
| Tests | attack cases with system verdicts, mutations where the model covers it | host tests under beamlet, plus one end-to-end bench case |
| Rounds | its own rounds until OK | batched with other Tier B packages; merged on green with one OK |
| Gates | the full bench, rv32, the unsafe ratchet, the size budget, the docs checker | the package's cases, the docs checker |

A Tier B package that turns out to touch a question above is re-tiered, not waved through.

### The pages move with the code

There is no documentation package. Every package delivers its page delta, and the bench enforces
it: the docs checker refuses a status line naming a test that does not exist, a rule cited that no
page owns, and a security register that disagrees with a page. The editor reviews the pages the
package names and nothing else; the red team's checklist includes "the page says what the code now
does". At acceptance the orchestrator updates the plan page's progress.

**Hotspots** get one writer at a time: the kernel's page tables, memory, messages, call dispatch and
architecture mapping code, the loader's verification, the bench's bundle builder, and `docs/`.
Runtime IPC and server changes are coordinated with their native callers. Generated wire code
changes only through its tables and the generator.

## Running a package

1. **Worktree.** The orchestrator makes `.worktrees/<package>` on `wp-<package>`, and launches the
   implementer and the reviewers on the package's node with it as their working directory, so the
   reviewers see the writer's diff.
2. **Design first.** If the package raises an open design question, the Architect settles it (or
   the owner decides) before any code is written.
3. **Implement,** gated on the package's acceptance command, by default the full bench.
4. **Review** in rounds. The panel is the package's tier ([two tiers](#two-tiers)); tests, docs,
   comments or tooling configuration alone take one reviewer, the red team for tests or the editor
   for documentation, adding the others only if the findings show more risk.

   The orchestrator creates a round's assignments together and waits for all of them. It then
   decides which findings to apply and sends one fix assignment that cites them by reviewer and
   number (for example "red 3, editor 1") rather than restating them. Each round refreshes the diff
   and the evidence; retained context is not evidence.
5. **Fix and re-review** until the verdicts are OK, or the remaining findings are recorded as
   follow-ups in `docs/todo/`.
6. **Accept and merge** ([acceptance](#acceptance)).

Review may be batched for small changes; trusted-code work gets its own round with the red team. A
package merged before its review keeps its node in the state `review-due`, not done, and that debt
is cleared before the next package starts.

## Questions and decisions

Questions are Wash QA threads, one per question, named `<package>-<topic>`. A question opens with
the page and rule involved, the contradiction or gap, a recommendation and the evidence. Answers
carry the thread, so the question, its answers and any owner decision stay together. Only the
orchestrator, or a reviewer on the thread's node, resolves a thread, with evidence; a pending
owner decision prevents that. A thread reopens with a reason when the evidence changes.

The decision itself lives in the book, not in QA:
- An accepted rule is written on the page that owns it, with its reason beside it; a new security
  property takes the next free rule ID.
- An undecided question on a planned section is an item of that section's **Open:** list; deciding
  it removes the item and writes the rule.
- An owner decision that changes a guarantee or a wall is written on the
  [tenets](../docs/TENETS.md).
- Provenance (who decided, when, on which thread) is the thread file and the merge commit's `QA:`
  trailer. The pages carry no decision numbers and no dates.

A package is not accepted with an unresolved blocking thread. A deferred non-blocking question is
recorded on its page's Open list or as a follow-up in `docs/todo/`.

A thread body is at most 2,000 bytes; a plan, a review report or any deliverable is a file in the
worktree, and the thread holds the pointer. Wash writes each thread to `.wash/qa/<thread>.md` as
it changes; nobody edits those files, and they are committed with the merge that resolves them.

## Simplification

Simplicity is a gate, not a suggestion. The measures:
- **Size is budgeted like `unsafe`.** Each trusted crate has a line ceiling in the bench that only
  falls without a reason stated in the commit
  ([the size budget](../docs/todo/size-budget.md), planned until its case exists).
- **Every Tier A acceptance report says what was deleted.** A kernel package that adds lines and
  deletes none states why, as an `unsafe` increase must.
- **Simplifier findings are P2 by default:** each is applied, or declined on the thread with a
  reason. The simplifier asks three questions of every diff: what can be deleted, what duplicates a
  mechanism that exists, and is this the one obvious way.
- **A deletion package after each milestone,** scheduled, with a target: the last one took the
  kernel from 18,000 lines to 10,400 and `unsafe` from 54 to 44.
- **A mechanism whose page cannot state its Why in two sentences** is a candidate for removal, and
  the editor says so.

## Acceptance

A package is done only when:
- its acceptance tests and attack cases pass in `cargo testbench`, each verdict from the system;
- the whole bench is green, and the docs checker finds nothing;
- rv32 still compiles;
- no `unsafe` is undocumented and no ratchet rose without a stated reason;
- every reviewer's finding is fixed or recorded as a follow-up;
- the pages it changes say what the code now does, with status lines naming the new tests;
- no blocking question is open.

The orchestrator then rebases the branch, reruns the bench, and calls `plan_accept` with each
gate's command and exit code. Wash sets the node done and returns the trailer block and the
`.wash/` files to stage; the orchestrator merges with the trailers as the message's last
paragraph, stages those files in the merge, and ends the package's members.

## Staging, commits and handoffs

- Stage by path. Never `git add -A` or `git commit -a` in a shared worktree, never stage another
  session's work, never `git stash` (the stash is shared by every worktree; set work aside with a
  WIP commit, and fold it before the merge).
- **A commit message says what changed and why,** in the book's voice: `<component>: <what>`, then
  a body. It carries no package IDs, thread names, answer numbers or review labels; those belong in
  trailers. An owner decision is stated in plain words ("Owner decision: …"). It ends with a
  trailer naming the model that wrote it.
- A package's merge commit carries the trailers `plan_accept` returns: `Plan-Node`, `QA`, `Gates`
  and `Reviewed-by`.
- `.wash/plan.toml` and `.wash/qa/` are committed only with a package's merge, or in one commit
  when the owner parks or ends the workspace. Nothing else commits them.
- Nobody pushes without the owner's word.
- **Handoff** at about 300K tokens of context (Wash warns the orchestrator at `context_warn`): the
  member finishes and commits its current step, writes its handoff with `member_update {handoff}`
  (state, next steps, traps, open questions; Wash keeps it in `.wash/local/`, out of git), reports
  and stops. The orchestrator ends it and launches a fresh member with `handoff_from`, and a
  reading list of one example of the work and the entries for its next step: never the whole plan.
  A reviewer hands off between rounds, never during one.
- A member never ends a turn without a report or a waiting status, and never polls.

## Cost

- Every call re-sends a member's whole context, so reading is the cost. A member reads what its
  assignment lists, and derives by grep where it can; a lead that must know a set of pages reads
  their headings and status lines, not their bodies.
- Test output fills context. Filter or tail bench and cargo output to the verdict and the failing
  lines; never read a whole log.
- Repeat a case about five times to confirm a result; twenty only when chasing a flake.
- Expensive tests run sparingly: the `consoled` flood test runs once per round, in loops of at most
  twenty.
- Build artefacts, caches and logs stay on the project root's filesystem.
