# Tenets

These outrank every other note. When a change conflicts with a tenet, either the change loses or
the tenet is amended here first, with the reason written down. Terms: [README.md](README.md).

## The adversary
Design for a capable, patient, automated adversary that has read every line of this repository, can
generate and test exploit candidates faster than a human can review them, and controls any code it
is allowed to run as an unprivileged process. Assume it finds every bug that is findable by reading.

That rules out security through obscurity, through "nobody would try that", or through complexity
that merely slows a human down. What is left: a small trusted base, mechanisms that are correct by
construction, and no ambient authority.

Out of scope for the software, stated so nobody assumes otherwise: physical attacks,
microarchitectural side channels (Spectre-class, cache timing), and malicious hardware. These need
hardware answers; on the FPGA target, side channels are handled in the RTL, and the hardware plan
may change to avoid them (PLATFORM-FPGA.md).

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

- The kernel stays a microkernel: memory, threads, IPC, interrupt routing. Nothing else. Drivers,
  filesystems and policy are unprivileged servers.
- One obvious way to do each thing. No clever tricks without a comment that explains why the
  obvious way does not work. If the comment is hard to write, the trick goes.
- Size is budgeted, not just observed. Growing the TCB needs a justification in the commit.
- Prefer deleting code to adding configuration. Features nobody uses on our targets are removed from
  the fork rather than carried.
- Every design decision has a short note in `planning/redoubt/` that a newcomer can follow.

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

## 3. Rust, and assembly only where Rust cannot reach
- Everything that runs on the machine is Rust: firmware, loader, kernel, servers, applications.
- Assembly is limited to what the language cannot express: trap entry and exit, context switch, the
  first instructions after reset. It is written as `global_asm!`/`asm!` inside Rust sources, never as
  separate prebuilt objects.
- No C, no C toolchain in the build, no binary blobs, no bindings to C libraries.
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
  programs and asserts on the console. No mocks of the kernel, no special test builds of it. The whole
  suite runs in seconds, so it runs on every change.
- **Every behaviour has a case.** New kernel or loader behaviour lands with a test in `redoubt/tests/`.
  A bug fix lands with the test that would have caught it.
- **Attack tests, not just happy paths.** Hostile images, hostile syscall arguments, hostile
  messages, resource exhaustion, malformed device trees. Expected outcome: a clean refusal, never a
  kernel panic, never silent corruption.
- **Every dimension we claim.** Each XLEN, each hart count, each supported firmware. A configuration
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
