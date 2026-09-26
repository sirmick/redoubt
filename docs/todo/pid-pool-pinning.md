# Untaken exit notices pin PIDs outside every limit

## What

PIDs are one global pool of 63. A process counts against the process limit of the budget it
runs in while it lives. When it ends, it stops counting at once, but its PID stays held until
its exit notice is taken. Nothing else bounds those PIDs except the creator's pages. So a
creator with a single one-process budget can start and end processes, never take their notices,
and hold every free PID; every other `process_create`, anywhere in the budget tree, then gets
`OutOfProcesses`.

This breaks R7 (carving): allocation fails only on the caller's own budget. The rule for the
process object, and for R6 (charging): "A process object counts one against its creator's
budget's process limit, from `process_create` until the object is freed, as its page is charged
there. The process also counts against the budget it runs in while it lives." Because limits
are carved from `root`'s, the live PIDs then never exceed `root`'s limit, so no budget can take
another's PIDs. Destroying the creator's budget frees the objects (R10 (destruction)), so
nothing leaks.

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

- A process object counts against its creator's budget's process limit until it is freed.
- An attack case, `pid-pinning-attack`: Alice's budget starts processes and never takes their
  notices; Bob's `process_create` still succeeds, and Alice gets `OutOfProcesses` at her own
  limit.
- A planted mutation that stops counting the object fails that case.
- The process-object counting rule is on the processes and budgets pages.
