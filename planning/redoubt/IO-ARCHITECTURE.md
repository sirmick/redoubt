# I/O architecture: drivers, storage, networking

Designed, not built (except the ns16550 UART in test programs). Owns: the driver model, DMA, the
storage and network stacks, the platforms, and the deferred ("Later") I/O designs. Tenet 7 is the
summary.

## The rule
Drivers are virtio, unless the device is trivial. A DMA driver is inside the TCB unless the hardware
confines its DMA. On messy hardware we reserve cores for Redoubt and let Linux run the hardware and
serve virtio to us (Later, below).

Why: one small driver set on every target, and board-specific complexity (clock trees, pinctrl,
PMICs, USB, Wi-Fi) never enters the OS.

## Platforms
Every platform presents the same contract: virtio-mmio devices, a standard interrupt controller
(PLIC, later AIA), SBI firmware, a device tree.

| Platform | Who serves virtio | DMA confinement | Role |
| --- | --- | --- | --- |
| QEMU `virt`, rv32 and rv64 | QEMU | none | development, the test bench |
| FPGA cards (PLATFORM-FPGA.md) | the host over PCIe, or device logic on the card | RTL: devices reach only the DMA memory channel | the secure configuration, under its stated assumptions |
| Messy SoC (e.g. Orange Pi RV2) | Linux on reserved cores | none; Linux is in the TCB | Later |

## Driver model
- **A driver is an unprivileged server.** What the kernel keeps: TENETS.md, tenet 1.
- **Resources are handed in, not discovered.** The loader reads the device tree; the boot manifest
  assigns each driver its device objects (an MMIO range, with a DMA flag if it may do DMA; an IRQ),
  as handles in its startup block (today: loader-emitted grants, DEVICE-GRANTS.md). Drivers do not
  parse the device tree or hardcode addresses.
- **Interrupts are received,** not handled: a driver thread waits in `receive` on its IRQ handle; the
  kernel masks the source when it fires and unmasks it at the next receive (KERNEL-SPEC.md, R5).
- **The server graph is declared** in the boot manifest (`fsd` holds a `blkd` partition; the shell
  holds a directory handle). No lookup by name.
- **Trivial drivers** are the only non-virtio ones: UART (ns16550), RTC (goldfish), and devices of
  similar size with no DMA. They are fully untrusted.
- **Crate:** `virtio-drivers` (rcore-os, pure Rust, `no_std`) under a thin server per device,
  audited as TCB while its driver is.
- **The device side is hostile.** Our virtio drivers validate every ring index, length and
  descriptor chain the device returns, as Linux does for confidential VMs. Each driver gets a fuzz
  target driven by a malicious device model.

### DMA
- **Driver in the TCB (no confinement).** `dma_alloc`, allowed only with an MMIO handle carrying the
  DMA flag, returns physically contiguous, zeroed pages and their physical address, to program the
  device with. A driver is told the physical address of pages the kernel gave it; it can never map
  RAM by physical address.
  Clients lend pages to the driver and the driver copies into its own DMA buffers, so client pages
  never reach the device. A DMA driver is trusted like the kernel: kept tiny and audited.
- **Driver confined (hardware).** On the FPGA, devices reach only the DMA memory channel, and
  per-master windows keep devices out of each other's buffers (PLATFORM-FPGA.md). The kernel
  allocates each driver's DMA pages from its window; the driver can then corrupt only its own
  buffers. The same handles describe both modes.

## Storage
`blkd (virtio-blk, partition table) -> fsd -> clients`
- **`blkd`:** the driver, the partition table, and block-range handles (a filesystem sees only its
  partition).
- **`blkd`'s contract**, which littlefs's power-loss safety rests on: writes overwrite whole
  sectors; requests complete in order; a torn write persists a prefix of the write, never an
  arbitrary subset of its units (as raw flash may); `sync` returns only after virtio-blk's flush
  has completed, so it is never acknowledged before the data is durable. `blkd` issues a flush on
  every `sync`, and `fsd` relies on nothing more. The residue (littlefs does not checksum data): NAMESPACES.md.
- **`fsd`:** littlefs, 9P, one per volume, labels per volume (NAMESPACES.md).
- **Every on-disk parser is attack surface** and gets a fuzz target.

## Networking
`netd (virtio-net) -> ipd -> clients (9P /net)`
- **Interface capability:** the one link-layer type, "send and receive Ethernet frames". Everything
  that moves frames attaches through it, so the Later designs add servers, not mechanisms.
- **`ipd`:** `smoltcp` (`no_std`, fuzzed). **One instance per network or trust domain**: a TCP bug
  reached from an untrusted network cannot touch another network's stack. Serves `/net`
  (NAMESPACES.md). A sink: it refuses labelled callers, and admits per account (CONTAINMENT.md).
- **Firewalling is mostly structural.** Egress: a process connects only where its socket capability
  allows (IP prefix and port). Ingress: nothing listens without a listen capability.
- **TLS and SSH are end to end**, so drivers and stacks carry ciphertext. `sshd` is Rust (INIT.md);
  users' own outbound TLS and SSH run in their VMs.

## The Elixir boundary (beamlet)
beamlet's `Platform` trait is one asynchronous 9P client: directories, files, sockets and the
console are namespace walks, seen by BEAM code as unforgeable resource terms. The kernel has no
queued sends, so the VM gets its asynchrony from a small pool of I/O threads, each making one
blocking call. OTP's `:crypto` is a C NIF upstream; beamlet implements it in Rust (RustCrypto).
Design: beamlet's DESIGN.md.

## Later (designed, deferred)
Kept so they are not redesigned from scratch; each carries its review findings.

### Disk encryption
Per-block authenticated encryption (AEAD) with a Merkle root, keyed through `keyd`, in `blkd` or a
server above it, so a hostile disk can neither read nor tamper undetected. Deferred: on QEMU the host
is the disk and is trusted, and on the FPGA the host is the root of trust (PLATFORM-FPGA.md); it
returns with a platform whose disk is outside the trust boundary.

### LLM gateway (`gatewayd`; milestone 3)
Holds API keys, meters token and money budgets per principal, logs calls; a label sink cleared for
nothing (an on-box model can be cleared for labels). Agents hold a handle to it, never a key.

### Browser GUI (`webd`)
The GUI follows sirmick/wash's shape (not its Go code): a desktop in the browser over one WebSocket,
a per-user router multiplexing channels to app processes, window state held by the server. `webd`
(Rust) terminates TLS (`rustls` with a pure-Rust crypto provider), authenticates, and routes each
user's WebSocket to their session; the router and apps are Elixir in the session VM, so the OS itself
contains no graphics code.
- **Finding:** the desktop is drawn by the untrusted session VM, and a passkey signs a hash the user
  never sees, so the browser can show one request while approving another. The GUI may **notify**
  that an approval is waiting; it never approves (CAPABILITIES.md).

### Link layer and routing (`linkd`, `routerd`)
- **`linkd`:** per NIC; 802.1Q tag and untag, each VLAN presented as its own interface capability; a
  small L2/L3 allowlist and rate limiter before any stack parses a byte; egress priority queues (QoS)
  keyed by the capability the traffic came from; DSCP marking.
- **`routerd`:** holds several interface capabilities and forwards between them: longest-prefix
  match, TTL, ARP/NDP, stateful filtering and NAT for forwarded traffic. Data plane in Rust (our own,
  or Netstack3's portable core if it earns its size). The control plane of a router shared between
  principals is Rust; an Elixir control plane is fine for a network one principal owns.

### Linux on reserved cores
For messy SoCs (e.g. the Orange Pi RV2, SpacemiT K1: no IOMMU, no hypervisor extension).
- **Trust model: Linux is in the TCB.** A compromised Linux (or its boot chain, which also supplies
  the device tree and RNG seed) is game over; we do not defend against it.
- **Partition with OpenSBI domains:** the device tree assigns harts, RAM and MMIO to a Linux domain
  and a Redoubt domain; PMP keeps each domain's harts out of the other's memory; SBI IPI and HSM
  calls stay within a domain.
- **One shared window** holds the virtio rings and buffers. Linux runs the device side (a small
  userspace backend over its drivers); Redoubt runs its normal virtio drivers.
- **Doorbells:** SBI IPIs do not cross domains. Start with polling; later a hardware mailbox or a
  small SBI extension.
- The firmware (OpenSBI, C) enforces the partition; whether RustSBI supports domains is unchecked.
  Testable on QEMU: OpenSBI domains work on `virt`.

### IOMMU and IOPMP
A RISC-V IOMMU backend (QEMU `iommu-sys=on`, QEMU 10 or later; open RTL exists) or IOPMP
(per-bus-master windows; spec not ratified as of 2025) would confine DMA on hardware that has one.
On our FPGA the DMA memory channel may make both unnecessary.

## Prior art
QNX Neutrino, seL4's driver framework and driver VMs, Xen driver domains, OpenAMP/rpmsg, Jailhouse,
Bao, Hubris.
