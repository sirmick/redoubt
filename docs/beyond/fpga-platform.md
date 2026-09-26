# The FPGA platform

## Idea

Redoubt on its own hardware: two FPGA cards on PCIe in a host machine that builds and serves
them.

- **The CPU card:** RV64GC with Sv39 on a standard platform (CLINT, PLIC, a 16550 UART), so
  RustSBI runs unmodified; eight cores of four hardware threads each; one DDR3 channel as main
  memory and **a second channel only for DMA**; hardware message channels as MMIO devices; a
  retirement trace and performance counters for the bench.
- **The GPU card:** a RISC-V SIMT GPU with an integer matrix unit, for **local inference**. A
  model on the machine keeps data on the machine, so it can be a sink cleared for labels that an
  external model API never is.
- **An approval button**, or a small display, on the card: a trusted path for approvals even
  against a compromised client machine ([hardware approval](hardware-approval.md)).

This is where DMA confinement and side-channel reduction can be done in hardware, which on QEMU
they cannot ([the tenets](../TENETS.md#side-channels)).

## Why it is not a goal

Every milestone runs on QEMU's `virt` machine, which is also the bench. The hardware plan can
still change, in particular to reduce side channels, and none of M1 (separation and containment)
through M5 (persist, install, share) needs it: its gains are confinement of drivers' DMA and
fewer shared microarchitectural channels, both stated today as residual risks.

## What it would need

**Trust assumptions,** each stated by the platform:
- The host cannot change the card: the bitstream loads from on-card flash with host writes
  disabled. A host that can reload the bitstream is trusted, whatever the DMA windows say.
- DMA window registers are reachable only from the kernel, never through a PCIe BAR.
- The shared second-level cache is partitioned between cores; until it is, cross-core cache timing
  is a stated channel.

**What Redoubt needs from the hardware:**
- **DMA confinement in RTL.** Every bus master (devices, PCIe inbound, the GPU) reaches only the
  DMA channel, never main memory, with per-master base and bound windows inside it: the essential
  half of an IOMMU by construction ([devices](../kernel/devices.md#residual-risks)).
- **Virtio from the host**, over PCIe, as QEMU serves it in development; as a DMA master the host
  is under the same rule, and TLS and SSH keep network data opaque to it.
- **One budget per core.** The four threads of a core share a first-level cache, so the scheduler
  runs a core's threads in one budget or idles them, and the hardware flushes when a core switches
  budgets. This needs [SMP](smp.md).
- **Fewer on-die channels** between a protected label set and a lower one: separate cores, a
  partitioned cache, isolated memory bandwidth, per-domain DMA windows, disk and network queues,
  and no shared GPU context. This reduces the channels; only placement (separate power and thermal
  domains, or machines) removes them.
- **`keyd` on its own core.**
- **Hardware channels are devices,** each endpoint reached through a device object like any other.
  Custom instructions for them would be a vendor extension, kept behind a capability with the MMIO
  path beside it (tenet 4).
- **GPU isolation:** each launch sees only its budget's memory window, or the GPU serves one budget
  at a time and is scrubbed between them.
- **A root of trust:** a boot ROM in the bitstream that verifies the loader, closing the gap that
  the loader itself is unchecked ([boot](../kernel/boot.md#residual-risks)).

**Attack cases:** a device, the GPU or PCIe inbound writing outside its window is stopped by the
hardware; a device writing into main memory is stopped; timing measured across cores with a
partitioned cache shows the partition; a bitstream reload from the host is refused.

**Undecided:** which devices sit on the card and which the host serves; the window granularity and
how the kernel programs it; whether the GPU needs memory windows added.
