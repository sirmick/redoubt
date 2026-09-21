---
name: architect-qa
description: The formal question-and-answer protocol for Redoubt's frozen design. Use when recording a new design question in docs/QUESTIONS.md, recording an answer in docs/ANSWERS.md, or applying an accepted answer to the design notes.
---

# Redoubt design Q&A protocol

`docs/QUESTIONS.md` holds open questions for the owner, each touching the frozen design
(v4). `docs/ANSWERS.md` holds the owner's answers, in dated tranches. Together they are the
audit trail: if a design note changed, a question and an answer explain why. This protocol
keeps that trail intact.

Numbering is global and monotonic. As of this writing **1-126 are answered**; later
questions (from WP-K2, WP-S1, WP-K3, WP-D1, WP-R4, userland) are open. Always re-read the
top of `QUESTIONS.md` before choosing a number; never trust a number repeated here.

## Rule 0: never fabricate a decision

The architect is not the owner. Deducing an answer from the design is allowed and expected.
Inventing one where the design does not decide is not. When the decision is the owner's,
write the question `Rec`/`Alt` and leave the answer to the owner.

## Step 1 — find the question's origin

Questions are grouped by where they came from. Use the existing section, adding a new one
only for a new source:

- `## Kernel: messages and IPC (KERNEL-SPEC.md)`
- `## Kernel: objects, arguments, errors`
- `## Containment (CONTAINMENT.md, CAPABILITIES.md)`
- `## Wire formats (WIRE.md)`
- `## Tooling and tenets`
- `## Added later`
- `## From <who>'s red team` / `## From the <role> (WP-XX)`
- `## From applying answers A-B (the design editor)`
- `## Design review round N (...)`
- `## From <server> (WP-XX)`

A review finding becomes a question only when it is a real hole: a concrete attack against
today's wording, an undefined case, or a contradiction between two notes. "This could be
clearer" is an editor finding, not a question.

## Step 2 — write the question

Use the next unused number, bold, with a one-line title, then the problem, then `*Rec:*` and
(usually) `*Alt:*`. The `Rec` is a decision, not a survey. Name the exact place the hole is.

```markdown
147. **A device still pointed at freed DMA frames.** <one or two sentences naming the
    concrete hole in the current wording, with the note and rule it violates.>
    *Rec:* <the recommended decision, stated as what the note will say, including any new
    constant, error or field and the code path it affects.>
    *Alt:* <the rejected option and the consequence that makes it worse.>
```

Rules:

- One question per number. If two decisions are entangled, split them.
- State the consequence the answer must close (an attack, an unbounded growth, a stranded
  caller). A question without a stake is not a question.
- Cite notes, rules (R1-R12), invariants (I1-I15) and questions by number.
- Do not write the `**Answered:**` line yet.

## Step 3 — record the answer in ANSWERS.md

Answer in the current (open) tranche. If the tranche is closed, start a new one headed
`# Answers to N-M (owner, <date>)` followed by a one-line summary of the shape:

```markdown
**All Rec, except 37, 51 and 55 (changed) and 33, 54 (clarified) below.**
```

Then sections, in this order, omitting empty ones:

- `## Changed` — the owner overrode or amended the recommendation. Explain the change, what
  replaces it, and any stated residual. This is the part people read; be exact.
- `## Clarified` — accepted, with a wording correction that matters.
- `## Accepted as recommended` — a plain list of numbers, plus short notes where a
  recommendation deserves emphasis.

An individual answer must state:

- the decision, precisely enough that the note can be edited from it (new constants, fields,
  errors, order of checks);
- which note(s) and section(s) change;
- any residual or trade-off left open, named as such.

Prefer bullets to prose. Match the existing entries' voice: direct, no hedging, no
re-arguing the whole design.

## Step 4 — backlink the question

Append to the question, indented under it, exactly the `Answered` form:

```markdown
    **Answered:** KERNEL-SPEC.md, R10 (the taken call's caller gets `Dead` at once); INIT.md,
    Restarts and reboots.
```

This line names where the decision now lives. A question with a `Rec` but no `Answered`
line is still open. `**Partly applied (<date>):**` exists for a partial landing; do not use
it to mean "decided".

Then update the summary paragraph at the top of `QUESTIONS.md`: the answered ranges and
which numbers remain open, and any earlier answer this tranche revised (the real project
revised 56 and replaced 57/58; say so).

## Step 5 — apply the answer to the design note

The answer is not real until the owning note says it. The note is the single owner; do not
leave the decision only in `ANSWERS.md`.

- Edit the named note(s) in `docs/`, in the exact sections the answer names. Keep the notes'
  style: tables for constants/costs, rule paragraphs for rules, and citations between notes.
- If the answer adds or changes an error, a constant, or the order of checks, update
  `KERNEL-SPEC.md`'s error table and constants, since it owns the ABI, even when the change
  originated in another note.
- If the answer changes what an implementer must build, name the follow-up package in the
  answer (the project files these as WP-A2, WP-R1b, WP-M1, WP-W2, WP-V1, WP-A3 rather than
  silently editing a merged package).

## Step 6 — HISTORY.md

Every design change gets a `docs/HISTORY.md` entry, as `QUESTIONS.md` states. Match the
existing voice: a short paragraph saying what changed and why, naming the answers, the
notes, and any consequence for packages. Read the surrounding entries first; do not invent
a new heading style.

## Authority and precedence

1. `docs/TENETS.md` outranks every other note. An answer that violates a tenet is not an
   answer; amend the tenet in `TENETS.md` first, with the reason, or reject the question.
2. Each note owns one topic (`docs/README.md` has the map). Put the answer in the owner,
   link from the others.
3. Source wins over docs for runtime behaviour. A docs-vs-code conflict is a finding.
4. The kernel ABI is owned by `KERNEL-SPEC.md` alone. The model conforms to the spec, never
   the reverse.
5. `ANSWERS.md` tranches are append-only. Never edit an earlier tranche; a revision is a
   new tranche that says what it revised.

## Anti-patterns

- Opening a duplicate of an already-answered question. Search `QUESTIONS.md` first.
- Marking something answered because a `Rec` exists.
- Rewriting `ANSWERS.md` history, or renumbering.
- Editing a frozen note without the `HISTORY.md` entry.
- Touching code or tests. You answer, record, and apply to notes; the implementer builds.
- A `Rec` that hides a trade-off without stating the residual.
- Answering a design question by reading only the one note that prompted it. Read the
  owning notes and the prior answers too.