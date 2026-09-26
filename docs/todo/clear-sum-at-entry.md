# The kernel does not clear `SUM` and `MXR` itself

## What

The kernel never writes `sstatus.SUM` or `sstatus.MXR`. It relies on the firmware entering
S-mode with both clear. With `SUM` set, a kernel bug that dereferenced a user address would read
or write the process's memory instead of faulting. With `MXR` set, kernel loads could read pages
that are execute-only.

The rule for the memory-layout page: "The kernel clears `sstatus.SUM` and `sstatus.MXR` at entry
and never sets either: S-mode cannot load or store through a user mapping, and a stray kernel
dereference of a user address faults."

## Why it matters

It is hardening: the kernel reaches user memory only through the physmap, after checking the
page, so no known path needs the fault. But the property rests on firmware the kernel does not
control, and a stray dereference would turn a kernel bug into a silent read of user memory.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/arch/riscv/mod.rs`](../../kernel/src/arch/riscv/mod.rs): the kernel's start,
  where `sstatus` is first set up.
- The page: [memory layout](../kernel/memory-layout.md#residual-risks).

## Done when

- The kernel clears both bits at entry (one `csrc`).
- A boot assertion checks both bits are clear.
- A kernel test shows that a kernel load through a user address faults.
