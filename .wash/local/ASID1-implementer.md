# ASID1: each process is its own ASID

Tier A (the kernel's memory core), size M. Needs K16 merged; start from main after it. Written
against K16's c5 (f6ff9cd8c, the 511 bound) and its tip ff3ad8464, whose `arch/`, `libs/paging`
and `loader/` are the same as c5's; recheck the line numbers below on main. Run every cargo and
bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## Context rules (read these first)

- **Don't read whole files.** `kernel/src/arch/riscv/mem.rs` is about 800 lines: `grep -n`, then
  Read the functions named below. In `asm.rs` read only `flush_mmu`; in `arch/riscv/process.rs`
  only the block beside `MAX_PROCESS_COUNT` and `PidSlots`; in `libs/paging/src/lib.rs` only
  `make_satp`, `satp_root`, `satp_is_active` and the `SATP_*` constants.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.** If you must open a QA file, read
  it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Read boot logs under `target/testbench/last/` only through `grep` or
  `tail`.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/ASID1-report.md`.

## Reading list (only these)

- `docs/kernel/memory-layout.md`: "The split by root entry", "Per-process kernel data", "`satp`",
  "Entry bits" (the `G` row) and "Residual risks".
- `docs/kernel/memory.md`: the residual on freed frames ("Every path that frees a mapped frame…").
- `docs/kernel/processes.md`: the PID-reuse paragraph ("The hardware address-space id is reused…").
- `docs/kernel/boot.md`: R17, and "Hardware bounds" (HW1's table).
- The privileged spec's `SFENCE.VMA` section and its software guidelines (leaf change: `rs1` =
  the address; non-leaf change: `rs1` = `x0`; `G` on the path: `rs2` = `x0`; ASID reuse: `rs1` =
  `x0`, `rs2` = the ASID; writing `satp` orders and invalidates nothing).

## The problem

`satp` carries ASID 0 (`make_satp`), and every switch (`MemoryMapping::activate`), map, unmap,
lend and permission change runs a bare `sfence.vma` (`flush_tlb`, `asm.rs`'s `flush_mmu`), which
drops every translation on the hart, the kernel's global ones with it. K16 held the process limit
to the ASID field (`MAX_PROCESS_COUNT` = 511 < 2^`ASID_BITS`, asserted in `arch/riscv/process.rs`)
so that a PID can be its own ASID. This package uses it.

## The settled design

1. **The ASID is the PID.** `make_satp(root_phys, asid)` in `libs/paging` (with `satp_asid`), on
   both widths; the loader keeps passing 0 for what it builds. `MemoryMapping::allocate` writes
   the new process's PID; `from_init_process` rewrites each boot process's `satp` with its own
   PID (the handoff record names it); the kernel's own space (PID 1) takes ASID 1 at boot.
2. **The width, at boot.** Before anything else writes `satp`, the kernel writes its current
   `satp` with the ASID field all ones, reads it back, restores it, and counts the bits that
   stuck. Fewer than `ASID_BITS` refuses the boot under R17, naming both numbers; otherwise it
   prints the width once (`asid: N bits`). Then it switches to ASID 1 and runs one bare
   `sfence.vma`: the boot's only whole flush, which also drops the loader's ASID 0 entries. The
   decision is a pure function of the read-back value, host-tested.
3. **A switch flushes nothing.** `activate` writes `satp` and returns.
4. **Every page-table write flushes what it changed, in the space it changed, running or not:**

   | Site (mem.rs) | What it changes | Becomes |
   | --- | --- | --- |
   | `activate` | `satp` | no flush |
   | `map_page_inner` | a leaf in the current space | `(virt, pid)`; whole ASID if `map_page_in` linked a table |
   | `map_kernel_page` | a kernel-half leaf (the DMA register window) | the leaf gains `G`; `(virt, x0)` |
   | `unmap_page_inner` | a leaf, current space | `(virt, pid)` |
   | `return_page_inner` | a leaf in each of two spaces | `(src, src pid)` and `(dest, dest pid)` |
   | `lend_out`, `lend_back`, `drop_lent`, `unmap_from` | a leaf in `space` | `(virt, space's pid)` |
   | `map_into`, `map_into_with` | a leaf in `space` | `(virt, space's pid)`; whole ASID if a table was linked |
   | `free_empty_tables` | a table pointer in `space` | whole ASID `(x0, pid)`, before the frame is freed |
   | `ensure_page_exists_inner` | a leaf, current space | `(virt, pid)`; for the kernel PID above `USER_AREA_END`, say which table it writes and flush to match |
   | `set_user_page_flags` | a leaf, current space | `(virt, pid)` |

   `map_page_in` reports whether it linked a new table. The ASID of a space is read from its
   `satp` (`satp_asid`), never passed beside it. `flush_tlb` goes; `asm.rs` keeps three
   routines (whole, `(x0, asid)`, `(addr, asid)`, with `asid` 0 meaning `x0` only for the
   global form, which gets its own name). If you find a site this table misses, it is a finding:
   report it.
5. **`G` exactly on the shared kernel half.** Every leaf the kernel half shares (the physmap,
   the image, the stacks, the DMA window) is global; nothing in the user half, and nothing on
   the path to `PROCESS_AREA`, the per-process header at the same address in every space, is.
   A `G` there would serve one process's header to the next.
6. **A PID given out is flushed first.** In `MemoryMapping::allocate`, called from
   `ProcessTable::allocate_process_slot` for the PID `random_free_pid` drew: after the `satp` is
   made and before `add_header_page`, the first map, flush `(x0, pid)`. Every allocation, not
   only a known reuse: the kernel does not track which PIDs have run.
7. **A destruction moves off the space before freeing it.** Before any frame of a dying
   process is freed (its tables included), if its space is the hart's `satp`, the kernel moves
   `satp` to its own space (PID 1); then it flushes `(x0, pid)`. A walker reading a freed table
   frame through a live `satp` could otherwise cache a garbage leaf, and a garbage `G` leaf
   survives every ASID flush. One call at the start of the frame release, inside `release_owned_frames`
   (`kernel/src/mem.rs`).
8. **The checked build's audit.** A per-hart log of unflushed changes: every page-table write
   records `(asid or global, address or all)`, every flush removes what it covers (a whole flush
   all, `(x0, asid)` that ASID's non-global records, `(addr, asid)` that pair, `(addr, x0)` that
   address in every ASID). An allocation records `(pid, all)`. At every return to user mode the
   log is empty, or the kernel stops naming the record. At boot and after each kernel-half
   change, a walk of the current root checks rule 5. The audit is the checked build's only and is
   excluded from the latency targets, as K15's are.
9. **Unchanged:** the model, the oracle, the ABI, every rule's behaviour, the loader beyond the
   `make_satp` signature.

## What stays out

- **Several harts.** Every flush here acts on the hart that runs it. Shootdowns, IPIs, which
  harts may hold an ASID, and moving an idle hart off a process are SMP1's (its brief is amended
  for this package).
- Raising rv64's process limit toward 16 bits (a package after IPC3, `K16-asid-bound-ruling.md`).
- Svinval, Svvptc and any other extension.

## The cases

1. **`asid-reuse-stale`** (checked build, both widths). Process A maps a page at V, writes a
   marker, reads it, and exits; the creator draws PIDs until one is A's (as
   `pid-reuse-authority` does); the new process maps V and reads its own fresh frame, never A's
   marker. Within one process: map V, write, unmap, map V to another frame, read it.
2. **Two recorded negatives** (features off in every default build, as `alloc-first-fit` is):
   `asid-no-reuse-flush` and `asid-no-leaf-flush` (unmap skips its flush). Each must fail case 1,
   by the read or by the audit. Under QEMU it will be the audit: QEMU's TLB is not tagged by
   ASID and, as far as we know, is emptied on every `satp` write. Confirm that from the QEMU
   version the bench pins (`target/riscv/csr.c`, `write_satp`, and the `sfence.vma` helper) and
   report what you find.
3. **Host tests:** the width decision (all ones, the exact width, one bit short refused, on each
   width's field); `make_satp`/`satp_asid`/`satp_root` round trips on both layouts; the audit
   log's cover rules (each flush kind against each record kind).
4. **`asid-cost`** (release build, both widths): the IPC round trip (an empty call and its reply)
   and the context switch, 10,000 of each, and a map/touch/unmap of one page, in host time and in
   guest instructions. Run it on main before your change (the case first, alone) and after.
   Report both; a saving under QEMU is not the acceptance (point 2), a regression is a finding.
5. **The whole bench, both widths;** K16's worst-walk numbers and the gate's targets do not regress.

## Page lines (in the commit that makes each true; exact text in the report)

- **memory-layout.md, `satp`.** The paragraph from "`satp` holds the mode" through "(…[several
  harts](…))." becomes:
  > `satp` holds the mode, the root table's physical page number and the ASID (`make_satp`), and
  > a process's ASID is its PID. `MAX_PROCESS_COUNT` is below 2 to the power of `ASID_BITS`, 9 in
  > Sv32 and 16 in Sv39, and PID 0 is never a process, so every PID fits the field with no table
  > between them. A compile-time assert holds it on each width, and the boot refuses a hart whose
  > field is narrower, found by writing ones to it. The kernel is PID 1; the loader numbers boot
  > processes from 2 and names each one's PID in a field of its own in the handoff record
  > ([boot](boot.md)), and the kernel writes each one's ASID when it takes them over. The kernel's
  > one record of the running PID is `current_pid`, set whenever it switches address space
  > (`set_current_pid`).

  The next paragraph's "Switching process writes `satp` and then runs `sfence.vma` with no
  arguments, which drops every cached translation on the hart. Every map, unmap, lend, return
  and permission change does the same." becomes:
  > The kernel half's shared leaves are global (`G`), so no ASID flush drops them. Switching
  > process writes `satp` and flushes nothing: a process's cached translations carry its ASID and
  > wait for its next turn. A change to a leaf flushes that address in that process's ASID,
  > running or not; a change to a table pointer flushes the process's whole ASID; a change to a
  > kernel-half leaf flushes that address in every ASID. A PID's ASID is flushed whole when the
  > PID is given out, before anything is mapped in it, and a dying process's space is left before
  > any of its frames is freed. A checked build logs every page-table write and stops if one is
  > unflushed when it returns to user mode.

  The status line names the new cases and says: partly tested: QEMU's TLB is not tagged by ASID,
  so a missing flush shows only through the checked build's audit.
- **memory-layout.md, `G` row:** "on the kernel's shared leaves: the physmap, the kernel image,
  the stacks and the DMA register window; never in the user half or on the per-process entry".
- **memory-layout.md, residual "Every change flushes everything"** becomes:
  > - **A flush acts on this hart only.** Each change flushes its address or its ASID on the hart
  >   that makes it; with more than one hart, another hart's cached translations would survive an
  >   unmap (M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts)).
  > - **QEMU cannot show a missing flush.** Its TLB is not tagged by ASID and is emptied on every
  >   `satp` write, so the bench finds a missing flush only through the checked build's audit.
  (Adjust the second to what case 2 finds.)
- **memory.md**, the freed-frames residual: "which no user code reaches before the global
  `sfence.vma` of the next address-space switch; on one hart that leaves no stale mapping."
  becomes "whose cached translations carry that process's ASID, which nothing runs under again
  until the PID is given out, and that flushes it first ([`satp`](memory-layout.md#satp)); on
  one hart that leaves no stale mapping."
- **processes.md**: "The hardware address-space id is reused with the PID, and every
  address-space switch flushes the whole TLB, so no cached translation of the earlier process
  survives." becomes "The PID is also the hardware address-space id, flushed whole when the PID is
  given out again and before anything is mapped, so no cached translation of the earlier process
  survives ([`satp`](memory-layout.md#satp))."
- **fpga-platform.md, the core bullet:** "The ASID is that wide so that every process ID the
  kernel can make fits it: once the kernel flushes by ASID ([ISA features](#isa-features)), the
  process ID is the tag, with no table between the two." becomes "Every process ID the kernel can
  make fits it, and the process ID is the tag, with no table between the two
  ([`satp`](../kernel/memory-layout.md#satp)); the kernel refuses a core generated with a
  narrower field."
- **fpga-platform.md, ISA features, "ASIDs, used properly":** the bullet becomes:
  > - **ASIDs, used properly** (built). Each process's ID is its ASID: a switch flushes nothing,
  >   and a page-table change flushes by address and ASID with the kernel's global entries
  >   standing, a whole ASID when its process ID is given out again
  >   ([`satp`](../kernel/memory-layout.md#satp)). QEMU cannot show the saving, since its TLB is
  >   not tagged by ASID; this core's is, and the IPC path is where it pays.
- **m2-usable-shell.md**, item 4's sentences from "Every address-space switch flushes the whole
  TLB" through "every hart that ran the process." become:
  > A process's translations carry its PID as their ASID, and a hart keeps them across switches.
  > A hart running the process when its tables lose a mapping is sent a shootdown, flushes that
  > ASID and acknowledges before the page is reused; any other hart that ran it flushes that ASID
  > before it next runs it, so a shootdown goes only to the harts running the process now. It
  > comes in two stages: first at a destruction, while a budget runs on one hart at a time; then
  > at every unmap, lend and return, once one process's threads run on several harts at once, as
  > a beamlet VM's schedulers do.
- **boot.md, "Hardware bounds"** (HW1's table, merged): after `MAX_PROCESS_COUNT`'s row
  (boot.md:255), the row
  `| ASID_BITS | the hart's satp ASID field, found by writing ones to it | at boot |`. The
  section's status and R17's each list the width test.

Give the exact lines in the report for the Architect to check.

## Owned paths

- `kernel/src/arch/riscv/mem.rs`, `asm.rs` (the flush routines), `arch/riscv/process.rs` (the
  probe beside `ASID_BITS`), the kernel's boot step that runs the probe, `libs/paging`
  (`make_satp`, `satp_asid`), the loader's `make_satp` call, and one call in the destruction
  path (rule 7).
- The cases and features above, and the page lines above.

**Not yours:** `message.rs` (IPC3), `sched.rs` and `ptable.rs` (K22) beyond reading them, the
model and the oracle, anything with several harts.

**Hotspots (rechecked at 0d207732f).** IPC3 runs beside you. It owns `message.rs` and `time.rs`,
and also touches `kernel/src/process.rs`, `main.rs`, `budget.rs`, `endpoint.rs` and `sched.rs`
lightly.
- Put rule 7's call inside `release_owned_frames` (`kernel/src/mem.rs:957`), not in a caller in
  `process.rs`.
- If the boot probe's step lands in `main.rs`, keep it to one call. Whichever of the two merges
  second rebases.

SMP1 follows this package.

## Gates

- The whole bench on both widths, alone (one whole bench at a time).
- The kernel's host tests, `libs/paging`'s, the model's tests and the stride differential.
- `cargo fmt --check`, the size and unsafe budgets (each new `unsafe` names its guarantor; report
  the net change), doccheck.

Report each command with its exit code, the flush table as built (any site added), where rules 6
and 7 live, the ASID width each width's QEMU reports, `asid-cost` before and after on both
widths, what case 2's negatives tripped on, QEMU's TLB behaviour as you found it, and each page
line as written.

## Rulings during the build (architect-14, 2026-10-03)

1. **Q1: A.** One `(x0, pid)` flush at the end of `MemoryMapping::allocate`, after
   `add_header_page`. It covers the PID's last holder, the copied root entries and the header's
   tables. `add_header_page` records its writes and does not flush.
   - The negative `asid-no-reuse-flush` skips that one flush, leaves `(pid, all)` in the log, and
     the audit stops at the first return.
   - Rule 6's page line: "flushed whole when the PID is given out, before it first runs".
2. **F1, F2 and F3 accepted.** Three sites join the flush table:
   - **F1:** `prepare_map` flushes `(x0, space's ASID)` when it linked a table.
   - **F2:** `map_page_inner` above `USER_AREA_END` (the PLIC window, `intc_plic` through
     `map_range`, PID 1) is a shared kernel-half leaf: it gains `G` and flushes `(virt, x0)`, as
     `map_kernel_page` does.
   - **F3:** `ensure_page_exists_inner` for the kernel PID above `USER_AREA_END` flushes
     `(virt, x0)`, and rule 5's walk checks the leaf's `G`. No caller reaches it today; say so in a
     comment.

   Report the table as built, with the three new rows.
3. **QEMU's TLB.** QEMU 11.0.2 flushes its whole TLB on every `sfence.vma` and every `satp` write
   that changes the mode, ASID or root. So no QEMU case can see a stale translation: the checked
   build's flush log is what the cases test, and `asid-cost` measures the kernel's own work, not
   TLB reuse. Add one residual sentence beside the page's ASID paragraph:
   > "On QEMU every `sfence.vma` and every `satp` change empties the whole TLB, so the cases check
   > the kernel's flushes through the checked build's log, not through a stale translation; the
   > TLB's reuse across switches shows only on hardware ([the FPGA platform](../beyond/fpga-platform.md))."
