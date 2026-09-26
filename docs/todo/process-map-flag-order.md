# `process_map` backs its source before it checks the flags

## What

`process_map` makes the caller's untouched source pages real, at the caller's cost, before it
refuses bad flags. A `process_map` with bad flags therefore leaves charges behind even though it
fails. The model's proof that a write-without-read mapping is refused goes through `set_flags`,
not through `process_map`, so no mutation shows `process_map`'s own refusal is needed.

## Why it matters

The charges fall only on the caller, so this is not a way to take another budget's memory. But
it breaks "a refused call changes nothing" for this call, and R11 (memory)'s flag refusal on
this path is shown by reading, not by a mutation.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `process_map` (the backing of the
  source, then `check_map_flags`).
- [`model/src/mutation.rs`](../../model/src/mutation.rs): `R11AllowsWriteOnly`.
- The pages: [processes](../kernel/processes.md#residual-risks) and
  [memory](../kernel/memory.md).

## Done when

`process_map` checks the flags before it backs anything, a case shows a refused `process_map`
leaves the caller's usage unchanged, and a mutation that drops `process_map`'s own flag check is
caught.
