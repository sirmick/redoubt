# The boot hart's interrupt context

## What

The firmware enters the loader on one boot hart and passes its hart ID in `a0`. The loader
prints that ID but does not use it: `read_plic` takes the PLIC's S-mode context of the CPU node
whose `reg` is 0 and writes it into the `Plic` tag. On firmware whose boot hart is not hart 0
(OpenSBI's boot-hart lottery under more than one hart, for example) this goes wrong in one of
two ways:
- **(a) Hart 0 has an S-mode context.** The kernel enables interrupts in hart 0's context, so
  the external interrupt is raised on hart 0, which is parked in the firmware. The running hart
  never takes a device interrupt, and every driver stalls. Claims are per source and
  R5 (interrupts) routes by source, so no interrupt reaches the wrong owner.
- **(b) Hart 0 has no S-mode context** (boards of the SiFive U54 family, where hart 0 is an
  M-mode-only monitor core). `read_plic` finds nothing, and the boot goes on with no `Plic` tag and no external
  interrupts, just as silently.

The rule, owned by the `Plic` row of the boot page's tag table under R17 (fail closed): the tag
carries "the PLIC's S-mode context of the boot hart (the hart ID the firmware passes in `a0`),
matched to the cpu node whose `reg` is that ID; a device tree with a PLIC but no S-mode context
for the boot hart stops the boot (R17)."

## Why it matters

It is a silent liveness failure, not a containment breach: the machine boots, looks clean, and
no driver ever hears its device. R17 asks that a boot the loader cannot describe truthfully
stop instead, with a message.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`loader/src/dt.rs`](../../loader/src/dt.rs): `read_plic`, which picks the CPU whose `reg` is
  0.
- [`loader/src/main.rs`](../../loader/src/main.rs): `rust_entry`, which receives the boot hart's
  ID.
- The page: [boot](../kernel/boot.md#the-argument-block) and its residual risks.

## Done when

- The loader passes the boot hart's ID into the device-tree read and matches the cpu node whose
  `reg` is that ID. A PLIC with no S-mode context for the boot hart stops the boot with a
  message, as a missing seed or timebase does.
- Host tests of `dt.rs` on device-tree fixtures: booting on hart 1 of 2 picks hart 1's
  context; a tree whose hart 0 has no S-mode context, booting on hart 1, picks hart 1's context; a missing S-mode context
  is refused; and a planted mutation that restores `reg == 0` fails the first two.
- The boot page drops its residual.
