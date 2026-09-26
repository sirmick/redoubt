# The model orders some checks differently from the kernel

## What

The kernel's order of checks is the ABI; the table on the ABI page is the kernel's. The
executable model differs from it in a few places:

- `budget_create`: the kernel decodes the spec record in slot order, so a spec with a process
  count wider than 32 bits and more than `MAX_LABELS` labels is `InvalidArgument`; the model
  checks the label count first and says `TooLarge`.
- `process_create`: the kernel checks for a free PID before the budget's process limit and
  before any charge; the model checks it last, so with no free PID and too little memory the
  kernel says `OutOfProcesses` and the model `OutOfMemory`.
- `receive`: the model clears the current call only after the record check.
- The model checks only a record's first page, and does not model the 32-run limit of
  `dma_alloc` or the size of the placement area.

## Why it matters

A model that orders checks differently cannot be replayed against the kernel, and replay is what
turns the model from a reference into evidence about the kernel. None of the differences is a
way past a check: every check in a row is still made. The fix is on the model's side only.

## Where

- [`model/src`](../../model/src): the call handlers.
- [`kernel/src/redoubt.rs`](../../kernel/src/redoubt.rs), `budget.rs`, `process.rs`,
  `message.rs`: the kernel's order.
- The pages: [ABI](../kernel/abi.md#residual-risks) and [model](../kernel/model.md).

## Done when

The model follows the kernel's order in each row above, models every record page, the
`dma_alloc` run limit and the placement area's size, and host tests pin each row. Trace replay
against the kernel is the check that finds the rest.
