# Redoubt

A small, auditable **RISC-V microkernel** in pure Rust, plus the **runtimes** that run on it.
Today the runtime is **beamlet**, a safe-Rust **BEAM (Erlang/Elixir) VM**. The OS is runtime-neutral,
so other runtimes can live beside it.

Redoubt is a hard fork of [Xous](https://github.com/betrusted-io/xous-core), rebuilt for RV32 and
RV64 on one width-generic code path and aimed at a specific threat: a capable adversary that has
read every line of the source and controls any code it is allowed to run. The whole design rests on
**no ambient authority** — a process can touch only what it was explicitly given — enforced by a
tiny kernel with capabilities, budgets and information-flow labels. Everything else is an
unprivileged server in Rust or Elixir.

**Status legend used throughout this page**

| Mark | Meaning |
| --- | --- |
| 🟢 | **Built** — in this tree and exercised by `cargo testbench` |
| 🟡 | **In progress** — a work package is building or in review ([SWARM.md](docs/SWARM.md)) |
| 🔴 | **Designed, not built** — specified in [`docs/`](docs/README.md), no code yet |
| ⚪ | **Deferred** — designed but deliberately later |

> **Read this page top to bottom for the tour.** Each section links to the owning design note
> (`docs/…`) and the real code. The authoritative status is [`docs/STATUS.md`](docs/STATUS.md) and
> [`docs/HISTORY.md`](docs/HISTORY.md); the frozen design is [`docs/`](docs/README.md).

---

## 🟢 What runs today, in one glance

```
  boots on QEMU virt, rv64 + rv32, under OpenSBI and RustSBI
  kernel: memory · threads · handles & endpoints (call/send/receive/reply/serve/mint)
          budgets · device objects · IRQ receive · timers · verified boot · W^X
  userland: beamlet BEAM VM — runs Elixir, its compiler, IEx, OTP crypto/ssl/ssh
  servers:  keyd (keys that sign, never export)
  harness:  cargo testbench — 55 cases, ~96 results, attack cases with system verdicts
```

```
   🟢 BUILT                                      🔴 DESIGNED, NOT BUILT
   ────────────────────────────────────────     ─────────────────────────────────────────
   bios/ (RustSBI) · loader/ (both widths)      init + boot manifest + loader stub
   kernel/ (memory, IPC, budgets, devices)      steward · sshd · fsd · blkd · netd · ipd
   libs/{sys,rt,wire,signing,littlefs,paging}   bootfsd · consoled
   servers/keyd · tools/testbench               packages · projects · sharing · A/B updates
   userland/otp (beamlet)                       FPGA cards · disk encryption · gatewayd · webd
```

---

## The system at a glance

```
┌──────────────────────────────────────────────────────────────────────────────────────────┐
│                                   U-mode (userspace)                                        │
│                                                                                            │
│   sessions & agents                    system servers                  runtime             │
│   ┌────────────────────┐         ┌──────────────────────────┐   ┌────────────────────┐   │
│   │  beamlet VMs        │         │ steward  keyd  sshd       │   │  beamlet VM         │   │
│   │  IEx · OTP · Elixir │         │ fsd  blkd  netd  ipd      │   │  BEAM bytecode      │   │
│   │  (one VM = one      │         │ bootfsd  consoled         │   │  OTP stdlib         │   │
│   │   trust domain)     │         │ (unprivileged, via IPC)   │   │  + crypto/re in Rust │   │
│   └─────────┬──────────┘         └────────────┬─────────────┘   └─────────┬──────────┘   │
│             │                                 │                           │              │
│             └────────── IPC: call / send, 9P, handles (no kernel queue) ───┘              │
│                                      │                                                     │
├──────────────────────────────────────┼─────────────────────────────────────────────────────┤
│                      syscall ABI  (libs/sys)                                               │
├──────────────────────────────────────┼─────────────────────────────────────────────────────┤
│   KERNEL (S-mode)                    ▼                            the TCB, ~14k lines       │
│   ┌────────────────────────────────────────────────────────────────────────────────────┐   │
│   │  scheduler (one stride queue)        handle tables (capabilities)                    │   │
│   │  memory (physmap, Sv32/Sv39)          endpoints + messages (call/send/receive/reply) │   │
│   │  IPC + mint + revocation              budgets (pages, processes, weight, labels)      │   │
│   │  interrupts (received, not handled)   device objects (MMIO/DMA, IRQ, Reset)           │   │
│   │  timer (owned)                        CSPRNG (seeded at boot)                          │   │
│   └────────────────────────────────────────────────────────────────────────────────────┘   │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│   LOADER (S-mode)   verify Ed25519 bundle · build address spaces · arg block               │
│   loader/src/main.rs                                                   │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│   BIOS / firmware (M-mode)   OpenSBI (QEMU) or RustSBI Prototyper  (bios/)              │
├───────────────────────────────────────────────────────────────────────────────────────────┤
│   HARDWARE   QEMU `virt` rv64 & rv32 🟢        FPGA cards (Sv39, 32 hw threads) 🔴         │
└───────────────────────────────────────────────────────────────────────────────────────────┘
```

A term to hold onto: a **budget** is a kernel container every process lives in. It is the unit of
accounting, CPU share, revocation, **information-flow labels**, and identity (its *account* travels
with every message). One object replaces five mechanisms. → [`docs/RESOURCES.md`](docs/RESOURCES.md),
[`docs/CONTAINMENT.md`](docs/CONTAINMENT.md).

---

## 🟢 Boot flow

```
  power-on
     │
     ▼
┌──────────────┐   M-mode
│ ROM / BIOS   │   OpenSBI (QEMU rv64)  ·  RustSBI Prototyper (rv32 always; rv64 optional)
│  firmware    │   [bios/, scripts/build-bios.sh]
└──────┬───────┘
       │  -kernel loader , -initrd bundle
       ▼
┌──────────────┐   S-mode, MMU off
│   LOADER     │   1. read device tree (RAM, MMIO, PLIC, rng-seed, timebase)
│              │   2. VERIFY the bundle signature  ── fail ⇒ power off
│              │   3. allocate + build Sv32/Sv39 address spaces per process
│              │   4. write the kernel argument block (XArg, MREx, Plic, Seed, …)
└──────┬───────┘
       │  a0=args a1=process-table
       ▼
┌──────────────┐   S-mode, Sv32/Sv39 on
│   KERNEL     │   creates root/system/users budgets, hands devices to init
└──────┬───────┘
       ▼
┌──────────────┐   U-mode
│   init 🔴    │   starts servers from the boot manifest, then hands off to the steward
└──────────────┘
```

- **Built:** firmware selection, loader both widths, signature verification, W^X, default-deny
  grants. → [`docs/BOOT.md`](docs/BOOT.md), [`docs/VERIFIED-BOOT.md`](docs/VERIFIED-BOOT.md),
  [`docs/MEMORY-LAYOUT.md`](docs/MEMORY-LAYOUT.md).
- **Signed, not encrypted.** The initrd is `signature(64) ‖ tar`; the signature covers
  `"redoubt.bundle.v1\0" ‖ u64_le(len) ‖ tar`. One crate builds the preimage for both sides, so the
  loader and the signer cannot drift. → [`libs/signing/src/lib.rs`](libs/signing/src/lib.rs),
  [`loader/src/verify.rs`](loader/src/verify.rs).
- **🔴 Designed change:** the loader will load only `kernel` + `init`; `init` launches every other
  process through a system-signed **loader stub** from the bundle's pages. → [`docs/PACKAGES.md`](docs/PACKAGES.md).

---

## Hardware supported

| Target | Width | Firmware | Status |
| --- | --- | --- | --- |
| QEMU `virt` | RV64 (Sv39) | OpenSBI (bundled) or [RustSBI Prototyper](bios/) | 🟢 booted in the bench |
| QEMU `virt` | RV32 (Sv32) | RustSBI Prototyper (QEMU ships no rv32 OpenSBI) | 🟢 booted in the bench |
| FPGA PCIe cards (XC7K480T) | RV64GC Sv39, 8 cores × 4 hw threads | RustSBI / OpenSBI | 🔴 planned hardware, RTL DMA confinement |
| Messy SoCs (e.g. Orange Pi RV2) | RV64 | OpenSBI **domains**, Linux on reserved cores | ⚪ deferred; Linux is in the TCB there |

Hardware differences are **capability features** (`sbi`, `plic`) composed by **board features**
(`qemu-virt`), never `target_arch` checks. RAM, MMIO and the interrupt controller are discovered
from the device tree. → [`docs/BOOT.md`](docs/BOOT.md), [`docs/IO-ARCHITECTURE.md`](docs/IO-ARCHITECTURE.md),
[`docs/PLATFORM-FPGA.md`](docs/PLATFORM-FPGA.md).

---

## The kernel — objects and calls

```
                         a process
        ┌──────────────────────────────────────────────┐
        │  handle table  (index → object, badge, stamp) │  MAX_HANDLES = 4096
        │  ┌────┬────┬────┬────┬────┬────┬────┐          │
        │  │ 0  │ h1 │ h2 │ h3 │ …  │    │    │  0 = none │
        │  └────┴────┴────┴────┴────┴────┴────┘          │
        │  open calls (≤64) · current call · exit endpoint│
        └──────────────────────────────────────────────┘
                 │ handle                        ▲ message
                 ▼                               │ (badge, account, labels, id)
        ┌──────────────────────┐       ┌──────────────────────┐
        │  BUDGET (container)  │       │  ENDPOINT (clients→) │
        │  pages processes     │       │  survives its server │
        │  weight class labels │       │  fair waiting (R2)   │
        │  account deadline    │       └──────────────────────┘
        └──────────────────────┘
        ┌──────────────┐  ┌──────────────┐  ┌──────────────┐
        │ MMIO (+DMA)  │  │    IRQ       │  │   Reset      │
        └──────────────┘  └──────────────┘  └──────────────┘
```

| System call | What it does | Status |
| --- | --- | --- |
| `map_anon` `unmap` `set_flags` `map_device` `dma_alloc` | memory, W^X, DMA | 🟢 |
| `thread_create` `thread_exit` | threads | 🟢 |
| `endpoint_create` `mint` | capabilities | 🟢 |
| `call` `send` `receive` `reply` `serve` | zero-copy IPC, lend/transfer | 🟢 |
| `budget_create` `budget_destroy` `budget_usage` | accounting, revocation, labels | 🟢 |
| `time_now` `random` `system_reset` | services | 🟢 |
| `process_create` `process_map` `process_start` `process_exit` | launch + exit notices | 🟡 |

</br>

```
   IPC in one picture (libs/sys ↔ kernel/src/syscall.rs):

     client ── call(handle, lend buffer) ─► ENDPOINT ──► server receive()
       ▲                                                     │
       │  reply(msg_id)                                      │ serve(msg_id)
       └──────────────── reply + lend returned ◄─────────────┘
   no kernel queue: a message is queued exactly while its sender is blocked.
   borrow dies / times out ⇒ abandoned-call notice ⇒ server replies ⇒ pages freed.
```

- The **precise spec** is [`docs/KERNEL-SPEC.md`](docs/KERNEL-SPEC.md): constants, the cost table,
  rules R1–R12, invariants I1–I15, errors and the *order of checks*. It is frozen for milestone 1.
- Implementation: [`kernel/src/main.rs`](kernel/src/main.rs) (boot/main loop),
  [`kernel/src/syscall.rs`](kernel/src/syscall.rs) (the call table),
  [`kernel/src/services.rs`](kernel/src/services.rs) (objects, budgets, IPC),
  [`kernel/src/mem.rs`](kernel/src/mem.rs), [`kernel/src/redoubt.rs`](kernel/src/redoubt.rs).
- Page tables (the only code that edits PTEs): [`libs/paging/src/lib.rs`](libs/paging/src/lib.rs).

---

## Servers

Every server is an unprivileged process in its own budget, talking over IPC/9P. Its authority is a
set of handles, never ambient. The roster (canonical names): → [`docs/README.md`](docs/README.md)

| Server | Role | Status |
| --- | --- | --- |
| `init` | holds all authority at boot; starts/wires/restarts every OS process | 🔴 |
| `consoled` | ns16550 UART driver, serves `/dev/cons` | 🟡 (UART exists today only in test programs) |
| `bootfsd` | read-only 9P over the verified bundle (`/boot`) | 🟡 |
| `blkd` | virtio-blk driver + partitions + block-range handles | 🟡 |
| `fsd` | littlefs, one instance per volume, serves 9P | 🔴 |
| `netd` | virtio-net driver | 🟡 |
| `ipd` | smoltcp IP stack, serves `/net` | 🟡 |
| `keyd` | holds every private key; signs, never exports | 🟢 [servers/keyd/src/lib.rs](servers/keyd/src/lib.rs) |
| `steward` | principals, sessions, the powerbox, launching, audit | 🔴 |
| `sshd` | SSH front door (`sunset`) and approval sessions | 🔴 |
| `gatewayd`, `webd`, `linkd`, `routerd` | LLM gateway; browser; VLAN/QoS; router | ⚪ |

```
   keyd's rule, in one line:  a badge names ONE key and ONE purpose
   ┌──────────────────────────────────────────────────────────────────┐
   │ sign_ssh_exchange │ sign_record │ public_key │ holds │ grant │ release│
   │           every signature is over a 32-byte digest KEYD COMPUTED  │
   │           there is no operation that returns a private key        │
   └──────────────────────────────────────────────────────────────────┘
```

Sessions and agents are **beamlet VMs in user budgets**, not servers. A session that crashes a
shared server three times (same account + label set) is logged out for 10 minutes; the verdict is
taken from the kernel's exit notice, never the attacker's output.
→ [`docs/INIT.md`](docs/INIT.md), [`docs/CONTAINMENT.md`](docs/CONTAINMENT.md).

---

## Userland: beamlet (the OTP runtime)

```
   Elixir / OTP stdlib / IEx
            │  File · IO · GenServer · :ssl · :ssh   (unchanged OTP code)
   ┌────────▼────────────────────────────────────────────────────────┐
   │  beamlet VM  —  per-process heaps, copying GC, several schedulers │
   │  userland/otp/vm/src/lib.rs         │
   │  natives: crypto (RustCrypto) · re · zlib · prim_file · console   │
   └────────┬──────────────────────────────────────────────────────────┘
            │  Platform trait: ONE asynchronous 9P client
            ▼
   Redoubt:  files · /net · /dev/cons  are namespace walks (resource terms)
```

- **🟢 Built and tested against the real BEAM** by differential tests: each `tests/<suite>/*.erl`
  runs on OTP and on beamlet and the output must be identical. OTP's `crypto`, `public_key`, `ssl`
  and `ssh` run unmodified. → [`userland/otp/DESIGN.md`](userland/otp/DESIGN.md).
- **🔴 On Redoubt:** beamlet's `Platform` is not yet retargeted to the Redoubt 9P client. That is
  WP-B1/WP-B2 (console, files, launching; then IEx on the UART). → [`docs/USERLAND.md`](docs/USERLAND.md).
- One VM = one trust domain. `spawn` stays inside the VM; a new trust domain is a new VM under a new
  budget (the steward's job).

---

## The security model in one diagram

```
   authority  ────────────────────────────────────────────── information
   (what you can do)                                         (what you can leak)

   CAPABILITY                          BUDGET                         LABEL
   handle = (object, badge, stamp)     container, charged in pages    64-bit name, fixed at
   unforgeable, revocable by stamp     CPU weight, account, deadline  budget creation
        │                                    │                             │
        └── attenuation = server mints a narrower badge                     │
        └── revocation  = destroy the budget the stamp names               │
                                            │                               │
                              ┌─────────────┴───────────────┐               │
                              │  a flow A→B is allowed iff  │◄──────────────┘
                              │  B is system-class          │
                              │  OR labels(B) ⊇ labels(A)   │   reads above your labels FAIL
                              └─────────────────────────────┘   writes need EQUAL labels
```

- **No ambient authority:** devices default-deny; physical RAM is never nameable by address; pages
  are zeroed; the loader verifies the bundle. → [`docs/DEVICE-GRANTS.md`](docs/DEVICE-GRANTS.md).
- **Agents:** own principal, accountable sponsor, task-scoped **leases** (budgets with a deadline),
  delegation only narrows, assume always compromised. A hijacked agent can run code it wrote, but
  never with more authority than it already had.
- **Approvals are out of band** (like 2FA): only the steward talks to `ssh approve@box`; nothing
  the requester runs can draw, type or listen there.
- → [`docs/CAPABILITIES.md`](docs/CAPABILITIES.md), [`docs/CONTAINMENT.md`](docs/CONTAINMENT.md),
  [`docs/TENETS.md`](docs/TENETS.md) (outranks everything).

---

## Getting started

Everything runs in a self-contained Docker container; the host needs only Docker.

### 1. Toolchain

```sh
./dev.sh                 # build the image (first time), then a shell in /work
./dev.sh --rebuild       # rebuild the image after editing the Dockerfile
```

This installs Rust with the RISC-V bare-metal targets (`riscv64imac`, `riscv32imac`, `riscv64gc`),
QEMU for both widths, OpenSSH, and the agent CLIs. The pinned OTP 28 / Elixir 1.20 toolchains live
in [`toolchains/`](toolchains/) and are used only by beamlet's tests.

Prefer your own environment? Install Rust + the three targets above, `qemu-system-misc`, and a C
toolchain; then work directly in the repo.

### 2. Build the firmware (once)

```sh
./scripts/build-bios.sh      # builds the vendored RustSBI in bios/ for both widths
```

Needed only for rv32; rv64 defaults to QEMU's bundled OpenSBI. `bios/target/` may already be built.

### 3. Build the OS

```sh
./build --arch rv64          # kernel + loader → target/<triple>/release/
./build --arch rv32
./build --arch rv64 --programs   # also the in-guest test programs
```

### 4. Launch it in a VM

```sh
./launch --arch rv64                       # print the exact QEMU line and boot; Ctrl-A X quits
./launch --arch rv64 --program log-server  # start a test program after the kernel
./launch --arch rv32 --smp 4
./launch --arch rv64 --print-only          # show the QEMU command and exit
./launch --arch rv64 --debug               # pause with a gdb stub on :1234
```

`launch` assembles and signs the boot bundle, prints the exact `qemu-system-riscv*` command line,
and attaches the guest serial console to your stdin/stdout. → [`docs/DEBUGGING.md`](docs/DEBUGGING.md).

### 5. Run the tests

```sh
./test --arch rv64            # the whole bench on rv64
./test --arch rv64 timer      # cases whose name contains "timer"
./test --arch rv32 budget
./test --list                 # list cases
```

`./test` wraps `cargo testbench`. It boots real images under QEMU and asserts on the console,
including adversarial cases (tampered bundles, corrupted ELFs, sycall attacks, SSH sessions,
virtio devices). Console logs land in `target/testbench/`. → [`docs/testbench.md`](docs/testbench.md).

### 6. Build an image

```sh
./mkimage                     # signed boot bundle → target/image/redoubt.bundle
```

The signed `signature ‖ tar` bundle the loader verifies. Disk-image recipes (littlefs) live in
[`image/`](image/) and are 🔴 not yet built. → [`image/README.md`](image/README.md).

### 7. beamlet (the BEAM VM)

```sh
cd userland/otp
. tools/env.sh                # put the pinned OTP/Elixir on PATH
cargo test                    # unit + hostile-input tests
tools/difftest                # differential tests against the real BEAM
tools/elixir-tests            # run Elixir's own suite on beamlet
```

---

## Testing, and why you can trust a green run

```
   tests/<case>.toml   ──►  tools/testbench  ──►  cargo build (kernel, loader, programs)
        │                        │                          │
        │                        │                          ▼
        │                        │              sign bundle ──► QEMU virt ──► boot
        │                        │                                    │
        │                        └────────── assert on console ◄──────┘
        │                                   + SSH sessions + virtio disk/net
        ▼
   attack cases: the verdict comes from the SYSTEM (kernel, a victim, or a clean power-off),
   never from the attacker's own output. `bench-*` cases keep the harness itself honest.
```

- One simple harness, real boots, no kernel mocks. A build of the same sources with debug
  assertions + overflow checks (the `checked` profile) is a normal case option.
- Tenet 6: *if it is not tested, it does not work.* → [`docs/TENETS.md`](docs/TENETS.md).
- 55 cases: IPC, budgets, devices, timers, loader rejections, verified-boot attacks, forged
  verdicts, SSH. → [`tests/`](tests/), [`docs/STATUS.md`](docs/STATUS.md).

---

## Where to start reading

```
   docs/TENETS.md ────────── outranks everything (adversary, review model, non-goals)
        │
        ├─ docs/README.md ───── index + glossary + server roster
        ├─ docs/STATUS.md ───── where the code stands, per tenet
        ├─ docs/PLAN.md ─────── the three milestones
        └─ design ───────────── the "why":
             ├─ docs/CAPABILITIES.md    handles, IPC, minting, principals, approvals
             ├─ docs/CONTAINMENT.md     labels, sessions/vaults, crash blame, the model
             ├─ docs/RESOURCES.md       budgets, stride scheduling, the timer
             ├─ docs/KERNEL-SPEC.md     the precise kernel (frozen for milestone 1)
             ├─ docs/INIT.md            init, the steward, boot manifest, startup block
             ├─ docs/NAMESPACES.md      9P, per-process namespaces, filesystems
             ├─ docs/WIRE.md            byte layouts for every message and file
             ├─ docs/PACKAGES.md        launching, signing, packages, updates
             ├─ docs/IO-ARCHITECTURE.md drivers, DMA, storage, network
             └─ docs/BUILD-PLAN.md      milestone 1 as work packages
```

**Where the code lives**

| Area | Notes | Code |
| --- | --- | --- |
| Boot & firmware | [BOOT.md](docs/BOOT.md), [VERIFIED-BOOT.md](docs/VERIFIED-BOOT.md) | [bios/](bios/), [loader/](loader/), [scripts/build-bios.sh](scripts/build-bios.sh) |
| Kernel | [KERNEL-SPEC.md](docs/KERNEL-SPEC.md), [MEMORY-LAYOUT.md](docs/MEMORY-LAYOUT.md) | [kernel/](kernel/) |
| ABI | KERNEL-SPEC.md | [libs/sys/](libs/sys/), legacy [libs/abi/](libs/abi/) |
| Runtime + server library | [CONTAINMENT.md](docs/CONTAINMENT.md) | [libs/rt/](libs/rt/) |
| Wire codecs + generator | [WIRE.md](docs/WIRE.md) | [libs/wire/](libs/wire/) |
| Page tables | [MEMORY-LAYOUT.md](docs/MEMORY-LAYOUT.md) | [libs/paging/](libs/paging/) |
| Bundle signature | [VERIFIED-BOOT.md](docs/VERIFIED-BOOT.md) | [libs/signing/](libs/signing/) |
| Filesystem | [NAMESPACES.md](docs/NAMESPACES.md) | [libs/littlefs/](libs/littlefs/) |
| Servers | [INIT.md](docs/INIT.md) | [servers/](servers/) |
| Runtime VM | [USERLAND.md](docs/USERLAND.md) | [userland/otp/](userland/otp/) |
| Test bench | [testbench.md](docs/testbench.md) | [tools/testbench/](tools/testbench/), [tests/](tests/) |
| Images | [image/README.md](image/README.md) | [image/](image/) |

**Repo layout**

```
  bios/       M-mode firmware (vendored, pinned RustSBI)      loader/   S-mode boot loader
  kernel/     the microkernel (the TCB)                       servers/  unprivileged servers
  userland/   runtimes — otp/ is beamlet (BEAM)               libs/     runtime-neutral crates
  image/      boot-bundle & disk-image recipes (source)        tools/    testbench, generators
  tests/      bench cases (*.toml) + programs/                 docs/     the design of record
  reference/  AtomVM/Elixir/OTP sources for difftests          toolchains/ pinned OTP 28 / Elixir 1.20
  vendor/     small vendored patches (getrandom)               scripts/  build-bios.sh
  build launch test mkimage   dev.sh  Dockerfile  README.md
```

---

## Milestones

```
  ┌─ M1 🟡 separation and containment ──────────────────────────────────────────┐
  │  Alice and Bob logged in over SSH, separated; Alice's agent under a lease,   │
  │  contained. Every property backed by an attack test.                        │
  │  kernel (K1–K5) 🟡 · init 🔴 · beamlet platform 🔴 · storage/net 🔴 ·        │
  │  steward/keyd/sshd 🟡/🔴 · the attack suite 🔴                               │
  └──────────────────────────────────────────────────────────────────────────────┘
  ┌─ M2 🔴 install, share, persist ─────────────────────────────────────────────┐
  │  packages + trust lists, projects, sharing, reboot memory, A/B updates +     │
  │  rollback.                                                                   │
  └──────────────────────────────────────────────────────────────────────────────┘
  ┌─ M3 🔴 self-hosted development ─────────────────────────────────────────────┐
  │  a real agent harness over gatewayd; compilers on the box; server APIs.      │
  └──────────────────────────────────────────────────────────────────────────────┘
  after M3: rv32 returned to the booted dimensions; SMP; FPGA.
```

The milestone-1 work packages, their order and the swarm that builds them:
→ [`docs/BUILD-PLAN.md`](docs/BUILD-PLAN.md), [`docs/SWARM.md`](docs/SWARM.md), [`docs/PLAN.md`](docs/PLAN.md).

---

## Heritage

Redoubt began as **Xous** by the betrusted.io project; the microkernel shape, the syscall ABI and
much of the legacy ABI crate come from there. Redoubt drops Xous's Precursor/Baochip hardware
support and its 32-bit-only, single-core, PDDB-centric assumptions, and takes the design 64-bit,
SMP-ready and filesystem-bearing. What changed and why: [`docs/HISTORY.md`](docs/HISTORY.md).

*Licensed under the terms in [`LICENSE`](LICENSE) / [`LICENSES/`](LICENSES/).*