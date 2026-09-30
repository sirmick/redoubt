# `process_map` backs its source before it can refuse

## What

`process_map(process, src, dst, len, flags)` checks the ranges and the flags, then backs the
whole source (`ensure_range_exists`) before its later checks: the source pages themselves, the
destination's overlap and what the child's budget can pay. When the source covers a reservation
that was never touched, such as the unbacked pages of a stack, those pages are backed with zeroed
frames and charged to the caller. The call can then still be refused, with the destination taken
(`InvalidArgument`) or the budget short (`OutOfMemory`), and the pages it backed stay backed and
charged. So a refused `process_map` changes something, while [memory](../kernel/memory.md#failure-and-restart)
says a refused one charges nothing.

## Why it matters

A refused call is meant to be a no-op, so a caller can retry it and a budget stays what the
caller expects. Here a refusal can move a caller's usage by up to `len` pages. It harms only the
caller, which named its own reservation, but it breaks the rule the page states, and the
`map-fixed-attack` checks of `process_map`'s refusals do not see it, because they use backed or
unmapped sources.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `process_map`, the
  `ensure_range_exists` call (about line 436) ahead of the per-page source checks and the
  destination and charge checks.
- [`kernel/src/mem.rs`](../../kernel/src/mem.rs): `ensure_range_exists` (about line 567).

## Done when

- Every check that can refuse `process_map` runs before the source is backed, or the backing is
  undone when the call is refused.
- A bench case, on both widths, asks for a `process_map` whose source is an untouched reservation
  and whose destination is taken (and one the child's budget cannot pay for), and shows the
  caller's usage unchanged after each refusal.
- The memory page's residual risks drop the item.
