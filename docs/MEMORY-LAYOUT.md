# Virtual memory (Sv32 and Sv39)

Built, both widths. Owns: the physmap, the address-space split, per-width layouts. Constants live in
`libs/abi/src/arch/riscv/mem.rs`; the page-table code is the `paging` crate (`libs/paging/`), the
only code that edits page-table entries.

## Decision 1: a direct physical map
Stock Redoubt maps every page table as a data page inside a window (a self-referential trick that works
with two levels but not three). Redoubt instead maps all of physical RAM once, in the kernel half:

    virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)   (supervisor-only, global, RW, never executable)

Use `physmap_virt(phys)`, never `PHYSMAP_BASE + phys` (the offset is nonzero on rv32).
Consequences:
- Page-table walks are plain software walks from `satp`.
- Creating a process needs no "map temp page, fill in, unmap" sequences.
- The kernel edits another process's tables without switching `satp` (needed for SMP).

Cost: the kernel can address all RAM, so a kernel arbitrary-write bug is not limited to mapped pages.
The mapping is S-mode only and `sstatus.SUM` stays clear, so userspace cannot reach it and the kernel
cannot touch user mappings by accident. The physmap alias of kernel code is read-only. MMIO is not in
the physmap: devices are mapped explicitly per server.

## Decision 2: address space split by root entry
The top root entries are the kernel's and are shared by every address space; creating a process copies
them from the current root. Those entries are created once by the loader and never change (the
loader pre-creates the shared intermediate tables, so later kernel mappings, such as the PLIC, appear
everywhere).

| | Sv39 (rv64) | Sv32 (rv32) |
| --- | --- | --- |
| Userspace | root 0..=255, below `0x40_0000_0000` | root 0..=511, below `0x8000_0000` |
| Physmap | root 256..=383 at `0xffff_ffc0_0000_0000`, from physical 0, 1 GiB leaves | root 512..=1019 at `0x8000_0000`, identity (QEMU RAM starts there), 4 MiB leaves |
| Interrupt controller | `0xffff_ffff_f000_0000` | root 1020..=1021 at `0xff00_0000` |
| Per-process kernel data | root 510 at `0xffff_ffff_8000_0000` | root 1022 at `0xff80_0000` |
| Kernel image, stacks, arguments | root 511 at `0xffff_ffff_c000_0000` | root 1023 at `0xffc0_0000` |

Per-process kernel data holds `ProcessImpl` at `THREAD_CONTEXT_AREA` (slot 0 is the process header,
slots 1..=31 are saved thread contexts: 2 pages on rv64, 1 on rv32) and `USERSPACE_BUFFER`
(temporary; the physmap should replace it). Userspace regions are the same on both widths
(`DEFAULT_HEAP_BASE = 0x2000_0000`, stack top `0x8000_0000`); spreading out over the rv64 space
(and ASLR) is a later, userspace-visible change. User space starts at address 0 on both widths:
no page is reserved at the bottom, so a call that names a user address (`map_fixed`,
`process_map`, `unmap`, `set_flags`) accepts page 0. SUM stays clear, so the kernel never
dereferences a user pointer and a mapped page 0 cannot turn a kernel null dereference into an
attack.

**The loader stub** (PACKAGES.md, Launching a process) is mapped in every launched process at
`STUB_ENTRY = 0x1FF0_0000` on both widths, its entry point and the start of its one fixed code
region, which runs to at most `0x1FF4_0000` (256 KiB, the stub's `link.x`), just below
`DEFAULT_HEAP_BASE`. The value is a convention between the stub and its launchers
(`stub::STUB_ENTRY`), not a kernel constant: `process_map`'s destination is the launcher's choice. It constrains every program: no loadable
segment may overlap that region, or the stub refuses the image and the process exits. Programs
link at `0x1_0000`, and their segments must end below `0x1FF0_0000`, which leaves just under
512 MiB, so large programs such as beamlet need no special link base.

## Sv32 vs Sv39
Same low 10 PTE flag bits; the physical page number starts at bit 10 in both. So one `usize`-based
`Pte` and one flag set serve both; only these are width-specific:

| | Sv32 | Sv39 |
| --- | --- | --- |
| levels | 2 | 3 |
| entries per table | 1024 | 512 |
| VPN bits per level | 10 | 9 |
| PTE width | 4 B | 8 B |
| leaf sizes | 4 KiB, 4 MiB | 4 KiB, 2 MiB, 1 GiB |
| `satp` mode | 1 << 31 | 8 << 60 |
| `satp` ASID | bits 22..30 | bits 44..59 |
| canonical addresses | all 32 bits | sign-extended at bit 38 |

Code keys on `target_pointer_width` only for these, the saved-context size, the trap assembly, and
the ABI's register encoding (PLAN.md). The last needs none in practice: a 64-bit argument always
takes two 32-bit register halves on both widths (KERNEL-SPEC.md, ABI), so `redoubt-sys` has no width
`cfg` at all.

## satp, PTEs, W^X
- `satp` = mode | (PID as ASID) | root PPN. All decoding goes through `arch::mem` helpers.
- Redoubt's software PTE bits sit in the RSW field on both widths: `S` (lent, bit 8); bit 9 is unused
  (swap is deleted).
- `paging::Pte::leaf` cannot express a writable and executable mapping; the kernel re-checks its
  own address space at boot and refuses to run otherwise (tests `wx`, `kernel-wx`).

## Accepted trade-offs
- The physmap makes all RAM kernel-addressable, including a writable alias of user code pages.
- Every map and unmap does a global `sfence.vma`.
- Kernel entry relies on the firmware delegating instruction page faults to S-mode (BOOT.md).
- For SMP, the exception stack and "current context" must become per-hart (PLAN.md, SMP).
