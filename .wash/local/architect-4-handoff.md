# Architect handoff (architect-4, 2026-10-02)

Read architect-handoff.md and architect-2-handoff.md first. Their rulings and working rules
stand: pages on main, stage by path, run doccheck
(`./dev.sh bash -c 'cd /work && cargo run -q -p redoubt-doccheck'`) before every commit, QA
bodies of at most 2000 bytes (answers to the orchestrator too), no thread names or dates on
pages. Two checker rules that bite: the first citation of a rule is written "R12 (scheduling)",
and a milestone is written in full, as "M5 (persist, install, share)". A page another package's
branch is rewriting gets no edits from you on main: give that package the exact lines and check
them at its review.

## Rulings today, and where each lives

| Thread | Ruling | Where |
| --- | --- | --- |
| STEWARD0-tables | C1-C12 and N1-N7 for the transition tables: owns_labels reads the manifest; every rule guard has a mutation; crossings split by kind (no effect branches on kind); every batch lands on a waiting state (the crossing's Closing); internal events run first in, first out inside decide; an unlabelled request for a labelled target is recorded under the target; a sponsor is told when a lease ends; a pushed item is not capped | steward.md a61477f7f; .wash/local/STEWARD0-tables-ruling.md |
| K17-creator-dying | "doomed" defined once: a process that R10's step 2 or 3 ends (it runs in a dying budget, or its creator's budget is dying) | budgets.md aa74eee32; ipc.md R4b wording merged with K17 |
| STEWARD0-keyd-keys | login_key and approval_key accept only the role's own keys; the mutations widen them to keyd's keys or to login keys; KeydAdd leaves the model | steward.md 22ed434d9 |
| STEWARD0-unreachable-mutations | One keeper per rule, and its mutation breaks that keeper. may_see and approver_holds are gone; `reaches(channel, domain)` carries PolicyShowLabelledToAll; PolicyApproverExceeds is retired (the fifth) | steward.md 213ace990 |
| STEWARD0-agent-unlabelled | (b): Agent{[]} is refused from every caller, and a labelled caller's agent request names exactly its own set (agent_own_set, PolicyAgentOtherSet) | steward.md, through STEWARD0's merge 22b659d9e |
| GATE1-notice-two-leases | Not the walks. A checked build's audits still moved the schedule, so owner decision A is applied in full: K18 (audits charged to no budget and outside the slice). Then B5 (every share judged net of audits) | scheduling.md residual and todo/audits-billed.md 307b5d818 (K18 removes both); .wash/local/GATE1-two-leases-architect.md, K18-implementer.md, B5-implementer.md, B5-k18-trim.md |

STEWARD0 is merged (22b659d9e).

## Open, and what waits on what

- **K18** is in its fix round; review was OK with notes. My ask, which the orchestrator relays:
  its trim adds the residual "Some shares are judged gross of audits",
  docs/todo/shares-judged-gross.md and its SUMMARY line, with the text in
  .wash/local/B5-k18-trim.md.
- **GATE1** holds its WIP fold (wp-gate1 e9ecc5ea5) and needs K18. It resumes unchanged on K18's
  merge: the sweep, the per-width seed sentence, the bench, the fold, the red team's re-review.
- **INIT1** (accepted) waits for GATE1's merge.
- **K16** needs GATE1 and INIT1. Its commit 1 (walks by live thread) stays in K16, measured
  against GATE1's numbers. There is no cycle: K18 needs only K17.
- **B5** needs K18 (brief B5-implementer.md, Tier B, S).
- **STEWARD1** has its brief (STEWARD1-implementer.md, Tier A, M). Checkpoint 1 is the
  implementer's proposal for getting the pinned OTP 28.5.0.6 and Elixir 1.20.4 into the dev
  container: they are in neither the repository nor the container today. Rule on it yourself if
  it only adds pinned, checksummed toolchains to the dev image. Take it to the owner
  (decision_request) if it changes how developers get or build the image in a bigger way.
  A SKIPped oracle is not acceptable.
- **Still unruled from earlier handoffs:** INIT2's usage check against INIT_PAGES; the
  INIT1-merge page reconciliations (devices.md, boot.md, budgets.md "Root, system and users",
  testbench.md "Starting a case's programs").

## What to watch

- **At K18's merge:**
  - scheduling.md "Targets exclude the checked build's audits" says that an audit neither fills
    a window nor moves the schedule, and the seed-3 table is re-measured;
  - the audit-billed residual and todo/audits-billed.md are gone, and the shares-gross residual,
    its todo page and SUMMARY line are in;
  - testbench.md "Checked builds" lists `audit-billed` beside `audit-unstamped`;
  - the old figures double-counted (the red team confirmed it), so no page should still quote
    them.
- **At GATE1's merge:**
  - the notice is met net on both widths with two leases live;
  - the sweep's worst is recorded with the target it sets, and the per-width seed sentence is on
    scheduling.md;
  - kernel/README.md#containment goes to built;
  - the "two live at any time" wording matches the fixture;
  - then INIT1 merges, and its page reconciliations are due (see "Still unruled").
- **At STEWARD1:** the Elixir reference is written from the page, not translated from the Rust.
  wire.md's "partly tested" line and residual go.
- **Fragile pages:**
  - steward.md is long, and only its fixed `##` headings are allowed (new sections are `###` or
    `####` under Interface);
  - scheduling.md's Responsiveness section keeps growing measurement prose.
