# Plan

Forward-looking only. TENETS.md outranks this; what is done is in STATUS.md and HISTORY.md.

## North star
**Alice and Bob logged in over SSH on QEMU, separated, every property backed by an attack test.**
Build one thin vertical slice toward it:
0. **Executable security model** (CONTAINMENT.md): a Rust crate of handles, stamps, minting,
   revocation, budgets and labels, with property tests of its invariants; handed to red-team agents.
1. **Kernel: handles, endpoints, budgets** built to the model: handle tables replacing SID connects,
   badges, budget stamps, the caller's budget id on every message, label sets and the subset check,
   per-page payer, charged kernel tables, lease deadlines, death notification.
2. **beamlet on Redoubt**, printing from Elixir over the console.
3. **init and the startup block**; device handles replace grants (DEVICE-GRANTS.md); `bootfsd` over
   9P (the shared 9P codec, fuzzed).
4. **Clocks and preemption:** kernel-owned timer, receive timeout, 1 ms user time; hierarchical
   stride with the two classes (RESOURCES.md).
5. **Storage and network:** `blkd -> blockd -> fsd` (littlefs); `netd -> ipd:lan`.
6. **steward, keyd and sshd** (Rust), sessions as beamlet VMs, the approval session.

After the north star: SMP (the FPGA has 32 hardware threads), then the Later designs
(IO-ARCHITECTURE.md).

## Working rules
- Record design decisions in `planning/redoubt/` before or with the code; keep STATUS.md current.
- Run `cargo testbench` before and after kernel or loader changes (see `redoubt/README.md`). New
  kernel behaviour gets a case in `redoubt/tests/` and, if needed, a program in
  `redoubt/test-programs/`. Every security property gets an attack case.
- Reuse a crate only if it is small, `no_std`, pure Rust, maintained and read (tenet 5).

## Hardware abstraction
CPUs and SoCs differ in ways unrelated to XLEN, so code never uses `target_arch` to mean "has a PLIC"
or "runs under SBI".
- **Capability features** select backends: `sbi` (S-mode under SBI firmware: console, shutdown,
  `kernel_syscall()`, RNG seed), `plic` (the interrupt controller). New hardware = a new backend file
  and feature (`aia`, `sstc`, ...).
- **Board features** only compose capabilities: `qemu-virt = ["sbi", "plic"]`. The FPGA card should be
  another one-line feature.
- **Discovered, not hardcoded:** RAM, MMIO regions, the PLIC and this hart's S-mode context come from
  the device tree via the loader. The kernel parses no device tree.
- **Width-bound only:** page-table geometry (`paging`), saved-context size and trap assembly key on
  `target_pointer_width`.
- Interrupt controller backend contract (`arch/riscv/irq.rs`): `enable_irq`, `disable_irq`,
  `disable_all_irqs`, `enable_all_irqs`, `pending`, `mask`, optional `init`.

## Targets
All present the same contract: virtio-mmio devices, PLIC, SBI, device tree (tenet 7).
1. QEMU `virt`, rv32 and rv64, OpenSBI or RustSBI: development and the bench.
2. The FPGA cards (PLATFORM-FPGA.md): the secure configuration.
3. Messy SoCs (Orange Pi RV2): Later, Linux on reserved cores.

## Open work outside the slice
- Audit `xous-rs` and `xous-ipc` for register punning and `u32` fields in ABI types (two such bugs
  found so far, both invisible to the compiler).
- Custom userspace target `riscv64gc-unknown-xous-elf` and `std` (`-Zbuild-std`); process `env`
  block and `.eh_frame` from the loader (needed by `std`).
- Wider IRQ numbering (the table has 32 entries and uses `1 << irq` masks).
- Test programs hardcode the UART address and IRQ; startup blocks fix this.
- `IniE` tags are emitted empty just so the kernel can count processes.
- Bench: inject device trees to test fail-closed paths (no rng-seed, no memory node, junk); wire in
  the kernel's hosted unit tests; fuzz targets for every parser.
- Report the two upstream bugs to betrusted-io/xous-core (HISTORY.md).
- `littlefs` in pure Rust, differentially tested against the C reference on the host.
- `gatewayd` (the LLM gateway).
- Retarget `std::fs` on the Xous target from PDDB to `fsd`.

## SMP (after the north star)
- OpenSBI picks the boot hart at random; never assume hart 0.
- Secondary hart bring-up via SBI HSM; per-hart trap stack and current (PID, TID) via `sscratch`.
- Big kernel lock at trap entry; one global run queue (RESOURCES.md).
- IPIs: reschedule, and TLB shootdown on unmap, lend and return.
- All hardware threads of a core run one budget (PLATFORM-FPGA.md).
- Locking for the shared per-process thread-context pages; finer-grained locking only after the
  above is stable and tested.

## Accepted trade-offs
The physmap makes all RAM kernel-addressable, including a writable alias of user code pages; every
map and unmap does a global `sfence.vma`; kernel entry relies on the firmware delegating
instruction page faults to S-mode.
