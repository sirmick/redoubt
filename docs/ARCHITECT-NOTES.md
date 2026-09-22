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

- A 9P server that must wait **parks the call**; `FileServer::read` returns `Read::Done`/`Read::Wait` (answers 156-159; NAMESPACES.md, Holding a call). WP-R1c owns the join; the mechanism is in WP-R4's unmerged `5d29d136e`.
- `RESERVED_TYPES` is correct, not over-broad (answer 155).
- `redoubt-rt`'s records are already backed; WP-W3b was dropped (answer 154).
- A docs table edit can break generation with no code change; run `cargo test -p redoubt-wire-gen`.
- The console's `size` is a `call` (opcode 16), not a push: a 9P connection is not an endpoint, so a
  server cannot push down it (question 160, open). `console_size` is `Option`, default `None`
  (answer 162).
- Bound every long-lived role: an unbounded reader returns nothing (orchestrator.md, architect.md).
- A branch built before the notes reorganisation may be blocked by a **lost design section**, not stale code: `blkd`'s section, `bootfs`'s table, and a superseded sentence repeated in three notes each blocked a port. Check the design first.
- The earlier "160, open" pointer is superseded by the owner's answer 160 (ANSWERS.md): parking is the push mechanism; only typed parking 163 remains open.
- ASTRA design triage opened 164-168, all awaiting the owner (QUESTIONS.md); no review recommendation is an accepted design change.
- ASTRA D4 preserves answers 49/70/81/107/116: 167 asks for caller disposition and 168 for server delivery; C1/A1 are the corresponding implementation symptoms, not duplicate questions.
- ASTRA D2 must preserve answer 150; D1 must preserve 152/153 unless explicitly revised; D3's one-slice promise is in answer 103 itself, so withdrawing it requires an owner decision.
- Package state already belongs to SWARM.md, Claims; ASTRA S3 calls for reconciliation with that table, not a competing source of truth.
- Owner accepted both 167/168 on 2026-09-22: KERNEL-SPEC.md, IPC completion/return registers; CAPABILITIES.md, IPC; CONTAINMENT.md, shared server transactions; implementation is WP-IPC1. The earlier all-open pointer is superseded only for these two.
- An IPC output record may lie inside its lend: return the lend before committing output; early-error disposition retains unconsumed input (answer 167, KERNEL-SPEC.md).
- Documentation reconciliation restores `ninep_common` code 2 from answer 114; generator inputs, not generated Rust, own the table (NAMESPACES.md).
- BOOT.md records current `Devs`/`Ctrl` and `map_device -> addr, len`; this does not answer still-open 143/146 (KERNEL-SPEC.md, Current conformance).
- Console resize remains milestone 1, including UART parking after typed dispatch is available (160/163, NAMESPACES.md); Redoubt size queries remain fresh (162/R-T1, USERLAND-API.md).
- rv32 compile-only is required milestone coverage, not a limitation of bench tooling; future Rust `std` remains PLAN.md work outside the draft `no_std` facade (PLAN.md, OS-API.md).
