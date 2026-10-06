# Redoubt hardware platform: the owner's design sketch (2026-10-05), card first

Owner's amendment (2026-10-06 ~00:30): the first goal is the XC7K480T PCIe card, not the
DE10-Nano. Everything below is the owner's sketch as given, with that amendment applied where
it changes a line (marked **card-first**).

## Goal
Run Redoubt on its own open soft-core SoC on the XC7K480T PCIe card (the real target, DMA
confined in hardware). **card-first:** the DE10-Nano scale model is no longer the first step;
it stays available as a cheap latency rig only if the card's bring-up needs one. Every gateware
piece builds with an open flow (openXC7 on the card).

## Cores
- VexiiRiscv (SpinalHDL -> Verilog), one hart per core, in-order, single-issue to start.
- RV64IMAC + B (Zba/Zbb/Zbs) + Zicbom, Sv39, 16-bit ASIDs, PMP, S/U, no FPU.
- BTB + RAS + GShare; 16-32 KB L1 I and D; perf counters; JTAG debug module.
- New RTL, in this order: Zicboz, Sstc. Dual issue only after timing closes.
- No hardware multithreading by default. At 60-80 MHz DDR3 latency is ~7 cycles, so a strict
  barrel halves per-hart throughput without filling stalls. Gate: add switch-on-miss MT only if
  counters show >30-40% of cycles stalled on memory (and then it joins the stated channels).

## SoC
- Memory map: QEMU `virt`'s (CLINT, PLIC, 16550, virtio-mmio slots, RAM at 0x8000_0000), so
  RustSBI and the kernel differ only by device tree.
- Interconnect: TileLink with coherent L1s, a coherency hub and a shared L2.
- Card: SpinalHDL TileLink SoC generator (not LiteX: `add_pcie` makes the host a bus master).
  4 cores to start, scaled by measurement. UberDDR3 on both channels: channel A main memory,
  channel B DMA-only. `pcie_7x` endpoint, Gen2 x1.
- **card-first:** the LiteX `cyclonev_hps` DE10 target and its ACP/udmabuf/UIO path are not on
  the critical path; LiteX vs the SpinalHDL generator is settled as the SpinalHDL generator.

## Hardware security properties (card)
- DMA confinement by construction: every non-core master (DMA engine, PCIe inbound) decodes only
  into channel B, inside base/bound windows only the kernel programs.
- BAR0 is a mailbox: doorbells and mailbox only; no CSR, reset, memory or window register.
- No reconfiguration path: no ICAP, flash writer or JTAG bridge reachable from the host or the
  cores; updates through the physical JTAG header only.
- Root of trust and entropy: a boot ROM verifies the loader (the same signed chain as R15, not a
  second one; the ROM key's custody and the loader key's rotation are open). Zkr `seed` readable
  by M-mode and the kernel only.
- Stated channels: shared L2, memory channels, cross-core timing.

## I/O: virtio-mmio shim + host userland backend
- Shim, per slot: a local register file the host programs once (magic, version 2, IDs, features,
  QueueNumMax, config space); the guest's hot reads never cross the link.
- Forwarded to the host as posted notify records: QueueNotify, Status, queue addresses and
  readiness, driver features.
- Interrupts: InterruptStatus/ACK in the shim; a host doorbell sets status; nonzero status drives
  the slot's level PLIC line.
- Signalling rule: only posted writes cross the link; each side spins on its own local memory.
- Host backend: one Rust process on rust-vmm (`virtio-queue`, `vm-memory`); VFIO (BAR mmap,
  MSI-X -> eventfd); spin while busy, then arm the IRQ and block; bounds-checks every descriptor
  against the window (protects the host process; the guest's protection from the host is the
  channel-B decode). Devices: console, then blk, then net.
- Contract: `VIRTIO_F_ACCESS_PLATFORM` advertised; all virtio buffers from `dma_alloc` in the
  window; EVENT_IDX on.
- Ordering: the guest does `fence w,w` before QueueNotify; the host does a release store on
  used->idx before the doorbell. (The DE10's Zicbom clean before each kick was an ACP artefact.)

## Kernel SMP
1. Ticket lock (FIFO, `pause`, Zawrs hook) replaces test-and-set: fair kernel entry, required by R12.
2. Lock to decide, unlock to do: frame zeroing, large copies and shootdown waits outside the lock,
   on frames no one else can name (the in-flight state of the free-frame bitmap; a rule beside
   R11 with an attack case before SMP3 touches it).
3. Per-hart frame magazines behind an `IrqsOff` token. One global stride run queue.
4. Next: one trap-entry lock whose guard owns a `TCellOwner`, so kernel globals become `TCell`s,
   with compile-time lock order if the lock is ever split. Shootdowns through SBI RFENCE, then
   AIA/IMSIC.

## Measurement gates
- memory-stall fraction -> hardware MT; lock hold time and contention -> finer locking; DDR
  bandwidth and coherence traffic -> core count; timing slack -> dual issue.
- The bench must be able to read the perf counters (an SBI or trace-ring path) or the gates stay prose.
- **card-first:** one core closes timing on the card under openXC7 before any step that adds cores.

## Bring-up order (card-first)
1. Card latency bitstream: PCIe endpoint, UberDDR3 channel A, the BAR0 mailbox; a fabric cycle
   counter timing doorbell and interrupt round trips over VFIO.
2. One core: RustSBI, the kernel, virtio-console through the host backend.
3. virtio-blk and virtio-net, then the full bench on the card.
4. Confined DMA (channel B windows), boot ROM, Zkr.
5. Two cores (the SMP work), then 4; measure and scale.
6. Zicboz, Sstc, AIA.

## What changes in Redoubt's pages and plan
- docs/beyond/fpga-platform.md: barrel -> gated experiment; "twelve to sixteen cores" -> "4,
  scaled by measurement"; the SoC generator settled; the shim's local reads; rust-vmm/VFIO named;
  ACCESS_PLATFORM and host-side bounds checks in the contract; barrel-specific ISA clauses
  (hart-tagged TLB, Zihintpause's barrel sentence) out; card-first order; the openXC7 gate; the
  ROM-key open item.
- docs/kernel/scheduling.md: FIFO-fair kernel entry as a rule beside R12 (next rule ID, a mutation).
- docs/plan/m2-usable-shell.md "several harts": ticket lock, out-of-lock zeroing with its invariant,
  per-hart magazines as steps; the run queue stays global. SMP1's brief absorbs the ticket lock;
  zeroing and magazines land in SMP3.
- kernel/src/cell.rs doc comment: a token owned by the big-lock guard is the intended multi-hart
  design, deferred, not rejected (code: SMP1's implementer).
