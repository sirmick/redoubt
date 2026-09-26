# Memory layout

Every address space has two halves. The lower half is user space, the process's own. The upper
half is the kernel's, the same in every address space and unreachable from user mode: it holds
the **physmap** (all of RAM at a fixed offset), the kernel image and stacks, the windows onto the
interrupt controller and DMA devices, and one area of per-process kernel data. This page gives
every fixed address, for Sv39 (rv64) and Sv32 (rv32), the conventions user space follows, and
what each page-table entry bit means to Redoubt.

## Purpose

The kernel, the loader, the loader stub and every launcher must agree on where things are. The
kernel-half constants live in one crate, `redoubt-layout` (`libs/layout`), which the loader uses
to build the map and the kernel uses to rely on it. The end of user space (`USER_AREA_END`) and
`PAGE_SIZE` (4096, on both widths) are `redoubt-sys`'s, because programs need them. The page-table
code is the `paging` crate (`libs/paging`), the only code that reads or writes a page-table entry.
What the mapping calls do with this layout is [memory](memory.md); the rules they keep are
R11 (memory) and R19 (kernel W^X).

## The direct physical map

Status: built · partly tested: no case has a process load, store or fetch at a physmap address; a case reads the boot-time check on rv64 only · tested: bench:kernel-wx

The loader maps all of RAM once, in the kernel half, at

    virt = PHYSMAP_BASE + (phys - PHYSMAP_PHYS_BASE)

Code computes it with `physmap_virt(phys)`, never `PHYSMAP_BASE + phys`, because the offset
differs by width. On Sv39 the map starts at physical 0 (`PHYSMAP_PHYS_BASE` = 0), so it also
covers the physical addresses below RAM, where the devices sit. On Sv32 it starts at RAM
(`0x8000_0000` on QEMU `virt`), which is also the start of the kernel half, so the map is the
identity there.

Every physmap entry is readable, global and supervisor-only: no `U` bit and never `X`. All are
writable but the kernel's read-only frames. The map is built from the largest leaves (1 GiB on
Sv39, 4 MiB on Sv32), except around those frames: for every kernel page that is not writable
(code and constants) the loader splits the superpages above it down to 4 KiB and clears `W` on
that frame's alias. At boot, before any
process runs, the kernel walks its own mappings and stops unless no executable kernel page is
writable and the physmap alias of each is neither writable nor executable (`verify_kernel_wx`,
[R19](memory.md#r19-kernel-wx)). It prints the verdict on every boot.

Nothing unmaps the physmap. So the kernel reaches any frame at any time, without switching
`satp` and without temporary mappings:
- page tables are walked and edited in software, from the root that `satp` names, through the
  physmap (`kernel/src/arch/riscv/physmap.rs`); another process's tables are edited the same
  way;
- kernel objects (budgets, handle-table pages) live in frames no process maps, reached through
  the physmap (`kernel/src/kframe.rs`);
- a system call reads or writes a user record by walking the caller's tables to the frame,
  checking the entry is a user mapping with the permission needed and not lent, and copying
  through the physmap (`user_frame`). The kernel never dereferences a user pointer.

## The split by root entry

Status: built · partly tested: a user-mode access to a mapped kernel-half address is not attacked by a case; the end of user space is attacked on the kernel on rv64 only, and on rv32 only in a host copy of the range check · tested: bench:map-fixed-attack, bench:loader-rejects-kernel-address, bench:loader-rejects-kernel-entry, host:redoubt-sys::map_fixed_range_check_refuses_rv32_wraparound

The root table's lower half is user space; its upper half is the kernel's. The kernel's root
entries are made once, by the loader, and never change afterwards. Creating an address space
allocates a root and copies every kernel-half entry into it from the current root, except the
one for per-process kernel data, which each address space fills with its own pages
(`MemoryMapping::allocate`). Sharing the kernel is therefore sharing those entries: the
physmap's leaves and the tables under the kernel area.

The kernel maps two things after boot: the interrupt controller (PLIC) and the DMA register
window ([devices](devices.md)). The loader creates the tables under both before any user root
copies the kernel's entries (`reserve_tables`), so those leaves appear in every address space
and the kernel maps them without allocating. Those are the only kernel-half mappings made after
boot.

The walls on the boundary:
- **Mapping calls stop at `USER_AREA_END`.** `map_fixed`, `process_map`, `unmap` and `set_flags`
  refuse a range that reaches past it with `InvalidArgument`, before they look at a page table.
  A demand-paging fault above it ends the process.
- **Boot images stay below it.** The loader refuses a program whose segment or entry point is
  outside `[PAGE_SIZE, USER_AREA_END)`, and the kernel image outside the kernel area
  ([R16 (image confinement)](boot.md#r16-image-confinement)).
- **No kernel-half entry has `U`.** A user-mode load, store or fetch there faults, and the fault
  handler ends the process.

## The two address maps

Status: built · partly tested: the constants' separation is checked at compile time and the PLIC's size at boot, not attacked by a case · tested: bench:kernel-wx, bench:legacy-gone

Addresses are from `libs/layout/src/lib.rs`, the kernel's `link.x` and `link64.x`, and
`kernel/src/arch/riscv/process.rs`. Compile-time assertions keep the DMA window clear of the
physmap, the per-process area and the kernel stacks; at boot the kernel stops if the PLIC the
device tree reports would run into the DMA window.

### Sv39 (rv64)

Root entries are 1 GiB each, 512 of them. Addresses from bit 38 up must all equal bit 38; any
other address is not canonical and every access to it faults.

| Root entries | From | What | Shared |
| --- | --- | --- | --- |
| 0..=255 | `0x0` | user space, up to `USER_AREA_END` (`0x40_0000_0000`, 256 GiB) | no |
| 256..=383 | `0xffff_ffc0_0000_0000` | physmap, `PHYSMAP_SIZE` (128 GiB), from physical 0, filled to the end of RAM | yes |
| 384..=509 | `0xffff_ffe0_0000_0000` | empty | |
| 510 | `0xffff_ffff_8000_0000` | per-process kernel data (`PROCESS_AREA`) | no |
| 511 | `0xffff_ffff_c000_0000` | the kernel area (`KERNEL_AREA`) | yes |

Inside the kernel area:

| Address | What |
| --- | --- |
| `0xffff_ffff_f000_0000` | PLIC window (`KERNEL_PLIC_BASE`), up to 64 MiB |
| `0xffff_ffff_f400_0000` | DMA register window (`KERNEL_DMA_REGS`): `KERNEL_DMA_PAGES` (16) pages, one per DMA device |
| `0xffff_ffff_ffd0_0000` | kernel code and constants (512 KiB) |
| `0xffff_ffff_ffd8_0000` | kernel data (512 KiB) |
| `0xffff_ffff_fff8_0000` | top of the kernel stack (`KERNEL_STACK_TOP`), 8 pages below it |
| `0xffff_ffff_ffff_0000` | top of the trap stack (`TRAP_STACK_TOP`), 8 pages below it |

```svgbob
 virtual address              root entries
+----------------------------+ 0xffff_ffff_ffff_ffff
| kernel area: image, kernel |
| and trap stacks, PLIC and  |  511         shared
| DMA register windows       |
+----------------------------+ 0xffff_ffff_c000_0000
| per-process kernel data    |  510         one per address space
+----------------------------+ 0xffff_ffff_8000_0000
| empty                      |  384 - 509
+----------------------------+ 0xffff_ffe0_0000_0000
| physmap: physical 0 to the |
| end of RAM, 1 GiB leaves   |  256 - 383   shared
+----------------------------+ 0xffff_ffc0_0000_0000
| not canonical: every       |
| access faults              |
+----------------------------+ 0x0000_0040_0000_0000
| user space, 256 GiB        |  0 - 255     one per address space
+----------------------------+ 0x0
```
*Figure: the Sv39 address map. Everything above the non-canonical gap is supervisor-only.*

### Sv32 (rv32)

Root entries are 4 MiB each, 1024 of them. Every 32-bit address is canonical.

| Root entries | From | What | Shared |
| --- | --- | --- | --- |
| 0..=511 | `0x0` | user space, up to `USER_AREA_END` (`0x8000_0000`, 2 GiB) | no |
| 512..=1019 | `0x8000_0000` | physmap, `PHYSMAP_SIZE` (2032 MiB), identity | yes |
| 1020..=1021 | `0xff00_0000` | PLIC window, up to 8 MiB less the DMA window | yes |
| 1021, last 64 KiB | `0xff7f_0000` | DMA register window: 16 pages | yes |
| 1022 | `0xff80_0000` | per-process kernel data (`PROCESS_AREA`) | no |
| 1023 | `0xffc0_0000` | kernel area: code at `0xffd0_0000`, data at `0xffd8_0000`, kernel stack top `0xfff8_0000`, trap stack top `0xffff_0000` (8 pages each) | yes |

A QEMU `virt` PLIC is 6 MiB, which is why the PLIC window takes two root entries.

```svgbob
 virtual address        root entries
+----------------------+ 0xffff_ffff
| kernel area: image,  |
| kernel and trap      |  1023          shared
| stacks               |
+----------------------+ 0xffc0_0000
| per-process kernel   |  1022          one per address space
| data                 |
+----------------------+ 0xff80_0000
| DMA register window  |  1021, 64 KiB  shared
+----------------------+ 0xff7f_0000
| PLIC window          |  1020 - 1021   shared
+----------------------+ 0xff00_0000
| physmap: RAM at its  |
| own address, 4 MiB   |  512 - 1019    shared
| leaves               |
+----------------------+ 0x8000_0000
| user space, 2 GiB    |  0 - 511       one per address space
+----------------------+ 0x0
```
*Figure: the Sv32 address map. Everything from `0x8000_0000` up is supervisor-only.*

### Per-process kernel data

`PROCESS_AREA` holds the current process's thread contexts: context 0 is the process header and
context N the saved registers of thread N, for `MAX_THREADS` (31) threads, 32 machine words each.
That is `THREAD_CONTEXT_PAGES` pages: 2 on Sv39, 1 on Sv32. The pages are the process's own,
charged to its budget, and carry no `U` bit. Because every address space maps its own pages at
the same address, the trap handler saves the interrupted thread's registers through one fixed
pointer, whichever process was running.

The rest of the per-process entry is never mapped. A new thread's return address is
`EXIT_THREAD`, an address there (`0xffff_ffff_8080_3000` on Sv39, `0xff80_3000` on Sv32): a thread
that returns from its entry function fetches there, takes an instruction page fault, and the
kernel ends the thread as if it had called `thread_exit` ([processes](processes.md)). A jump to
any other address there is an ordinary fault.

## User space

### Regions

Status: built · partly tested: the placement areas and the stack are conventions of the kernel and loader that no case attacks as addresses · tested: bench:map-fixed-attack, bench:map-fixed-tables

User space uses the same addresses on both widths, all below 2 GiB. On Sv39 the rest, up to
256 GiB, is free for `map_fixed` and `process_map` and nothing is placed there by default.

| Range | What | Placed by |
| --- | --- | --- |
| `0x0`..`0x1_0000` | free; page 0 is user space | nobody |
| `0x1_0000`..`0x1FF0_0000` | the program image: the link range | the program's linker |
| `0x1FF0_0000`..`0x1FF4_0000` | the loader stub, in a launched process (at most 256 KiB) | the launcher |
| `0x4000_0000`..`0x4040_0000` | the message area (4 MiB): where the kernel maps a lend or transfer the process receives | the kernel |
| `0x6000_0000`..`0x7000_0000` | the `map_anon` area (256 MiB): where `map_anon` places pages | the kernel |
| `0x7FF0_0000` | the startup block, in a launched process | the launcher |
| `0x7FFE_0000`..`0x8000_0000` | the first thread's stack: 32 pages (128 KiB) reserved, only the top one backed; the rest are backed on first touch | the loader for a boot process; a launcher places its own |

The kernel searches its two areas for the first free run of pages, starting after its last
choice; `map_anon`'s choice is [memory](memory.md)'s. A process started by the loader gets its
image, its stack and nothing else; its first thread starts with `sp` 16 bytes below
`0x8000_0000`. On Sv32 the stack top is also the end of user space.

```svgbob
+--------------------------+ 0x8000_0000   Sv32: end of user space
| first thread's stack     |               Sv39: user space goes on
| (128 KiB reserved)       |               to 0x40_0000_0000
+--------------------------+ 0x7ffe_0000
| startup block (launched) |
+--------------------------+ 0x7ff0_0000
| free                     |
+--------------------------+ 0x7000_0000
| map_anon area, 256 MiB   |
+--------------------------+ 0x6000_0000
| free                     |
+--------------------------+ 0x4040_0000
| message area, 4 MiB      |
+--------------------------+ 0x4000_0000
| free                     |
+--------------------------+ 0x1ff4_0000
| loader stub (launched)   |
+--------------------------+ 0x1ff0_0000
| program image            |
| (link range)             |
+--------------------------+ 0x0001_0000
| free, page 0 included    |
+--------------------------+ 0x0
```
*Figure: user space, the same on both widths. Launched processes get the stub and a startup block; boot processes do not.*

### Page 0

Status: built · partly tested: mapping page 0 is attacked on the kernel on rv64 only · tested: bench:map-fixed-attack, host:redoubt-sys::map_fixed_range_check_refuses_rv32_wraparound

User space starts at address 0 on both widths. No page is reserved at the bottom: `map_fixed`,
`process_map`, `unmap` and `set_flags` accept page 0 like any other page. A mapped page 0
affects only the process that maps it, or the child a parent maps it into with `process_map`,
because the kernel never dereferences a user pointer (see Why). Program images do not start
there: the loader and the stub refuse a segment on page 0.

### The loader stub and the link range

Status: built · tested: bench:stub-launch, host:stub::plan_refuses_a_segment_touching_page_zero, host:stub::plan_refuses_a_segment_reaching_into_the_stub_region

A launched process runs the [loader stub](../servers/init.md) first. Its launcher maps the stub's
flat binary read-only and executable at `STUB_ENTRY` (`0x1FF0_0000`, the same on both widths) and
starts the child's first thread there. The stub's `link.x` fixes that origin, limits the stub to
256 KiB, and fails the build if `_start` is not its first byte or if the stub has any writable
data. `STUB_ENTRY` is a constant of the `stub` crate, shared by the stub and every launcher, not
a kernel constant: `process_map`'s destination is the launcher's choice.

Programs link at `0x1_0000`. The stub refuses, before it maps anything, a segment that touches
page 0, that ends past `STUB_ENTRY`, or that overlaps the stub's region, the startup block or the
image bytes it came from. The child then exits with the stub's bad-image code, and nothing else
is affected. That leaves just under 512 MiB of link range, and `MAX_IMAGE_LEN` (`0x1FF0_0000`)
caps the image the startup block names at the same size.

### Launcher placement

Status: built · tested: bench:stub-launch

A launcher places the startup block and the child's stack outside the link range: `stub-launch`,
the launcher the bench runs, puts the stack top at `0x8000_0000`, the startup block at
`0x7FF0_0000` and the copy of the ELF image at `0x4000_0000`. The stub cannot see the stack. With
the stack outside the link range, no segment of an honest image can meet it; a hostile segment
that names the stack's pages is still refused, by `map_fixed`, which never replaces a mapping
([R11](memory.md#r11-memory)). The stub does not check for a gap between a segment and the
stack; see Residual risks.

## Sv32 and Sv39 compared

Status: built · tested: bench:wx, bench:touch-beyond-ram, bench:stub-launch, bench:map-fixed-tables

The two modes share the low ten entry bits, and the physical page number starts at bit 10 in
both. So one `usize`-sized `Pte` type and one flag set (`PteFlags`) serve both widths, and the
kernel's page-table code is written in terms of `LEVELS`, `vpn()` and `leaf_size()` and is the
same source for both. Only these differ, as `cfg(target_pointer_width)` constants in `paging`:

| | Sv32 (rv32) | Sv39 (rv64) |
| --- | --- | --- |
| levels | 2 | 3 |
| entries per table | 1024 | 512 |
| VPN bits per level | 10 | 9 |
| entry width | 4 bytes | 8 bytes |
| leaf sizes | 4 KiB, 4 MiB | 4 KiB, 2 MiB, 1 GiB |
| `satp` mode | bit 31 | `8 << 60` |
| `satp` ASID | bits 22-30 (9 bits) | bits 44-59 (16 bits) |
| `satp` root PPN | bits 0-21 | bits 0-43 |
| canonical addresses | all | bits 63-38 all equal |
| `USER_AREA_END` | `0x8000_0000` | `0x40_0000_0000` |
| `PROCESS_AREA` pages | 1 | 2 |

User mappings are always 4 KiB leaves; only the physmap uses superpages. On Sv39 the table below
a level-1 entry is named by its address, not by its index, because an index recurs in every
gigabyte: counting missing tables by index would undercount a range that crosses one
([R22 (range cost)](memory.md#r22-range-cost)). Sv32 has one level below the root, so the
question does not arise.

## `satp` and page-table entries

### `satp`

Status: built · partly tested: no case attacks a translation that outlives an address-space switch or an unmap

`satp` holds the mode, the process's PID as its ASID, and the root table's physical page number
(`make_satp`). PIDs fit every ASID width, because a PID is a byte. The kernel is PID 1; the loader
numbers boot processes from 2. The kernel reads the running PID back out of `satp`
(`current_pid`), so the running address space and the running process are one fact.

The kernel runs in whichever address space was current when it trapped, because every address
space maps the kernel half. Switching process writes `satp` and then runs `sfence.vma` with no
arguments, which drops every cached translation on the hart. Every map, unmap, lend, return and
permission change does the same. The loader enters the kernel through `satp` too: it points
`stvec` at the kernel's entry and writes `satp`; the next fetch, from the loader's own unmapped
address, faults, and the trap lands in the kernel with the arguments still in registers.

### Entry bits

Status: built · tested: bench:wx, bench:map-fixed-attack, bench:kernel-wx

```svgbob
 Sv39: bits 63-54 zero; PPN in bits 53-10
 Sv32: PPN in bits 31-10

 XLEN-1          10   9   8   7   6   5   4   3   2   1   0
+------------------+---+---+---+---+---+---+---+---+---+---+
| PPN              | P | S | D | A | G | U | X | W | R | V |
+------------------+---+---+---+---+---+---+---+---+---+---+
                   RSW: software
```
*Figure: a page-table entry, the same low ten bits on both widths.*

| Bit | Name | How Redoubt uses it |
| --- | --- | --- |
| 0 | `V` valid | set on every live mapping and every pointer to a next-level table |
| 1-3 | `R`, `W`, `X` | a leaf has at least one. Never `W` with `X`, never `W` without `R` ([R11](memory.md#r11-memory)): `Pte::leaf` refuses both, and a mapping call's flags are checked before that |
| 4 | `U` user | on every user-half leaf of a process; on no kernel-half entry |
| 5 | `G` global | on the kernel's shared leaves: the physmap, the kernel image and stacks |
| 6-7 | `A`, `D` | set when the entry is made, because not every hart updates them in hardware; Redoubt never reads them |
| 8 | `S` lent | software bit: see below |
| 9 | `P` | software bit, never set |

A non-leaf entry is `V` alone. An entry with permissions and no `V` is a **reservation**: the
page is backed with a zeroed frame on first touch. A permission fault on a valid page ends the
process; it never counts as a page that wants backing.

### The lent bit

Status: built · partly tested: a lender's own load or store to a page it has lent out is not attacked by a case · tested: bench:return-lent-unmapped, bench:move-borrowed-page, bench:map-fixed-attack, bench:uaf-lent-page

A [lend](ipc.md) is recorded in the page tables themselves, with the software bit `S`:

| `V` | `S` | Meaning |
| --- | --- | --- |
| 1 | 0 | an ordinary mapping |
| 0 | 1 | the **lender's** entry while the page is lent: no access, and the entry keeps the frame, so it is the loan's only record |
| 1 | 1 | the **borrower's** alias: normal loads and stores, but no call may unmap it, change its flags, pass it on, or use it as a system-call record |
| 0 | 0 | empty, or a reservation |

Lending clears `V` and sets `S` on the caller's entries; the pages are mapped into the server,
with `S`, only when it takes the call. A reply clears the server's entries and restores `V` on the
caller's, checking first that both still name the same frame. A transfer is mapped into its
receiver without `S`: the page is simply the receiver's. Only the end of the loan changes a
lender's entry: a reply restores it, and a transfer or an abandoned call removes it. No new
mapping, reservation or unmap overwrites it (I9 (pages W^X, zeroed, lends unmapped);
[R3 (lends and abandoned calls)](ipc.md#r3-lends-and-abandoned-calls)).

## Residual risks

- **The physmap has a writable alias of user code.** Every user page, code included, is also
  mapped writable in the physmap. Only the kernel can use that mapping, but a kernel bug that
  writes through a wrong address can change any process's code. The kernel's own code has no
  writable alias (R19).
- **The kernel can address all RAM.** A kernel arbitrary-write bug is not limited to the pages
  the current process maps. On Sv39 the physmap also covers the physical range below RAM, where
  device registers sit, as ordinary kernel read-write memory; the kernel never uses those
  addresses, but a stray write through them reaches a device.
- **RAM larger than the physmap is not refused cleanly.** The loader does not check RAM against
  `PHYSMAP_SIZE` (128 GiB on Sv39, 2032 MiB on Sv32). A larger machine stops at boot on a later
  assertion, not on a clear refusal. Follow-up: [todo](../todo/physmap-ram-bound.md).
- **Every change flushes everything.** Each map, unmap, lend and address-space switch runs a
  global `sfence.vma`, so ASIDs save no work, and each flush costs page-table walks afterwards.
  It is also a flush of this hart only: with more than one hart, another hart's cached
  translations would survive an unmap ([beyond M5: SMP](../beyond/smp.md)).
- **The firmware must delegate instruction page faults to S-mode.** Kernel entry from the loader
  and a thread's return to `EXIT_THREAD` are both instruction page faults the kernel must take.
  The vendored RustSBI delegates them; with a firmware that did not, the boot would never reach
  the kernel. The firmware is in the TCB ([boot](boot.md)).
- **`SUM` is clear by default, not by the kernel's hand.** The kernel never sets `sstatus.SUM`,
  and it never clears it either: it relies on the firmware entering S-mode with it clear. With
  `SUM` set, a kernel bug that dereferenced a user address would read the process's memory
  instead of faulting. Follow-up: [todo](../todo/clear-sum-at-entry.md).
- **No guard gap between a segment and the stack.** The stub checks segments against the stub,
  the startup block and the image, not against the stack, and the kernel refuses only an
  overlap. A launcher that puts the stack inside the link range can get a child whose data ends
  right at its stack's bottom, so a stack overflow writes the data instead of faulting. The
  launcher convention above prevents it; nothing enforces it.
- **No ASLR.** Every address on this page is fixed, and `map_anon` places pages deterministically.
  A memory-safety bug in a program is easier to exploit, within that program's own process.
  Address randomisation is [beyond M5](../beyond/aslr.md).

## Why

- **A direct physical map, not a recursive window.** Mapping the page tables into themselves
  works with two levels but not three. With every frame at a fixed address, a table walk is a
  plain software walk, creating a process needs no map-fill-unmap dance, and the kernel edits
  another process's tables without switching `satp`. The cost is that all RAM is
  kernel-addressable (Residual risks).
- **Split by root entry.** Sharing the kernel half is copying some root entries. Because the
  loader makes every kernel-half table before the first user root exists, and the kernel never
  adds a root entry, no kernel mapping ever has to be pushed into existing address spaces.
- **Page 0 in user space, `SUM` clear.** The null-page attack needs the kernel to dereference a
  pointer that user space controls. The Redoubt kernel never does: it walks the caller's tables
  and copies through the physmap, and with `SUM` clear any direct supervisor access to a user
  page faults. So a mapped page 0 can harm only its own process, and refusing it would be a rule
  with no wall behind it.
- **The stub at a fixed address, high in the link range.** Every launcher and the stub agree
  without a kernel constant, the kernel stays ignorant of ELF, and programs keep just under
  512 MiB to link into, so large programs need no special base.
- **The same user addresses on both widths.** One convention serves rv32 and rv64 programs and
  their launchers. Sv39's extra user space stays free for programs that ask for it by address.
- **The lent bit in the entry.** The lender's own entry is the record of the loan, so the kernel
  keeps no separate table of loans that could disagree with the page tables, and a teardown finds
  every lent-out frame by walking them.
