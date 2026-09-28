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
counts PIDs, and every held PID counts once, against the budget the process runs in, for as long
as the PID is held: for a created process until its object is freed, for a program the loader
started while it lives. If that budget is destroyed while the PID is still held, the count moves
to the destroyed budget's parent after the carve comes back (R10 (destruction), step 8, as
quarantined DMA pages do). Because limits are carved from `root`'s, a `process_create` into a
budget under its limit always finds a free PID, and no budget can take another's.

Why not the creator's budget: `init` and the steward launch everyone's processes, so counting in
the creator would pool every principal's PIDs in one `system` budget, where one principal could
spend what the others need and a vault session's launches would show on its unlabelled side.
Counting in both budgets would halve the machine's processes for no gain.

## Why it matters

One principal can stop every other principal from starting a process. That is a denial across
principals, which separation must stop.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/process.rs`](../../kernel/src/process.rs): `process_create` (the free-PID draw
  before the limit check), the end of a process (which stops the count now), and the notice
  path that frees the object.
- [`kernel/src/budget.rs`](../../kernel/src/budget.rs): the per-budget process counts, and R10's
  step 8.
- The pages: [processes](../kernel/processes.md#residual-risks) and
  [budgets](../kernel/budgets.md#r6-charging).

## Done when

- A created process counts against the budget it runs in from `process_create` until its object
  is freed, not only while it lives. A program the loader started counts there while it lives.
- Destroying a budget moves each still-held PID it counted to the destroyed budget's parent,
  after the carve returns; the count drops there when the notice goes.
- `process_create` checks the budget's process limit, and the PID draw follows it.
- An attack case, `pid-pinning-attack`: Alice's budget starts processes and never takes their
  notices; Bob's `process_create` still succeeds, and Alice gets `OutOfProcesses` at her own
  limit. It also destroys a child budget of Alice's holding ended processes whose notices are
  untaken, and checks the parent's process usage holds them and Bob is still unaffected.
- A planted mutation that stops counting at the process's end fails that case, and one that drops
  the count at destruction fails it too.
- The pages move with it: the departures on the processes and budgets pages go, and the
  [ABI reference](../kernel/abi.md#errors-and-the-order-of-checks)'s `process_create` row and
  error table, and its model-disagreement residual for that row, say what the kernel now does.
