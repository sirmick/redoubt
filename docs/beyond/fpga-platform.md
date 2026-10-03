# The FPGA platform

## Idea

Redoubt on its own softcores, on two boards, with each board's virtio devices served by a
userland driver app on the host beside it, as QEMU serves them in development:

- **A Terasic DE10-Nano (Cyclone V SoC), the scale model:** two cores in the fabric. The board's
  own ARM cores run Linux and the driver app. It proves the platform, the doorbells and SMP
  cheaply, on a machine whose host is trusted, as QEMU is.
- **A YPCB-00338-1P1 card (Kintex-7 XC7K480T, PCIe), the real target:** about twelve to sixteen
  cores, an x86 host running the driver app over PCIe, and **DMA confined in hardware**: the
  card's two DDR3 channels are split into main memory and a DMA-only region that no bus master
  can leave.

Both boards run the same system-on-chip:
- **Cores:** VexiiRiscv, RV64IMAC with Sv39 and no FPU, coherent first-level caches,
  and **two hardware threads per core** (a strict barrel, the threads taking alternate cycles).
- **Platform:** QEMU `virt`'s map, so RustSBI and the kernel run with nothing but a device tree
  to tell them apart: CLINT, PLIC, a 16550 UART, virtio-mmio slots, RAM at `0x8000_0000`.
- **Debug and measurement:** a debug module, performance counters and a retirement trace for
  the bench.

The card is where DMA confinement can be done in hardware, which on QEMU it cannot
([the tenets](../TENETS.md#side-channels)). The whole bitstream builds with an open toolchain
(openXC7: Yosys and nextpnr-xilinx), so the gateware can be audited and rebuilt like the
software.

## Why it is not a goal

Every milestone runs on QEMU's `virt` machine, which is also the bench, and none of
M1 (separation and containment) through M5 (persist, install, share) needs hardware. The card's
gains are confinement of drivers' DMA, a verified loader, and an entropy source; all three
are stated today as residual risks.

## What it would need

### The system-on-chip

- **The core.** VexiiRiscv generated as Verilog: RV64IMAC, Sv39, supervisor mode, coherent
  first-level caches (TileLink), an optional shared second-level cache, 16-bit ASIDs (Sv39's
  whole field; the width is a generation parameter), PMP, the debug module and performance
  counters. Its interrupt controller, timer and I/O region are remapped to `virt`'s addresses.
  The ASID is that wide so that every process ID the kernel can make fits it: once the kernel
  flushes by ASID ([ISA features](#isa-features)), the process ID is the tag, with no table
  between the two.
- **Two harts per core.** VexiiRiscv carries a hart ID down its pipeline and indexes its
  register file by it, but about 25 assertions still force one hart. The barrel removes the
  cross-hart bypass and hazard logic; what is duplicated per hart is the PC, the CSRs, the MMU
  state, the PMP entries and the LR/SC reservation. No open Linux-class RISC-V core does this
  yet, so it is new RTL, worth offering upstream. The two harts of a core share its
  first-level cache and its TLB; that is a stated channel ([side channels](#side-channels)).
  - **The TLB tags each entry with its hart.** Each hart's translations stay its own, as the
    privileged spec requires of a translation cache harts share. The kernel's TLB rules rely on
    it ([several harts](../plan/m2-usable-shell.md#several-harts)). A hart's flush may empty
    its sibling's entries too, which costs time, never correctness.
  - **The barrel stays strict.** A hart's rate does not depend on what its sibling runs, which
    the scheduler's shares across harts count on
    ([R12 (scheduling)](../kernel/scheduling.md#r12-scheduling)).
- **The 16550** from the `pcie_7x` project, so the kernel keeps one console driver.
- **The virtio-mmio shim:** each slot is a register window. A write to `QueueNotify` becomes a
  doorbell to the driver app; the app's answer raises a PLIC line.
- **The DMA region** is an uncached I/O region of the core: VexiiRiscv's memory attributes are
  fixed when it is generated and have no write-through, so a region shared with a host is either
  uncached or reached coherently. `dma_alloc` hands out frames from it alone; today it draws them
  from the ordinary pool ([devices](../kernel/devices.md)).

### The driver app

One Rust program on the host serves every virtio device (block, network, console); only its
transport differs between the boards. It sits where QEMU sits in the
[threat model](../TENETS.md#host-virtio-emulation): trusted to serve virtio honestly, and kept
from network data by TLS and SSH.

The signalling rule: **only posted writes cross between the softcores and the app, and each side
waits by spinning on memory local to it.** A read across a bridge or a PCIe link costs a whole
round trip and appears nowhere on the hot path.

- **Softcore to app.** The fabric turns a `QueueNotify` write into a write of a notify record
  into the app's memory. The app spins on it from a core Linux leaves alone (`isolcpus`,
  `nohz_full`, the app's memory locked), and after a quiet spell arms an interrupt and sleeps.
- **App to softcore.** The app updates the used ring, fences, and makes a posted write to a
  doorbell register wired to the PLIC.

### The DE10-Nano

- **Memory.** The softcores' RAM is their own: an FPGA-to-SDRAM port into a slice of the board's
  DDR3 that Linux never maps, or the MiSTer SDRAM board. Rings and I/O buffers live in a second,
  shared slice, reserved in Linux's device tree and mapped cacheable for the app (`udmabuf` with
  `dma-coherent`); the softcores reach it only through the ARM's accelerator coherency port
  (ACP). A softcore cleans its own data cache before each kick (Zicbom clean or flush). Keeping
  the two slices apart also keeps clear of the ARM cores' erratum on mixed coherent and
  non-coherent access to the same lines.
- **Doorbells.** The fabric writes a whole 32-byte line through the ACP, coherent (`AxCACHE`
  0111, `AxUSER[0]` set), which invalidates the ARM's cached copy; the app sees it on its next
  spin. The return doorbell is a posted store on the lightweight bridge. The published numbers:
  a read across the lightweight bridge takes about 500 ns, and an FPGA interrupt reaches a Linux
  thread in 25 to 144 µs under PREEMPT_RT. The ACP path is estimated at 150 to 400 ns, not yet
  measured.
- **Integration.** LiteX's `cyclonev_hps` target (a pre-booted HPS, with the ACP,
  FPGA-to-SDRAM ports and FPGA interrupts exposed). The SDRAM ports are configured before Linux
  starts; Linux can only switch them on and off. Quartus Prime Lite builds it.
- **What it does not give.** The ARM sees all of DDR3 and can reload the FPGA, so none of the
  card's hardware guarantees below hold here, and the board says so: it is a trusted-host
  configuration, like QEMU.

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
  - Our own glue: the BAR block, the DMA engine and the interconnect. LiteX's PCIe and DDR3
    support is not used here: its `add_pcie` makes the host a master on the main bus, and its
    DDR3 target for this card is unfinished.
- **Speed.** Expect the cores at 60 to 80 MHz under openXC7 at first. Vivado is used only to tell
  a tool fault from a design fault.
- **Transport.** The driver app uses VFIO. The card writes notify records into pinned host
  memory, where they land in the host's last-level cache, about 1 µs one way; MSI-X through VFIO
  is the idle fallback, at several µs. The host rings the card with posted BAR writes. At first
  the app reads rings from card memory across the link (about 1 µs a read); later a forwarder in
  the fabric pushes descriptors to the host instead.

### What the card guarantees

**Trust assumptions,** each stated by the platform:
- **The host cannot change the card.** The bitstream loads from on-card flash, and the gateware
  contains nothing that can rewrite or reload it: no ICAP core, no flash-writing core, no JTAG or
  debug bridge reachable over PCIe or from a softcore. Updates go through the JTAG header, which
  is physical access ([out of scope](../TENETS.md#threat-model)). Whether the edge connector
  wires PERST# or JTAG to the configuration pins is checked on the board.
- **The host's window into the card is a mailbox.** BAR0 decodes only to doorbell and mailbox
  registers; no CSR, core reset, memory or window register is reachable through it.

**What Redoubt needs from the hardware:**
- **DMA confinement in RTL.** Every bus master other than the cores (the DMA engine, PCIe
  inbound) reaches only the DMA channel, never main memory, with base and bound windows inside
  it that only the kernel programs: the essential half of an IOMMU by construction
  ([devices](../kernel/devices.md#residual-risks), [IOMMU](iommu.md)).
- **A root of trust.** A boot ROM in the bitstream verifies the loader, closing the gap that the
  loader itself is unchecked ([boot](../kernel/boot.md#residual-risks)).
- **An entropy source** (Zkr's `seed` CSR, for M-mode and the kernel only), since a card that
  boots on its own has no host to write a seed into the device tree. RustSBI needs
  `mseccfg.SSEED`.
- **Hardware channels are devices,** each reached through a device object like any other
  (tenet 4).

### Side channels

Microarchitectural channels are out of scope ([the tenets](../TENETS.md#threat-model)), and the
card does not try to close them: the two harts of a core share a first-level cache and a TLB, the
cores share the second-level cache and the memory channels, and cross-core timing is not
partitioned. Each is a stated channel. Removing them is a matter of placement, separate cards or
machines.

### ISA features

Standard extensions that make Redoubt faster without weakening it, each testable on QEMU first:
- **Early:**
  - **ASIDs, used properly.** The kernel already puts the process ID in `satp`, but flushes the
    whole TLB on every switch and after every page-table change, the kernel's global entries
    with it. Flushing by address and ASID, and a whole ASID when its process ID is reused, is the
    biggest saving on the IPC path.
  - **Sstc,** so setting the timer is a CSR write, not a trap into the firmware on almost every
    dispatch.
  - **Zicboz,** for the zeroing [R11 (memory)](../kernel/memory.md#r11-memory) does on every
    fresh frame (new RTL in VexiiRiscv).
  - **Zba, Zbb and Zbs.**
  - **Zihintpause,** for the kernel's spins. On this card's strict barrel it gives the sibling
    nothing, since each hart keeps its alternate cycles; it pays on a core whose harts share
    issue slots, and runs as a no-op on one without it.
  - **Zkt,** stated by the platform for `keyd`'s constant-time signing
    ([R45 (constant-time signing)](../servers/keyd.md#r45-constant-time-signing)), after an
    audit of the multiplier's and the bit-manipulation unit's timing.
- **With SMP and the card:** Zkr (above), AIA with IMSIC for inter-processor interrupts and
  remote fences without firmware traps, Zawrs, scalar crypto (Zkne, Zknh, Zbkc) for `keyd` and
  TLS, Smepmp and Smstateen, Svinval.
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
  lock, inter-processor interrupts and TLB shootdowns.
- `dma_alloc` drawing only from the DMA region, and the loader reading it from the device tree
  ([`loader/src/dt.rs`](../../loader/src/dt.rs)).
- The ISA features above. (`senvcfg` is already written 0 on every hart at boot:
  [backing and zeroing](../kernel/memory.md#backing-and-zeroing).)

### Order

1. A latency bitstream on the DE10-Nano: a fabric cycle counter timing the ACP doorbell,
   register polling and the interrupt path.
2. One core without the second hart: RustSBI, the kernel, and virtio-console through the driver
   app.
3. virtio-blk and virtio-net, then the full bench on the DE10-Nano.
4. Two cores (the SMP work), then two harts per core.
5. The card: `pcie_7x` and its MSI demo, UberDDR3 on both channels, the mailbox BAR, the
   confined DMA engine, the VFIO transport, the boot ROM, then more cores.
6. Zkr, AIA and the descriptor forwarder.

**Attack cases:**
- A DMA master, or PCIe inbound, writing outside its window is stopped; one writing into main
  memory is stopped.
- Every BAR offset other than the mailbox reads as zero and ignores writes.
- A bitstream reload or flash write from the host or a softcore has no path.
- A loader the boot ROM does not accept does not run.
- User `seed`, `cbo.inval` and counter reads fault.
- Every scheduling and memory case reruns on several harts.
- Two harts of one core, running two processes at one virtual address, each read their own page.

**Undecided:**
- Whether the DE10-Nano's softcore RAM is a slice of the HPS DDR3 or the MiSTer SDRAM board.
- The window granularity and how the kernel programs it.
- Whether LiteX or VexiiRiscv's own SoC generator carries the card's glue.
- The number of cores, which waits on synthesising one.
