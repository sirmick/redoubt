# Redoubt design notes

Start here. [TENETS.md](TENETS.md) outranks everything else; each other note owns one topic and
links to the rest instead of repeating it. Design v4 is frozen for milestone 1: changes need a
reason recorded in HISTORY.md.

## Built today vs designed
**Built:** a width-generic (rv32 and rv64) microkernel forked from Xous that boots on QEMU `virt`
under OpenSBI or RustSBI; a loader that verifies an Ed25519-signed boot bundle and builds Sv32/Sv39
address spaces; W^X enforced and checked at boot; default-deny device grants; physical RAM never
nameable by address; a two-hart SMP spike behind the `smp` feature; a QEMU test bench with attack
tests. In the sibling beamlet repository: a BEAM
VM that runs Elixir, its compiler and IEx over a console. See [STATUS.md](STATUS.md).
**Designed, not built:** everything from capabilities onward (handles, IPC, budgets, labels, init
and the steward, 9P namespaces, packages, storage and network servers). Order: [PLAN.md](PLAN.md).

## Reading order
**The design**

| Note | What it owns |
| --- | --- |
| [TENETS.md](TENETS.md) | Adversary, review model, timing principle, non-goals, the seven tenets. |
| [CAPABILITIES.md](CAPABILITIES.md) | Why handles; how IPC and minting are used; revocation policy; principals, agents, projects; the powerbox and approvals. |
| [CONTAINMENT.md](CONTAINMENT.md) | Labels, sessions and vaults, declassification, the shared server library, crash blame, covert and timing channels, the executable model. |
| [RESOURCES.md](RESOURCES.md) | Why budgets look as they do; scheduling policy; the timer. |
| [KERNEL-SPEC.md](KERNEL-SPEC.md) | The precise kernel: objects, system calls, messages, rules, errors, constants, invariants. |
| [INIT.md](INIT.md) | After the kernel: init, the boot manifest, the steward, keyd, sshd, restarts, the startup block; the worked example. |
| [NAMESPACES.md](NAMESPACES.md) | 9P, per-process namespaces, `/dev/cons`, `/net`, filesystem servers, littlefs. |
| [WIRE.md](WIRE.md) | Byte layouts: 9P's encoding for every message, strict JSON for files people write. |
| [PACKAGES.md](PACKAGES.md) | Launching and the loader stub, what is signed, signer trust, per-principal packages, system updates. |
| [IO-ARCHITECTURE.md](IO-ARCHITECTURE.md) | Drivers (virtio), DMA, storage and network stacks; the "Later" designs. |
| [PLATFORM-FPGA.md](PLATFORM-FPGA.md) | The FPGA target, its trust assumptions, what its hardware changes. |
| [PLAN.md](PLAN.md) | The three milestones, the milestone 1 slice and attack suite, what comes after. |

**Built today**

| Note | What it owns |
| --- | --- |
| [STATUS.md](STATUS.md) | Where the code stands against each tenet; the test cases. |
| [BOOT.md](BOOT.md) | Firmware, hardware abstraction, loader, bundle, kernel argument block, the hart timer today. |
| [VERIFIED-BOOT.md](VERIFIED-BOOT.md) | The bundle signature and its container. |
| [DEVICE-GRANTS.md](DEVICE-GRANTS.md) | Today's device authority (interim, until device handles). |
| [MEMORY-LAYOUT.md](MEMORY-LAYOUT.md) | Physmap, address-space split, Sv32 vs Sv39, accepted trade-offs. |
| [DEBUGGING.md](DEBUGGING.md) | No debugger in the kernel; QEMU gdb now, a gated userspace debugger later. |
| [HISTORY.md](HISTORY.md) | What was done, bugs found, review rounds. |

## Servers
Canonical names, used in every note. Class: `system` runs before everyone else (RESOURCES.md).

| Name | Role | Class | State |
| --- | --- | --- | --- |
| `init` | Holds all authority at boot; starts, wires and restarts every OS process | system | designed |
| `consoled` | ns16550 UART driver | system | exists as test programs |
| `bootfsd` | Read-only 9P server over the verified boot bundle (`/boot`) | system | designed |
| `blkd` | virtio-blk driver, partition table, block-range handles | system | designed |
| `fsd` | littlefs filesystem, one instance per volume (e.g. `fsd:data`), serves 9P | system | designed |
| `netd` | virtio-net driver | system | designed |
| `ipd` | smoltcp IP stack, one instance per network (e.g. `ipd:lan`), serves `/net` | system | designed |
| `keyd` | Holds every private key; signs, never exports | system | designed |
| `steward` | Principals, authentication, sessions, the powerbox, launching, audit file; packages from milestone 2 | system | designed |
| `sshd` | SSH front door (`sunset`) and the approval sessions | system | designed |
| `gatewayd` | LLM gateway | system | milestone 3 |
| `webd`, `linkd`, `routerd` | browser GUI; VLAN/filter/QoS; router | system | Later |

Sessions and agents are beamlet VMs in user budgets, not servers.

## Glossary
- **TCB** (trusted computing base): the code whose bugs can break the security guarantees; everything
  else is contained by it.
- **Hart**: a RISC-V hardware thread. **XLEN**: the register width (32 or 64 bits).
- **SBI** (Supervisor Binary Interface): the RISC-V firmware call interface. Extensions used:
  **HSM** (hart start/stop), **SRST** (reset/power-off), TIME, IPI, RFENCE (remote TLB flush).
- **PLIC** (Platform-Level Interrupt Controller); **AIA** (its successor); **CLINT** (core-local
  timer/IPI device); **Sstc** (S-mode timer compare extension).
- **PMP** (Physical Memory Protection): M-mode-programmed physical access rules per hart.
- **DTB** (device tree blob): the machine description the firmware passes to the loader.
- **Sv32 / Sv39**: the RISC-V 32-bit (2-level) and 64-bit (3-level) page-table formats. **ASID**:
  the address-space tag in `satp` (Redoubt uses the PID). **RSW**: PTE bits reserved for software.
  **SUM**: the `sstatus` bit that would let S-mode touch user pages (kept clear).
- **physmap**: the kernel's single mapping of all RAM (MEMORY-LAYOUT.md).
- **W^X**: no page is ever writable and executable at once.
- **Capability feature**: a Cargo feature selecting a hardware backend (`sbi`, `plic`, `sstc`).
  **Board feature**: a feature that only composes capability features (`qemu-virt`). (BOOT.md)
- **RTL** (register-transfer level): the hardware description of the FPGA design.
- **DMA**: devices reading and writing memory directly. **IOMMU**: translates and checks device DMA
  addresses. **IOPMP**: simpler per-bus-master physical windows.
- **AEAD** (authenticated encryption with associated data): encryption that also detects tampering.
- **Handle**: an index into a process's kernel-held capability table; unforgeable, meaningless in any
  other process. **Capability**: the authority a handle carries. **Badge**: an unforgeable tag a
  server attaches when minting, telling it which grant a request came through (badge 0 is an
  endpoint's receive right). **Stamp**: the budget a handle is revoked with. **Device object**: a
  kernel object for an MMIO range (with a DMA flag), an interrupt, or the reset right.
- **Endpoint**: the kernel object clients call; it outlives the server process receiving on it.
- **call / send**: the two IPC primitives (CAPABILITIES.md). **Lend**: map a buffer into the
  receiver for the length of a call. **Transfer**: give pages to the receiver for good.
- **Budget**: a kernel container every process lives in; it pays for and bounds everything, carries
  labels, a deadline and an account (RESOURCES.md). **Lease**: a budget with a deadline.
  **Revocation scope**: a budget with zero limits, used only to be destroyed. **Account**: a 64-bit
  number on a principal's top budget, inherited below it and carried by every message; the unit of
  admission and crash blame. **Pass / stride**: the per-budget counters of stride scheduling.
- **Exit notice**: the one message a process's creator receives when it exits, faults or is killed.
- **Principal**: an accountable identity (a human, an agent, or a project). **Sponsor**: the
  principal accountable for another. **Session**: processes started from a principal's
  capabilities. **Vault session**: `ssh alice+X@box`, a session carrying exactly the label
  `alice-X` (CONTAINMENT.md).
- **Steward**: the Rust server holding principals, sessions, packages and the powerbox (INIT.md).
- **Powerbox**: the steward's escalation service; grants authority after an out-of-band approval.
  **Approval session**: `ssh approve@box`, where only the steward talks (from milestone 2 also the
  physical **console**, the UART on the board). **Approver credential** (Later): a FIDO security key
  used as an SSH `sk-` key with user verification, for `ssh approve-hs@box`.
- **Label**: an information-flow tag on budgets and volumes (CONTAINMENT.md); the approach is
  **DIFC** (decentralized information flow control). **Sink**: a server whose output leaves a
  principal or the machine. **Declassify**: the label owner releasing one item.
- **Manifest**: (1) the **boot manifest**, strict JSON in the boot bundle: servers, devices,
  budgets, volumes, labels and (milestone 1) principals (INIT.md); (2) a **package manifest**, the
  contents and requested capabilities of a package (PACKAGES.md). Today's bundle has instead a
  `grants` entry (DEVICE-GRANTS.md). **Startup block**: the page a parent writes for a new process:
  its namespace table, service handles, arguments (INIT.md).
- **Loader stub**: the small, system-signed flat binary every process starts as; it parses and maps
  its own ELF (PACKAGES.md). Not the S-mode boot loader.
- **Milestone**: one of the three build goals in PLAN.md (1: separation and containment; 2: install,
  share, persist; 3: self-hosted development).
- **Profile**: a principal's chosen package versions, kept by the steward (PACKAGES.md). **Trust
  list**: the signing keys whose code a principal runs.
- **9P** (9P2000): Plan 9's file protocol. **fid**: a 9P handle to a file within one connection.
  **qid**: 9P's file identity and version.
- **beamlet**: the safe-Rust BEAM VM running the Elixir userland (sibling repository). **IEx**:
  Elixir's interactive shell. **OTP**: Erlang's standard library. **NIF**: a BEAM native function.
- **LPM** (longest-prefix match): IP route lookup. **VLAN** (802.1Q): tagged virtual LANs.
- **virtio**: the standard virtual-device interface (virtio-mmio, virtio-blk, virtio-net).
- **SIMT**: single instruction, multiple threads (the GPU's execution model).
