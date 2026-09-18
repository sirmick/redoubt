# xous64: RV64 + SMP + filesystem fork

Hard fork of xous-core (forked at c025441, 2026-09-15). Branch: `xous64`.

## Working rules
- Prefer maintained pure-Rust `no_std` crates over hand-rolled code (`sbi-rt`, `riscv`, `fdt`, ...).
  Hand-roll only what is Xous-specific.
- Keep rv32 building: rv64 code lives in new files selected by `cfg`, shared code goes through helpers.
- Record design decisions in `planning/xous64/` before or with the code.
- Run `xous64/test.sh` (boots QEMU, exits non-zero unless the IPC test passes) before and after
  kernel or loader changes.

## Hardware abstraction
CPUs and SoCs differ in ways that have nothing to do with XLEN, so code never uses
`target_arch = "riscv64"` to mean "has a PLIC" or "runs under SBI".
- **Capability features** select backends: `sbi` (S-mode under SBI firmware: console, shutdown,
  `kernel_syscall()` soft trap, RNG seed), `plic` (interrupt controller; otherwise VexRiscv CSRs).
  New hardware = new backend file + feature (`aia`, `clic`, a different timer, ...).
- **Board features** only compose capabilities: `qemu-virt = ["sbi", "plic"]`. A softcore or the
  Orange Pi RV2 should be another one-line feature, not new `cfg`s through the kernel.
- **Discovered, not hardcoded**: RAM, MMIO regions, PLIC address and this hart's S-mode context come
  from the device tree via the loader (`MREx`, `Plic` tags). The kernel parses no device tree.
- **XLEN-bound only**: paging (`mem.rs` Sv32 / `mem_sv39.rs`), saved-context size and the trap
  assembly (`asm.S` / `asm64.rs`) key on pointer width.
- Interrupt controller backend contract (`arch/riscv/irq.rs`): `enable_irq`, `disable_irq`,
  `disable_all_irqs`, `enable_all_irqs`, `pending`, `mask`, optional `init`.

## Goals
- RV64 (Sv39), SMP, a real filesystem.
- Stay a microkernel: drivers, block layer and filesystems are userspace servers.
- Pure Rust as far as possible. No C in the trusted path, no C-binding crates.

## Targets
1. QEMU `virt` (rv64, OpenSBI/RustSBI, ns16550, PLIC, virtio-mmio).
2. RV64 softcore with virtio devices.
3. Possibly Orange Pi RV2 (8-core, Sv39, PLIC + CLINT + OpenSBI).

## Phase 0: platform
- [x] Install `qemu-system-riscv64` (8.2.2, bundled OpenSBI v1.3).
- [x] `riscv64imac-unknown-none-elf` / `riscv64gc-unknown-none-elf` rustup targets (kernel, loader).
- [x] Patch workspace to use local `xous-rs` instead of crates.io.
- [x] `loader64`: new loader crate for SBI + device tree platforms. Boots in S-mode on QEMU virt, prints over
      SBI, parses the DTB (RAM, harts, virtio-mmio slots), queries HSM hart state, shuts down via SRST.
      Entry is `global_asm!`, so no C toolchain. Run with `loader64/run-qemu.sh`.
      The old `loader/` (M-mode, secboot, swap, OLED) is left alone; generic pieces (args, minielf,
      phase1/phase2) get ported into `loader64` as needed.
- [ ] Custom userspace target `riscv64gc-unknown-xous-elf` (JSON spec, `-Zbuild-std`, needs nightly).

## Phase 1: RV64 uniprocessor
Design: `planning/xous64/MEMORY-LAYOUT.md` (direct physmap instead of the page-table window; address
space split by root entry). Build: `cargo build -p xous-kernel --target riscv64imac-unknown-none-elf --features qemu-virt`.

**Milestone 2026-09-18: boots to userspace on QEMU virt and passes the IPC test** (`xous64/test.sh`:
scalar, blocking scalar with 64-bit values, lend, 1000x lend_mut, move; 1 and 4 harts).
First reached userspace the same day: loader64 -> kernel (Sv39, SBI console) -> a
`no_std` process in U-mode that claims the UART MMIO page and prints, then yields millions of times
through the scheduler. Boot flow: `planning/xous64/BOOT.md`.

Kernel (release .text = 54 KiB):
- [x] Compile census: after widening 11 `target_arch = "riscv32"` gates, the only compile error was the
      `ProcessImpl` page-size assertion. Everything else was rv32 assumptions the type checker can't see.
- [x] `xous-rs`: Sv39 layout constants (`PHYSMAP_BASE`, `PROCESS_AREA`, `KERNEL_AREA`, ...). The rv32
      window constants do not exist on rv64, which turned every hidden use into a compile error.
- [x] `ProcessImpl` is XLEN-generic: header padded to one context, 2 pages on rv64, const-asserted.
- [x] `arch/riscv/mem_sv39.rs`: three-level walk through the physmap, same API as the Sv32 `mem.rs`.
      lend/return/move and `virt_to_phys_pid` no longer switch `satp` to reach another address space.
- [x] `satp` PID decoding goes through `mem::pid_from_satp()` on both XLENs.
- [x] `arch/riscv/asm64.rs`: entry/trap/resume as `global_asm!`; rv64 `.a` blobs deleted (they were
      assembled from rv32 offsets: `<< 7` context index, zero-extended addresses). `kernel/link64.x`.
- [x] Upstream `riscv` 0.16 crate for CSRs on rv64 (vendored 0.5.6 + blobs stays for rv32 only).
- [x] `platform/qemu_virt`: kernel console over the SBI debug console (`sbi-rt`), no utralib.
- [ ] Kernel RNG is seeded from the `time` CSR only. Loader must pass `/chosen/rng-seed`. (FIXME in code)
- [x] Interrupt controller split into backends: `intc_vexriscv.rs` and `intc_plic.rs` (rustsbi `plic`
      crate, claim in `pending()`, complete in `enable_all_irqs()`, masking via `sie.SEIE`).
      Verified with `xous64/hello-uart`: claims UART IRQ 10, handler runs in userspace, returns through
      the magic ISR address, PLIC completes and re-arms. Kernel IRQ table is 32 entries; QEMU's PCIe
      INTx are 32-35, so widen it when PCI matters.
- [x] Fixed: `scause` interrupt causes were matched as `0x8000_000x` (bit 31). The flag is the top bit,
      so bit 63 on rv64; now normalized in `RiscvException::from_regs`.
- [x] Fixed a real 64-bit bug: syscall results were returned by reading the `repr(C)` `Result` enum as
      eight registers, which only works when every field is one word. Now serialized with `to_args()`.
      **Audit for the same class** (struct/enum memory punned as register arrays, `u32` fields in ABI
      types): `xous-rs` message/envelope paths, `xous-ipc`, `std`'s Xous PAL.
- [ ] Timer. There is no MMIO timer to give a userspace ticktimer; the S-mode timer is a CPU resource
      (SBI TIME / Sstc `stimecmp`). Design needed: kernel exposes it as a virtual IRQ + "set deadline"
      call, behind a `timer` backend like the interrupt controller. Also gives preemption.
- [ ] `kernel/src/mem.rs` (generic): audit `u32`/4 GiB assumptions in the RAM allocation tables.
- [ ] Swap and gdb-stub are not ported (features stay rv32-only).

- [x] XArg v2 / 64-bit MREx parsing; early console so boot panics are visible; panic powers off via SBI.
- [x] `kernel_syscall()`: kernel-internal syscalls no longer use `ecall` (that is SBI's on these platforms).

Loader (`loader64`), see BOOT.md:
- [x] Boot bundle = ustar of ELFs via initrd (`tar-no-std` + `elf`), replacing create-image/MiniELF.
- [x] Page allocator + RPT, Sv39 table builder, kernel and per-process address spaces.
- [x] Argument block (XArg v2, MREx from the device tree, IniE, PNam); `stvec`-trap kernel entry.
- [ ] Process `env` block, `.eh_frame` in IniE, rng-seed, DTB hand-off to userspace.

Userspace:
- [ ] `riscv64gc-unknown-xous-elf` target spec + `-Zbuild-std`; audit `std`'s Xous PAL and `xous-ipc`.
- [x] First init process: `xous64/hello-uart`, `no_std`, `uart_16550` crate over claimed MMIO, echoes
      input from a userspace interrupt handler. Try it: `loader64/run-qemu.sh target/riscv64imac-unknown-none-elf/release/hello-uart`.
- [x] `xous64/ipc-test`: `no_std` server (owns the UART) + client covering every message type.
- [ ] `xtask` support for building an rv64 bundle (today: `loader64/run-qemu.sh`).
- Exit: minimal server set boots to a UART shell in QEMU.

### Next up (in order)
1. Timer backend + preemption (design note first: virtual IRQ + set-deadline, SBI TIME vs Sstc).
2. Audit for register-punning / `u32`-in-ABI bugs in `xous-rs` and `xous-ipc` (two found so far, both
   invisible to the compiler).
3. `riscv64gc-unknown-xous-elf` target + `std`, then bring over `xous-log`, `xous-names`, `xous-ticktimer`.
4. Process `env` block and `.eh_frame` from the loader (needed by `std`).
5. `virtio-drivers` crate in a userspace block server (start of Phase 2).

## Phase 2: filesystem (userspace; can proceed in hosted mode in parallel)
- [ ] `virtio-blk` server, `blockcache` server, `vfs` server; `lend_mut` page buffers for zero-copy.
- [ ] Filesystem choice deferred. Constraint: native pure Rust.
- [ ] Later: retarget `std::fs` on the Xous target from PDDB to the VFS server.

## Phase 3: SMP
- Note: OpenSBI picks the boot hart at random (observed hart 2 of 4). Never assume hart 0.
- [ ] Secondary hart bring-up via SBI HSM; per-hart trap stack; per-hart current (PID, TID) via `sscratch`.
- [ ] Big kernel lock at trap entry.
- [ ] 39 `static mut`s -> locked or per-hart (`MEMORY_MANAGER`, `PROCESS_TABLE`, `SWITCHTO_CALLER`, `PREVIOUS_PAIR`, ...).
- [ ] IPIs: reschedule + TLB shootdown on unmap / lend / return-memory.
- [ ] Per-hart run queues; locking for the shared per-process thread-context pages.
- [ ] Finer-grained locking only after the above is stable and tested.
