# K16 handoff from k16-implementer-5 to k16-implementer-6

Worktree `.worktrees/k16`, branch `wp-k16`, REBASED onto main f8c1543f3 (K21, RT1, B7 in). Scratch
`.k16/` (never staged). Clean tree; size-budget and unsafe-budget PASS at the tip.

## Commits (final, no WIP)
d2de84357 c1 · f055adb28 churn · 1ca536860 c4 · aa7683d4b c2 · eb443afb6 c3 · 0c8e380e8 stride
(dense queue) · c40f6c668 c5 (the values, with the walks folded in) · 390d9fdc1 c6 process-fill.
Size lines: c4 kernel 7,981->8,009; c2 ->8,048; c3 falls to 8,029 (loader 866->868); stride 329->332;
c5 kernel ->8,081. Unsafe count is 18 (main's), and bits.rs is listed.

## c5 contains
The values; TidMask/Bits<W> (bits.rs); a live_pids set and a process_pids set, with the walks over
them (find_thread, next_timeout, budgets_dying, sweep_handles, victims, holds_process, fill,
next_thread, find_process, migrate_held_pids) and checked audits; the 1 MiB data region; the SERVED
static; the case fixes (thread-limit, process-lifecycle 520, redoubt-ipc 256 at 4x attempts,
budget-test ladder, budget-syscall-attack, pid-reuse-authority TRIES 8192 and 120 s, the sys
tests); the model fixes (flood sizing, 450-step traces); the pages; label.rs.

## Gate (rv64 seed 13, rebased tip)
PASS, share 831. R10 p50/p99 20,674/25,873 us (K21 main 18,648/22,383). budget_destroy 55.2 ms.
The checked audit check_object_indexes was 37.9 ms p50 per destruction before the rebase.
sched-latency both widths, process-lifecycle, pid-reuse-authority, docs, fmt: PASS.

## Rebase notes (done)
- c4: FreeFrames.bits is a LoaderTable (.data stays 32 B).
- c2: add_header_page with K21's unwind; release_ipc_frames before release_owned_frames.
- c3: owners 0xffff/0xfffe. fpga-platform.md keeps main's text, and the ASID item says the PID
  becomes the ASID ([the core](#the-system-on-chip)).

## random_free_pid
Left as is (reported): a uniform draw needs a free-slot set for the process table, kept at 6
writes. That is over 15 lines, and the cost is a bounded constant.

## Next (brief K16-implementer.md lines 144-157; acceptance 159-166)
6. Done: process-fill, PASS both widths (24/22 pages a child).
7. thread-limit at 255 (line 148): the program already counts 255 TIDs. Add the exact charge (a
   page per thread plus the header page) and its full return. Both widths, checked build.
8. The worst walk (line 151): rv64 release, fill RAM with live threads, time one pump, one expiry
   and one destruction from the trace. The numbers go into ipc.md, timer.md and budgets.md as
   residuals. Over 30 ms R10 is stop-and-report.
9. Depth 16 (line 156): budget-test's ladder is already MAX_DEPTH-4 levels (c5). Make it destroy
   from the top so for_each_descendant_post recurses the full depth; check the toml has
   debug_assertions.
Then the whole bench, only on the orchestrator's word.

## Traps
- Console logs are now in target/testbench/run-*/ (.k16/attr.sh is fixed).
- Benches run serially. Never cat .wash/qa/K16-limits.md.

## What consumed my context
The model failures (16-minute runs), the re-rolls, and the rebase conflicts.
