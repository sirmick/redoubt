# redoubt virtual memory design (Sv39)

Status: decided 2026-09-18, implementation in progress. Change this file first if the design changes.

## Decision 1: direct physical map instead of the page-table window

rv32 Xous maps every L0 page table as a data page inside a 4 MiB window (`0xff40_0000`), and the root
at `0xff80_0000`. The window's own L0 table is mapped into itself. That is neat with two levels. With
three levels it needs a 1 GiB window for L0 tables, a 2 MiB window for L1 tables, and tables that map
the windows that map the tables.

redoubt instead maps all of physical RAM once, in the kernel half, with Sv39 gigapages:

    virt = PHYSMAP_BASE + phys          (supervisor-only, global, RW, never executable)

Page tables are then reached as `PHYSMAP_BASE + table_phys`. Consequences:
- `pagetable_entry()` becomes a plain three-level software walk from `satp.ppn`.
- Creating a process no longer needs "map temp page, fill in, unmap" sequences.
- A page table never needs a virtual mapping of its own, so there is no self-referential bootstrap.
- The kernel can edit another process's tables without switching `satp` (needed for SMP later, where
  switching the local hart's address space to poke another process is a bad idea).

Cost: the kernel can address all RAM, so a kernel arbitrary-write bug is not limited to mapped pages.
The mapping is S-mode only and `sstatus.SUM` stays clear, so userspace cannot reach it and the kernel
cannot touch *user* mappings by accident. This is the standard trade on 64-bit kernels; we accept it.
MMIO is not in the physmap: devices stay explicitly mapped per server, as in stock Xous.

## Decision 2: address space split by root entry

Sv39: 3 levels x 9 bits, root entry = 1 GiB, canonical addresses only.

| Root idx   | Virtual range                         | Use                                                    | Shared? |
| ---------- | ------------------------------------- | ------------------------------------------------------ | ------- |
| 0..=255    | `0x0000_0000_0000_0000`..`0x3f_ffff_ffff` | Userspace (256 GiB)                                 | no      |
| 256..=383  | `0xffff_ffc0_0000_0000` + phys        | Physmap, up to 128 GiB, gigapage leaves                | yes (G) |
| 384..=509  | --                                    | Reserved                                               | --      |
| 510        | `0xffff_ffff_8000_0000`               | Per-process kernel data                                | no      |
| 511        | `0xffff_ffff_c000_0000`               | Kernel image, stacks, args, kernel MMIO                | yes (G) |

Sharing the kernel into a new process = copy root entries 256..=509 and 511 from the current root.
Those entries are created once by the loader and never change afterwards (the L1 table under 511 is
shared, so later kernel mappings appear everywhere without touching any root).

### Root 511: kernel (shared)
Addresses are the rv32 ones sign-extended, so the two ports stay easy to compare:

| Address                   | Use                                   |
| ------------------------- | ------------------------------------- |
| `0xffff_ffff_ffc0_0000`   | Kernel arguments, allocation tables   |
| `0xffff_ffff_ffcf_0000`   | Kernel console MMIO (if any)          |
| `0xffff_ffff_ffd0_0000`   | Kernel image + data                   |
| `0xffff_ffff_fff8_0000`   | Kernel stack top                      |
| `0xffff_ffff_ffff_0000`   | Exception stack top (boot hart; SMP makes this per-hart) |

### Root 510: per-process kernel data
| Address                   | Use                                                           |
| ------------------------- | ------------------------------------------------------------- |
| `0xffff_ffff_8000_0000`   | `THREAD_CONTEXT_AREA`: `ProcessImpl`, 2 pages (see below)     |
| `0xffff_ffff_8010_0000`   | `USERSPACE_BUFFER` (temporary; physmap should replace it)     |
| `0xffff_ffff_8080_2000`.. | Magic never-mapped return addresses (`RETURN_FROM_ISR`, `EXIT_THREAD`, ...) |

`ProcessImpl` on rv64: a saved context is 32 x 8 = 256 bytes. Slot 0 is the process header, padded to
256 bytes; slots 1..=31 are contexts. 32 x 256 = 8192 bytes = exactly 2 pages. Trap entry computes
`sp = THREAD_CONTEXT_AREA + (context_nr << 8)` (rv32: `<< 7`).

### Userspace (root 0..=255)
For now the rv32 constants are kept (`DEFAULT_HEAP_BASE = 0x2000_0000`, stack top `0x8000_0000`, ...)
so the userspace runtime ports unchanged. `USER_AREA_END = 0x40_0000_0000`. Spreading regions out over
the 256 GiB (and ASLR) is a later, userspace-visible change.

## satp / ASID
`satp = (8 << 60) | (pid << 44) | root_ppn`. PID stays the ASID (16 bits available, 8 used). All decoding
goes through helpers in `arch::mem` rather than open-coded shifts.

## PTE format
Sv39 PTEs are 64-bit with the PPN at bit 10, same bit positions as Sv32, and Xous's software bits
(`S` shared = bit 8, `P` swap = bit 9) sit in the RSW field in both. So PTE bit-twiddling code carries
over; only the walks and the PPN width change. Swap is not ported (feature stays rv32-only).

## SMP notes (Phase 3, recorded here so the layout doesn't paint us into a corner)
- Exception stack and "current context number" are per-process/global today; both must become per-hart.
  Plan: `sscratch` points at a per-hart block in root 511 holding the hart's trap stack and current
  (PID, TID); the context-number slot in `ProcessImpl` moves there.
- Unmap/lend/return must shoot down remote TLBs (SBI RFENCE, by ASID) before the page is reused.
