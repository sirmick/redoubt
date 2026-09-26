# RAM beyond the physmap is not refused at boot

## What

The loader maps the kernel's direct physical map to the end of RAM, but the kernel's window
stops at `PHYSMAP_SIZE` (128 GiB on Sv39, 2032 MiB on Sv32), and the loader never compares the
two. The kernel hands out frames lowest first. So on a machine with more RAM than the physmap
covers, the kernel boots, and stops the first time it uses a frame past the bound, which a
process can cause by allocating.

The rule for the boot page: "The loader refuses to boot when RAM extends past `PHYSMAP_SIZE`,
with a clear message."

## Why it matters

The machine fails closed, but late and at a time a process chooses, as a kernel stop during a
call rather than a refusal at boot. That breaks I14 (no call panics the kernel) on such a
machine. The loader is in the TCB, so the check belongs there.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`loader/src/paging.rs`](../../loader/src/paging.rs): the physmap mapping, sized from the end
  of RAM.
- [`kernel/src/arch/riscv/physmap.rs`](../../kernel/src/arch/riscv/physmap.rs) and
  [`libs/layout/src/lib.rs`](../../libs/layout/src/lib.rs): `PHYSMAP_SIZE`.
- The pages: [memory layout](../kernel/memory-layout.md#residual-risks) and
  [boot](../kernel/boot.md).

## Done when

- The loader refuses to boot, with a message, when RAM ends past the physmap's end.
- A host test of the loader's check passes with RAM at the bound and refuses one page over.
- The boot page states the refusal under R17 (fail closed).
