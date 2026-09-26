# An interrupt raised before the first `receive` can be lost

## What

In one diagnostic run on QEMU's `virt` machine, a goldfish RTC alarm raised while its IRQ object
was masked, before the driver's first `receive` after the handle was handed over, was not
delivered to that `receive`. Every later masked raise and every fresh edge in the same run was
delivered. No kernel defect was found by reading. The candidates are QEMU's PLIC pending
semantics for a disabled level line, and stale state left by other probing of the IRQ handle.

## Why it matters

R5 (interrupts) promises that a fired source stays pending while masked and is delivered by the
next `receive`. A lost interrupt stalls a driver that waits for it. Drivers drain their rings
after every `receive`, which hides a lost interrupt on a busy device but not on an idle one.

## Where

- [`kernel/src/device.rs`](../../kernel/src/device.rs): `irq_fired`, and the unmask on
  `receive`.
- [`kernel/src/arch/riscv/intc_plic.rs`](../../kernel/src/arch/riscv/intc_plic.rs): claim,
  complete, enable.
- The page: [devices](../kernel/devices.md#residual-risks).

## Done when

The first-`receive` loss is reproduced by a bench case (a source raised while masked before the
first `receive`, repeated from a fresh boot), and the cause is found and fixed, or shown to be
QEMU's and stated as a residual of QEMU's PLIC with the case pinning the kernel's side.
