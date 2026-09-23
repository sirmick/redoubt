# FPGA platform

Planned hardware, not built. The hardware plan can still change, in particular to avoid side
channels; this note records what Redoubt needs from it.

## The cards
Two XC7K480T PCIe cards (Baidu/Inspur; about 300K LUT6, 34 Mb BRAM, 4 GB DDR3 each, PCIe Gen2 x8),
in a host machine that builds and serves them.
- **Card A, the CPU:** RV64GC with Sv39 on a standard platform (CLINT, PLIC, 16550), so RustSBI
  and Linux run unmodified. 8 cores x 4 hardware threads (in-order, dual-issue,
  barrel-scheduled), 32 KB L1s, 2 MB shared L2. DDR3 channel A is main memory; **channel B is for
  DMA**. Hardware channels (send, receive, park on empty) as MMIO devices, later possibly as custom
  instructions. A retirement trace and performance counters for the test harness.
- **Card B, the GPU:** Vortex (RISC-V SIMT) with an INT8 systolic unit; 4 GB DDR3 as VRAM. Launches
  are descriptors in a VRAM command ring plus a doorbell over PCIe peer-to-peer; completions are
  written into a card A hardware channel.

## Trust assumptions
The FPGA is "the secure configuration" only if these hold:
- **The host cannot change the card.** The bitstream loads from on-card flash with host write access
  disabled. A host that can reload the bitstream owns the card, whatever the DMA windows say;
  otherwise the host is in the TCB (like Linux in the partition mode).
- **DMA window registers are reachable only from the kernel**, never through a PCIe BAR.
- **The shared L2 is partitioned** between cores by the RTL; until it is, cross-core cache timing is
  a stated residual channel (CONTAINMENT.md).

## What Redoubt needs
- **DMA confinement in RTL.** Bus masters (devices, PCIe inbound, the GPU) can reach **only channel
  B**, never main memory. Per-master base/bound windows inside channel B keep devices out of each
  other's buffers: the essential half of an IOMMU by construction (IO-ARCHITECTURE.md, DMA).
- **Virtio from the host.** The host serves virtio devices over PCIe, as QEMU does in development. As
  a DMA master into the card it is covered by the channel-B rule, and end-to-end TLS/SSH keep network
  data opaque to it.
- **One budget per core.** The 4 hardware threads of a core share an L1, the classic cross-thread
  side channel. The scheduler runs all threads of a core in one budget or idles them, so the RTL
  isolates only core from core and flushes when a core switches budgets. The kernel tells the
  hardware when it switches.
- **Non-observability for a protected label set.** Covert communication is out of scope (TENETS.md,
  Purpose and threat model), but the RTL can still shrink the enumerated on-die channels between a protected label
  set and a lower one: separate cores, a partitioned L2, isolated memory bandwidth, per-domain DMA
  windows (channel B already helps), per-domain disk and NIC queues, and no shared GPU context. Good
  practice, not a design claim; only placement — separate power/thermal domains or machines — is zero.
- **`keyd` on its own core** once there is SMP.
- **Hardware channels are devices.** Each channel endpoint is reached through a handle, granted like
  any device; they also serve as doorbells between harts. Custom instructions are a vendor extension
  (tenet 4): only behind a capability feature, with the MMIO path kept.
- **GPU isolation.** Each launch sees only its budget's VRAM window (base/bound registers set by the
  GPU driver per launch), or the GPU serves one budget at a time with VRAM scrubbed between them. GPU
  contexts are handles owned by budgets.
- **Local inference as a label sink.** A model running on card B keeps data on the machine, so it can
  be cleared for labels an external API never is (CONTAINMENT.md).
- **Root of trust.** A boot ROM in the bitstream that verifies the loader closes the "loader itself
  is unverified" gap (VERIFIED-BOOT.md).
- **Approval button (option).** A physical approval button or small display on the card would be a
  trusted path even against a compromised client machine (CAPABILITIES.md).

## Open
- Which devices sit on card A itself versus served by the host.
- Per-master window granularity and how the kernel programs it.
- Whether Vortex needs VRAM windows added, or already isolates contexts.
- Scale: 32 hardware threads makes SMP the first kernel work after milestone 1 (PLAN.md).
