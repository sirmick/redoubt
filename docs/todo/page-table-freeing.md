# Empty page tables are kept until the process ends

## What

A page table is freed only when its process ends. `unmap` gives back the pages but keeps any
table that now maps nothing, and `map_anon`'s rollback after a failure keeps the tables it
allocated. The tables that mapped a device range or a DMA run stay too. All of them stay charged
to the process's own budget.

## Why it matters

A process can strand only its own pages, so this is not a way to take another budget's memory
(R6 (charging)). But the process's usage stays above what it has mapped, a long-running server
that maps and unmaps can run itself out of memory, and R11 (memory)'s accounting of what a
mapping costs is true only until the first `unmap`.

## Where

- [`kernel/src/mem.rs`](../../kernel/src/mem.rs): `unmap`, and the rollback in `map_anon`.
- [`kernel/src/arch/riscv/mem.rs`](../../kernel/src/arch/riscv/mem.rs) and
  [`libs/paging/src/lib.rs`](../../libs/paging/src/lib.rs): the table walk.
- The pages: [memory](../kernel/memory.md#residual-risks) and
  [devices](../kernel/devices.md#residual-risks).

## Done when

A table that maps nothing after `unmap` or a rollback is freed and uncharged, with a bench case
that maps and unmaps a range repeatedly and shows the process's usage return to where it
started, or the memory page states when tables come back as the rule.
