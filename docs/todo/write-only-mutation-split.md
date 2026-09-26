# One mutation breaks the write-without-read rule in two calls

## What

The model's `R11AllowsWriteOnly` mutation turns off the refusal of writable-but-unreadable pages in
two calls at once: `set_flags` and `process_map` both pass it to the same flag check. The model's
sequences catch the mutation through `set_flags`; no sequence shows the `process_map` half caught
on its own, so a model that broke the rule only in `process_map` could pass. On the kernel side,
`write-only-attack` covers both calls.

## Why it matters

A mutation shows that the model's checks catch the rule it breaks
([the model](../kernel/model.md#residual-risks)). When one mutation breaks a rule in two places and
is caught through one, the other place is untested in the model, and the rule's status there rests
on the bench alone ([R11 (memory)](../kernel/memory.md#r11-memory)).

Belongs to no follow-up package: test-only (the model).

## Where

- [`model/src/mutation.rs`](../../model/src/mutation.rs): `R11AllowsWriteOnly`.
- [`model/src/kernel.rs`](../../model/src/kernel.rs): `check_flags`, used by `set_flags` and
  `process_map`.
- The page: [processes](../kernel/processes.md#residual-risks).

## Done when

- Either the mutation is split in two, one per call, each caught by the model; or a `process_map`
  sequence catches the existing mutation with `set_flags` left correct.
