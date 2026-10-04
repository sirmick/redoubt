# K16 round-1 review notes on 19102cb9b (c1-c5), for the implementer

Verdicts: editor OK with notes; simplifier OK with notes; Architect OK with two page edits; red
pending (its findings come as an instruction). Each change goes into the commit that owns the
code, no fix-ups.

## Architect (required)

1. `docs/kernel/memory-layout.md`, the satp paragraph, last sentence: "ASIDs for several harts
   are designed in M2" contradicts SMP1's ruling and M2 step 4. Replace it with:
   > Several harts need no ASID either: a TLB shootdown goes to the harts running the process
   > ([several harts](../plan/m2-usable-shell.md#several-harts)).
2. `docs/beyond/fpga-platform.md`, the "ASIDs, used properly" bullet, replace whole with:
   >   - **ASIDs, used properly.** The kernel writes ASID 0 in `satp` and flushes the whole TLB on
   >     every switch and after every page-table change, the kernel's global entries with it. Its
   >     process IDs are 16 bits, so on this core each can be its own ASID
   >     ([the core](#the-system-on-chip)): flushing by address and ASID, and a whole ASID when its
   >     process ID is reused, is the biggest saving on the IPC path.

## Red (OK with notes; no blocker)

1. (take) `process_ended` only `debug_assert`s an empty TID mask, so a release build would leak
   frames silently: `assert!`, or free them there.
2. (report) `reconcile`'s wake loop is O(R^2 * N), R up to 512; it matters for R12's bounds when
   many budgets wake at once. Measure it in commit 8's worst-walk numbers and say whether it needs
   the Architect's item 3 (the marked reconcile).
3. (confirm) `left()` now fires in swap order: confirm nothing in the model or the trace depends
   on it, and say so in the report.
4. (take) The gate's numbers at 19102cb9b are not in `.k16/` (report-items12 has runs at 10281b636
   and 3f4630826). Rerun the gate at the tip you report and quote it in the file; state once that
   budget_destroy's call-to-return (~46-55 ms) includes the checked build's audit (~38 ms), which
   is outside R10 under K15/K18, so the 30 ms line applies to R10's trace time only.

## Editor (take)

1. `docs/kernel/memory-layout.md` ~l.303, the table "Only these differ": the row
   `PROCESS_AREA pages | 1 | 1` does not differ; delete it.
2. `docs/kernel/model.md:197` ("run up to 31 threads each") and `model/src/check.rs:354,426,525`
   keep 31: true of the flood generator, not MAX_THREADS; say "a fixed 31" or why.
3. Over-100-column prose lines added in `docs/kernel/ipc.md` (two: "already has WAIT_CAP (32)...",
   "and only the threads that have an IPC page..."), `budgets.md` (the R2 walk, the status line)
   and `model.md` (the flood paragraph): reflow to the book's width.

## Simplifier

- P2-1 (required before merge): `.k16/` is untracked and not ignored (dstat.rs, dstat-kernel.rs,
  dstat.patch, walkstat.*, ~250 logs). It must never be staged; move anything worth keeping to
  `.wash/local/` and delete the rest before the merge.
- P2-2 (take): `sched.rs:165 Runnable::fill`: `p.free()` is redundant for a live PID (the live
  set equals accounts with a budget, `check_live_pids`), and `self.n < self.list.len()` after the
  contains is unreachable; keep the dedupe, drop both guards.
- P2-4 (judgment): two TID masks per process, `Account.live` and `ArchProcess.allocated_threads`,
  differ only in timing (thread_ended frees the page after destroy_thread). If `thread_exists`
  can read `Account.live`, drop one (~16 KB RAM, ~8 lines); keep if the ordering matters, and say
  which in the report.
- P2-6 (take): no caller of `Default` for `Bits` (ArchProcess/Account use `EMPTY`); delete the
  impl unless a derive needs it.
- Keep: the dense queue's `Option<B>` slots (const init), `process_pids` vs `live_pids`.
