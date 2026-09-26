# An OS debugger

## Idea

A small kernel introspection mechanism reached only through a debug capability (read and write a
process's memory and registers, stop and continue it), and a userspace debug server that speaks
the GDB remote protocol to `gdb` on the host.

## Why it is not a goal

The kernel is debugged from the host, through QEMU's GDB stub, and on the FPGA through JTAG and the
card's trace. An in-kernel debugger is trusted code with authority over every process; the kernel
carries no test-only channel in production
([R23 (no test channels)](../kernel/scheduling.md#r23-no-test-channels)), and a debug path is the
most powerful such channel there is.

## What it would need

- The debug capability as a device-like object, created only at boot and never issued by a
  production manifest.
- Introspection limited to processes in budgets under the holder's own, so it grants nothing the
  holder could not already destroy.
- The debug server in its own budget, reached over SSH forwarding like any other inbound path.

**Attack cases:** a production boot has no debug capability anywhere; a holder cannot inspect a
process outside its own budgets.
