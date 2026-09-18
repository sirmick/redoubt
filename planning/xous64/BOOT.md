# xous64 boot flow

Status: working on QEMU `virt` (2026-09-18). `cargo testbench --run <program>...` builds and boots it.

```
SBI firmware (OpenSBI / RustSBI), M-mode
  └─ loader64, S-mode, MMU off         a0 = hart id, a1 = device tree
       └─ kernel, S-mode, Sv39         a0 = args, a1 = process table, a2 = RPT, a3 = XPT
            └─ initial processes, U-mode
```

## Boot bundle
A plain ustar archive of ELF executables, passed as the initrd (`-initrd` on QEMU, `initrd` in U-Boot).
First entry = kernel (PID 1), the rest = initial processes in PID order; the file name becomes the
process name. No custom image tool: `tar --format=ustar -cf bundle.tar kernel a b c`.
Parsed with the `tar-no-std` and `elf` crates. This replaces `create-image` and the MiniELF format
for rv64. (Trade-off: ELFs are loaded by program header at boot, not pre-flattened, so there is no
execute-in-place from flash. Not needed on these targets.)

## What loader64 does
1. Reads RAM, initrd location and every MMIO `reg` region from the device tree (`fdt` crate).
2. Allocates pages from the top of RAM down, skipping firmware, itself, the DTB and the bundle.
   Every allocation is recorded in the RPT (1 byte/page = owning PID), which becomes the kernel's
   allocation table. Firmware and DTB stay owned by PID 1; loader and bundle memory is left free.
3. Builds the kernel address space: physmap gigapages, kernel ELF, kernel + exception stacks,
   `ProcessImpl` pages. Then one address space per initial process: shared kernel root entries, ELF,
   one stack page + demand-paged reservation, `ProcessImpl` pages.
4. Writes the kernel argument block. Same tag framing as rv32; 64-bit payloads:
   `XArg` v2 `{words, version=2, ram_start: u64, ram_size: u64, name}`,
   `MREx` entries `{start: u64, size: u64, name: u32, pad: u32}`, plus `IniE`/`PNam` per process.
5. Enters the kernel without an identity mapping: `stvec` = kernel entry, then `csrw satp`. The next
   fetch faults and the hart traps straight to the kernel entry with a0-a3 and sp intact.
   All pointers handed to the kernel are physmap addresses.

## SBI consequences for the kernel
- S-mode `ecall` belongs to the firmware. The kernel's syscalls to itself (`SwitchTo` in the main
  loop) use `arch::syscall::kernel_syscall()`, which sets `sepc`/`scause`/`sstatus` as an `ecall` trap
  would and jumps to the trap vector.
- Kernel console = SBI debug console. The kernel owns no devices; the ns16550 belongs to userspace.
- A kernel panic powers the machine off through SBI SRST, so test runs terminate.

## Hardening
- The kernel RNG is keyed from `/chosen/rng-seed` (`Seed` tag). Without one the kernel prints a loud
  warning, which the test bench treats as a failure.
- Every ELF segment and entry point is range-checked: programs must lie in the user area, the kernel
  in the kernel area. Kernel tables are shared by all address spaces, so this is not just hygiene.
- **No secure boot.** Stock Xous verifies ed25519 signatures on its images; the bundle is unverified.

## Not done yet
- `env` block for processes (loader passes 0) and `.eh_frame` reporting in `IniE` (needed by `std`).
- Handing the DTB to a userspace device manager.
- Secondary harts are left parked in the firmware.
