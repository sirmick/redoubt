# FPGA platform

Planned hardware, not built. The hardware plan can still change, in particular to avoid side
channels; this note records what Redoubt needs from it.

## The cards
Two XC7K480T PCIe cards (Baidu/Inspur; about 300K LUT6, 34 Mb BRAM, 4 GB DDR3 each, PCIe Gen2 x8),
in a host machine that builds and serves them.
- **Card A, the CPU:** RV64GC with Sv39 on a standard platform (CLINT, PLIC, 16550), so OpenSBI,
  RustSBI and Linux run unmodified. 8 cores x 4 hardware threads (in-order, dual-issue,
  barrel-scheduled), 32 KB L1s, 2 MB shared L2. DDR3 channel A is main memory; **channel B is for
  DMA**. Hardware channels (send, receive, park on empty) as MMIO devices, later possibly as custom
  instructions. A retirement trace and performance counters for the test harness.
- **Card B, the GPU:** Vortex (RISC-V SIMT) with an INT8 systolic unit; 4 GB DDR3 as VRAM. Launches
  are descriptors in a VRAM command ring plus a doorbell over PCIe peer-to-peer; completions are
  written into a card A hardware channel.

## What Redoubt needs
- **DMA confinement in RTL.** Bus masters (devices, PCIe inbound, the GPU) can reach **only channel
  B**, never main memory. Per-master base/bound windows inside channel B keep devices out of each
  other's buffers. This gives the essential half of an IOMMU by construction; a DMA driver can then
  corrupt only its own buffers (IO-ARCHITECTURE.md, DMA).
- **Virtio from the host.** The host serves virtio devices over PCIe, as QEMU does in development.
  The host is a DMA master into the card, so the channel-B rule covers it: it sees only DMA buffers,
  and `blockd` encryption and end-to-end TLS/SSH mean those hold ciphertext.
- **One budget per core.** The 4 hardware threads of a core share an L1, the classic cross-thread
  side channel. The scheduler runs all threads of a core in one budget, or idles them (like Linux
  core scheduling), so the RTL has to isolate only core from core, plus flush on a core's switch
  between budgets. The kernel tells the hardware when it switches domains.
- **Hardware channels are devices.** Each channel endpoint is reached through a handle, granted like
  any device. They also serve as doorbells between harts. Custom instructions are a vendor extension
  (tenet 4): only behind a capability feature, with the MMIO path kept.
- **GPU isolation.** Each launch sees only its budget's VRAM window (base/bound registers set by the
  GPU driver per launch), or the GPU serves one budget at a time with VRAM scrubbed between them.
  GPU contexts are handles owned by budgets.
- **Local inference as a label sink.** A model running on card B keeps data on the machine, so its
  gateway can be cleared for labels an external API never is (CONTAINMENT.md).
- **Coarse time for user mode** can also be enforced in hardware, backing the kernel rule
  (RESOURCES.md, Clocks).
- **Root of trust.** A boot ROM in the bitstream that verifies the loader closes the "loader itself
  is unverified" gap (VERIFIED-BOOT.md).
- **Approval button (option).** A physical approval button or small display on the card would be a
  trusted path even against a compromised client machine (CAPABILITIES.md).

## Open
- Which devices sit on card A itself versus served by the host.
- Per-master window granularity and how the kernel programs it.
- Whether Vortex needs VRAM windows added, or already isolates contexts.
- Scale: 32 hardware threads makes SMP the first work after the north star (PLAN.md).
