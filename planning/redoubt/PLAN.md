# Plan

Forward-looking only. TENETS.md outranks this; what is done is in STATUS.md and HISTORY.md.
Design v4 is frozen for milestone 1 (TENETS.md).

## Milestone 1: separation and containment
**Alice and Bob logged in over SSH on QEMU, separated, and Alice's agent running under a lease,
contained. Every property backed by an attack test.**

Build one thin vertical slice toward it:
0. **Executable security model** (CONTAINMENT.md): a Rust crate implementing exactly the objects,
   system calls, errors and invariants of KERNEL-SPEC.md, plus the steward's policy, with property
   tests; handed to red-team agents.
1. **Kernel, to KERNEL-SPEC.md:** handles, endpoints, `call`/`send`/`receive`/`reply` with lend and
   transfer, `mint`, budgets, device objects and IRQ receive, `process_create`/`process_map`/
   `process_start`, exit notices. Replaces SID connects, scalar message kinds, `ClaimInterrupt`,
   device grants and name lookup.
2. **beamlet on Redoubt**, printing from Elixir over the console.
3. **IEx on the UART console:** an interactive Elixir shell on the box, before SSH exists.
4. **init, the boot manifest, the startup block and the loader stub** (INIT.md, PACKAGES.md); the
   boot loader loads only the kernel and `init`; the wire codec and its generator (WIRE.md);
   `bootfsd` over 9P (the shared 9P codec, fuzzed).
5. **The timer and preemption:** kernel-owned timer, timeouts, stride over budgets, the two classes
   (RESOURCES.md).
6. **Storage and network:** `blkd -> fsd` (littlefs; `fsd:data` and a labelled volume);
   `netd -> ipd:lan`.
7. **steward (stateless), keyd and sshd** (Rust): principals from the boot manifest, sessions as
   beamlet VMs running IEx, vault sessions, `ssh approve@box`.
8. **Alice's agent** under a lease, and the attack suite below.

### Milestone 1 attack suite
Deterministic programs in the test bench; each outcome is asserted by the system (kernel, victim or
a clean power-off), never by the attacker's own output (BUILD-PLAN.md).
- **Scripted hostile agent** (Alice's leased agent): cannot read outside `/work` or reach the
  network; a labelled agent reaches no uncleared sink; lease expiry destroys everything, including
  handles it passed on and its sub-agents; a lease over `MAX_LEASE` is refused; its approval
  requests cannot spoof the approval screen (control, bidi and format characters, swapped
  requests), and from a vault carry no free text; it cannot use its sponsor's 9P connection (it got
  a fresh one); it cannot slow Bob beyond its weight.
- **Scripted hostile user** (Bob attacking Alice):
  - system-call fuzzing: any arguments get an error, never a kernel panic;
  - endpoint flooding: 10,000 blocked senders on `fsd`, and Alice is still served in her turn;
  - budget death mid-call: a lender destroyed while `fsd` holds its lent pages, and `fsd` survives;
  - crash blame: Bob crashes `fsd` three times while Alice is busy; Bob is logged out, Alice is not,
    also when `fsd` panics rather than faults and when the crashing thread holds Alice's calls open
    too; a vault session's crashes do not log out its owner's unlabelled session;
  - loopback login: a session connecting to the box's own `sshd` with a `keyd`-held key is refused;
  - no leaky state: an unlabelled observer sees no change in usage, request ids, file versions,
    qids or directory listings while a vault session works, and cannot write, truncate, create or
    remove anything in the vault's volume;
  - hostile launch: a malformed ELF or startup block from a user parent hurts only the child;
  - approval flood: requests hit the per-account cap; the steward and Alice's approval screen are
    unaffected.
- **Kernel cases:** revocation by budget (mint into a revocation scope, destroy it, the handles are
  dead everywhere); the budget, scheduler and timer tests in RESOURCES.md.

## After milestone 1
- **The real-agent harness ("escape room")** comes first, alongside milestone 2: a real LLM agent on
  Redoubt (through `gatewayd`) told to break out, with red-team agents from several vendors taking
  turns. Every successful trick becomes a scripted test.
- **SMP** (the FPGA has 32 hardware threads): below.

## Milestone 2: install, share, persist
**Alice installs and runs a native tool she built and signed; Alice and Bob share a project; the box
reboots and remembers its users, shares, leases and packages; the system updates A/B and rolls
back. Every property attack-tested.**
- Packages: `pkg add`/`use`/`gc`, per-principal directories, trust lists, user signing (PACKAGES.md).
- Persistence: the steward's volume and re-minting after a restart; `budget_children`
  (KERNEL-SPEC.md); first-boot enrolment on the console; creating users at run time.
- Sharing: project principals and the share workflow (CAPABILITIES.md).
- Recovery: the namespace library re-walks after a server restart.
- System updates: A/B slots, M-of-N signatures, the version counter outside both slots, "healthy"
  before any user session (PACKAGES.md).

## Milestone 3: self-hosted development
**Develop Redoubt on Redoubt:** the real-agent harness with an agent calling off-box models through
`gatewayd`; compilers on the box for Elixir, Erlang and Rust; the server APIs (USERLAND.md, below).
- **Open:** `rustc` normally needs LLVM (C++), which conflicts with tenet 3. Options: a `rustc` built
  with only the Cranelift backend (needs investigation), Rust builds off-box with signed binaries
  shipped in, or a stated exception. Decide when planning milestone 3.

## After milestone 3: rv32
A small goal: bring the full stack up on rv32 and add it back to the bench's booted dimensions.
Until then rv32 is compiled, not booted (HISTORY.md). Width-specific code is allowed only in paging
geometry, trap entry and saved context, and the ABI's register encoding; anywhere else a
`target_pointer_width` `cfg` fails review. 64-bit values are `u64`, never `usize`.

## Working rules
- Record design decisions in `planning/redoubt/` before or with the code; keep STATUS.md current.
  Changes to the frozen design need a reason in HISTORY.md.
- Run `cargo testbench` before and after kernel or loader changes (see `redoubt/README.md`). New
  kernel behaviour gets a case in `redoubt/tests/` and, if needed, a program in
  `redoubt/test-programs/`. Every security property gets an attack case.
- Reuse a crate only if it is small, `no_std`, pure Rust, maintained and read (tenet 5).

## Open work outside the slice
- Finish the ABI audit: `xous-ipc` and `std`'s Xous PAL for register punning and `u32` fields in ABI
  types (`xous-rs` `Result` marshalling is now covered by a round-trip test).
- The kernel's default features include `debug-proc`, which `kernel/Cargo.toml` describes as adding
  kernel attack surface; decide whether a default build should carry it.
- Custom userspace target `riscv64gc-unknown-xous-elf` and `std` (`-Zbuild-std`); process `env`
  block and `.eh_frame` (needed by `std`).
- Test programs hardcode the UART address and IRQ; startup blocks fix this.
- Bench: inject device trees to test fail-closed paths (no rng-seed, no memory node, junk); wire in
  the kernel's hosted unit tests; fuzz targets for every parser.
- Report the two upstream bugs to betrusted-io/xous-core (HISTORY.md).
- `littlefs` in pure Rust, differentially tested against the C reference on the host.
- Retarget `std::fs` on the Xous target from PDDB (Xous's key-value store) to `fsd`.

## Userland API (a future USERLAND.md; milestone 3)
Deliberately deferred; build what is designed first. Points already agreed:
- No libc. Rust `std`'s Xous backend is retargeted (namespace + 9P for files, kernel time and
  threads, `/net` sockets); `no_std` programs use a thin syscall crate plus client crates. Crates
  that bind the `libc` crate will not build (accepted).
- Each server publishes three layers: its protocol (owned by its note, in the WIRE.md format), a
  `no_std` Rust client crate taking a handle, and an Elixir binding written in pure Elixir over a
  fixed set of beamlet natives (handles as terms, user syscalls, `call`/`send`, a 9P client and
  server). No per-server natives in the VM.
- Shell layer to design: launching native programs from Elixir (`System.cmd`/`Port`), pipes and
  standard I/O (the shell VM serves 9P `/dev/cons` to its children), namespaces, budgets, labels
  and agents from Elixir, the steward client, IEx helpers or a command mode.

## SMP (after milestone 1)
The two-hart spike works: a second hart started through SBI HSM runs kernel code and contends on the
spinlock `KernelCell` (the `smp` feature; bench case `smp-spike`). Remaining:
- OpenSBI picks the boot hart at random; never assume hart 0.
- Per-hart trap stack and current (PID, TID) via `sscratch`; scheduling on every hart.
- Big kernel lock at trap entry; one global run queue (RESOURCES.md).
- IPIs: reschedule, and TLB shootdown (SBI RFENCE, by ASID) on unmap, lend and return before a page
  is reused.
- One budget per core; `keyd` on its own core (PLATFORM-FPGA.md).
- Locking for the shared per-process thread-context pages; finer-grained locking only after the
  above is stable and tested.
