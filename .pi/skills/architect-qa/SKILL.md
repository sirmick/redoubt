---
name: architect-qa
description: The formal question-and-answer protocol for Redoubt's frozen design. Use when recording a new design question in docs/QUESTIONS.md, recording an answer in docs/ANSWERS.md, or applying an accepted answer to the design notes.
---

# Design decision protocol

The specifications own behavior and its essential rationale. TENETS has precedence. Runtime
behavior is established by source and tests; discrepancies are findings. Do not invent owner
approval or treat a recommendation as a decision.

1. Read the owning note and search `docs/QUESTIONS.md`, `docs/ANSWERS.md` and their linked
   decision archive. Cite settled rules; do not reopen them. Use QUESTIONS' next unused ID.
2. For a genuine unresolved decision, add `### N. Title` to QUESTIONS with the concrete gap,
   stakes, recommendation and meaningful alternative. Keep the ID stable. Recommendations
   remain open until the authorized decision-maker answers.
3. Record an accepted decision once in ANSWERS: ID, date, decision-maker, exact decision,
   short reason, owning note/section and any residual. If accepting a recommendation by
   reference, preserve enough of its proposal to make the approval unambiguous.
4. Apply it to the owning specification, including constants/errors/encoding when affected.
   Remove the resolved question from the open list only after approval provenance and the
   final rule are preserved. Update the next unused ID; never reuse or renumber IDs.
5. Propose a follow-up package when implementation must change. Approval is not implementation.
   A revision gets a new approval entry naming the decision it supersedes; old approval
   records and dated archives are immutable.

Keep QUESTIONS limited to open decisions. Do not append the same decision to HISTORY,
ARCHITECT-NOTES, STATUS and SWARM. HISTORY is for milestones; STATUS for current behavior;
SWARM for claims and outstanding review debt. Link to the owner rather than retelling reviews.

The architect edits specifications and decision records, not code/tests. Escalate only genuine
owner decisions; editorial consolidation and applying already-authorized decisions need no
new approval. User instructions govern task scope and take precedence over this workflow.
