# The unsafe budget's stated reason

## What

The unsafe budget's rule is that a budget only falls, and raising one needs a stated reason in the
change that does it ([the unsafe budget](../testbench.md#the-unsafe-budget)). The case enforces the
ceilings but not the rule: it never reads the history, so a commit that raises `max_unsafe` with
no reason passes, and only review catches it. The size budget does read its history
([the size budget](../testbench.md#the-size-budget)).

## Why it matters

The unsafe count is the trusted computing base's sharpest number. A raise that nobody explained
is the drift the ratchet exists to stop.

## Where

- [`tools/testbench/src/budget.rs`](../../tools/testbench/src/budget.rs), and the history check
  in [`tools/testbench/src/size.rs`](../../tools/testbench/src/size.rs) it would share.
- [`tests/unsafe-budget.toml`](../../tests/unsafe-budget.toml).

## Done when

Review still checks the reason and no one has asked for more; or the unsafe budget checks each
commit that raised a budget for an `Unsafe budget: <name>: <reason>` line, the way the size budget
does, with a host test.
