# Plan

Forward-looking only. TENETS.md outranks this; what is done is in STATUS.md and HISTORY.md.

## North star
**Alice and Bob logged in over SSH on QEMU, separated, and Alice's agent running under a lease,
contained. Every property backed by an attack test.**

Build one thin vertical slice toward it:
0. **Executable security model** (CONTAINMENT.md): a Rust crate of handles, stamps, minting,
   revocation, budgets, labels and volumes, calls and sends, and the powerbox, with property tests of
   its invariants; handed to red-team agents.
1. **Kernel: handles, IPC, budgets** built to the model: handle tables replacing SID connects; `call`
   and `send` with lend and transfer, no kernel queue; badges, stamps, mint into a budget handle;
   budgets (pages, processes, weight, class, labels, deadline) with never-reused ids; caller budget,
   principal and label set on every message and the label check between user budgets; exit
   messages; device objects replacing grants.
2. **beamlet on Redoubt**, printing from Elixir over the console.
3. **IEx on the UART console:** an interactive Elixir shell on the box, before SSH exists.
4. **init, the startup block and the loader stub**; the boot loader loads only the kernel and `init`;
   `bootfsd` over 9P (the shared 9P codec, fuzzed).
5. **The timer and preemption:** kernel-owned timer, receive timeout, stride over budgets with the
   two classes (RESOURCES.md).
6. **Storage and network:** `blkd -> fsd` (littlefs, a labelled volume); `netd -> ipd:lan`.
7. **steward, keyd and sshd** (Rust): sessions as beamlet VMs running IEx, vault sessions, the
   approval sessions.
8. **Alice's agent** under a lease, and a scripted hostile agent in the bench that tries to escape
   (CAPABILITIES.md, agents; INIT.md, worked example).

After the north star: SMP (the FPGA has 32 hardware threads), then the Later designs
(IO-ARCHITECTURE.md, PACKAGES.md).

## Working rules
- Record design decisions in `planning/redoubt/` before or with the code; keep STATUS.md current.
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

## Userland API (designed after the north-star build: USERLAND.md)
Deliberately deferred; build what is designed first. Points already agreed in discussion:
- No libc. Rust `std`'s Xous backend is retargeted (namespace + 9P for files, kernel time and
  threads, `/net` sockets); `no_std` programs use a thin syscall crate plus client crates. Crates
  that bind the `libc` crate will not build (accepted).
- Each server publishes three layers: its protocol (owned by its note: 9P tree plus typed
  messages), a `no_std` Rust client crate taking a handle, and an Elixir binding written in pure
  Elixir over a fixed set of beamlet natives (handles as terms, user syscalls, `call`/`send`, a 9P
  client and server). No per-server natives in the VM.
- One encoding for every typed message in the system; candidates CBOR (leaning) or restricted ETF.
- Shell layer to design: launching native programs from Elixir (`System.cmd`/`Port`), pipes and
  standard I/O (the shell VM serves 9P `/dev/cons` to its children), namespaces, budgets, labels
  and agents from Elixir, the steward client, IEx helpers or a command mode.

## SMP (after the north star)
- OpenSBI picks the boot hart at random; never assume hart 0.
- Secondary hart bring-up via SBI HSM; per-hart trap stack and current (PID, TID) via `sscratch`.
- Big kernel lock at trap entry (the `smp` feature's spinlock `KernelCell` is the first step); one
  global run queue (RESOURCES.md).
- IPIs: reschedule, and TLB shootdown (SBI RFENCE, by ASID) on unmap, lend and return before a page
  is reused.
- All hardware threads of a core run one budget; `keyd` on its own core (PLATFORM-FPGA.md).
- Locking for the shared per-process thread-context pages; finer-grained locking only after the
  above is stable and tested.
