# The FPGA platform

## Idea

Redoubt on its own open soft-core system-on-chip, on a PCIe card whose console and virtio devices
are served by a userland backend on the host beside it ([the card's host backend](card-host.md)),
as QEMU serves them in development:

- **A YPCB-00338-1P1 card (Kintex-7 XC7K480T, PCIe), the target:** four cores to start, scaled
  by measurement; a host running the backend over PCIe; and **DMA confined in hardware**:
  the card's two DDR3 channels are split into main memory and a DMA-only channel that no bus
  master but the cores can leave.
- **A Terasic DE10-Nano (Cyclone V SoC)** is not a step on the way. It stays available as a
  cheap latency rig only if the card's bring-up needs one; nothing below depends on it.

The system-on-chip:
- **Cores:** VexiiRiscv, one hart per core, in order and single-issue to start; RV64IMAC with
  Zba, Zbb, Zbs and Zicbom, Sv39, 16-bit ASIDs, PMP, S and U modes, no FPU; coherent first-level
  caches, a shared second level.
- **Platform:** QEMU `virt`'s map, so RustSBI and the kernel run with nothing but a device tree
  to tell them apart: CLINT, PLIC, a 16550 UART, virtio-mmio slots, RAM at `0x8000_0000`.
- **Debug and measurement:** a debug module and performance counters the bench can read.

The card is where DMA confinement can be done in hardware, which on QEMU it cannot
([the tenets](../TENETS.md#side-channels)). The whole bitstream builds with an open toolchain
(openXC7: Yosys and nextpnr-xilinx), so the gateware can be audited and rebuilt like the
software.

## Why it is not a goal

Every milestone runs on QEMU's `virt` machine, which is also the bench, and none of
M1 (sessions over SSH, kept apart) through M6 (persist, install, share) needs hardware. The card's
gains are confinement of drivers' DMA, a verified loader, and an entropy source; all three
are stated today as residual risks.

## What it would need

### The system-on-chip

- **The core.** VexiiRiscv generated as Verilog (SpinalHDL): RV64IMAC, the B extensions and
  Zicbom, Sv39, supervisor mode, coherent first-level caches of 16 to 32 KB each, a branch
  target buffer, a return address stack and a GShare predictor, 16-bit ASIDs (Sv39's whole
  field; the width is a generation parameter), PMP, the debug module and performance counters.
  Its interrupt controller, timer and I/O region are remapped to `virt`'s addresses. Every
  process ID the kernel can make fits the ASID field, and the process ID is the tag, with no
  table between the two ([`satp`](../kernel/memory-layout.md#satp)); the kernel refuses a core
  generated with a narrower field. New RTL, in this order: Zicboz, then Sstc. Dual issue only
  after timing closes.
- **No hardware multithreading by default.** At 60 to 80 MHz a DDR3 access is about seven
  cycles, so a strict barrel (two harts taking alternate cycles) halves each hart's throughput
  without filling the stalls it is meant to fill. It is an experiment with a gate, not a part of
  the design: switch-on-miss multithreading is added only if the counters show more than 30 to
  40 per cent of cycles stalled on memory, and a core that gets it shares its first-level cache
  and TLB between its harts, which then join the stated channels
  ([side channels](#side-channels)).
- **The interconnect.** TileLink: coherent first-level caches, a coherency hub and a shared
  second-level cache, from the SpinalHDL TileLink SoC generator. LiteX is not on the path: its
  `add_pcie` makes the host a master on the main bus, and its DDR3 target for this card is
  unfinished.
- **Memory.** UberDDR3 on both channels: channel A is main memory, channel B is DMA only.
- **The 16550** from the `pcie_7x` project, so the kernel and `consoled` keep one console driver
  and the console works from the firmware's first instruction. A UART is trivial
  ([tenet 7](../TENETS.md#7-devices-speak-virtio)), so it is not a virtio device. Its byte stream
  crosses the link like everything else: what it transmits becomes a posted record in the host's
  inbox, and what the host types is a posted write into its receive FIFO.
- **The virtio-mmio shim,** one per slot. Its register file is local to the card: the host
  programs it once (magic, version 2, the IDs, the features, `QueueNumMax`, the config space),
  and the guest's reads of it never cross the link. What the guest writes that the host must
  see, `QueueNotify`, `Status`, the queue addresses and readiness, the driver's features, is
  forwarded to the host as posted notify records. `InterruptStatus` and its acknowledgement
  live in the shim; a doorbell from the host sets the status, and a nonzero status drives the
  slot's level-triggered PLIC line.
- **The DMA region** is channel B, an uncached I/O region of the core: VexiiRiscv's memory
  attributes are fixed when it is generated and have no write-through, so a region shared with
  a host is either uncached or reached coherently. `dma_alloc` hands out frames from it alone;
  today it draws them from the ordinary pool ([devices](../kernel/devices.md)).
- **The ring region** is block RAM, uncached like channel B and confined like it, where drivers
  put their virtio rings: an uncached read of a ring index costs a cycle or two there instead of a
  DDR3 access. A queue of 256 entries is about 6.5 KB, so every queue fits.

### The host backend

One Rust program on the host, `cardd`, serves the console, the disk and the network, in that
order, over VFIO: BAR0 mapped into the process, MSI delivered as eventfds
([the card's host backend](card-host.md)). It sits where QEMU sits in the
[threat model](../TENETS.md#host-virtio-emulation): trusted to serve virtio honestly, and kept
from network data by TLS and SSH.

The signalling rule: **each side's inbox lives in its own memory, and only posted writes cross
into it.** The host's inbox is pinned host memory; the card's is the BAR0 mailbox. A read across
the PCIe link costs a whole round trip (about a microsecond) and appears nowhere on a core's hot
path; the one read that crosses is the DMA engine fetching bulk reply data from host staging.

- **Card to host.** The shim turns a `QueueNotify` into a notify record written into the host's
  inbox. The backend waits on it from a core Linux leaves alone (`isolcpus`, `nohz_full`, the
  backend's memory locked): spinning, then sleeping on the line with `UMWAIT` or `MWAITX`, and
  after a quiet spell it arms the interrupt and blocks, and MSI wakes it, at several
  microseconds.
- **Host to card.** The backend posts a completion into the mailbox and rings the slot's
  doorbell, wired to the PLIC; the card writes the used ring, with a release store on the used
  index.
- **Ordering.** The guest driver issues `fence w,w` before `QueueNotify`; the used ring's entries
  precede the release store on its index, and that store precedes the interrupt.
- **The contract.** The shim advertises `VIRTIO_F_ACCESS_PLATFORM`, so every virtio buffer the
  guest offers comes from `dma_alloc`, inside a channel-B window, and `EVENT_IDX` is on. The host
  never parses a guest pointer: every descriptor is checked against its window on the card, by
  the DMA engine or the forwarder, and the backend parses only records in its own memory. Those
  checks protect the host process from a hostile guest; the guest's protection from the host is
  channel B's decode, below, not anything the backend does.

### The card

- **The board.** An ex-datacentre XC7K480T-2FFG1156 with two independent 72-bit DDR3 ECC
  channels (2 GB each, on HR banks, so DDR3-1066 at most), PCIe Gen2 x8 wiring, a 64 MB BPI
  parallel NOR flash, a JTAG header and no other I/O. A look-alike "HPC" K420T/K480T board has a
  different pinout: check the silkscreen before trusting a constraints file.
- **The parts, all built by the open flow:**
  - `pcie_7x`: an endpoint on the PCIe hard block with no vendor IP, Gen2 x1 on this card. One
    lane gives about 400 MB/s, three times a gigabit link; the signalling is bound by latency,
    which the lane count barely moves.
  - UberDDR3 on both channels. Under openXC7 it runs at DDR3-667, about 5 GB/s a channel before
    overhead. Bringing it up on this card needs workarounds kept in the build: openXC7 drops
    `INTERNAL_VREF` (the 0.75 V reference is patched into the FASM), a zero-bit pseudo-pip is
    filtered out before `fasm2frames`, and the router is run with fixed seeds.
  - Our own glue: the BAR block, the DMA engine, the shims and the interconnect.
- **Speed.** Expect the cores at 60 to 80 MHz under openXC7 at first. Vivado is used only to tell
  a tool fault from a design fault.
- **Transport.** VFIO, as above. At first the backend walks the rings through mailbox commands
  that have the DMA engine copy a checked channel-B range into host staging (a round trip or two
  a copy); later a forwarder in the fabric walks them and pushes whole requests to the host
  instead ([two phases](card-host.md#two-phases)).

### What the card guarantees

**Trust assumptions,** each stated by the platform:
- **The host cannot change the card.** The bitstream loads from on-card flash, and the gateware
  contains nothing that can rewrite or reload it: no ICAP core, no flash-writing core, no JTAG or
  debug bridge reachable over PCIe or from a core. Updates go through the JTAG header, which is
  physical access ([out of scope](../TENETS.md#threat-model)). Whether the edge connector wires
  PERST# or JTAG to the configuration pins is checked on the board.
- **The host's window into the card is a mailbox.** BAR0 decodes only to doorbell and mailbox
  registers, and the registers that tell the card where the host's inbox is; no CSR, core reset,
  memory or window register is reachable through it. The inbox's addresses are host addresses: a
  host that lies about them misdirects the card's writes into its own memory.

**What Redoubt needs from the hardware:**
- **DMA confinement in RTL.** Every bus master other than the cores (the DMA engine, PCIe
  inbound) decodes only into channel B, never main memory, and inside it only into base and
  bound windows that the kernel alone programs: the essential half of an IOMMU by construction
  ([devices](../kernel/devices.md#residual-risks), [IOMMU](iommu.md)).
- **A root of trust.** A boot ROM in the bitstream verifies the loader, closing the gap that the
  loader itself is unchecked ([boot](../kernel/boot.md#residual-risks)). It is the same signed
  chain the loader applies to the bundle, not a second one. Open: who holds the ROM's key, and
  how the loader's key is rotated under it.
- **An entropy source** (Zkr's `seed` CSR, for M-mode and the kernel only), since a card that
  boots on its own has no host to write a seed into the device tree. RustSBI needs
  `mseccfg.SSEED`.
- **Hardware channels are devices,** each reached through a device object like any other
  (tenet 4).

### Side channels

Microarchitectural channels are out of scope ([the tenets](../TENETS.md#threat-model)), and the
card does not try to close them: the cores share the second-level cache and the memory
channels, and cross-core timing is not partitioned. Each is a stated channel; a core given
hardware multithreading adds its shared first-level cache and TLB to the list. Removing them is
a matter of placement, separate cards or machines.

### ISA features

Standard extensions that make Redoubt faster without weakening it, each testable on QEMU first:
- **Early:**
  - **ASIDs, used properly** (built). Each process's ID is its ASID: a switch flushes nothing,
    and a page-table change flushes by address and ASID with the kernel's global entries
    standing, a whole ASID when its process ID is given out again
    ([`satp`](../kernel/memory-layout.md#satp)). QEMU cannot show the saving, since its TLB is
    not tagged by ASID; this core's is, and the IPC path is where it pays.
  - **Sstc,** so setting the timer is a CSR write, not a trap into the firmware on almost every
    dispatch.
  - **Zicboz,** for the zeroing [R11 (memory)](../kernel/memory.md#r11-memory) does on every
    fresh frame (new RTL in VexiiRiscv).
  - **Zba, Zbb and Zbs.**
  - **Zihintpause,** for the kernel's spins: the kernel lock's wait and the shootdown's. It runs
    as a no-op on a core without the extension.
  - **Zkt,** stated by the platform for `keyd`'s constant-time signing
    ([R45 (constant-time signing)](../servers/keyd.md#r45-constant-time-signing)), after an
    audit of the multiplier's and the bit-manipulation unit's timing.
- **With SMP and the card:** Zkr (above), AIA with IMSIC for inter-processor interrupts and
  remote fences without firmware traps, Zawrs for the kernel lock's wait, scalar crypto (Zkne,
  Zknh, Zbkc) for `keyd` and TLS, Smepmp and Smstateen, Svinval.
- **Refused:**
  - Svpbmt: an uncached alias of a frame the physmap maps cached is stale data waiting to happen.
  - Svadu: puts a hardware writer on the page tables.
  - Svnapot: a lend is not physically contiguous.
  - Zacas: the kernel takes one big lock.
  - Hardware IPC around the kernel.
  - User access to `seed`, the counters or `cbo.inval`
    (`cbo.inval`: [R11 (memory)](../kernel/memory.md#r11-memory)).

### Kernel work it implies

- Several harts, planned for M2 (usable shell)
  ([several harts](../plan/m2-usable-shell.md#several-harts)): per-hart state, the big kernel
  lock as a FIFO ticket lock ([R78 (fair kernel entry)](../kernel/scheduling.md#r78-fair-kernel-entry)),
  inter-processor interrupts and TLB shootdowns, then frame zeroing and other long work moved
  outside the lock, and per-hart frame magazines. The run queue stays global. Later, one
  trap-entry lock whose guard owns a token, so that kernel globals become token-guarded cells,
  with a compile-time lock order if the lock is ever split; shootdowns through SBI's remote
  fences, then AIA.
- `dma_alloc` drawing only from the DMA region and the ring region, and the loader reading both
  from the device tree
  ([`loader/src/dt.rs`](../../loader/src/dt.rs)).
- The ISA features above. (`senvcfg` is already written 0 on every hart at boot:
  [backing and zeroing](../kernel/memory.md#backing-and-zeroing).)

### Measurement gates

Each step that adds hardware is taken on a measurement, never on an estimate:
- the fraction of cycles stalled on memory decides hardware multithreading;
- lock hold time and contention decide finer locking;
- DDR bandwidth and coherence traffic decide the core count;
- timing slack decides dual issue;
- **one core closes timing on the card under openXC7 before any step that adds cores.**

The gates are prose until the bench can read the performance counters, through an SBI call or
the trace ring; that path is part of the bring-up.

### Order

1. A latency bitstream on the card: the `pcie_7x` endpoint, UberDDR3 channel A and the BAR0
   mailbox, with a fabric cycle counter timing doorbell and interrupt round trips over VFIO.
2. One core: RustSBI, the kernel, and the 16550 through the host backend.
3. virtio-blk and virtio-net, then the full bench on the card.
4. Confined DMA (channel B and its windows), the boot ROM, Zkr.
5. Two cores (the SMP work), then four; measure, then scale.
6. Zicboz, Sstc, AIA, and the descriptor forwarder.

**Attack cases:**
- A DMA master, or PCIe inbound, writing outside its window is stopped; one writing into main
  memory is stopped.
- Every BAR offset other than the mailbox reads as zero and ignores writes.
- A bitstream reload or flash write from the host or a core has no path.
- A loader the boot ROM does not accept does not run.
- User `seed`, `cbo.inval` and counter reads fault.
- A descriptor outside its window is refused by the DMA engine or the forwarder before anything
  reaches the host, and a device writing through it is stopped by the decode.
- Every scheduling and memory case reruns on several harts.

**Open:**
- The ROM key's custody, and the loader key's rotation under it.
- A frame being zeroed or copied outside the kernel lock is one no other hart can name: the
  invariant on the free-frame bitmap's in-flight state, to be stated beside R11 with an attack
  case before the work that relies on it ([several harts](../plan/m2-usable-shell.md#several-harts)).
- The window granularity and how the kernel programs it.
- The core count beyond four, which waits on the measurements above.
