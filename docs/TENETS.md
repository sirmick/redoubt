# Tenets

These outrank every other note. When a change conflicts with a tenet, either the change loses or
the tenet is amended here first, with the reason written down. Terms: [README.md](README.md).

**Design v4 is frozen for milestone 1:** a change to it needs a stated reason, recorded in
ANSWERS.md.

## Purpose and threat model

Run untrusted, potentially hostile agents alongside their human principals, under explicit
grants and human-revocable leases. Assume the adversary has the full source, can automate
exploit attempts, controls its unprivileged processes and may compromise its VM or a server.
The BEAM is outside the TCB; a compromised shared server exposes what its clients entrusted to it.

The target guarantees are:

1. **Capability closure.** An agent's reachable authority never exceeds its initial grants plus what
   a human-approved steward action adds; every delegation step narrows (R9, `mint`; CAPABILITIES.md).
   Authority is monotone non-increasing absent a human approval.
2. **Label non-interference, in software.** No *intentional* path crosses a label boundary: not a
   `call` or `send` (R1), a write (`check`), a read up, a sink, metadata or a counter. Data crosses
   only by steward declassification. This is a property of the OS, it is by construction, and it is
   testable. **The label set, not the capability set, is the isolation unit; two budgets with
   different handle sets but equal label sets are one trust domain, and two budgets with differing
   label sets have no path the OS can carry a message over.**
3. **Human control.** Every increase in authority, and every declassification, is an out-of-band human
   approval; leases bound time; the sponsor can always end a lease (CAPABILITIES.md).

**Open qualifications to these target guarantees:** question **165** asks how the capability-closure
claim above accounts for permitted same-label delegation; answer 150's equal-label trust domain
remains settled. Question **164** asks how confined placement accommodates the already-specified
trusted mediation for approvals, push and declassification (answers 152-153). These are unresolved
claim/topology questions, not accepted exceptions or proof that the current implementation meets
the guarantees. Question **166** separately challenges the one-slice responsiveness promise in
RESOURCES.md; human control does not establish that timing bound by itself.

The [high/low scenario](GAME.md) tests a secret-holding agent colluding with a networked one.
Labels must close intentional software paths; confined placement and permitted mediation are
owned by CONTAINMENT and INIT, with the open qualifications above.

Out of scope: physical attacks, malicious hardware, microarchitectural side channels and
covert communication between co-located budgets (including power, heat, EM and clocks).
Hardware isolation and placement belong to [PLATFORM-FPGA](PLATFORM-FPGA.md).
Assume a perfect attacker clock; protect secrets through constant-time code and isolation,
not hidden time. Adversarial agents and real attacks test the design; review alone cannot
bound damage from bugs.

Clarity takes priority over speed. Optimize measured problems only while retaining auditability.
There is no POSIX/Linux ABI, C-library, legacy-hardware or vendor-SDK compatibility goal.
Support a small set of open-standard targets and omit unneeded features.

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
  moves data across one, and no intentional (software-mediated) flow crosses it, by construction.
  Covert flows are out of scope (Purpose and threat model).

## 3. Rust, and assembly only where Rust cannot reach
- Everything that runs on the machine is Rust: RustSBI firmware, loader, kernel, servers and
  applications. RustSBI is the only supported firmware on both widths and is always TCB.
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
  the bench boots chosen cases with it). Run the whole suite on every change.
- **Every behaviour has a case.** New kernel or loader behaviour lands with a test in `tests/`.
  A bug fix lands with the test that would have caught it.
- **Attack tests, not just happy paths.** Hostile images, hostile syscall arguments, hostile
  messages, resource exhaustion, malformed device trees. Expected outcome: a clean refusal, never a
  kernel panic, never silent corruption.
- **Every dimension we claim.** Milestones 1 to 3 require rv64 boot acceptance and rv32
  compilation; available rv32 boot cases are optional, and required full-stack rv32 boot
  acceptance belongs to the goal after milestone 3 (PLAN.md, ANSWERS.md). Each claimed XLEN,
  each hart count, each supported firmware. A configuration
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

## Implementation and method

[STATUS](STATUS.md) distinguishes implemented behavior from these target constraints.
[SWARM](SWARM.md) applies them through isolated work packages, explicit dependencies, attack
acceptance and defensive/simplifier/editor review. [PLAN](PLAN.md) owns future outcomes.
