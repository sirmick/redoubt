# The loader stub's unsafe budget

## What

The loader stub runs on the target and uses `unsafe` (raw slices over its argument page and the
image in `stub/src/main.rs`, the mapping calls, and `NullAlloc`'s `unsafe impl GlobalAlloc` in
`stub/src/lib.rs`), but `stub/src` is in no `[[budget]]` of `tests/unsafe-budget.toml`. The
ratchet counts nothing it is not given, so the stub's `unsafe` is neither capped nor checked for
its justifications.

## Why it matters

The stub is the code every loader-launched process runs first
([launching through the loader stub](../servers/init.md#launching-through-the-loader-stub)), and
[the unsafe budget](../testbench.md#the-unsafe-budget) claims to cap `unsafe` in on-target code.
The ratchet cannot see a directory nobody configured, so this gap was found by reading, not by the
bench.

Belongs to the kernel follow-up package.

## Where

- [`stub/src/main.rs`](../../stub/src/main.rs), [`stub/src/lib.rs`](../../stub/src/lib.rs)
- [`tests/unsafe-budget.toml`](../../tests/unsafe-budget.toml)
- [`tools/testbench/src/budget.rs`](../../tools/testbench/src/budget.rs): the ratchet.

## Done when

- `stub/src` has a `[[budget]]` at its current count, with every use justified.
- The ratchet, or the no-cruft gate, fails when a crate built for the target has a source
  directory in no budget, so the next one is caught by the bench.
