# xous64: RV64 + SMP + filesystem fork

Hard fork of xous-core (forked at c025441, 2026-09-15). Branch: `xous64`.

**Read `TENETS.md` first.** It outranks this plan.

## Working rules
- Prefer maintained pure-Rust `no_std` crates over hand-rolled code (`sbi-rt`, `riscv`, `fdt`, ...).
  Hand-roll only what is Xous-specific.
- Keep rv32 building: rv64 code lives in new files selected by `cfg`, shared code goes through helpers.
- Record design decisions in `planning/xous64/` before or with the code.
- Run `cargo testbench` before and after kernel or loader changes (about 3 s; see `xous64/README.md`).
  New kernel behaviour gets a test case in `xous64/tests/` and, if needed, a program in
  `xous64/test-programs/`.

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
      Entry is `global_asm!`, so no C toolchain. Run with `cargo testbench --run <program>`.
      The old `loader/` (M-mode, secboot, swap, OLED) is left alone; generic pieces (args, minielf,
      phase1/phase2) get ported into `loader64` as needed.
- [ ] Custom userspace target `riscv64gc-unknown-xous-elf` (JSON spec, `-Zbuild-std`, needs nightly).

## Phase 1: RV64 uniprocessor
Design: `planning/xous64/MEMORY-LAYOUT.md` (direct physmap instead of the page-table window; address
space split by root entry). Build: `cargo build -p xous-kernel --target riscv64imac-unknown-none-elf --features qemu-virt`.

**Milestone 2026-09-18: boots to userspace on QEMU virt and passes the IPC test** (`cargo testbench`:
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
- [x] Kernel RNG keyed from `/chosen/rng-seed` via a `Seed` tag (was: the `time` CSR). Test: `rng`.
- [x] Interrupt controller split into backends: `intc_vexriscv.rs` and `intc_plic.rs` (rustsbi `plic`
      crate, claim in `pending()`, complete in `enable_all_irqs()`, masking via `sie.SEIE`).
      Verified with `uart-echo` in `xous64/test-programs`: claims UART IRQ 10, handler runs in userspace, returns through
      the magic ISR address, PLIC completes and re-arms. Kernel IRQ table is 32 entries; QEMU's PCIe
      INTx are 32-35, so widen it when PCI matters.
- [x] Fixed: `scause` interrupt causes were matched as `0x8000_000x` (bit 31). The flag is the top bit,
      so bit 63 on rv64; now normalized in `RiscvException::from_regs`.
- [x] Fixed a real 64-bit bug: syscall results were returned by reading the `repr(C)` `Result` enum as
      eight registers, which only works when every field is one word. Now serialized with `to_args()`.
      **Audit for the same class** (struct/enum memory punned as register arrays, `u32` fields in ABI
      types): `xous-rs` message/envelope paths, `xous-ipc`, `std`'s Xous PAL.
- [x] Hart timer (design: `planning/xous64/TIMER.md`): delivered as IRQ 0, programmed through
      `PlatformSpecific` calls (allowed from interrupt context), `rdtime` readable from U-mode, backend
      `timer_sbi.rs` / `timer_none.rs`. Verified by `timer-test` (5 one-shot ticks at 20 Hz = 251 ms).
      No time-slice preemption yet; that is a scheduling decision for Phase 3.

Loader (`loader64`), see BOOT.md:
- [x] Boot bundle = ustar of ELFs via initrd (`tar-no-std` + `elf`), replacing create-image/MiniELF.
- [x] Page allocator + RPT, Sv39 table builder, kernel and per-process address spaces.
- [x] Argument block (XArg v2, MREx from the device tree, IniE, PNam); `stvec`-trap kernel entry.
- [ ] Process `env` block, `.eh_frame` in IniE, rng-seed, DTB hand-off to userspace.

Userspace:
- [ ] `riscv64gc-unknown-xous-elf` target spec + `-Zbuild-std`; audit `std`'s Xous PAL and `xous-ipc`.
- [x] First init process: `uart-echo` in `xous64/test-programs`, `no_std`, `uart_16550` crate over claimed MMIO, echoes
      input from a userspace interrupt handler. Try it: `cargo testbench --run uart-echo`.
- [x] `xous64/ipc-test`: `no_std` server (owns the UART) + client covering every message type.
- [x] Building and booting an rv64 bundle lives in the test bench (`xous64/testbench`), not `xtask`.
- Exit: minimal server set boots to a UART shell in QEMU.

### Next up (in order)
1. rv32 on QEMU virt, so both XLENs are boot-tested, not just built: Sv32 tables (with the page-table
   window the Sv32 kernel expects) in the SBI loader, an rv32 SBI firmware (Ubuntu's QEMU ships only
   the rv64 OpenSBI; RustSBI is the pure-Rust option), upstream `riscv` crate for rv32+`sbi`.
   The bench already lists rv32 for every boot case and reports them as SKIP until then.
2. Audit for register-punning / `u32`-in-ABI bugs in `xous-rs` and `xous-ipc` (two found so far, both
   invisible to the compiler).
3. `riscv64gc-unknown-xous-elf` target + `std`, then bring over `xous-log`, `xous-names`, `xous-ticktimer`.
4. Process `env` block and `.eh_frame` from the loader (needed by `std`).
5. `virtio-drivers` crate in a userspace block server (start of Phase 2).

### Test bench (done 2026-09-18)
`xous64/testbench`: declarative TOML cases, injects workspace or prebuilt binaries into the boot bundle,
boots QEMU per hart count, feeds console input on triggers, asserts ordered `expect` regexes and
`forbid` patterns, keeps logs, non-zero exit on failure. Self-checked against timeout, forbidden
output and missing-program failures. Also: ELF corruption for hostile-input tests, cross-boot
distinctness checks, `--firmware`, and interactive `--run`. Cases: ipc, timer, uart-irq, rng,
all-together, loader-rejects-kernel-address/-entry (rv64) and a Precursor build check (rv32). Not covered yet: the kernel's hosted-mode unit tests (`kernel/src/test.rs`).

## Review 2026-09-18: debt and risks
Fixed in this pass (each with a test where one makes sense):
- Predictable kernel RNG -> seeded from the device tree (`rng`: IDs differ within and across boots).
- Loader trusted ELF addresses -> segments and entry are range-checked (`loader-rejects-kernel-*`).
- Duplicated PTE flag code -> `arch/riscv/mmu_flags.rs`, shared by Sv32 and Sv39.
- `run-qemu.sh` duplicated the bench -> `cargo testbench --run`.
- The loader used `fdt.memory()`, which panics on trees it dislikes -> lookup by `device_type`.

Second pass, against the tenets (same day): typed page tables in a shared `sv39` crate, W^X enforced
and verified at boot, `KernelCell` for globals, RNG fails closed, the `unsafe` ratchet, attack tests.
Found and fixed an upstream bug on the way: `interrupt_free` bounds check was off by one, so any
process could panic the kernel. **Worth reporting upstream to betrusted-io/xous-core.**

**Security bug found and fixed (2026-09-18), confirmed by test `uaf-lent-page`:** a process that
terminated while one of its pages was lent to (and still mapped in) another process had that physical
frame freed and re-allocated to a third process -- a cross-process use-after-free / memory disclosure.
Root cause: `release_all_memory_for_process` called `page_is_lent(phys)` with a physical address where
a virtual one was required, so lent pages were never detected. Fix: detect lent frames by walking the
dying process's page table (`arch::mem::for_each_lent_frame`) and reparent them to PID 1 so the frame
is not reused while a borrower holds it. **Present in upstream betrusted-io/xous-core; worth reporting.**
Remaining: the return path when the borrower later returns a page to a dead lender (possible frame
leak, not a safety hole) is not yet handled -- see "lent pages at exit".

Open, in rough priority order:
- [x] Kernel core `unsafe` 82 -> 67: memory manager and its allocation tables moved behind `KernelCell`.
- [ ] Inherited `unsafe`: RISC-V arch layer 35 and kernel core 67 uses, none justified. Convert the
      remaining `static mut` globals to `KernelCell` (SWITCHTO_CALLER, PREVIOUS_PAIR, PROCESS_TABLE,
      MEMORY_ALLOCATIONS, ...), then justify what is left. The ratchet in `xous64/tests/unsafe-budget.toml`
      records progress.
- [x] **Ambient authority (devices)**: default-deny device grants. A manifest in the boot bundle
      (`grants` entry) lists each process's allowed MMIO regions and IRQs; the loader emits `Grnt`
      tags; the kernel enforces at MapMemory (device pages) and ClaimInterrupt. Design: DEVICE-GRANTS.md.
      Tests: `grant-attack`. Open: server-ID capabilities lack revocation; no runtime grant delegation.
- [ ] Delete what we do not run (tenet 1), pending the rv32 decision: swap, gdb stub, ARM, Precursor
      and bao1x platforms, the Sv32 window code, prebuilt blobs.
- [ ] Bench: inject a device tree, to test fail-closed paths (no rng-seed, no memory node, junk).
- [ ] **Firmware is C (OpenSBI), in M-mode.** That contradicts "no C in the trusted path". Goal: RustSBI
      on both XLENs, OpenSBI kept as a second firmware in the test matrix. Findings: RustSBI Prototyper
      (HEAD eae4cc7) builds for rv64 and rv32 in ~10 s each with its pinned nightly, and boots
      `loader64`. But it re-serializes the device tree (to add a reserved-memory node for itself), and
      the `fdt` 0.1.5 parser asserts "bad node" on the result, so RAM and initrd cannot be read.
      Not yet determined whether the tree violates the spec (properties after child nodes?) or the
      parser is too strict. Reproduce: `cargo testbench ipc --firmware <rustsbi-prototyper elf>`.
      Options: fix/report upstream, try `fdt` 0.2, or a more tolerant parser.
- [ ] **No secure boot** (regression vs. stock Xous). Sign the bundle; verify in `loader64`.
- [ ] rv32 cannot be boot-tested (needs Sv32 in the SBI loader + rv32 firmware, i.e. the item above).
- [ ] rv32 still links prebuilt assembly blobs (`kernel/bin/*.a`, needs a C toolchain to regenerate);
      rv64 uses `global_asm!`. Unify once rv32 boots in the bench, so breakage is visible.
- [ ] IRQ numbers: 32-entry table and `1 << irq` bitmasks. Fine on QEMU, wrong for SoCs with 100+
      PLIC sources (Orange Pi RV2).
- [ ] Test programs hardcode the UART address and IRQ. Userspace needs a device-tree service.
- [ ] Any process may claim any unclaimed MMIO region (stock Xous behaviour). On QEMU that includes the
      power/reset device. Needs a policy once untrusted programs exist.
- [ ] `IniE` tags are emitted empty just so the kernel can count processes.
Accepted trade-offs (documented where they live): the physmap makes all RAM kernel-addressable,
including a writable alias of kernel text; every map/unmap does a global `sfence.vma`; kernel entry
relies on the firmware delegating instruction page faults to S-mode.

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
