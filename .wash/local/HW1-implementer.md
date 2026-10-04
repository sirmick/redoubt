# HW1: every constant held to the hardware field it mirrors

Tier A (kernel constants, `libs/layout`, the loader), size S. Needs K16 merged (it adds the ASID
assert beside the same line in `kernel/src/arch/riscv/process.rs`); start from main after it. Run
every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the
worktree.

## Context rules (read these first)

- **Don't read whole files.** `grep -n`, then Read a range. You need: the asserts beside
  `MAX_PROCESS_COUNT` and `TID_WORDS` in `kernel/src/arch/riscv/process.rs`; the `sv32`/`sv39`
  modules and the `const _` block in `libs/layout/src/lib.rs`; `USER_AREA_END` in
  `libs/sys/src/lib.rs`; the device loop (search `MAX_IRQ`, `MAX_MMIO`) and `mod tests` in
  `loader/src/dt.rs`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/HW1-report.md`.

## Reading list (only these)

- `.wash/local/hardware-bounds.md`: the sweep this package closes (its table and "What is
  missing").
- `docs/kernel/boot.md`: "Hardware abstraction" and R17 (fail closed).
- `docs/kernel/memory-layout.md`: "The two address maps" and `satp` (K16's ASID rule).

## The rule

A constant that names or must fit a field the ISA fixes is held to it by a compile-time assert on
each width. A limit the platform reports is checked when the loader or kernel reads it, and a
machine past it is refused at boot (R17), never truncated and never left to fail later.

## Deliverables

1. **`MAX_THREADS <= u8::MAX`**, beside the TID-mask assert in `arch/riscv/process.rs`, with a
   comment naming `last_tid_allocated: u8`.
2. **The user half**, in `libs/layout` (it sees both `USER_AREA_END` through `redoubt_sys` and
   the kernel-half bases; if it does not depend on `redoubt_sys`, put them in the kernel's
   `arch/riscv/mem.rs` beside `ROOT_PROCESS_AREA`'s assert):
   - rv64: `USER_AREA_END <= 1 << 38`, Sv39's lower half;
   - rv32: `USER_AREA_END <= PHYSMAP_BASE`, the lowest kernel-half address;
   - each with a comment naming the field.
3. **Sv39 canonical addresses**, rv64 only: every kernel-half base (`PHYSMAP_BASE`,
   `KERNEL_PLIC_BASE`, `KERNEL_DMA_REGS`, `PROCESS_AREA`, `KERNEL_AREA`, `KERNEL_TEXT`, the stack
   tops) satisfies `(addr as isize) >> 38 == -1`. A `const fn canonical(addr) -> bool` and one
   assert per base, or one assert over an array of them.
4. **The loader refuses a 33rd device.** `dt.rs` drops an MMIO region past `MAX_MMIO` or an
   interrupt past `MAX_IRQ` without a word. Refuse the boot instead, the way it refuses a boot hart
   without an S-mode context (a panic naming the count and the limit, which powers off through
   SBI SRST under R17). Host tests in `dt.rs`'s `mod tests`:
   `a_33rd_mmio_region_is_refused` and `a_33rd_interrupt_is_refused` (`should_panic`), and
   `thirty_two_devices_are_kept`. If 32 is too small for QEMU `virt` with the image's devices,
   report it; don't raise it without saying so.
5. **The page section** below.

Each assert must fail when its bound is broken: try each once by hand (change the constant, see
the build fail, change it back) and list them in the report. No mutation harness.

## Page lines (exact)

**boot.md**: a new section after "Hardware abstraction":

> ### Hardware bounds
>
> Every constant that mirrors a hardware field or a platform limit is held to it. A field the
> ISA fixes is a compile-time assert on each width, so a build that breaks it does not exist. A
> limit the platform reports is checked when the loader or kernel reads it, and a machine past it
> is refused ([R17 (fail closed)](#r17-fail-closed)), never truncated.
>
> | Constant | Field or limit | Checked |
> | --- | --- | --- |
> | `MAX_PROCESS_COUNT` | `satp`'s ASID, 9 bits in Sv32 and 16 in Sv39 ([`satp`](memory-layout.md#satp)) | compile time |
> | `MAX_THREADS` | the kernel's 8-bit last-TID field | compile time |
> | `USER_AREA_END` | Sv39's lower half (2^38 bytes); Sv32's half below the kernel's | compile time |
> | the kernel-half bases (rv64) | Sv39 canonical addresses | compile time |
> | the handle slot's frame index | the physmap's frames | compile time |
> | the kernel's windows (PLIC, DMA registers, process area, stacks) | each other and the physmap | compile time |
> | the physmap | the RAM the platform reports | at boot |
> | the PLIC window | the PLIC's reported size | at boot |
> | `MAX_IRQS` (1024) | the PLIC's sources, 1 to 1023 | at boot, per device |
> | the loader's device table (32 regions, 32 interrupts) | the device tree's devices | at boot |
> | the timebase | one 32-bit cell, not 0 | at boot |
> | the kernel's tables | its 1 MiB data region | at link time |
>
> Two hold by design rather than by a check: every 64-bit value in the ABI takes two registers
> on both widths ([ABI](abi.md)), and the DMA pool lies within every device's reach because
> virtio addresses are 64 bits. The firmware programs PMP; the kernel and loader program none.

If ASID1 has merged, its row stands in the table; if not, add it after `MAX_PROCESS_COUNT`'s:
`| ASID_BITS | the hart's satp ASID field, found by writing ones to it | at boot |` (ASID1
builds the check; the row is true only once it has merged, so add it only then).

Use the anchors as doccheck accepts them. If a row's check differs from what you find in the
code, the code wins: report it and write the row as the code is.

**boot.md, R17**: the status's test list gains the three loader tests (count + 3), and its
"partly tested" clause adds "the loader's device-table refusal" to the host-only list.

## Owned paths

- `kernel/src/arch/riscv/process.rs` (one assert), `kernel/src/arch/riscv/mem.rs` (if the
  user-half asserts go there), `libs/layout/src/lib.rs` (asserts), `loader/src/dt.rs` (the
  refusal and its tests).
- `docs/kernel/boot.md`.

**Not yours:** any constant's value. HW1 adds checks only.

## Gates

- The whole bench on both widths, alone.
- The loader's, the layout's and the kernel's host tests.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, each assert and its by-hand failure, the tests, and the
page lines as written.
