# Untaken exit notices pin PIDs outside every limit

## What

PIDs are one global pool of 63. A process counts against the process limit of the budget it
runs in while it lives. When it ends, it stops counting at once, but its PID stays held until
its exit notice is taken. Nothing else bounds those PIDs except the creator's pages. So a
creator with a single one-process budget can start and end processes, never take their notices,
and hold every free PID; every other `process_create`, anywhere in the budget tree, then gets
`OutOfProcesses`.

This breaks R7 (carving): allocation fails only on the caller's own budget. The rule, on
[processes](../kernel/processes.md#creating-and-starting) and in R6 (charging): a process limit
counts PIDs, and every held PID counts once. A created process counts against its creator's
budget's process limit from `process_create` until its object is freed, exactly as long as it
holds its PID, and not again in the budget it runs in; a program the loader started, which has
no object, counts against the budget it runs in while it lives. Because limits are carved from
`root`'s, a caller under its own limit always finds a free PID, and no budget can take
another's. Destroying the creator's budget frees the objects (R10 (destruction)), so nothing
leaks. Counting a created process in both budgets would halve the machine's processes for no
gain: the budget it runs in already bounds it by its pages.

## Why it matters

One principal can stop every other principal from starting a process. That is a denial across
principals, which separation must stop.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `process_create` (the free-PID draw
  before the limit check) and the notice path that frees the object.
- [`kernel/src/budget.rs`](../../kernel/src/budget.rs): the per-budget process counts.
- The pages: [processes](../kernel/processes.md#residual-risks) and
  [budgets](../kernel/budgets.md#r6-charging).

## Done when

- A created process counts once, against its creator's budget's process limit, from
  `process_create` until its object is freed; it no longer counts in the budget it runs in. A
  program the loader started counts against the budget it runs in while it lives.
- `process_create` checks the caller's process limit, and the PID draw follows it.
- An attack case, `pid-pinning-attack`: Alice's budget starts processes and never takes their
  notices; Bob's `process_create` still succeeds, and Alice gets `OutOfProcesses` at her own
  limit.
- A planted mutation that stops counting the object fails that case.
- The pages move with it: the departures on the processes and budgets pages go, and the
  [ABI reference](../kernel/abi.md#errors-and-the-order-of-checks)'s `process_create` row and
  error table, and its model-disagreement residual for that row, say what the kernel now does.
