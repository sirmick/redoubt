# I/O architecture: drivers, storage, networking

Status: agreed direction, 2026-09-18. Nothing here is built yet. Tenet 7 is the one-line summary.

## The rule
Drivers are virtio, unless the device is trivial. A driver's DMA is either inside the TCB (no
IOMMU) or confined by an IOMMU; the kernel supports both. On real, messy hardware we reserve cores
for xous64 and let Linux run the hardware, serving virtio to us.

Why: one small driver set on every target, the device side is untrusted as far as the hardware can
enforce, and board-specific complexity (clock trees, pinctrl, PMICs, USB, Wi-Fi) never enters the OS.

## Platforms
Every platform presents the same contract: virtio-mmio devices, a standard interrupt controller
(PLIC, later AIA), SBI firmware, a device tree.

| Platform | Who serves virtio | DMA confinement | Role |
| --- | --- | --- | --- |
| QEMU `virt` | QEMU | none, or QEMU's RISC-V IOMMU (`iommu-sys=on`, QEMU >= 10) | development, the test bench |
| FPGA softcore (e.g. CVA6) | gates, or a small helper core running Rust device-side firmware | RISC-V IOMMU (open RTL exists, zero-day-labs/riscv-iommu) or IOPMP | the secure configuration |
| Messy SoC (e.g. Orange Pi RV2, SpacemiT K1) | Linux on reserved cores | none on K1 (no IOMMU found); an IOMMU SoC (e.g. K3's T100) would confine Linux's devices | daily use on real hardware |

The Orange Pi RV2 is no longer a native-driver target. We write no K1 drivers.

## Driver model
- **A driver is an unprivileged server.** The kernel keeps only the interrupt controller, the timer
  and SBI.
- **Resources are handed in, not discovered.** The loader reads the device tree. The boot manifest
  assigns each driver its MMIO region, IRQ and (if any) DMA grant, passed as startup arguments.
  Drivers do not parse the device tree or hardcode addresses.
- **The server graph is declared.** The manifest also says who holds a connection to whom (the fs
  server holds blk0; the shell holds a directory handle). No lookup by name: holding a connection is
  the authority (see "Kernel prerequisites").
- **Trivial drivers** are the only non-virtio ones: UART (ns16550), RTC (goldfish), and devices of
  similar size with no DMA. They are fully untrusted.
- **Crate:** `virtio-drivers` (rcore-os, pure Rust, `no_std`) under a thin server per device,
  audited as TCB while its driver is.
- **The device side is hostile.** Our virtio drivers validate every ring index, length and
  descriptor chain the device returns, as Linux does for confidential VMs. Each driver gets a fuzz
  target driven by a malicious device model.

### DMA, two modes
1. **Driver in the TCB (no IOMMU).** A syscall allocates physically contiguous DMA pages and returns
   their physical address, only for processes whose manifest grants DMA. Clients lend pages to the
   driver and the driver copies into its own DMA buffers, so client pages never reach the device.
   A DMA driver is therefore trusted like the kernel: it is kept tiny and audited.
2. **Driver confined (IOMMU).** The kernel programs the IOMMU from the same manifest grants: each
   device may reach only its driver's DMA pages. The driver leaves the TCB. The manifest format
   carries the device's IOMMU identity from the start so mode 2 needs no redesign.

IOPMP (per-bus-master physical windows, no translation) is a simpler alternative for the FPGA. Its
spec was a release candidate in 2025; revisit when ratified.

## Storage
`virtio-blk driver -> block server -> fs server -> clients`
- **Block server:** partitions, cache, and block-range capabilities (a filesystem sees only its
  partition). Also per-block authenticated encryption with a Merkle root, so a hostile disk (or the
  Linux serving it) sees ciphertext and cannot tamper undetected.
- **fs server:** no global namespace. Clients hold directory capabilities; paths resolve only below
  a held handle and `..` never escapes it (as WASI preopens, Capsicum, Fuchsia).
- **Every on-disk parser is attack surface** and gets a fuzz target.
- **Filesystem:** pure Rust, crash-consistent. Candidates: RedoxFS (CoW, pure Rust), or a small
  specified CoW design of our own. FAT (`fatfs`) only for interop, as a separate untrusted server.

## Networking
`virtio-net driver -> net server (smoltcp) -> clients`
- **Socket capabilities are scoped:** "may listen on TCP 22", "may connect to 10.0.0.0/8:443". An
  application without one has no network.
- **TLS and SSH are end to end** (in the Elixir userland: OTP `:ssh` / `:ssl`), so the driver, the
  stack and anything serving virtio-net carry only ciphertext.
- **Keys live in a key server.** VMs ask it to sign; they never hold private keys.

## The Elixir boundary (beamlet)
beamlet's `Platform` trait grows handles for directories, files and sockets. BEAM code sees them as
unforgeable resource terms; each VM gets only what its process was granted. OTP's `:crypto` is a C
NIF upstream, so beamlet implements its natives in Rust (RustCrypto), or forwards to the crypto/key
server.

## Linux on reserved cores
For messy SoCs. Not a hypervisor: the K1 does not appear to have the H extension, and none is needed.
- **Partition with OpenSBI domains.** The device tree assigns harts, RAM and MMIO to a Linux domain
  and a xous64 domain; PMP stops each domain's harts from touching the other's memory. SBI IPI and
  HSM calls are confined to their own domain.
- **One shared window** holds the virtio rings and buffers. Linux runs the device side (a small
  userspace backend over its real drivers); xous64 runs its normal virtio drivers.
- **Doorbells:** SBI IPIs do not cross domains. Start with polling; later a hardware mailbox or a
  small SBI extension.
- **Trust:**
  - Linux cannot read or write xous64 memory from its CPUs (PMP).
  - Data is protected end to end (block-layer AEAD, TLS/SSH), so Linux sees ciphertext; a
    compromised Linux can deny service, not read or forge data.
  - **Hole: DMA.** Without an IOMMU, a root-compromised Linux can program a device to write xous64's
    memory. On such a SoC, Linux is in the TCB for memory integrity. Stated, not hidden.
  - **Hole: firmware.** OpenSBI (C) enforces the partition and is TCB for both sides. Tenet 3 wants
    a Rust firmware with domain support; RustSBI's support is unchecked.
- **Testable on QEMU:** OpenSBI domains work on `virt`, so Linux + xous64 side by side runs in the
  bench, including a hostile device-side backend.

## Kernel prerequisites (in order)
1. Transferable connections: send a connection (capability) to another process over IPC. Name-based
   lookup through `xous-names` stays only as a boot-time convenience, if at all.
2. Server-death notification and connection revocation, so drivers can be restarted by a supervisor.
3. DMA page allocation, gated by a manifest grant.
4. Wider IRQ numbering (currently 32 entries).
5. IOMMU backend (a capability feature, like `plic`), programmed from the grants.

## Open questions
- Directory capabilities with no global `/`: agreed as the secure choice; the shell UX is to be designed.
- RedoxFS versus our own CoW filesystem.
- IOMMU versus IOPMP on the FPGA; which softcore (CVA6 has the IOMMU integrated; VexiiRiscv continues
  the Xous lineage).
- Doorbell mechanism for the Linux partition.

## Prior art
QNX Neutrino (drivers as resource-manager processes, interrupts as messages, fine-grained abilities,
SMMU manager service; but a global path namespace and drivers loaded into stack processes), seL4's
driver framework and driver VMs, Xen driver domains, OpenAMP/rpmsg (virtio over shared memory),
Jailhouse and Bao (static partitioning), Hubris (build-time task graph).
