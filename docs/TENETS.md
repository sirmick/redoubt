# Tenets

These outrank every other note. When a change conflicts with a tenet, either the change loses or
the tenet is amended here first, with the reason written down. Terms: [README.md](README.md).

**Design v4 is frozen for milestone 1:** a change to it needs a stated reason, recorded in
HISTORY.md.

## The use case
Redoubt exists to run **untrusted, potentially hostile agents** — frontier models, and the code they
write — on one machine, alongside their human principals, without letting an agent escape its grants,
reach another agent, or reach a network it was not given. The design assumes the agent is the best
available: it has read every line of this repository and the running system, and it is trying.

- **The escape room** (GAME.md, PLAN.md): several real agents, each its own principal with a distinct
  label set and budget, full source, told they coexist, tasked to break out, to expand their
  authority, and to collude.
- **Everyday co-existence:** a human's own agents doing useful work under leases, visible to and
  stoppable by the human.

This is a **containment** boundary, not a prevention one. Any agent is assumed to compromise its own
VM (the BEAM is not in the TCB; INIT.md), and possibly a server it talks to. The design guarantees:

1. **Capability closure.** An agent's reachable authority never exceeds its initial grants plus what
   a human-approved steward action adds; every delegation step narrows (R9, `mint`; CAPABILITIES.md).
   Authority is monotone non-increasing absent a human approval.
2. **Label non-interference, in software.** No *intentional* path crosses a label boundary: not a
   `call` or `send` (R1), a write (`check`), a read up, a sink, metadata or a counter. Data crosses
   only by steward declassification. This is a property of the OS, it is by construction, and it is
   testable. The **label set, not the capability set, is the isolation unit**; two budgets with
   different label sets have no path the OS can carry a message over.
3. **Human control.** Every increase in authority, and every declassification, is an out-of-band human
   approval; leases bound time; the sponsor can always end a lease (CAPABILITIES.md).

It does **not** guarantee that two agents cannot signal each other *covertly*, because on one machine
that is impossible (The high/low pair). Collusion is a threat the design bounds, not a goal it meets.

### The high/low pair
The sharpest case: a labelled agent with secrets and no network (**A**) and an unlabelled agent with
network (**B**) are colluding, having agreed a code in advance. Labels remove every *intentional*
path between them — `call`/`send` (R1), writes (`check`), sinks, metadata, global counters, the
approval screen. They do **not** remove *covert* ones, and **on one machine those cannot be reduced
to zero.** The design is honest about the difference:

- **Collusion cannot be prevented; it can be bounded.** Two processes on one die always share physical
  state: at minimum power delivery, heat and electromagnetic emission, and usually a clock (DVFS
  couples a workload on one core to timing on another). Even a perfect RTL for every enumerated
  on-die resource leaves a physical substrate. A pre-agreed code removes all negotiation cost, so the
  smallest channel carries meaning: **one bit can be a key**. The goal is not silence; it is a
  **known, small, measured** channel.
- **The OS's claim is exact and strong: zero intentional paths, by construction.** Everything software
  mediates is closed (CONTAINMENT.md's channel table). That claim is testable, and the bench tests it.
- **Hardware reduces the covert channel; only placement zeroes it.** The RTL closes the enumerated
  resources (cores, caches, memory bandwidth, DMA, disk, NIC, GPU); power, heat, EM and the clock
  remain and cannot be closed in software or in RTL. The only true zero is not co-locating the secret
  with the domain that wants it — separate power/thermal domains, separate machines. That is a
  deployment decision, and the OS cannot substitute for it.
- **Read-down is an intentional path, closed for a confined domain.** A labelled session reading an
  unlabelled volume is how data enters a vault, but with a colluding lower domain it is a low-to-high
  path. A confined domain reads no shared unlabelled data; input arrives by an audited push from the
  steward.
- **Sharing is the attack.** Two differing label sets in a confined deployment share no server
  instance, volume, endpoint, network instance or core. A manifest that places them together is
  refused (INIT.md).

Where the hardware cannot yet provide the enumerated closures, the path is a stated residual, and two
domains co-reside only if the residual's capacity is below what the secret is worth. **No
non-interference claim — even software's exact one — should be read as a silence guarantee**, and none
is made on QEMU or general hardware.

## The adversary
Design for a capable, patient, automated adversary that has read every line of this repository, can
generate and test exploit candidates faster than a human can review them, and controls any code it
is allowed to run as an unprivileged process. It may run several such processes that know of each
other and collude; a resource shared across a label boundary is a channel (The use case). Assume it
finds every bug that is findable by reading.

That rules out security through obscurity, through "nobody would try that", or through complexity
that merely slows a human down. What is left: a small trusted base, mechanisms that are correct by
construction, and no ambient authority.

Out of scope for the software, stated so nobody assumes otherwise: physical attacks,
microarchitectural side channels (Spectre-class, cache timing), and malicious hardware. These need
hardware answers; on the FPGA target, side channels are handled in the RTL, and the hardware plan
may change to avoid them (PLATFORM-FPGA.md).

**Timing.** Assume the attacker has a perfect clock: it can count on another core or timestamp
against a machine it controls. Secrets are protected by constant-time code and by not sharing
hardware state between budgets, never by hiding time (CONTAINMENT.md).

**Review model.** We do not rely on human review. Adversarial agents from several vendors, and real
attacks, test the system, so **the design, not review, must bound the damage**. Every component will
have bugs; a compromised process holds only its own capabilities, and a compromised server reaches
only what its clients entrusted to it (for a shared server, that is every client's data).

## What this is not
Said up front, because these are the pressures that erode the tenets below.

- **Not fast.** When speed and clarity conflict, clarity wins. A global TLB flush that is obviously
  correct beats a targeted one that is subtly wrong. We optimize only what measurement shows is
  unusable, and only in ways that stay easy to audit.
- **Not compatible with everything.** No POSIX, no Linux ABI, no C libraries, no legacy hardware, no
  vendor SDKs. A small set of supported targets, all describable by open standards. Software runs
  here because it was written or ported for it in Rust.
- **Not feature-complete.** Anything we do not need is absent rather than optional.

## 1. Simple enough to audit in full
A competent reader should be able to read the entire trusted computing base (firmware interface,
loader, kernel) and hold it in their head. It should read like a textbook example of each mechanism.

- **The kernel keeps memory, threads, IPC, interrupt delivery and the timer. Nothing else.**
  Drivers, filesystems and policy are unprivileged servers. (Its objects and calls: KERNEL-SPEC.md.
  This is the one statement of what the kernel keeps; other notes link here.)
- One obvious way to do each thing. No clever tricks without a comment that explains why the
  obvious way does not work. If the comment is hard to write, the trick goes.
- Size is budgeted, not just observed. Growing the TCB needs a justification in the commit.
- Prefer deleting code to adding configuration. Features nobody uses on our targets are removed from
  the fork rather than carried.
- Every design decision has a short note in `docs/` that a newcomer can follow.

## 2. Secure by construction
- **No ambient authority.** A process can touch only what it was explicitly given: memory it mapped,
  connections it holds, devices it was granted. "First to ask gets it" is not a policy.
- **Least privilege all the way down.** The kernel does not trust processes; servers do not trust
  clients; the loader does not trust images; nothing trusts input it has not validated.
- **W^X everywhere**, including in the kernel's own mappings. No page is ever both writable and
  executable, under any alias.
- **Verified boot.** Every byte that runs in a privileged mode is authenticated before it runs.
- **Fail closed and loudly.** On a violated invariant the kernel stops; it never limps on. Insecure
  fallbacks (a guessable RNG seed, a missing signature) are boot failures, not warnings.
- **`unsafe` is a budget.** Every `unsafe` block states the invariant it relies on. Unsafe code lives
  in few, small modules with safe interfaces. The count is tracked and only goes down without a reason.
- **The requester can never influence the approval channel.** An approval happens where nothing the
  requester runs can draw, type or listen (CAPABILITIES.md: approvals are out of band, like 2FA).
  Any convenience that puts approval inside a session must first amend this tenet.
- **Tested like it will be attacked.** See tenet 6: every security property has a test that tries to
  break it.
- **Non-interference across labels.** The label set is the isolation unit; only steward declassification
  moves data across one. Labels remove every intentional (software-mediated) flow, by construction.
  They cannot remove covert flows on one machine — power, heat, EM and the clock — so the OS's claim
  is exactly zero intentional paths, and covert collusion is a bounded, measured, hardware-and-
  placement matter, never a goal the OS meets (The use case).

## 3. Rust, and assembly only where Rust cannot reach
- Everything that runs on the machine is Rust: loader, kernel, servers, applications, and the
  firmware where we choose it (RustSBI). OpenSBI (C) is tolerated on QEMU and in the Linux-partition
  mode; firmware is always TCB.
- Assembly is limited to what the language cannot express: trap entry and exit, context switch, the
  first instructions after reset. It is written as `global_asm!`/`asm!` inside Rust sources, never as
  separate prebuilt objects.
- No C, no C toolchain in the build, no binary blobs, no bindings to C libraries. One exception, on
  the host only: **test oracles and fuzz drivers** (the littlefs C reference, libFuzzer) may be C or
  C++, in crates outside the workspace build, never linked into anything that runs on the machine.
- The build is one toolchain (`rustc` + `cargo`), pinned, and reproducible.

## 4. Open, auditable standards
- Hardware interface: ratified RISC-V specifications only (privileged architecture, SBI, PLIC/AIA,
  Sv32/Sv39, Sstc). No dependence on one vendor's extensions; where a board needs one, it sits
  behind a capability feature and has a standards-based alternative.
- Machine description: device tree. Devices: virtio where we have the choice.
- Formats and protocols: published ones with independent implementations (ELF, tar, FAT/ext if we
  must interoperate, TLS). We invent a format only when no open one fits, and then we specify it.
- Everything needed to rebuild and audit the system is public: sources, specs, toolchain.

## 5. Dependencies are part of the TCB
This one reconciles "reuse good crates" with "auditable". A crate we link into the kernel or loader is
our code, as far as the adversary is concerned.

- Reuse beats hand-rolling **when** the crate is small, `no_std`, pure Rust, maintained, and we have
  read it. Otherwise we write the 50 lines.
- Privileged code keeps a short, reviewed dependency list, pinned by lockfile and vendored or
  audited (`cargo vet` / `cargo audit`). New dependencies there need a stated reason.
- Userspace servers have more latitude than the kernel; applications more than servers.
- A dependency that panics on malformed input is a denial-of-service bug in us.

## 6. Tested to hell and back
If it is not tested, it does not work; we just have not found out yet. The harness is part of the
system, held to the same standard of simplicity as the kernel.

- **One simple harness, real boots.** `cargo testbench` boots the real kernel under QEMU with injected
  programs and asserts on the console. No mocks of the kernel, no special test builds of it (a build of the same sources with debug
  assertions and overflow checks on is not a special build: it is the kernel checked harder, and
  the bench boots chosen cases with it). The whole
  suite runs in seconds, so it runs on every change.
- **Every behaviour has a case.** New kernel or loader behaviour lands with a test in `tests/`.
  A bug fix lands with the test that would have caught it.
- **Attack tests, not just happy paths.** Hostile images, hostile syscall arguments, hostile
  messages, resource exhaustion, malformed device trees. Expected outcome: a clean refusal, never a
  kernel panic, never silent corruption.
- **Every dimension we claim.** (Milestones 1 to 3 claim rv64 only; rv32 is built, not booted, until
  its goal after milestone 3, HISTORY.md.) Each XLEN, each hart count, each supported firmware. A configuration
  that is not booted in the bench is not supported.
- **The harness can fail.** It is itself checked against known-bad runs, so a green result means something.
- **Fuzz what parses.** Anything that parses untrusted bytes (ELF, tar, device tree, syscall
  arguments, messages) gets a fuzz target.
- A test that is flaky is a bug, in the test or in the system, and is fixed rather than retried.

## 7. Devices speak virtio
Drivers are virtio, unless the device is trivial (a UART, an RTC: small, no DMA). A driver that does
DMA is inside the TCB unless the hardware confines its DMA (an IOMMU, or the FPGA's DMA-only memory
channel); the platform states which. On messy hardware we write no drivers: we reserve cores for
Redoubt and let Linux run the hardware and serve virtio to us. There, Linux is in the TCB; we do not
pretend to contain it. Design: IO-ARCHITECTURE.md.

## Where we stand
See [STATUS.md](STATUS.md).
