# Redoubt design notes

Start here. [TENETS.md](TENETS.md) outranks everything else; each other note owns one topic and
links to the rest instead of repeating it.

## Built today vs designed
**Built:** a width-generic (rv32 and rv64) microkernel forked from Xous that boots on QEMU `virt`
under OpenSBI or RustSBI; a loader that verifies an Ed25519-signed boot bundle and builds Sv32/Sv39
address spaces; W^X enforced and checked at boot; default-deny device grants; a QEMU test bench
with attack tests. See [STATUS.md](STATUS.md).
**Designed, not built:** everything from capabilities onward (handles, budgets, labels, init and the
steward, 9P namespaces, packages, storage and network servers). See [PLAN.md](PLAN.md) for the order.

## Reading order
| Note | What it owns |
| --- | --- |
| [TENETS.md](TENETS.md) | Adversary, review model, non-goals, the seven tenets. |
| [STATUS.md](STATUS.md) | Where the code stands against each tenet; the test cases. |
| [CAPABILITIES.md](CAPABILITIES.md) | Handles, badges, budget-stamped revocation, principals, agents, the powerbox and approvals. |
| [CONTAINMENT.md](CONTAINMENT.md) | Information-flow labels, covert channels, shared-server admission, the executable model. |
| [RESOURCES.md](RESOURCES.md) | Budgets, scheduling, clocks. |
| [INIT.md](INIT.md) | Boot sequence after the kernel: init, the steward, keyd, sshd, restarts, startup block; worked example. |
| [NAMESPACES.md](NAMESPACES.md) | 9P, per-process namespaces, filesystem servers, littlefs, process launching. |
| [PACKAGES.md](PACKAGES.md) | Package format, the store, profiles, code signing and native user code. |
| [IO-ARCHITECTURE.md](IO-ARCHITECTURE.md) | Drivers (virtio), DMA, storage and network stacks; the "Later" designs. |
| [PLATFORM-FPGA.md](PLATFORM-FPGA.md) | The FPGA target and what its hardware changes. |
| [DEVICE-GRANTS.md](DEVICE-GRANTS.md) | Today's device authority (interim, until device handles). |
| [VERIFIED-BOOT.md](VERIFIED-BOOT.md) | Bundle signature check. |
| [BOOT.md](BOOT.md) | Firmware -> loader -> kernel, the bundle and the kernel argument block. |
| [MEMORY-LAYOUT.md](MEMORY-LAYOUT.md) | Physmap, address-space split, Sv32 vs Sv39. |
| [TIMER.md](TIMER.md) | The hart timer today, and the decided kernel-owned design. |
| [DEBUGGING.md](DEBUGGING.md) | No debugger in the kernel; QEMU gdb now, a gated userspace debugger later. |
| [PLAN.md](PLAN.md) | North star and what to build next. |
| [HISTORY.md](HISTORY.md) | What was done, bugs found, review rounds. |

## Servers
Canonical names, used in every note. Class: `system` runs before everyone else (RESOURCES.md).

| Name | Role | Class, budget | State |
| --- | --- | --- | --- |
| `init` | Holds all authority at boot; starts, wires and restarts every OS process | system | designed |
| `consoled` | ns16550 UART driver | system | exists as test programs |
| `bootfsd` | Read-only 9P server over the verified boot bundle (`/boot`) | system | designed |
| `blkd` | virtio-blk driver (DMA) | system | designed |
| `blockd` | Partitions, cache, per-block authenticated encryption, block-range capabilities | system | designed |
| `fsd` | littlefs filesystem, one instance per volume (e.g. `fsd:data`), serves 9P | system | designed |
| `netd` | virtio-net driver (DMA) | system | designed |
| `ipd` | smoltcp IP stack, one instance per network (e.g. `ipd:lan`), serves `/net` | system | designed |
| `keyd` | Holds every private key; signs, never exports | system | designed |
| `steward` | Principals, authentication, sessions, powerbox, launching, audit file | system | designed |
| `sshd` | SSH front door (`sunset` candidate) and the approval session | system | designed |
| `gatewayd` | LLM gateway: holds API keys, meters budgets, is a label sink | system | designed |
| `webd`, `linkd`, `routerd` | Browser GUI front door; VLAN/filter/QoS; router | system | later (IO-ARCHITECTURE.md) |

Session and agent VMs are beamlet processes in user budgets, not servers.

## Glossary
- **TCB** (trusted computing base): the code whose bugs can break the security guarantees; everything
  else is contained by it.
- **SBI** (Supervisor Binary Interface): the RISC-V firmware call interface (console, timer, harts).
  Extensions used: **HSM** (hart start/stop), **SRST** (reset/power-off), TIME, IPI.
- **PLIC** (Platform-Level Interrupt Controller); **AIA** (its successor, Advanced Interrupt
  Architecture); **CLINT** (core-local timer/IPI device); **Sstc** (S-mode timer compare extension).
- **PMP** (Physical Memory Protection): M-mode-programmed physical access rules per hart.
- **DTB** (device tree blob): the machine description the firmware passes to the loader.
- **Sv32 / Sv39**: the RISC-V 32-bit (2-level) and 64-bit (3-level) page-table formats.
- **physmap**: the kernel's single mapping of all RAM at `PHYSMAP_BASE + phys` (MEMORY-LAYOUT.md).
- **W^X**: no page is ever writable and executable at once.
- **IOMMU**: translates and checks device DMA addresses. **IOPMP**: simpler per-bus-master physical
  windows, no translation. **DMA**: devices reading and writing memory directly.
- **AEAD** (authenticated encryption with associated data): encryption that also detects tampering.
- **Handle**: an index into a process's kernel-held capability table; unforgeable, meaningless in any
  other process. **Capability**: the authority a handle carries. **Badge**: an unforgeable tag a
  server attaches when minting, telling it which grant a request came through.
- **Endpoint**: the kernel object clients send to; it outlives the server process receiving on it.
- **Budget**: a kernel resource container (memory, handles, threads, CPU weight, labels); every
  process is in exactly one. **Lease**: a budget with a deadline.
- **Principal**: an accountable identity (a human or an agent). **Sponsor**: the principal
  accountable for an agent. **Session**: processes started from a principal's capabilities.
- **Powerbox**: the steward's escalation service; grants authority a principal lacks, after approval.
- **Label**: an information-flow tag on data and budgets (CONTAINMENT.md); the approach is known as
  **DIFC** (decentralized information flow control). **Sink**: a server whose output leaves a
  principal or the machine. **Declassify**: the label owner releasing an item.
- **9P** (9P2000): Plan 9's file protocol. **fid**: a 9P handle to a file within one connection.
- **beamlet**: the safe-Rust BEAM VM running the Elixir userland (sibling repository).
  **NIF**: a BEAM native function (C upstream; Rust in beamlet).
- **LPM** (longest-prefix match): IP route lookup. **VLAN** (802.1Q): tagged virtual LANs.
- **virtio**: the standard virtual-device interface (virtio-mmio, virtio-blk, virtio-net).
