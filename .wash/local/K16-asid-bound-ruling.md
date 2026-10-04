# K16: the process limit within the ASID field (architect-11)

Owner (via the orchestrator): "stick within the ASID size … 9 bit on 32 bit, and 16 bit for 64
bit". Sv32's `satp` ASID is 9 bits, Sv39's 16 (the privileged spec).

## The rule

Every PID fits its width's ASID field: `MAX_PROCESS_COUNT < 2^ASID_BITS`, with `ASID_BITS` 9 on
rv32 and 16 on rv64. PID 0 is never a process, so PIDs 1..=`MAX_PROCESS_COUNT` all fit, and once
the kernel flushes by ASID a process's PID is its ASID with no table and no offset between them.
ASID 0, which no PID takes, stays what the kernel writes until then.

## K16's value moves: 512 -> 511

K16 numbers PIDs from 1 (the kernel's) to `MAX_PROCESS_COUNT`: `account_index` is `pid - 1`,
`pids()` is `1..=MAX_PROCESS_COUNT`, and process-fill draws 2..=512. PID 512 is 0x200, ten bits:
it does not fit Sv32's nine. So the value becomes **511** on both widths (one value, as now):
PIDs 1..=511, 510 besides the kernel's.

Rejected: keeping 512 with ASID = PID - 1. It is an offset every flush must remember, and it puts
the kernel's PID 1 on ASID 0, the value the kernel writes until it uses ASIDs.

## The assert (kernel/src/arch/riscv/process.rs, beside `MAX_PROCESS_COUNT`)

```rust
pub const MAX_PROCESS_COUNT: usize = 511;

/// The width of `satp`'s ASID field: 9 bits in Sv32, 16 in Sv39 (the privileged spec).
#[cfg(target_pointer_width = "32")]
pub const ASID_BITS: u32 = 9;
#[cfg(target_pointer_width = "64")]
pub const ASID_BITS: u32 = 16;

// Every PID, 1..=MAX_PROCESS_COUNT, fits the ASID field, so a PID can be its own ASID with no table
// between them (kernel/memory-layout.md, `satp`).
const _: () = assert!(MAX_PROCESS_COUNT < 1 << ASID_BITS);
```

(Use the file's existing width `cfg`s if they differ in form. mem.rs's and budget.rs's 16-bit
asserts stay: they guard the PID's own type.)

## Page lines

- **memory-layout.md, `satp`**: replace "its ASID is 0 on both widths. A PID is 16 bits, wider than
  Sv32's 9-bit ASID, and nothing needs it there: every switch, map and unmap flushes the whole TLB
  (below), so no translation is ever looked up by ASID." with:
  > its ASID is 0 on both widths. Every switch, map and unmap flushes the whole TLB (below), so no
  > translation is looked up by ASID yet. The process limit is held to the ASID field so that one
  > can be: `MAX_PROCESS_COUNT` is below 2 to the power of `ASID_BITS`, 9 in Sv32 and 16 in Sv39,
  > and PID 0 is never a process, so every PID fits the field, and once the kernel flushes by ASID
  > a process's PID is its ASID with no table between them. A compile-time assert holds it on each
  > width.
- **processes.md:44**: "(512: the PIDs there are, the kernel's included)" becomes "(511: the PIDs
  there are, the kernel's included, held below 2^9 so that each fits Sv32's ASID field
  ([`satp`](memory-layout.md#satp)))".
- **Every other 512 / 511 that counts PIDs** moves by one: budgets.md:146 (root's 511 -> 510
  processes), :641 (512 -> 511), ipc.md:496 and timer.md:293 (512 x 255 -> 511 x 255),
  objects.md:464, docs/todo/kernel-attack-gaps.md:35, process-fill (PIDs 2..=511, 509 others),
  pid-reuse-authority's and proc-lifecycle's comments, and c8's conditions line ("N live threads
  across 510 processes"). Grep `512` and `511` in the K16 diff; list each change in the report.
- **fpga-platform.md** needs nothing: "every process ID the kernel can make fits it" stays true.

Rerun: process-fill and thread-limit (both widths), pid-reuse-authority, and c8's worst walk at
the new full occupancy; the gate numbers do not move.

## The other reading: rv64 toward 16 bits

Raising rv64's limit toward 65,535 is not a value change. K16's per-PID static tables (about 800
bytes a PID) would be about 52 MB, against a 1 MiB kernel data region; the walks bounded by
`MAX_PROCESS_COUNT` (the constants clause of R12) would grow 128-fold. It needs PID-indexed state
allocated per process, from the process's own charged pages, with an index that costs no walk:
a design of its own, a package after IPC3 (whose per-endpoint lists remove the thread walks that
would otherwise scale with it). The owner's words allow both readings; the value above fits both.
