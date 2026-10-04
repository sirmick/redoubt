# Hardware bounds: every constant that mirrors a hardware field or platform limit (architect-11)

Swept on wp-k16 (the newest kernel). "Compile" = a `const _: () = assert!`; "boot" = checked
when the loader or kernel reads the platform, failing closed; "-" = not checked.

## The rule (proposed)

A constant that names or must fit a field the ISA fixes is held to that field by a compile-time
assert on each width. A limit the platform reports (RAM, the PLIC's size, the harts, the timebase,
the devices) is checked when the loader or kernel reads it, and a machine past it is refused at
boot, never truncated and never left to fail later at a moment a process chooses (R17's reason).

## The table

| Constant | Hardware field or limit | Bound | Checked | Where stated |
| --- | --- | --- | --- | --- |
| `MAX_PROCESS_COUNT` (511 after the ASID ruling) | `satp.ASID`: 9 bits Sv32, 16 Sv39 | < 2^ASID_BITS | - (K16 adds compile) | memory-layout.md `satp` (K16) |
| `MAX_PROCESS_COUNT` | the PID's own type (u16) | < 0xfffe | compile (mem.rs, budget.rs) | processes.md |
| `MAX_THREADS` (255) | TID mask words; `last_tid_allocated: u8` | < 4 x 64; <= u8::MAX | compile for the mask; **- for the u8** | processes.md |
| `USER_AREA_END` | Sv39's lower half is 2^38 bytes; Sv32's user half below the kernel half | rv64 <= 1 << 38; rv32 <= the lowest kernel-half address | **-** (debug_asserts on use only) | memory-layout.md "The two address maps" |
| `PHYSMAP_BASE`, `PROCESS_AREA`, `KERNEL_AREA` (rv64) | Sv39 canonical addresses: bits 63..39 equal bit 38 | each canonical | **-** | memory-layout.md Sv39 |
| `PHYSMAP_SIZE` vs RAM | RAM the platform reports | RAM inside the physmap | boot (loader, `physmap_covers`) | memory-layout.md, R17 |
| `PHYSMAP_PHYS_BASE + SIZE` | usize | no overflow | compile (layout) | - |
| `FRAME_BITS` (28) | physmap frames | physmap pages <= 2^28 | compile (handle.rs) | objects.md |
| PTE / `satp` PPN | Sv32 22 bits, Sv39 44 | every frame the physmap reaches | follows from the physmap bound (rv32 physmap ends below 2^32) | memory-layout.md "Sv32 and Sv39 compared" |
| `KERNEL_PLIC_BASE` window | the PLIC's MMIO size | PLIC ends below `KERNEL_DMA_REGS` | boot (against its reported size) | memory-layout.md, devices.md |
| `KERNEL_DMA_REGS`, `KERNEL_DMA_PAGES` | the kernel half's layout | clear of physmap, process area, stacks | compile (layout) | devices.md |
| `MAX_IRQS` (1024) | PLIC: sources 1..=1023 | every device IRQ < 1024 | boot (device.rs) | devices.md |
| `MAX_DMA_DEVICES` (16) | `Account::dma_mapped: u16` | <= 16 | compile (dma.rs) | devices.md |
| DMA pool (1,024 pages, top of RAM) | device DMA address width | pool below the narrowest device's reach | - ; holds: virtio addresses are 64-bit (legacy PFNs 44-bit), physmap ends at 2^37 | devices.md (not stated) |
| loader `MAX_MMIO`, `MAX_IRQ` (32 each) | the device tree's devices | every device kept | **- : the 33rd is dropped silently** (dt.rs:245, :251) | boot.md (not stated) |
| timebase | `timebase-frequency`, one DT cell | 0 < hz <= 2^32 - 1 | boot (0 refused; one cell bounds it) | timer.md |
| µs <-> ticks | 64-bit `time`, SBI `set_timer` u64 | saturating | by construction (checked_mul, saturate to NEVER) | timer.md |
| ABI 64-bit values, badges | rv32 registers are 32 bits | two registers each | by design (abi.md "One layout on both widths") | abi.md |
| record `usize` slots | target `usize` | a slot above 2^32 - 1 refused on rv32 | at decode (abi.md:142) | abi.md |
| `MAX_HANDLE_PAGES`, `HANDLES_PER_PAGE` | handle slot's u8 fields | <= 255 | compile (handle.rs) | objects.md |
| `MAX_HARTS` (SMP1, 8) | hart ids from the DT / SBI HSM; PLIC S-mode contexts in the mapped window | dense index < MAX_HARTS; context of the last hart inside the window | - (SMP1 adds) | m2 plan, SMP1 |
| kernel data region (1 MiB) | link.x `LENGTH` | every .bss table fits | link time (the linker) | memory-layout.md |
| PMP entries | firmware's | - | not ours: the kernel and loader program no PMP | fpga-platform.md |
| FPGA core: ASID 16, TLB tagged by hart, 2 harts a core | generation parameters | ASID >= 9 bits on any core we run | follows from the ASID assert | fpga-platform.md |

## What is missing

1. **ASID**: K16's item (K16-asid-bound-ruling.md).
2. **`MAX_THREADS <= u8::MAX`** for `last_tid_allocated`: one compile assert beside the mask
   assert in arch/riscv/process.rs.
3. **The user half and Sv39 canonical addresses**: in `libs/layout` (or `libs/sys` beside
   `USER_AREA_END`): rv64 `USER_AREA_END <= 1 << 38`; rv32 `USER_AREA_END <= PHYSMAP_BASE`;
   rv64, for each kernel-half base, `(addr as isize) >> 38 == -1`.
4. **The loader's device table**: dt.rs drops the 33rd MMIO region or IRQ without a word. Refuse
   the boot instead, naming the count, as RAM past the physmap is refused. A host test in the
   loader's DT tests with 33 devices.
5. **SMP1** (already its own package): `MAX_HARTS` with a dense hart index; a hart past it is not
   started and is reported; the last hart's PLIC S-mode context inside `KERNEL_PLIC_BASE`'s
   window, checked at boot against the PLIC's reported size.

## Where the rule lives

A new section **"Hardware bounds"** in `docs/kernel/boot.md`, after the loader's checks: the rule
above and a short table (constant, field, how it is checked), linking memory-layout.md's `satp`
for the ASID and devices.md for the PLIC and DMA rows. Written by the package that adds items
2-4, so the page states only what is built.

## Recommendation

Not K16: it is at commits 8-9 and should merge; it carries only the ASID item, which changes its
value. Items 2-4 are a small package, **HW1** (Tier A: kernel, layout, loader; size S): three
compile asserts, the loader's refusal with its test, and boot.md's section. Needs K16 (it edits
the same process.rs line). Item 5 goes into SMP1's brief, which I will add. DMA reach needs no
code today; boot.md's table states why it holds.
