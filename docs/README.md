# Redoubt design notes

Start here. [TENETS.md](TENETS.md) outranks everything else; each other note owns one topic and
links to the rest instead of repeating it. Design v4 is frozen for milestone 1: changes need a
reason recorded in HISTORY.md.

## Built today vs designed
**Built:** a width-generic (rv32 and rv64) microkernel forked from Xous that boots on QEMU `virt`
under OpenSBI or RustSBI; a loader that verifies an Ed25519-signed boot bundle and builds Sv32/Sv39
address spaces; W^X enforced and checked at boot; default-deny device grants; physical RAM never
nameable by address; a two-hart SMP spike behind the `smp` feature; a QEMU test bench with attack
tests. Milestone 1 so far (BUILD-PLAN.md): budgets and handle tables, endpoints and messages, and
device objects and interrupts are built on the new `redoubt-sys` call path beside the legacy
interface (WP-K1 to WP-K3); `redoubt-rt`, the runtime and shared server library; `keyd`; the wire
codecs `redoubt-wire` and their generator; littlefs in pure Rust; the bundle signing domain; the
bench's SSH sessions, virtio disk and network, and attack verdicts taken from the system; fixes for
three kernel panics reachable from any process (WP-K0). Process creation and exit, the timer and
preemption, `init` and the servers, and beamlet's Redoubt platform are in progress or designed. In
the sibling beamlet repository (`userland/otp`): a BEAM VM that runs Elixir, its compiler and IEx.
See [STATUS.md](STATUS.md).
**Designed, not built:** init and the steward, 9P namespaces, packages, storage and network
servers, and the milestone-1 attack suite. The **use case and threat model** (TENETS.md) and the
**game** (GAME.md) are stated; **confinement** (the `confined` manifest flag, one server instance per
trust domain, no shared read-down) is designed and lands with WP-R3, WP-D2, WP-D3 and WP-S2. Order:
[PLAN.md](PLAN.md).

## Reading order
**The design**

| Note | What it owns |
| --- | --- |
| [TENETS.md](TENETS.md) | What the project believes — outranks everything; the use case, the high/low pair, the seven tenets. |
| [CAPABILITIES.md](CAPABILITIES.md) | Why handles; how IPC and minting are used; revocation policy; principals, agents, projects; the powerbox and approvals. |
| [CONTAINMENT.md](CONTAINMENT.md) | Labels, sessions and vaults, declassification, the shared server library, crash blame, covert and timing channels, the executable model. |
| [RESOURCES.md](RESOURCES.md) | Why budgets look as they do; scheduling policy; the timer. |
| [KERNEL-SPEC.md](KERNEL-SPEC.md) | The precise kernel: objects and their costs, system calls, messages, rules, errors and the order of checks, constants, invariants. |
| [INIT.md](INIT.md) | After the kernel: init, the boot manifest, the steward, keyd, sshd, restarts, the startup block; the worked example. |
| [NAMESPACES.md](NAMESPACES.md) | 9P, per-process namespaces, `/dev/cons`, `/net`, filesystem servers, littlefs. |
| [USERLAND.md](USERLAND.md) | The Elixir interface: beamlet's natives, file I/O, pipes, launching, the shell. |
| [WIRE.md](WIRE.md) | Byte layouts: 9P's encoding for every message, the typed-message table format and replies, strict JSON for files people write. |
| [PACKAGES.md](PACKAGES.md) | Launching and the loader stub, what is signed, signer trust, per-principal packages, system updates. |
| [IO-ARCHITECTURE.md](IO-ARCHITECTURE.md) | Drivers (virtio), DMA, storage and network stacks; the "Later" designs. |
| [PLATFORM-FPGA.md](PLATFORM-FPGA.md) | The FPGA target, its trust assumptions, what its hardware changes. |
| [PLAN.md](PLAN.md) | The three milestones, the milestone 1 slice and attack suite, what comes after. |
| [BUILD-PLAN.md](BUILD-PLAN.md) | Milestone 1 as work packages: what each reads, delivers and must pass; order; hotspots. |
| [SWARM.md](SWARM.md) | How the build runs: one orchestrator, parallel packages in worktrees, review, merge, claims. |
| [GAME.md](GAME.md) | The adversarial-agent game: scenarios (single escape, the high/low pair, authority expansion, the human), setup, win conditions, the design-hole vs implementation-bug verdict. |

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
Canonical names, used in every note. Class: `system` is a trust class (the kernel's label checks),
never a scheduling one; every budget shares one stride queue by weight (RESOURCES.md).

| Name | Role | Class | State |
| --- | --- | --- | --- |
| `init` | Holds all authority at boot; starts, wires and restarts every OS process | system | designed |
| `consoled` | ns16550 UART driver | system | exists as test programs |
| `bootfsd` | Read-only 9P server over the verified boot bundle (`/boot`) | system | designed |
| `blkd` | virtio-blk driver, partition table, block-range handles | system | designed |
| `fsd` | littlefs filesystem, one instance per volume (e.g. `fsd:data`), serves 9P | system | designed |
| `netd` | virtio-net driver | system | designed |
| `ipd` | smoltcp IP stack, one instance per network (e.g. `ipd:lan`), serves `/net` | system | designed |
| `keyd` | Holds every private key; signs, never exports | system | built |
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
  receiver for the length of a call. **Transfer**: give pages to the receiver for good. **Open
  call**: a call a server has taken and not yet replied to. **Current call**: the open call a
  thread is working on (the one it took last, or named with `serve`); whom a crash blames
  (KERNEL-SPEC.md). **Abandoned call**: an open call whose caller died, timed out or was revoked;
  the server is told and replies to free it.
- **Budget**: a kernel container every process lives in; it pays for and bounds everything, carries
  labels, a deadline and an account (RESOURCES.md). **Lease**: a budget with a deadline, at most
  `MAX_LEASE` (24 h, a steward constant). **Revocation scope**: a budget with zero limits, used only
  to be destroyed. **Class**: `system` or `user`, a budget's trust class (R1's exemption,
  `budget_usage`, adding labels), never its place in the queue: `init`, the steward and the drivers
  are scheduled by their large manifest weights like everyone else (RESOURCES.md).
  **Account**: a 64-bit number on a principal's top budget, inherited below it and carried by every
  message; with the label set, the unit of admission and crash blame (CONTAINMENT.md). **Pass /
  stride**: the per-budget counters of stride scheduling.
- **Exit notice**: the one message a process's creator receives when it exits, faults or is killed;
  a fault names the blamed account and labels. It lives in the process object, which is charged to
  the creator and outlives the process until the notice is received, so the notice never allocates
  (KERNEL-SPEC.md). **Connection id**: the random id a server returns with a new connection; only
  its holder can `disconnect` it, freeing the connection and everything minted under it
  (NAMESPACES.md).
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
  **DIFC** (decentralized information flow control). **Trust domain**: a label set and everything
  carrying it; the OS's isolation unit — capabilities bound authority, labels bound flow, so two
  budgets with equal label sets are one domain (TENETS.md, The use case). **Sink**: a server whose
  output leaves a principal or the machine. **Declassify**: the label owner releasing one item. **Reader budget**:
  a short-lived budget the steward creates with exactly an item's labels, to read it for
  declassification (the steward itself stays unlabelled).
- **Manifest**: (1) the **boot manifest**, strict JSON in the boot bundle: servers, devices,
  budgets, volumes, labels and (milestone 1) principals (INIT.md); (2) a **package manifest**, the
  contents and requested capabilities of a package (PACKAGES.md). Today's bundle has instead a
  `grants` entry (DEVICE-GRANTS.md). **Startup block**: the page a parent writes for a new process,
  whose address `process_start` passes it: its namespace table, named handles, arguments (INIT.md).
- **Loader stub**: the small, system-signed flat binary every process starts as; it parses and maps
  its own ELF (PACKAGES.md). Not the S-mode boot loader.
- **Milestone**: one of the three build goals in PLAN.md (1: separation and containment; 2: install,
  share, persist; 3: self-hosted development).
- **Profile**: a principal's chosen package versions, kept by the steward (PACKAGES.md). **Trust
  list**: the signing keys whose code a principal runs.
- **9P** (9P2000): Plan 9's file protocol. **fid**: a 9P handle to a file within one connection.
  **qid**: 9P's file identity and version. **`Malformed`**: reply status 1, in every protocol and in
  9P calls: the request did not decode (WIRE.md).
- **beamlet**: the safe-Rust BEAM VM running the Elixir userland (sibling repository). **IEx**:
  Elixir's interactive shell. **OTP**: Erlang's standard library. **NIF**: a BEAM native function.
- **`redoubt-sys` / `redoubt-abi`:** `redoubt-sys` (libs/sys/) is the new Redoubt syscall ABI
  (call numbers, register encodings, errors, WP-A1); the kernel's Redoubt call path uses it.
  `redoubt-abi` (libs/abi/) is the legacy Xous ABI — memory layout constants, process structures,
  the old syscall interface — still used by the kernel's RISC-V arch layer and the loader.
  **WP-K6 removes the legacy interface and resolves the split.**
- **virtio**: the standard virtual-device interface (virtio-mmio, virtio-blk, virtio-net).
- **SIMT**: single instruction, multiple threads (the GPU's execution model).
