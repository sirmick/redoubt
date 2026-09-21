# Debugging

Decided; QEMU debugging is built. Tenet 1 (small TCB), tenet 2 (no ambient authority).

## Principle
The kernel has no debugger. Debugging is done from the host, or later by a userspace server holding a
debug capability. An in-kernel debug stub can read and write any process: ambient authority.

| Need | Tool | TCB cost |
| --- | --- | --- |
| Bring-up ("did the loader run?") | QEMU `-s -S`; JTAG on real hardware | none (external) |
| Kernel / loader, source level | QEMU gdb stub + host `gdb`, using the ELF's symbols | none (external) |
| OS-level ("process 5, thread 2"), multicore | a userspace debug server + a small capability-gated kernel mechanism | small, gated |
| Disassembly | host `gdb` / `objdump` | never in the guest |

## Kernel and loader debugging today (QEMU)
Our ELFs carry full `.debug_info` even in release, so this is source-level out of the box:

    cargo testbench --run <program>... --debug     # boots QEMU paused with -s -S, prints the gdb line
    # then, in another shell:
    gdb target/riscv64imac-unknown-none-elf/release/redoubt-kernel
    (gdb) target remote :1234
    (gdb) break kmain
    (gdb) continue

(Plain `gdb` on this host understands riscv64; if a build does not, use `gdb-multiarch`.)

## Later: an OS-level debugger
A small kernel introspection mechanism reached only through a debug capability (read and write a
process's memory and registers, stop and continue it), and a userspace debug server speaking the GDB
remote protocol to host `gdb`. In production the capability is never issued. Prior art: seL4, Fuchsia.

## What we removed
Stock Redoubt shipped an in-kernel GDB stub (about 7,000 lines, most of them a hand-written RISC-V
disassembler) reached over the Precursor UART. Deleted: TCB bloat and ambient authority. On the FPGA,
bring-up uses JTAG and the card's trace and performance counters.
