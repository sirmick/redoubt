# The boot hart's interrupt context

## What

The firmware enters the loader on one boot hart and passes its hart id in `a0`. The loader
prints that id but does not use it: it reads the PLIC's S-mode context of the CPU whose `reg` is
0 and writes that context into the `Plic` tag. The kernel then enables, claims and completes
device interrupts in that context. On firmware that boots on a hart other than hart 0, the
kernel would be working another hart's context and its drivers would get no interrupts.

## Why it matters

The kernel's own interrupt handling rests on a device-tree reading the loader gets wrong on
such a machine, with no refusal and no message: the boot looks clean and every driver stalls.
The loader is in the TCB, and a boot it cannot describe truthfully should stop, as
R17 (fail closed) asks.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`loader/src/dt.rs`](../../loader/src/dt.rs): `read_plic`, which picks the CPU whose `reg` is
  0.
- [`loader/src/main.rs`](../../loader/src/main.rs): `rust_entry`, which receives the boot hart's
  id.
- The page: [boot](../kernel/boot.md#residual-risks).

## Done when

- The loader takes the PLIC context of the hart id it was entered with, or refuses to boot, with
  a message, when it cannot find that hart's context.
- A host test of the device-tree reading picks the right context for a tree whose boot hart is
  not hart 0.
- The boot page drops its residual.
