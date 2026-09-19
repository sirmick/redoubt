# redoubt: RV64 + SMP + filesystem fork

Hard fork of xous-core (forked at c025441, 2026-09-15). Branch: `redoubt`.

**Read `TENETS.md` first.** It outranks this plan.

## North star
**Alice and Bob logged in over SSH on QEMU, separated, every property backed by an attack test.**
Design is settled (CAPABILITIES, CONTAINMENT, RESOURCES, INIT, NAMESPACES, PACKAGES, IO-ARCHITECTURE)
; build one thin vertical slice toward it:
0. Executable security model (CONTAINMENT.md): capabilities, derivation, revocation, budgets,
   labels; invariants model-checked and red-teamed.
1. Kernel: handles, endpoints, budgets, built to the model; label slots from the start.
2. beamlet on Redoubt, printing from Elixir over the console.
3. init + startup block; `bootfs` over 9P (shared 9P codec, fuzzed).
4. Preemption: hierarchical stride scheduling, two classes; then donation.
5. virtio-blk -> blockd -> littlefs; virtio-net -> linkd -> ip:lan.
6. steward and sshd (Rust), sessions as beamlet VMs.

## Working rules
- Prefer maintained pure-Rust `no_std` crates over hand-rolled code (`sbi-rt`, `riscv`, `fdt`, ...).
  Hand-roll only what is Xous-specific.
- Keep rv32 building: rv64 code lives in new files selected by `cfg`, shared code goes through helpers.
- Record design decisions in `planning/redoubt/` before or with the code.
- Run `cargo testbench` before and after kernel or loader changes (about 3 s; see `redoubt/README.md`).
  New kernel behaviour gets a test case in `redoubt/tests/` and, if needed, a program in
  `redoubt/test-programs/`.

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
All targets present the same contract: virtio-mmio devices, PLIC, SBI, device tree (tenet 7,
`IO-ARCHITECTURE.md`).
1. QEMU `virt` (rv64, OpenSBI/RustSBI, ns16550, PLIC, virtio-mmio; later `iommu-sys=on`).
2. RV64 softcore on an FPGA (e.g. CVA6) with virtio devices and a RISC-V IOMMU: the secure configuration.
3. Messy SoCs such as the Orange Pi RV2: Linux on reserved cores serves virtio, redoubt on the rest,
   partitioned by OpenSBI domains. No native K1 drivers.

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
Design: `planning/redoubt/MEMORY-LAYOUT.md` (direct physmap instead of the page-table window; address
space split by root entry). Build: `cargo build -p xous-kernel --target riscv64imac-unknown-none-elf --features qemu-virt`.

**Milestone 2026-09-18: boots to userspace on QEMU virt and passes the IPC test** (`cargo testbench`:
scalar, blocking scalar with 64-bit values, lend, 1000x lend_mut, move; 1 and 4 harts).
First reached userspace the same day: loader64 -> kernel (Sv39, SBI console) -> a
`no_std` process in U-mode that claims the UART MMIO page and prints, then yields millions of times
through the scheduler. Boot flow: `planning/redoubt/BOOT.md`.

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
      Verified with `uart-echo` in `redoubt/test-programs`: claims UART IRQ 10, handler runs in userspace, returns through
      the magic ISR address, PLIC completes and re-arms. Kernel IRQ table is 32 entries; QEMU's PCIe
      INTx are 32-35, so widen it when PCI matters.
- [x] Fixed: `scause` interrupt causes were matched as `0x8000_000x` (bit 31). The flag is the top bit,
      so bit 63 on rv64; now normalized in `RiscvException::from_regs`.
- [x] Fixed a real 64-bit bug: syscall results were returned by reading the `repr(C)` `Result` enum as
      eight registers, which only works when every field is one word. Now serialized with `to_args()`.
      **Audit for the same class** (struct/enum memory punned as register arrays, `u32` fields in ABI
      types): `xous-rs` message/envelope paths, `xous-ipc`, `std`'s Xous PAL.
- [x] Hart timer (design: `planning/redoubt/TIMER.md`): delivered as IRQ 0, programmed through
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
- [x] First init process: `uart-echo` in `redoubt/test-programs`, `no_std`, `uart_16550` crate over claimed MMIO, echoes
      input from a userspace interrupt handler. Try it: `cargo testbench --run uart-echo`.
- [x] `redoubt/ipc-test`: `no_std` server (owns the UART) + client covering every message type.
- [x] Building and booting an rv64 bundle lives in the test bench (`redoubt/testbench`), not `xtask`.
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
5. Phase 2 kernel prerequisites, then `virtio-blk` (see Phase 2).

### Test bench (done 2026-09-18)
`redoubt/testbench`: declarative TOML cases, injects workspace or prebuilt binaries into the boot bundle,
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
      MEMORY_ALLOCATIONS, ...), then justify what is left. The ratchet in `redoubt/tests/unsafe-budget.toml`
      records progress.
- [x] **Ambient authority (devices)**: default-deny device grants. A manifest in the boot bundle
      (`grants` entry) lists each process's allowed MMIO regions and IRQs; the loader emits `Grnt`
      tags; the kernel enforces at MapMemory (device pages) and ClaimInterrupt. Design: DEVICE-GRANTS.md.
      Tests: `grant-attack`. Open: server-ID capabilities lack revocation; no runtime grant delegation.
- [x] Deleted ARM/x86 ports and the in-kernel gdb stub (7k lines, 6k a redundant disassembler; an
      ambient backdoor). Debugging is now QEMU's gdb stub for kernel/loader, a userspace debug server
      later for OS-level. Design: DEBUGGING.md. `cargo testbench --run <p> --debug` starts QEMU paused.
- [ ] Still to delete when rv32 is re-homed: swap, Precursor + bao1x platforms, the Sv32 window code,
      prebuilt asm blobs.
- [ ] Bench: inject a device tree, to test fail-closed paths (no rng-seed, no memory node, junk).
- [x] **Pure-Rust firmware works (tenet 3).** loader64 now boots identically under OpenSBI and RustSBI
      Prototyper, verified with the ipc bundle. The blocker was NOT RustSBI: it emits a valid device
      tree. The bug was ours -- the `fdt` 0.1.5 crate mis-parses RustSBI's re-serialized tree (cannot
      find `/chosen` or the memory node, panics on a valid tree). Fixed by switching loader64 to the
      mature `fdt-rs` parser (new `loader64/src/dt.rs`, one-pass `Platform` extraction), which handles
      both firmwares' trees. Per tenet 5, a parser that fails on a valid tree is a bug in us. RustSBI is
      not the working firmware in the bench yet (needs `--firmware` wired into more cases), but it boots.- [x] **Verified boot**: the loader authenticates the whole bundle with an embedded Ed25519 key
      (`ed25519-compact`, pure Rust, self-contained) before running any of it; tamper -> fail closed.
      Design: VERIFIED-BOOT.md. Test: `verified-boot-rejects-tamper`. Open: loader itself unverified on
      QEMU (needs firmware/ROM); no rollback protection or key rotation.
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

## Phase 2: I/O (design: `IO-ARCHITECTURE.md`)
Kernel prerequisites first:
- [ ] Transferable connections (send a capability over IPC); manifest declares the boot server graph.
- [ ] Server-death notification and connection revocation (restartable drivers).
- [ ] DMA page allocation gated by a manifest grant; manifest carries IOMMU device identity.
- [ ] Wider IRQ numbering.
- [ ] Drivers get MMIO/IRQ from startup arguments, not hardcoded (fixes the test programs).
Storage:
- [ ] `virtio-blk` driver server (`virtio-drivers`), hardened against a hostile device side, fuzzed.
- [ ] Block server: partitions, cache, block-range capabilities, per-block AEAD + Merkle root.
- [ ] fs server speaking 9P, one per volume (design: `NAMESPACES.md`).
- [ ] littlefs format in pure Rust (custom attributes for metadata), differentially tested against
      the C reference on the host; fuzz + crash injection. Fallback: our own spec'd CoW fs.
- [ ] Read-only boot-bundle fs server at `/boot`.
Namespaces and launching (`NAMESPACES.md`):
- [ ] Shared 9P2000 codec, fuzzed; namespace library (prefix table, lexical `..`); beamlet Platform.
- [ ] Startup block carries the namespace table.
- [ ] Launcher server + bare address-space primitive: only when runtime launching is first needed.
Network:
- [ ] Interface capability (frames in/out): the one link-layer type everything attaches through.
- [ ] `virtio-net` driver server; link server (802.1Q, frame filter, rate limits, egress queues).
- [ ] IP stack server on `smoltcp`, one instance per network, serving Plan 9 `/net` over 9P.
- [ ] Mid term: router server (LPM forwarding, stateful filter, NAT); control plane in Elixir.
- [ ] Key server; beamlet `:crypto` natives in Rust.
Trivial drivers: ns16550 (done), goldfish RTC.
Later:
- [ ] IOMMU backend (QEMU `iommu-sys`, needs QEMU >= 10; FPGA).
- [ ] Linux + redoubt partition under OpenSBI domains on QEMU, then the Orange Pi RV2.
- [ ] Retarget `std::fs` on the Xous target from PDDB to the fs server.

## Decisions without their own note
- **No dynamic linking** (2026-09-18). Native code is statically linked, signed as one binary, and
  fixed at build time. Shared-at-runtime code is a server, not a library. The launcher shares
  read-only pages of identical ELFs. The dynamic part of the system is the BEAM: modules load at
  runtime through beamlet's `Platform::load_module`, where signatures are checked.

## Design backlog (not yet designed), in the order we intend to settle them
Kernel-interface-bound first:
1. **Capability mechanism** (designed: `CAPABILITIES.md`, packages: `PACKAGES.md`): unforgeable handles instead of 128-bit
   password SIDs, transfer, badges, attenuation, revocation, death notification.
2. **Principals: users and AI agents as first-class, equal principals** (designed, same note).
3. **Resource accounting and quotas** (designed: `RESOURCES.md`, budgets).
4. **Startup block format** (designed: `INIT.md`).
5. **Init and supervision** (designed: `INIT.md`; all OS-process restarts in Rust init).
6. **Scheduling** (designed: `RESOURCES.md`, two classes + hierarchical stride + donation).
System structure: keys and root of trust (key server, sealed storage, disk key); signed A/B updates,
rollback protection, key rotation.
Services: entropy server, wall clock + NTP (Elixir), DNS (`inet_res`), log/audit server, userspace
debug server, terminals for SSH sessions, packaging and reproducible builds, Rust `std` target.
Later: desktop (virtio-gpu/input, display protocol, maybe a 9P tree as Plan 9's rio), SMP (Phase 3).

## Phase 3: SMP
- Note: OpenSBI picks the boot hart at random (observed hart 2 of 4). Never assume hart 0.
- [ ] Secondary hart bring-up via SBI HSM; per-hart trap stack; per-hart current (PID, TID) via `sscratch`.
- [ ] Big kernel lock at trap entry.
- [ ] 39 `static mut`s -> locked or per-hart (`MEMORY_MANAGER`, `PROCESS_TABLE`, `SWITCHTO_CALLER`, `PREVIOUS_PAIR`, ...).
- [ ] IPIs: reschedule + TLB shootdown on unmap / lend / return-memory.
- [ ] Per-hart run queues; locking for the shared per-process thread-context pages.
- [ ] Finer-grained locking only after the above is stable and tested.
