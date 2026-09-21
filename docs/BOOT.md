# Boot flow

Built: rv32 and rv64 on QEMU `virt`. `cargo testbench --run <program>...` builds and boots it.
Owns: firmware, hardware abstraction, the loader, the boot bundle, the kernel argument block, and
the hart timer as it works today. After the kernel starts: INIT.md.

```
SBI firmware (OpenSBI / RustSBI), M-mode
  └─ loader, S-mode, MMU off           a0 = hart id, a1 = device tree
       └─ kernel, S-mode, Sv32/Sv39    a0 = args, a1 = process table, a2 = RAM page-owner table,
                                       a3 = MMIO page-owner table
            └─ initial processes, U-mode
```

## Firmware
rv64 boots under OpenSBI (bundled with QEMU) or the RustSBI Prototyper (pure Rust; the bench case
`rustsbi-boot`). QEMU ships no rv32 OpenSBI, so rv32 always boots under RustSBI.
`scripts/build-bios.sh` builds both Prototyper firmwares where the bench expects them (override
with `RUSTSBI_PROTOTYPER` / `RUSTSBI_PROTOTYPER_RV32`).

## Hardware abstraction
CPUs and SoCs differ in ways unrelated to XLEN, so code never uses `target_arch` to mean "has a PLIC"
or "runs under SBI".
- **Capability features** select backends: `sbi` (S-mode under SBI firmware: console, shutdown,
  `kernel_syscall()`, RNG seed), `plic` (the interrupt controller). New hardware = a new backend file
  and feature (`aia`, `sstc`, ...).
- **Board features** only compose capability features: `qemu-virt = ["sbi", "plic"]`. The FPGA card
  should be another one-line feature.
- **Discovered, not hardcoded:** RAM, MMIO regions, the PLIC and this hart's S-mode context come from
  the device tree via the loader. The kernel parses no device tree.
- **Width-bound only:** page-table geometry (`paging`), saved-context size and trap assembly key on
  `target_pointer_width`.
- Interrupt controller backend contract (`arch/riscv/irq.rs`): `enable_irq`, `disable_irq`,
  `disable_all_irqs`, `enable_all_irqs`, `pending` (at most one interrupt, from the PLIC claim or the
  timer), `mask`, optional `init`. The handler table covers the PLIC's 10-bit source space (IRQ 0 is
  the timer, 1..=1023 the PLIC).

## Boot bundle
A plain ustar archive of ELF executables, passed as the initrd and signed (VERIFIED-BOOT.md owns the
container). First entry = kernel (PID 1), the rest = initial processes in PID order; the file name
becomes the process name. A `grants` entry carries device grants (DEVICE-GRANTS.md). Parsed with the
`tar-no-std` and `elf` crates. ELFs are loaded by program header, so there is no execute-in-place
from flash; not needed on these targets.

## What the loader does
One loader serves both widths; paging comes from the width-generic `paging` crate.
1. Reads RAM, the initrd, the PLIC and this hart's S-mode context, the timebase, `/chosen/rng-seed`
   and every MMIO `reg` region from the device tree (`fdt-rs`).
2. Verifies the bundle signature before using any of the bundle; refuses to boot on failure.
3. Allocates pages from the top of RAM down, skipping firmware, itself, the device tree and the
   bundle. Every allocation is recorded in the RAM page-owner table (1 byte per page = owning PID),
   which becomes the kernel's allocation table; MMIO pages get a second owner table.
4. Builds the kernel address space (physmap, kernel ELF, stacks, `ProcessImpl` pages) and one address
   space per initial process. Every ELF segment and entry point is range-checked: programs must lie
   in the user area, the kernel in the kernel area.
5. Writes the kernel argument block (below) and the initial-process table: one page holding one
   `InitialProcess` record (satp, entry point, stack pointer, environment block) per process,
   kernel first. A bundle with more processes than the kernel has room for (64) is refused.
6. Enters the kernel without an identity mapping: `stvec` = kernel entry, then `csrw satp`. The next
   fetch faults and the hart traps straight to the kernel entry with a0-a3 and sp intact. All
   pointers handed to the kernel are physmap addresses.

## The kernel argument block
A page-aligned buffer of 32-bit words (`ARGS_PAGES` = 4 pages), built by `loader/src/args.rs` and
read by `kernel/src/args.rs`. It opens with `XArg` and is a sequence of tags:

    tag: u32 (four ASCII bytes)   crc16: u16   words: u16   data: [u32; words]

Everything is word-aligned and nothing else is: the kernel reads the block as words and never
casts a tag's data to a struct, because a struct with a `u64` field needs 8-byte alignment that
the block does not promise (a misaligned `MREx` cast was WP-K0b's bug). The same loader binary
serves both widths, so every address and size is two words, low word first; the kernel narrows
them through `u64` with a checked conversion (`args::wide`), so a value that does not fit a
`usize` stops the boot instead of being truncated.

The iterator stops at a header that does not fit, and refuses a tag whose data would run past the
end of the block, so a truncated or malformed block cannot make the kernel read outside it.

| Tag    | Data                                                                     |
| ------ | ------------------------------------------------------------------------ |
| `XArg` | total block size in words, version (2), RAM start (2 words), RAM size (2), RAM name |
| `MREx` | MMIO regions, six words each: start (2), size (2), name, padding. Sizes are whole pages |
| `Plic` | PLIC base (2 words), size (2), this hart's S-mode context                 |
| `Seed` | RNG seed (32 bytes); a kernel with no `Seed` panics                       |
| `Time` | timebase in ticks per second (2 words)                                    |
| `Grnt` | one per granted process: pid, MMIO count, IRQ count, then the regions (four words each) and IRQs (DEVICE-GRANTS.md) |
| `IniE` | one per initial process; today only counted, to size the process table    |
| `PNam` | one per initial process: pid, name length in bytes, then the name, padded to a word. `process_name` walks these records within the tag's own length |

**Decided change** (PACKAGES.md, launching): the loader will verify the bundle and load only the
kernel and `init`; `init` launches every other process through the loader stub, from the bundle's
pages. `IniE`/`PNam` per process and the `grants` entry then go, replaced by the boot manifest
(INIT.md).

Default builds run only the boot hart; with 2 or 4 harts the extra harts stay parked in the
firmware. With the `smp` feature, `arch/riscv/smp.rs` starts a second hart through SBI HSM at a
position-independent trampoline that turns paging on and enters the kernel (the `smp-spike` case).
Real SMP scheduling is after milestone 1 (PLAN.md).

## SBI consequences for the kernel
- S-mode `ecall` belongs to the firmware. The kernel's syscalls to itself (`SwitchTo` in the main
  loop) use `arch::syscall::kernel_syscall()`, which sets `sepc`/`scause`/`sstatus` as an `ecall`
  trap would and jumps to the trap vector.
- Kernel console = SBI debug console. The kernel owns no devices; the ns16550 belongs to userspace.
- A kernel panic powers the machine off through SBI SRST, so test runs terminate.

## The hart timer today
The RISC-V S-mode timer is a hart resource, not a device: the `time` counter, and a deadline
programmed through SBI TIME (or, with Sstc, `stimecmp`). Its interrupt arrives as a supervisor timer
trap, not through the PLIC. Today the kernel exposes it to userspace as if it were a device
(`arch/riscv/timer_sbi.rs`):
- the timer interrupt is delivered as **IRQ 0** (PLIC source 0 does not exist), claimed with
  `ClaimInterrupt`;
- user mode reads `rdtime` directly (`scounteren.TM` set);
- `PlatformSpecific` calls `TIMER_TIMEBASE` and `TIMER_SET_DEADLINE`, allowed only to IRQ 0's owner;
- one-shot: when it fires the kernel masks `sie.STIE` and dispatches IRQ 0; the handler re-arms it.

There is no preemption: threads are rescheduled when messages are delivered or they yield. The
decided design (the kernel owns the timer; interrupts are received, not handled) is in RESOURCES.md
and KERNEL-SPEC.md.

## Fail closed
- No usable `/chosen/rng-seed` (at least 16 bytes): the loader refuses to boot; a kernel started
  without a `Seed` tag panics. The kernel RNG is ChaCha8 keyed from it.
- A bad bundle signature, or an ELF outside its area: refuse to boot.
