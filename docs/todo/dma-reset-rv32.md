# DMA reset and quarantine are attacked only on rv64

## What

`bench:dma-reset-reuse` and `bench:dma-reset-quarantine` run only on rv64. On rv32 the test
build that makes a device's first reset fail (`dma-reset-deaf`) is compiled, never run. The
reuse case also reads the device's status only after `dma_alloc` has handed the frame to a new
holder, so it shows the reset came before the new holder's use, not that it came before the
frame went back into the pool; that stricter order rests on the kernel's own assertion and the
model's I16 (DMA pages reset before reuse) check.

## Why it matters

I16 is what stops one DMA driver's device from writing into pages another process has been
given. Every milestone requires rv64 boots and rv32 compilation, but a security claim made for
both widths should be attacked on both.

## Where

- [`tests/dma-reset-reuse.toml`](../../tests/dma-reset-reuse.toml) and
  [`tests/dma-reset-quarantine.toml`](../../tests/dma-reset-quarantine.toml): `arch = ["rv64"]`.
- [`kernel/src/dma.rs`](../../kernel/src/dma.rs): reset, release and quarantine.
- The page: [devices](../kernel/devices.md#residual-risks).

## Done when

Both cases run on rv32 as well, or the page states why rv32 cannot run them. Optionally, a
checked build records the pool operation's order so a case can show the reset preceded it.
