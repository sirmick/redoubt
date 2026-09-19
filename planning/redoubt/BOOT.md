# Boot flow

Built: rv32 and rv64 on QEMU `virt`. `cargo testbench --run <program>...` builds and boots it.
Owns: firmware -> loader -> kernel, the boot bundle and the kernel argument block. After the kernel
starts: INIT.md.

```
SBI firmware (OpenSBI / RustSBI), M-mode
  └─ loader, S-mode, MMU off           a0 = hart id, a1 = device tree
       └─ kernel, S-mode, Sv32/Sv39    a0 = args, a1 = process table, a2 = page-owner table, a3 = XPT
            └─ initial processes, U-mode
```

## Firmware
rv64 boots under OpenSBI (bundled with QEMU) or the RustSBI Prototyper (pure Rust; the bench case
`rustsbi-boot`). QEMU ships no rv32 OpenSBI, so rv32 always boots under RustSBI.
`scripts/fetch-rustsbi.sh` builds both Prototyper firmwares where the bench expects them (override
with `RUSTSBI_PROTOTYPER` / `RUSTSBI_PROTOTYPER_RV32`).

## Boot bundle
A plain ustar archive of ELF executables, passed as the initrd, preceded by a 64-byte Ed25519
signature over the archive (VERIFIED-BOOT.md). First entry = kernel (PID 1), the rest = initial
processes in PID order; the file name becomes the process name. A `grants` entry carries device
grants (DEVICE-GRANTS.md). Parsed with the `tar-no-std` and `elf` crates. (Trade-off: ELFs are
loaded by program header, so there is no execute-in-place from flash; not needed on these targets.)

## What the loader does
One loader serves both widths; paging comes from the width-generic `paging` crate.
1. Reads RAM, the initrd, the PLIC and this hart's S-mode context, the timebase, `/chosen/rng-seed`
   and every MMIO `reg` region from the device tree (`fdt-rs`).
2. Verifies the bundle signature before using any of the bundle; refuses to boot on failure.
3. Allocates pages from the top of RAM down, skipping firmware, itself, the device tree and the
   bundle. Every allocation is recorded in the page-owner table (1 byte per page = owning PID), which
   becomes the kernel's allocation table.
4. Builds the kernel address space (physmap, kernel ELF, stacks, `ProcessImpl` pages) and one address
   space per initial process. Every ELF segment and entry point is range-checked: programs must lie
   in the user area, the kernel in the kernel area.
5. Writes the kernel argument block: tagged entries with 64-bit payloads on both widths.
   `XArg` v2 `{words, version=2, ram_start: u64, ram_size: u64, name}`, `MREx` MMIO regions,
   `Plic`, `Seed` (RNG seed), `Time` (timebase), `Grnt` (device grants), and `IniE`/`PNam` per
   process. The kernel parses 64-bit fields through `u64`, never `usize`.
6. Enters the kernel without an identity mapping: `stvec` = kernel entry, then `csrw satp`. The next
   fetch faults and the hart traps straight to the kernel entry with a0-a3 and sp intact. All
   pointers handed to the kernel are physmap addresses.

Only the boot hart runs. Some bench cases boot QEMU with 2 or 4 harts; the extra harts stay
parked in the firmware; real SMP is later (PLAN.md).

## SBI consequences for the kernel
- S-mode `ecall` belongs to the firmware. The kernel's syscalls to itself (`SwitchTo` in the main
  loop) use `arch::syscall::kernel_syscall()`, which sets `sepc`/`scause`/`sstatus` as an `ecall`
  trap would and jumps to the trap vector.
- Kernel console = SBI debug console. The kernel owns no devices; the ns16550 belongs to userspace.
- A kernel panic powers the machine off through SBI SRST, so test runs terminate.

## Fail closed
- No usable `/chosen/rng-seed` (at least 16 bytes): the loader refuses to boot; a kernel started
  without a `Seed` tag panics. The kernel RNG is ChaCha8 keyed from it.
- A bad bundle signature, or an ELF outside its area: refuse to boot.

## Not done yet
- `env` block for processes (the loader passes 0) and `.eh_frame` reporting in `IniE` (for `std`).
- Startup blocks and device handles (INIT.md, DEVICE-GRANTS.md).
