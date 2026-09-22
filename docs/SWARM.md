# Building milestone 1: how the work runs

Owns: how BUILD-PLAN.md's work packages are executed: one orchestrating session (the
`orchestrator` agent) that runs independent packages in parallel as sub-agents, each in its
own git worktree, and reviews every package before merging it. Three of the roles are project
agents in `.pi/agents/`; the rest are builtins (below). The orchestrator launches them through
the `subagent` tool.

## Roles
- **Orchestrator** (`orchestrator` agent, one session): owns the claims table below, starts packages whose needs
  are met, reviews results, merges, and keeps BUILD-PLAN.md, STATUS.md and HISTORY.md current. It
  does not implement packages itself.
- **Architect** (one long-lived sub-agent session per build): knows the frozen design back and
  forth and answers the questions a package cannot be built without. It is **resident, not a
  fresh consult per question**: the orchestrator spawns it once and then sends each later question
  with `subagent({ action: "resume", id: <architect-run>, message: ... })`, so it answers from
  context it already holds rather than re-reading the repository. Spawn a fresh one only when the
  old session is unrecoverable. The design is read-only to the swarm; the architect follows
  `.pi/skills/architect-qa/SKILL.md` (opens the question, records the answer, backlinks it, applies
  it to the owning note, adds the HISTORY entry). A genuine owner decision comes back still open,
  with `Rec` and `Alt`, and is not merged until the owner answers.
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
   owns (BUILD-PLAN.md "Delivers"). Never `git add -A` or `git commit -a` — a reviewer or another
   session can leave a file in the tree, and `-A` sweeps it into your commit. **Reviewers are
   read-only**: a reviewer that finds a missing note proposes it in its findings, it does not write
   it ("the design is read-only to the swarm", rule 3). After any round, the orchestrator checks
   `git status` for files it did not create before staging.
3. **The design is read-only.** An implementer that finds a spec problem stops and reports it; the
   orchestrator asks the **architect**, who records the question in QUESTIONS.md and the answer in
   ANSWERS.md and raises a genuine owner decision with the owner. Nothing merges while its design
   question is open. Changes need a HISTORY.md entry.
4. **Done means:** the package's acceptance tests and attack cases pass in `cargo testbench`; the
   whole bench is still green; no undocumented `unsafe` and the ratchet does not rise; rv32 still
   compiles; and the package has had its **round of review** (rule 8).
5. **Merging.** The orchestrator rebases the branch on `redoubt`, re-runs the bench, and merges one
   package at a time. One line per package in HISTORY.md.
6. **Other sessions.** Any other session working in this repository finishes or pauses its work
   before the build starts, and follows the same staging rule.
7. **The record is the source of truth.** Every design decision a package depends on is traceable
   to a numbered question and answer (QUESTIONS.md, ANSWERS.md). An implementer is told the
   question and answer number it is building to; an instruction without one is not a design change,
   it is a guess.
8. **Review is batched, and mandatory.** A package may merge on its acceptance gate alone, but it
   is not *done* until it has been reviewed — by a **round** that covers one or more packages, not
   necessarily one round per package. This is a deferral, never a waiver:
   - A round is **bounded by risk**: a codec, docs, vectors or bench diff may share a round with
     others; anything touching the TCB (kernel, loader, ABI, signing) or a security rule gets its
     own round and a red-team reader.
   - The orchestrator keeps a **review debt** list — every merged-but-unreviewed package and its
     commit range — and runs it down before starting a new wave, or sooner if the debt touches the
     TCB.
   - A round's findings are fixed or recorded, the HISTORY line for each package is amended to say
     it was reviewed and what the round found, and the debt entry is cleared.
   - The three angles are the design's: **red team** (attack it against the spec and the attack
     suite), **simplifier** (what can be deleted), **editor** (code, comments and notes agree).
     Order them by what the round is for.

## Agents and workflows

The roles above are `.pi/agents/` project agents (`architect`, `implementer`, `orchestrator`)
or builtins shipped by the extension; `orchestrator`'s `allowedAgents` names exactly the set it
may spawn. The workflows are the runnable path — the orchestrator spawns `architect`,
`implementer` and `reviewer` directly only when it does not use them.

| Role | Agent | Source |
| --- | --- | --- |
| Orchestrator | `orchestrator` | `.pi/agents/orchestrator.md` |
| Architect | `architect` (protocol: `.pi/skills/architect-qa/SKILL.md`) | `.pi/agents/architect.md` |
| Implementer | `implementer` | `.pi/agents/implementer.md` |
| Reviewers | `reviewer`, one child per angle (red team, simplifier, editor) | builtin |
| Orientation | `scout` (source recon before starting a package) | builtin |
| Implementation help | `worker` | builtin |
| Decision consistency | `oracle` | builtin |
| Lightweight tasks | `delegate` | builtin |

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
| 1 | M0/M1 model, W1 codecs, L1 littlefs, T1 bench extensions, A1 ABI crate |
| 2 | K1 budgets and handles; R1 runtime; L1 and T1 continue |
| 2b | design review of answers 1-55; A2 ABI update; W2 generator update |
| 3 | K2 endpoints and messages (carries A3); host-side parts of D1/D2/D3 against R1 |
| 4 | K3 devices and interrupts; R4 bootfsd and consoled (once K3 lands) |
| 5 | K4 process creation; B1 beamlet platform; D1 blkd; D3 netd and ipd; S1 keyd; W3a opcode floor (merged) |
| 6 | K5 timer and preemption; R2 loader stub; D2 fsd |
| 7 | R3 init (carries `confined`); C1 conformance; B2 IEx on the UART |
| 8 | K6 delete legacy; S2 steward |
| 9 | S3 sshd |
| 10 | E1 the agent and the attack suite: milestone 1 done |

## Claims
The claims table is the source of truth for package state; `docs/BUILD-PLAN.md`'s Order derives from
it. A package whose work landed inside another is recorded as `folded` and gets no branch of its own.

| Package | State | Branch | Notes |
| --- | --- | --- | --- |
| M0 | review | wp-m0 | round 3 |
| M1 | review | wp-m1 | carried by wp-m0 (answers 28-101) |
| W1 | merged | wp-w1 | d52896bee |
| W2 | merged | wp-w2 | 3715363a9 |
| W3a | merged (review due) | wp-w3 | 612a0a599; the 9P opcode floor and the `copy_file` rename (answers 113, 155) |
| A1 | merged | wp-a1 | 44f1780a1 |
| A2 | merged | wp-a2 | c98034520 |
| A3 | folded | | into wp-k2 (answer 103; the `first` flag) |
| L1 | merged | wp-l1 | 25ab39296 |
| T1 | merged | wp-t1 | 987bacbed |
| T1b | merged | wp-t1b | 6cd067a39 |
| T1c | review | wp-t1c | Checker repair and three-reviewed runtime reduction included in the owner-authorized primary remediation commit; nine checker tests pass. Primary full bench and every configured unsafe budget PASS, runtime9/9 with zero undocumented. No budget increases |
| V1 | merged | wp-v1 | 05955bf86 |
| K0 | merged | wp-k0 | f7b9fdd16 |
| K0b | merged | wp-k0b | e30d43304 |
| K1 | merged | wp-k1 | e1d2c6216 |
| K2 | merged | wp-k2 | 95788dcd0 (carried A3) |
| K3 | merged | wp-k3 | 12c52c2d7 |
| K4 | building | wp-k4 | kernel track |
| K5 | ready | | needs K2; serialized behind K4 on the kernel Hotspots, not on dependencies |
| K6 | waiting | | needs K1-K5, R1b |
| IPC1 | review | wp-ipc1 | Reviewed ABI/kernel/runtime/server changes and R1 borrowed-alias fix included in the owner-authorized primary remediation commit. Three-review follow-up PASS; isolated and primary full benches99 executions PASS each, primary283 host tests PASS/3 ignored, affected crates compile both widths. Model/K5/multi-hart acceptance and native process-exit integration remain outstanding. Full IPC1 acceptance remains pending |
| R1 | merged | wp-r1 | 8298608af (carried the answers 39-42, 50-53 part of R1b) |
| R1b | merged | wp-r1b | 86117e7af |
| R1c | merged | wp-r1c | cd65fa610; joined `Parked` to the 9P skeleton (recovery of `5d29d136e`, answers 156-158); reviewed R-R1c |
| R1d | waiting | | let a typed call park (answer 163); needs R1c (merged); blocks `resize` in B2a |
| R2 | waiting | | needs R1b, K4 |
| R3 | waiting | | needs R2, W1, K3, K5; carries the `confined` manifest |
| R4 | merged | wp-r4b | 69466924c; bootfsd and consoled recovered from wp-r4; reviewed R-R4b |
| B1 | waiting | | needs R1b, R4 (both merged) |
| B2 | waiting | | IEx on the UART; needs B1, R3 |
| B2a | waiting | | the console library (`consol` codec, `Redoubt.Console`/`.Key`, answer 162); needs B2, R4b |
| B2b | waiting | | `Redoubt.Ed`, `Shell.top()`; needs B2a |
| D1 | merged | wp-d1b | 8681f2648; blkd recovered from wp-d1, reviewed R-D1 (editor BLOCK fixed; red team 4/4 OK) |
| D2 | waiting | | needs D1, L1, R1b |
| D3 | building | wp-d3 | netd and ipd |
| S1 | merged | wp-s1 | 14bcc6e9d |
| S2 | waiting | | needs R3, B1, D2 |
| S3 | waiting | | needs D3, S1, S2 |
| C1 | waiting | | needs M1, K5, T1, IPC1 (answers 167-168 outcomes in traces) |
| E1 | waiting | | needs everything (milestone) |

States: `waiting` (needs not merged), `ready`, `building`, `review`, `merged (review due)` (merged on
its acceptance gate, round not yet run), `merged`, `folded` (landed inside another package).

**Local coordination check (2026-09-22):** the recorded M0/M1, K4 and D3 claims have no
corresponding branch or worktree in this checkout. Their external state is unverified, not
silently completed or cancelled. Under the owner's renewed remediation instruction, IPC1 is
the sole local kernel writer, in its existing isolated worktree; it does not implement K4/K5.
Any external kernel changes must be reconciled before integration. The architect is reconciling
settled documentation findings; tooling/codec fixes will use `wp-doc-accuracy`. No commits or
pushes are authorized in this pass. Working-tree integration is distinct from a merged package.

## Review debt

Rounds owed, newest first (SWARM.md rule 8). Run down before a new wave.

| Round | Range | What | Angles |
| --- | --- | --- | --- |
| R-1 | `6b224c0ab..612a0a599` | WP-W3a (codec generator: opcode floor + `copy_file`) | **complete**: red team (2 P2), simplifier (2 deletions), editor (mis-documented error-marker behaviour) all applied; debt cleared |
| R-2 | `3e5b49f46^..58601c588` | the use case / tenets / confinement arc, answers 150-155, and GAME.md | **simplifier + red team + editor done** (trims `58601c588`; device gap `8f5c08fdb`; citations fixed); debt cleared |
| R-3 | `659adbdcd` | the swarm protocol change (resident architect, bounding rule) | **simplifier done** (`ca7a2a4ca`); debt cleared |
| R-R1c | `b8e456eeb..eae54bf1f` | WP-R1c (`libs/rt`: join `Parked` to the skeleton) | **complete**: editor P0 fixed (`eae54bf1f`); red team OK, no issues; debt cleared |
| R-R4b | `89e05e360` | WP-R4b (`bootfsd` + `consoled`) | **complete**: bootfsd red team (2 P2, `8647a4fd9`); consoled park path reviewed directly after 3 timeouts; debt cleared |
| R-T1 | `80cc5634d..HEAD` | the terminal change (push rule, `consol`, `Redoubt.Console`) | **complete**: security (cross-channel leak, `e888d88f1`); simplifier (5 trims); editor (`size()` cache contradiction); debt cleared |

## Cross-cutting review records

**Commit authorization (2026-09-22).** The owner subsequently requested a commit of the
reviewed primary remediation. The no-commit/uncommitted wording in the dated checkpoints below
describes their state before that authorization. Committing the correctness slice does not
close IPC1's remaining acceptance gates. No push was requested.

**R-IPC1-Sol (2026-09-22): three-review PASS; implementation applied uncommitted.**
The owner authorized Sol after the prior agent restriction. The same-process borrowed-alias
regression failed before the fix on rv64 and rv32, then passed with explicit borrower protection
and identity-checked return. Editor, defensive reviewer and simplifier approved the correction.
The editor's missing release-error invariant check was fixed, with an exact post-abandon page
charge assertion; no reachable ownership mismatch was demonstrated. Parent reran 229 isolated
host tests (2 ignored) and the complete isolated bench (99 passing executions, 56 cases).
The coherent source and the previously reviewed runtime9/9 reduction are now applied to primary;
283 primary host/wire/checker tests pass, 3 ignored, and the final primary full bench passes
99 executions. Affected crates compile on both widths. All three integration rechecks passed
the small API/status edits. Model, K5 timer,
simultaneous multi-hart completion races and native process-exit integration remain outstanding;
the shared server's terminal fallback is host-tested only. No commit, push or package acceptance.
The earlier records below are historical checkpoints, superseded by this integration state.

**R-DOC-accuracy (2026-09-22): three-review PASS, applied uncommitted.** Scope: the
documentation-review correctness findings, not its proposed verbosity reduction. Architect edits
preserve accepted 114/160/162/167-168 and explicitly retain open 143/146/163/164-166. Tooling fixes
correct prerequisite/CLI/debug/status/publication claims; Rust and Elixir codecs were regenerated.
Editor/red-team findings fixed: the new server regression decodes the actual Disconnect opcode,
and TENETS distinguishes optional rv32 boots from later required full-stack acceptance. Simplifier
found no additional issue. Parent reran in the primary checkout: 53 wire/generator tests pass,
1 ignored; actual-server regression 1 pass; publication-link unittest 1 pass; whitespace clean.
Release source-debug command was verified against a kernel compilation unit in the isolated docs
tree. Elixir is not installed, so generated Elixir execution is unverified. No live-site check,
commit or push. This does not accept IPC1's kernel changes.

**R-T1c-runtime (2026-09-22): three-review PASS, runtime integration pending.** A private mapped-byte
view replaces three raw slice constructions with one, using safe reborrows and releasing the view
before ownership-changing syscalls. Runtime unsafe genuinely falls 11 to 9, with unchanged limits;
`message.rs` joins kernel-core coverage at zero additional unsafe. Parent reran 122 host/doc tests
and the configured checker, all passing. Reviewers confirmed lifecycle/error-path consistency and
no count gaming. Existing safe raw mapping/syscall escape routes remain a broader inherited
soundness boundary; this round does not certify arbitrary combinations of raw and owning APIs.
The checker repair alone is now applied to primary: its nine tests pass and its production gate
honestly fails on primary's still-unmodified runtime11/9. The isolated runtime change is not delivered.

**R-IPC1-kernel (2026-09-22): first round BLOCK, fix in progress.** Editor and simplifier
passed the new completion/rollback changes; the defensive reviewer found an inherited dependency
that prevents ownership acceptance: a same-process received-lend alias can pass PID ownership
checks, be unmapped/freed, and leave a stale frame for restoration. The architect confirmed that
R3/R4/I9 already require protection independently of PID equality; no new owner decision is needed.
The new regression is drafted, but its pre-fix execution and the lifetime fix were interrupted by
a platform restriction that repeated on retry. Parent reran the earlier checked
outcome test on both widths successfully, then the whole bench: every case passed except
`bench-bundle-file`, whose fixture source had the obsolete `redoubt/` prefix. That separate
one-line fixture fix is applied and its focused boot passes. Green existing tests do not close
the newly discovered lifetime defect. No kernel source was integrated into primary.

**R-ASTRA (2026-09-22): initial architect triage; subsequent implementation below.**
[ASTRA.md](../ASTRA.md) records the consistency and simplification reviews, the completed
design-only adversarial review, and the earlier partial code-adversarial observations. This was a
live-tree assessment initially based on `d9ed817ec`, with the design-only restart after
`61fd197f9`; it is not acceptance of a package commit range. The owner's RustSBI work was excluded.
The resident architect routed genuine decisions to **QUESTIONS.md 164-168**, initially all open:
confinement mediation, authority closure, scheduling latency, caller IPC disposition and server
reply disposition. C1/A1 share D4's design issues; C2/C3/C4/A3 are implementation follow-ups under
existing contracts; A2 needs bounded validation. ASTRA.md holds the per-finding dispositions and
proposed follow-ups. No implementation package was dispatched or marked done by this round, and
no existing package review debt is cleared by it. The owner subsequently approved **167-168 as
recommended**; the architect applies them to the IPC contract and records their answers.
**164-166 remain open.** At this triage checkpoint WP-IPC1 was filed as ready, not dispatched, with separate start and
completion gates in BUILD-PLAN.md. Approval and specification edits do not fix C1/C2/A1 or
complete an implementation review; follow-up TCB/security changes require their own risk-bounded
review rounds.

**R-IPC1-design (2026-09-22): three-review round complete.** Scope: the application of approved
answers 167-168 in KERNEL-SPEC.md, CAPABILITIES.md and CONTAINMENT.md, their Q&A/history links,
and the WP-IPC1 plan/claims/report. This is an uncommitted documentation change, not a merged
implementation range. **Consistency/editor PASS; design-only adversarial PASS; simplifier PASS.**
Findings resolved and rechecked: R1 protects output mapping/lifecycle state throughout completion;
S1 assigns initial output validation (C3) to IPC1; S2 makes C4's unsafe-coverage prerequisite
explicit without adding repository-wide policy. ASTRA.md section 5 records the dispositions;
HISTORY.md records the round. The generator check passed 16 tests. WP-IPC1 was ready, not
dispatched at this design checkpoint; its implementation requires its own acceptance tests and three-review TCB round.

**R-T1c (2026-09-22): checker-diff reviews complete; acceptance still red.** Scope: uncommitted
`tools/testbench/src/budget.rs` and `tests/unsafe-budget.toml` in branch `wp-t1c`, based on
`61fd197f9`. Consistency/editor, defensive failure-path and simplifier reviewers all PASS with
no required edits. Parent reran the host tests (9 passed). The focused unsafe-budget case now
correctly fails: runtime 11 documented uses versus its unchanged budget 9; all other configured
components pass. This is pre-existing debt uncovered by real coverage, not permission to raise
the budget. No merge or package acceptance is recorded. The same reviewed two-file repair is
overlaid in the IPC1 worktree for honest measurement, not a separate IPC1 implementation claim.

**R-IPC1-host (2026-09-22): partial host-slice review complete; IPC1 not accepted.** Scope:
the staged ABI/runtime/server/client/test changes in `wp-ipc1` based on `61fd197f9`, not kernel or
model code. **Editor, defensive adversarial reader and simplifier PASS after fixes.** E1/R1
found a rejected reply losing its still-open request: the finish helper now attempts a handle-free
fallback, preserving the original error for provisional rollback, and exits under R4b if fallback
also fails. Stateful serving-path regressions cover open-call/lend cleanup. S1 replaced a copied
validity predicate in ABI tests with explicit lifecycle rows; E2 corrected a stale server-loop
comment. Parent reran 219 passing host/doc tests (2 ignored timing tests) and affected-crate checks
on rv32/rv64. Production unsafe counts are unchanged; the corrected checker still fails runtime
11 versus 9. No kernel/model/full-boot acceptance, commit, merge or prior review-debt clearance is
implied. Kernel coordination and external prerequisites remain outstanding; see ASTRA.md section 6.
