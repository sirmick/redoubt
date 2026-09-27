# User cache-block invalidation

## What

The firmware turns on cache-block operations for the modes below it:
`configure_hart_environment` sets `menvcfg.CBIE` to invalidate, and `CBCFE` and `CBZE` with it.
Whether user mode may use them is then up to `senvcfg`, which the kernel never writes, and its
reset value is not fixed by the privileged specification. On a hart with a write-back data
cache where `senvcfg.CBIE` is not `00`, a process can run `cbo.inval` on its own pages.

`cbo.inval` discards a cache line without writing it back. The kernel zeroes a frame through the
physmap before mapping it ([R11 (memory)](../kernel/memory.md#r11-memory)); until those zeroes
are evicted, they live only in the cache. A process that invalidates the lines of a page it was
just given reads the frame's DRAM contents, which are the previous owner's data.

## Why it matters

It breaks R11's zeroing on any cached hardware, and with it every separation that rests on a
frame being clean when it changes owner. QEMU models no cache, so `cbo.inval` there cannot bring
old data back and no bench case can show the hole. It matters from the first softcore
([the FPGA platform](../beyond/fpga-platform.md)).

## Where

- [`bios/firmware/prototyper/src/sbi/features.rs`](../../bios/firmware/prototyper/src/sbi/features.rs):
  `configure_hart_environment`, which sets `menvcfg::CBIE_INVALIDATE`.
- The kernel's hart setup under [`kernel/src/arch/riscv/`](../../kernel/src/arch/riscv/): nothing
  writes `senvcfg`.

## Done when

- The kernel writes `senvcfg` at boot on every hart, with `CBIE` at `00` (user `cbo.inval`
  traps) and `CBCFE` off, failing closed whatever the reset value; `CBZE` may stay on, since
  `cbo.zero` only writes zeroes the process could store itself.
- The firmware sets `CBIE` to flush (`01`), not invalidate, so that a supervisor `cbo.inval` cannot
  discard data either.
- A bench case runs `cbo.inval`, `cbo.clean` and `cbo.flush` from user mode and expects an
  illegal-instruction fault; a planted mutation that leaves `senvcfg` unwritten fails it on a QEMU
  CPU whose `senvcfg` resets nonzero, or through a boot argument that plants the old value.
- The memory page's residual risks drop the item.
