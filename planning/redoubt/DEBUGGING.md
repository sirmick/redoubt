# Debugging

Decided; QEMU debugging is built. Tenet 1 (small TCB), tenet 2 (no ambient authority).

## Principle
The kernel provides *mechanism*, not a debugger. A debugger is protocol + UI + disassembly;
none of that belongs in the most privileged code. Debugging is either external (the host)
or a userspace process holding an explicit capability. There is no debugger in the kernel.

An in-kernel debug stub can read and write any process and hijack execution: ambient
authority, so there is none.

## Layers

| Need | Tool | TCB cost |
| --- | --- | --- |
| Bring-up ("did the loader run?") | QEMU `-s -S`; JTAG on real hardware | none (external) |
| Kernel / loader, source level | QEMU gdb stub + host `gdb`, using the ELF's symbols | none (external) |
| OS-level ("process 5, thread 2"), multicore | a userspace debug server + a small capability-gated kernel mechanism | ~300 lines, gated |
| Disassembly | host `gdb` / `objdump` | never in the guest |

QEMU's gdb stub and JTAG see raw hardware — harts, registers, physical memory — not our
abstractions (per-process `satp`, threads). That is fine for the kernel and loader, which
live in one supervisor address space, and for bring-up. OS-level debugging that knows about
processes and threads is the userspace server's job.

## Kernel/loader debugging today (QEMU)
Our ELFs carry full `.debug_info` even in release, so this is source-level out of the box:

    cargo testbench --run <program>... --debug     # boots QEMU paused with -s -S, prints the gdb line
    # then, in another shell:
    gdb target/riscv64imac-unknown-none-elf/release/xous-kernel
    (gdb) target remote :1234
    (gdb) break kmain
    (gdb) continue

(Plain `gdb` on this host understands riscv64; if a build does not, use `gdb-multiarch`.)

## The future OS-level debugger (deferred)
When there is userspace worth stepping through, add:
- **Kernel mechanism**, reached only through a debug capability (a handle): `debug_read_mem(pid, addr, len)`,
  `debug_write_mem`, `debug_regs(pid, tid)`, `debug_stop/continue(pid)`, `debug_wait_exception(pid)`.
  Expressed in processes/threads/address-spaces, so it spans harts naturally. Only a process
  granted `debug` authority may call it; in production the grant is simply never issued.
- **Userspace debug server** holding that capability, speaking the GDB remote protocol to host
  `gdb` over whatever transport it is granted. Host `gdb` disassembles; the guest never does.

This is the seL4 / Fuchsia model: minimal rights-gated introspection in the kernel, the
debugger in userspace. It is multicore-capable, keeps a disassembler and the
protocol out of the TCB, and debug authority is a capability like any other.

## What we removed
Stock Xous shipped an in-kernel GDB stub (about 7,000 lines, about 6,000 of them a hand-written
RISC-V disassembler) reached over the Precursor UART, to source-debug an FPGA device with no QEMU.
Deleted: TCB bloat, a redundant disassembler, and ambient authority. On our FPGA target, bring-up
uses JTAG and the card's trace and performance counters; OS-level debugging uses the userspace
server above.
