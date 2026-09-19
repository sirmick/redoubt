# Status against the tenets

What the code does today (not the design). `cargo testbench` enforces the rows marked (enforced).
Update this note with each milestone.

| Tenet | Today |
| --- | --- |
| 1 Simple | Kernel about 11.4k lines (`kernel/src`), loader about 1.1k, `paging` crate about 280. Trap entry and exit are `global_asm!` in `arch/riscv/asm.rs`. Kernel globals are `KernelCell`, not `static mut`. ARM, x86, the in-kernel gdb stub, Precursor, bao1x, VexRiscv, swap and the prebuilt assembly blobs are deleted. |
| 2 No ambient authority | Devices: (enforced) default deny; a process maps a device page or claims an IRQ only if the bundle's manifest granted it (DEVICE-GRANTS.md, interim). Physical RAM cannot be named by address, and anonymous pages are zeroed. Server connections are still stock Xous password capabilities (128-bit IDs): no handles, badges or revocation yet. |
| 2 W^X | (enforced) `paging::Pte::leaf` cannot express a writable and executable mapping; syscalls asking for one get `InvalidArgument`; the physmap alias of kernel code is read-only; the kernel checks its own address space at boot. Known gap: user code pages have a writable alias in the kernel-only physmap. |
| 2 Verified boot | (enforced) The loader verifies an Ed25519 signature over the whole bundle and refuses to boot otherwise. One public development key. Open: M-of-N, rollback protection, verifying the loader itself. |
| 2 Fail closed | No usable RNG seed or a bad bundle signature: refuse to boot. A kernel panic powers off. |
| 2 `unsafe` budget | (enforced) Every `unsafe` in the kernel, loader and `paging` has a SAFETY justification (0 undocumented; 211 undocumented at the fork). Budgets: `paging` 12, loader 17, SBI/PLIC/timer backends 15, RISC-V arch 10, kernel core 24. Totals only fall. |
| 3 All Rust | Kernel and loader: Rust plus `global_asm!`, no C toolchain, on both widths. Firmware can be pure Rust (RustSBI Prototyper; rv32 always uses it). |
| 4 Standards | SBI, PLIC, Sv32/Sv39, device tree, ELF, ustar. The kernel argument block is our own format, specified in BOOT.md. |
| 5 Dependencies | Kernel about 20 crates in its dependency tree, loader 8 direct (`fdt-rs`, `sbi-rt`, `elf`, `tar-no-std`, `crc`, `ed25519-compact`, `xous`, `paging`). None vendored or formally audited. |
| 6 Tested | (enforced) 16 cases, most on both widths, OpenSBI and RustSBI; attack cases listed below. No fuzzing yet; the kernel's hosted unit tests are not wired into the bench. |
| 7 Virtio | Nothing built; the only driver is the ns16550 UART in test programs. |

## Test cases (`redoubt/tests/`)
| Case | Checks |
| --- | --- |
| `ipc` | Every message type across address spaces: scalar, blocking scalar, lend, lend_mut, move |
| `timer` | Hart timer as IRQ 0: ownership, timebase, one-shot ticks re-armed from the handler |
| `uart-irq` | External interrupt through the PLIC to a userspace handler, with injected input |
| `rng` | Kernel RNG seeded from the device tree: server IDs differ within and between boots |
| `all-together` | IPC and timer tests sharing one log server |
| `rustsbi-boot` | The same loader and bundle boot under RustSBI |
| `kernel-wx` | The kernel's code is not writable, directly or through the physmap (attack) |
| `wx` | No writable and executable mapping through the syscall interface (attack) |
| `irq-attack` | Hostile interrupt syscalls: out-of-range, unowned, doubly claimed (attack) |
| `grant-attack` | An ungranted process is denied every device page and interrupt (attack) |
| `mem-attack` | Physical RAM cannot be mapped by address; anonymous RAM is zeroed (attack) |
| `uaf-lent-page` | A frame lent out by a dying process is not reused under the borrower (attack) |
| `loader-rejects-kernel-address` | A program segment in the kernel's range is refused (attack) |
| `loader-rejects-kernel-entry` | A program entry point in the kernel's range is refused (attack) |
| `verified-boot-rejects-tamper` | A bundle changed after signing is refused (attack) |
| `unsafe-budget` | The `unsafe` ratchet |
