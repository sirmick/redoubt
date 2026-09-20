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
  every `sync`, and `fsd` relies on nothing more. The residue (littlefs does not checksum data):
  NAMESPACES.md.
- **`fsd`:** littlefs, 9P, one per volume, labels per volume (NAMESPACES.md).
- **Every on-disk parser is attack surface** and gets a fuzz target.

### blkd: ranges, badges and messages

**What `blkd` is trusted for.** On QEMU no hardware confines DMA, so `blkd` is inside the TCB
(tenet 7): it programs a device with physical addresses, and a device that ignores them can write
anywhere in RAM. `blkd` cannot make that untrue; what it is written to guarantee is the other half:
**it never asks the device to touch anything but the pages `dma_alloc` gave it, and nothing the
device puts in those pages can corrupt `blkd`'s own memory or stop it answering.** It hands the
device exactly one contiguous DMA region — the virtqueue, one request header, one status byte and
one data buffer — and every physical address it ever writes into a descriptor is that region's base
plus a constant offset. A client's lent pages never reach the device: `blkd` copies through the
data buffer in both directions, and copies each completed read out of DMA memory once, with a
length of its own, before it looks at a byte of it.

**Nothing the device says is ever used as an index or a length.** The descriptor table and the
available ring are written fresh from constants for every request and never read back, so a device
that rewrites them (a loop, an out-of-range `next`, a length that overflows) changes nothing `blkd`
believes. Of the used ring, `blkd` reads three words and checks each against what it sent: the
index must have advanced by exactly one, the entry's id must be the descriptor it submitted, and
the reported length must not exceed what the device was given. A used entry for a request never
sent, a jump or a step backwards in the index, a status byte outside the three the specification
defines, or a completion that never arrives before the deadline, all end the same way: the request
fails and the device is marked broken, after which `blkd` refuses every request rather than
trusting a device that has already lied. One request is outstanding at a time, which is what makes
"requests complete in order" true by construction.

**Partitions.** `blkd` reads a GPT (UEFI 2.10, §5.3) from LBA 1: the primary header only, checked
for its signature, its own LBA, a header size in range, both CRC32s, an entry array that lies
inside the disk, and entries whose first and last LBA lie inside the usable range and do not
overlap each other. An overlap would let two volumes alias each other's bytes, so a table with one
is refused whole. There is no backup-header fallback and no repair: `blkd` never writes a partition
table, so a table that does not check out is a refusal, not damage to work around.

**A range is a badge.** **The root badge of partition *i* is *i*** (from 1, in GPT entry order),
as `keyd`'s is the badge of key *i* (INIT.md), so `init` mints each volume's range from the
manifest without asking `blkd` anything, and a `blkd` restarted on the same disk gives the same
badges the same meaning while holding no state across the restart. Badges at or above 2^63 are
minted by `grant`, which narrows a range to a window inside the caller's own and is the only way a
range is delegated after boot; **only a root badge may grant**, so grants never chain and one
system client cannot open a bucket per link (CONTAINMENT.md, admission keys account 0 by badge).
`release(id)` frees a grant and everything granted under it, for the holder of the id and nobody
else; `release(0)` frees everything the caller granted, since no grant is ever given the id 0.

**Ranges carry no labels in milestone 1**, so `check` lets any caller read one and only an
unlabelled caller write to it: `blkd`'s clients are the `fsd` instances `init` hands a range to,
and a volume's labels are enforced in `fsd`, which is the one place they are written down
(NAMESPACES.md). **Admission counts grants, and only grants** — `blkd` parks no call and keeps no
other per-client state, so a flood of reads makes it grow by nothing; what bounds that flood is the
kernel's fair waiting per (account, label set) (R2) and the bound on one request below.

**What it is handed.** `blkd`'s startup block names the endpoint it receives on, `blkd`, and two
device handles: `disk`, the MMIO region (which must carry the DMA flag), and `disk-irq`, its
interrupt. Those are the manifest's device names; `blkd` parses no device tree and hardcodes no
address, and without both handles it does not start. It takes no arguments.

**Bounds.** At most 128 partitions; at most 64 sectors (32 KiB) of data in one `read` or `write`,
so one request's work is a number stated here rather than whatever fits the caller's lend; at most
8 live grants per (account, label set), across at most 8 of those at once.

**What each error answers.** `malformed` (code 1, as in every protocol): the request did not
decode, or its lengths are ones no sender could mean — a `write` whose data is empty or not a whole
number of sectors. `not_permitted`: the badge names no range, the caller's labels fail `check`, a
granted badge asked to grant, a grant asked for a window outside the caller's range, or a `release`
named an id the caller did not receive — one answer for all of them, so a refusal says only "not
you". `out_of_range`: the sectors asked for are not inside the range this badge names.
`too_many`: a cap is reached — more sectors than one request may carry, a reply that would not fit
the caller's lend, or a bucket or share with no room for another grant. `failed`: the device failed
or lied, `blkd` had no memory or no randomness, or the kernel refused to mint. None of them says
which.

<!-- wire: blkd -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `info` | - | `sectors: u64`, `sector_size: u32`, `read_only: u32` |
| 2 | `read` | `sector: u64`, `count: u32` | `data: bytes` |
| 3 | `write` | `sector: u64`, `data: bytes` | - |
| 4 | `flush` | - | - |
| 5 | `grant` | `sector: u64`, `count: u64` | `id: u64`, `range: handle[0] endpoint` |
| 6 | `release` | `id: u64` | - |

<!-- wire-errors: blkd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `out_of_range` |
| 4 | `too_many` |
| 5 | `failed` |

- `sector` is relative to the range the badge names, never to the disk, so a client cannot express
  an address outside its own partition; `sector_size` is 512, virtio-blk's unit, and `read_only` is
  1 when the device refused writes at feature negotiation.
- `read` and `write` move whole sectors: `count` is 1 to 64, and `write`'s data is that many whole
  sectors. A `write` is one virtio-blk write of the whole run, so a power failure persists a prefix
  of it (the contract above), and `flush` is one virtio-blk flush, which returns only when the
  device says the flush completed. `fsd` calls `flush` for every `sync` and relies on nothing more.
- `grant` mints a range `sector..sector + count` inside the caller's own, stamped like the handle
  the request came through, and returns a random id, exactly as `keyd`'s does (WIRE.md, granting
  and releasing). A window wider than the caller's, or one that leaves it, is `not_permitted`;
  nothing granted is ever wider than the badge it came through.

**Stated residuals.**
- A DMA handle is kernel-level trust, so a compromised `blkd` is a compromised kernel on a
  platform with no IOMMU. What `blkd` guarantees is the other half: it never *asks* the device for
  anything outside the pages `dma_alloc` gave it, and nothing the device puts in those pages can
  corrupt its own memory or stop it answering.
- **A restart leaves the device pointed at freed frames.** `blkd`'s DMA pages return to the free
  pool when it dies, and nothing stops a device already programmed with their physical addresses
  from writing to them; the restarted `blkd` resets the device at bring-up, but only after those
  frames may already have been handed to somebody else. Closing it needs the kernel to reset a
  device whose DMA pages are freed, or the hardware to confine it; until then `blkd`'s restart is
  a hole the same size as trusting `blkd`, which is what tenet 7 already says of it.
- A device can return wrong bytes for a sector it was asked for, and `blkd` cannot tell: the
  virtio-blk protocol has no checksum, and littlefs checksums only metadata (NAMESPACES.md). That
  is the same residual the filesystem already states, and what disk encryption (Later) would close.

## Networking
`netd (virtio-net) -> ipd -> clients (9P /net)`
- **Interface capability:** the one link-layer type, "send and receive Ethernet frames". Everything
  that moves frames attaches through it, so the Later designs add servers, not mechanisms.
- **`ipd`:** `smoltcp` (`no_std`, fuzzed). **One instance per network or trust domain**: a TCP bug
  reached from an untrusted network cannot touch another network's stack. Serves `/net`
  (NAMESPACES.md). A sink: it refuses labelled callers, and admits through the shared server
  library (CONTAINMENT.md).
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
