# IOMMU

## Idea

Confine each device's DMA with a RISC-V IOMMU (QEMU has one, and open RTL exists) or an IOPMP,
which gives each bus master a set of windows. A driver that programs DMA would then no longer be
trusted: a device could reach only the pages its driver was given.

## Why it is not a goal

On QEMU the host is trusted anyway, and on the FPGA platform the DMA-only memory channel gives the
essential half of an IOMMU by construction ([the FPGA platform](fpga-platform.md)). The kernel today
resets a device before its DMA pages are reused, which closes a dead driver's hole but does not
confine a live driver ([devices](../kernel/devices.md#residual-risks)).

## What it would need

- A backend for one of the two, programmed only by the kernel, with a device object's DMA runs
  mapped into that device's window and nothing else.
- The IOPMP specification ratified, if that is the one chosen.
- Reset-before-reuse kept: a window closes only once the device confirms it has stopped.

**Attack cases:** a device told to write outside its driver's runs is stopped by the hardware; a
live driver's device cannot reach another driver's buffers.
