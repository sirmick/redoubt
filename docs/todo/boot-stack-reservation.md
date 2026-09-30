# The boot stack reservation

## What

A process the loader starts should have 32 stack pages reserved below `0x8000_0000`, the top one
backed ([memory layout](../kernel/memory-layout.md#regions)). It has 33. The loader reserves the
32 (`USER_STACK_PAGES`). Then the kernel's `setup_loader_process` reserves the stack a second
time, `DEFAULT_STACK_SIZE` (128 KiB) down from the thread's `sp`, rounded down to a page. Because
`sp` sits 16 bytes below the top, that range starts one page lower, and the kernel adds a
reservation at `0x7FFD_F000`. The other 32 pages were reserved already. On both widths
`map_fixed(0x7FFD_F000)` is refused as an overlap from a boot process's first instruction, while
`0x7FFD_E000` is free.

## Why it matters

It is not a hole: the extra page is the process's own reservation, charged only when touched.
But two places reserve one stack, the page's number is wrong, and a case that relies on the
page below the stack being free fails (`map-fixed-attack` avoids it on rv32 for that reason).

## Where

- [`kernel/src/arch/riscv/process.rs`](../../kernel/src/arch/riscv/process.rs): line 248,
  `setup_loader_process`, and `DEFAULT_STACK_SIZE`.
- [`loader/src/main.rs`](../../loader/src/main.rs): the stack's mapping and reservations
  (`USER_STACK_TOP`, `USER_STACK_PAGES`).

## Done when

- One place reserves a boot process's stack, and the reservation is the page's 32 pages.
- A bench case shows it on both widths: `map_fixed` of the page just below the 32 succeeds
  in a boot process, and of the lowest of the 32 is refused as an overlap.
- The memory layout page's residual risks drop the item.
