# Architect notes

The architect's warm-start index, not the design record. `QUESTIONS.md` and `ANSWERS.md` are the
record; the design notes are the truth. This file exists so a *new* architect session starts warm
instead of re-deriving what earlier sessions learned.

**Link, do not restate.** A fact that lives in a note or an answer gets one line here and a pointer,
never a second copy: a copy goes stale and becomes a second source of truth. Terse, append-only.

## Decisions that bind (pointer only)

- The isolation unit is the label set (answer 150; TENETS.md, The use case; CONTAINMENT.md, Labels).
- Covert communication is out of scope (answer 151; TENETS.md, The adversary — the canonical statement).
- `confined` is a per-boot manifest flag; `init` refuses the boot on a crossing (answer 152; INIT.md, The boot manifest).
- The steward push is the mirror of declassification (answer 153; CONTAINMENT.md, Push).

## Traps

- `RESERVED_TYPES` is correct, not over-broad (answer 155).
- `redoubt-rt`'s records are already backed; WP-W3b was dropped (answer 154).
- A docs table edit can break generation with no code change; run `cargo test -p redoubt-wire-gen`.
- Bound every long-lived role: an unbounded reader returns nothing (orchestrator.md, architect.md).