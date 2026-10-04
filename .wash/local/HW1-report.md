# HW1 report: every constant held to the hardware field it mirrors

Branch wp-hw1 from main 53bcd9704. Three commits:

- a0fa4d527 kernel: a TID always fits the byte that records the last one
- add73f8f2 layout: user space and the kernel half held to the page-table formats
- 1fdd112f0 loader, docs: a device past the loader's table stops the boot

No constant's value changed.

## Delivered

1. `kernel/src/arch/riscv/process.rs`: `MAX_THREADS <= u8::MAX` (comment names
   `last_tid_allocated: u8`). It is folded into the existing TID-mask assert because the kernel
   was at its 8115-line ceiling, so a second line would have needed a raise.
2. `libs/layout/src/lib.rs` (layout depends on redoubt-sys, so the brief's first branch):
   - rv64: `redoubt_sys::USER_AREA_END <= 1 << 38`
   - rv32: `redoubt_sys::USER_AREA_END <= PHYSMAP_BASE`
   - rv64: `const fn canonical(addr) = (addr as isize) >> 38 == -1`, with one assert looping over
     PHYSMAP_BASE, KERNEL_PLIC_BASE, KERNEL_DMA_REGS, PROCESS_AREA, KERNEL_AREA, KERNEL_TEXT,
     KERNEL_STACK_TOP and TRAP_STACK_TOP.
3. `loader/src/dt.rs`: the silent drops are now
   `assert!(irq_len < MAX_IRQ, "interrupt {irq} is past the loader's {MAX_IRQ}")` and
   `assert!(mmio_len < MAX_MMIO, "MMIO region {name} is past the loader's {MAX_MMIO}")`. A loader
   panic prints `loader PANIC` and powers off (R17). The MMIO count includes the PLIC's region,
   since the loader always recorded it there. The message names the item and the limit; it
   does not spell out "33", so that each fits on one line under the loader's size ceiling.
   Tests: `a_33rd_mmio_region_is_refused` (32 virtio + PLIC), `a_33rd_interrupt_is_refused`
   (1 device raising 1..=33) and `thirty_two_devices_are_kept` (31 + PLIC = 32 regions, 32 irqs).
   The fixture grew `devices(n, irqs)` and `machine_with`.
4. `docs/kernel/boot.md`: "Hardware bounds" after "Hardware abstraction", with the brief's text
   and table plus a Status line (doccheck C1). The timebase row is written as the code checks
   it: "`timebase-frequency`, not 0". The loader reads any cell count as a u64, so "one 32-bit
   cell" is not checked; the orchestrator agreed. There is no ASID_BITS row because ASID1 has
   not merged. R17 changes:
   - the status list goes from (5) to (8);
   - the "partly tested" clause gains the device-table refusal;
   - the prose gains "and so does one with more MMIO regions or interrupts than the loader's 32
     of each".
5. `docs/SECURITY.md`: R17's row lists the three tests (doccheck C7).
6. `tests/size-budget.toml`: libs/layout goes from 61 to 83, as the orchestrator agreed. The
   `Size budget: libs/layout:` line is in add73f8f2.

QEMU virt and 32: rv64 virt has about 15 to 20 reg'd devices (8 virtio-mmio, uart, rtc, test,
pci, fw-cfg, flash, clint, plic) and about 14 interrupts. I argued that from QEMU's virt
machine rather than running QEMU (the host is shared). The whole bench will show it.

## Asserts broken by hand (each build failed with E0080, then was restored)

| Change | Width | Failure |
| --- | --- | --- |
| MAX_THREADS = 256 | rv64 | the TID assert |
| MAX_THREADS = 256, TID_WORDS = 5 (only the u8 half false) | rv64 | the TID assert |
| rv64 USER_AREA_END = 0x40_0000_1000 | rv64 | "user space past Sv39's lower half" |
| rv32 USER_AREA_END = 0x8000_1000 | rv32 | "user space reaches the kernel half" |
| PHYSMAP_BASE = 0xffff_ff80_0000_0000 | rv64 | "a kernel-half base is not a canonical Sv39 address" |
| TRAP_STACK_TOP = 0x7fff_ffff_ffff_0000 | rv64 | the same |

The loader's two refusals are exercised by the should_panic tests.

## Commands (all `/home/mcloonan/redoubt/.wash/local/in-dev ...` from the worktree, at 1fdd112f0)

| Command | Exit | Result |
| --- | --- | --- |
| cargo testbench size-budget | 0 | kernel 8115/8115, loader 868/868, layout 83/83 |
| cargo testbench unsafe-budget | 0 | unchanged; the only unsafe in my code is the existing test fixture's |
| cargo testbench formatting | 0 | |
| cargo testbench docs | 0 | |
| cargo testbench host-tests | 0 | the filter matched 16 host-test cases, all PASS |
| cargo test -p loader -p redoubt-layout | 0 | 6 + 4 pass |
| cargo build --release --target riscv64imac-unknown-none-elf -p redoubt-kernel --features redoubt-kernel/qemu-virt -p loader -p redoubt-layout -p redoubt-sys | 0 | |
| the same for riscv32imac-unknown-none-elf | 0 | |

Not run: the whole bench on both widths, which the orchestrator runs.

## Risks and notes

- The kernel and the loader are now exactly at their size ceilings, so any further line there
  needs a raise.
- I read every hunk of the diff in full, not whole files, per the brief's context rules.

## Fold 2 (hw1-implementer-2) — tip 585de34d9

- (1) dt.rs: fixture `devices()` now gives every device interrupts `1..=irqs` (was the first only; the three existing tests are unchanged in meaning); new host:loader::an_interrupt_two_devices_raise_takes_one_slot (`devices(2, 32)`: 64 entries -> irq table exactly 1..=32). R17 list and Hardware bounds status line gain it, tested (8) -> (9); SECURITY.md R17 row gains it. Commit message: "its four tests", the fourth fixture described.
- (2) boot.md Hardware bounds: `| the PTE and satp PPN | Sv32's 22 bits, Sv39's 44 | follows from the physmap's bounds |` after the kernel-half bases row, before the handle slot's frame index (the address-format rows, then the physmap-derived ones).
- Size: test lines do not count; loader stays 868/868, no budget change.
- Commands (in-dev): cargo testbench docs 0; size-budget 0; host-tests 0 (16 PASS); rustfmt +nightly --check dt.rs 0; git diff --check 0. No QEMU, no whole bench.
- Diff-of-diffs: .wash/local/HW1-fold2-range-diff.txt (git range-diff 53bcd9704..1fdd112f0 53bcd9704..585de34d9).
