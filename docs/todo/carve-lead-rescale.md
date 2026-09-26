# A lead accrued while carved down is never rescaled

## What

A budget that runs while most of its weight is carved away to children accrues its pass lead at
the small weight it has left. When the children end and the weight returns, the lead is not
rescaled, so the budget is charged as if it still ran at the small weight. This only ever
over-charges the carving budget, so it is not a way to gain time (R12 (scheduling)).

## Why it matters

An honest shell or steward that carves heavily while it runs can be held off the CPU well past
its restored share. The steward carves for every lease, so this can show as lease-handling
latency.

## Where

- [`kernel/src/sched.rs`](../../kernel/src/sched.rs): where a carve and its return change a
  budget's weight.
- [`libs/stride/src/lib.rs`](../../libs/stride/src/lib.rs): `rescale`, which the rescale would
  use.
- The page: [scheduling](../kernel/scheduling.md#residual-risks).

## Done when

Real carve patterns (the steward's and the shell's) are measured, and either the lead is
rescaled when weight returns (the lead times the weight it ran at, over the weight restored),
with a `host:redoubt-stride` test and a bench case that carves while running and gets its share
back, or the measurements show the over-charge does not matter and the page says why.
