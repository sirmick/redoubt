# K16: the kernel data region grows to 1 MiB (Architect's ruling, 2026-10-03)

Commit 5 (512 PIDs, `TidMask [u64;4]`) stopped per the brief's ruling 6: release-kernel
`.data + .bss` left 59,920 B of the 512 KiB region on rv64 (68,192 B on rv32), under the 64 KiB
floor. The biggest table is `MEMORY_MANAGER` at ~410 KB (~800 B a PID: `Account` with its
handle table, the object tables); the brief's 375 KB estimate counted only `Account`'s handles
and IPC.

## The ruling

K16 grows the kernel data region from 512 KiB to 1 MiB on both widths, (a) only. No shrink:
ruling 6 keeps per-PID tables static (none becomes budget-charged frames), and moving
`Account`'s handle table out would reopen that for about 16 KB of gain. 1 MiB leaves about
570 KiB of headroom on rv64 (580 KiB on rv32) for SMP's per-hart state and the next limit.

## What K16-4 does

- `kernel/link.x` and `link64.x`: RAM `LENGTH = 1024K`, ORIGIN unchanged (`0x..ffd8_0000`). The
  region then ends at `0x..ffe8_0000`, below the kernel stack's lowest page (`0x..fff7_8000`) on
  both widths. Assert that in the linker scripts, the way they already assert alignment.
- Check and report two things: the loader backs only the segments' `p_memsz`, so the larger
  region costs no RAM until it is used; and the loader's `allowed` range for the kernel and
  anything in `libs/layout` that bounds the data region take the new end. Say where each lives.
- The 64 KiB headroom rule stays, now of 1 MiB. Report `.data`, `.bss` and headroom on both
  widths again.

## Page lines (memory-layout.md, in K16's commit)

- the Sv39 kernel-area table row "`0xffff_ffff_ffd8_0000` | kernel data (512 KiB)" becomes
  "kernel data (1 MiB)";
- the Sv32 root-entry 1023 row: after "data at `0xffd8_0000`" insert " (1 MiB)";
- if any other line states the data region's size, change it to match.

K16's report states the ~800 B a PID cost of `MEMORY_MANAGER` as the real number.
