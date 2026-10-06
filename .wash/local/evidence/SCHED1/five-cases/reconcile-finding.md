# SCHED1: why a "reconcile" costs about 9k instructions, and why 3.2 run per slice end (2026-10-06)

This is a finding from reading the code at `a787bd3d2`, checked against the saved traces. No kernel code
was changed. Sources are `walk-large-weight.console.log` (walk-trace spans) and the four `traced-*`
consoles, read by `decompose.py`.

## Summary

- The candidate named in the handoff, `marks.settle -> ready_now -> ready_count()` over thread slots,
  is **not** the cost.
- The cost is **queue walks**. Every call to `Queue::raise_floor` reads the 7-word `State` of every
  queued budget through `kframe`. A slice end makes about 9 such walks, whatever changed.
- So the per-switch kernel time grows linearly with the number of queued budgets L. It is about
  11-12 µs per queued budget per slice end in the checked build, for each of the two halves measured
  (about 23 µs in all). The release build gives about 28 µs per budget from useful work.
- About 3.2 spans run per slice end because every kernel exit runs one (`leave`), and so does kmain's
  `pick`. A slice end is two exits plus one pick.
- A fixed cost of about 100-150 µs per slice end remains even with L near 0. It is not explained by
  the walks. Its likely parts are the traps and the two SBI `set_timer` ecalls, and they are not
  measured separately.

## What walk-trace's RECONCILE span covers

`trace::walk(trace::RECONCILE)` is opened in two places.

1. **`sched::leave`** (`kernel/src/sched.rs:353`). Its span covers:
   - `settle`;
   - `cpu.switch` (`Queue::fold` and `Queue::deschedule`);
   - `reconcile`, including the debug `check_visited`.

   So in `leave` the span is the settle, the switch **and** the reconcile, not the reconcile alone.
2. **`sched::pick`** (`kernel/src/sched.rs:408`). Its span covers `settle` and `reconcile`. The
   `cpu.pick` and `next_thread` that follow it are outside the span.

## Why about 3.2 per slice end

`leave` runs at every kernel exit. The two exit paths are:
- `kernel/src/arch/riscv/irq.rs:20`, in `return_registers`;
- `kernel/src/arch/riscv/syscall.rs:22`, in `resume`.

`pick` runs once per kmain loop, at `kernel/src/main.rs:133`.

A plain slice end therefore runs three spans:

| Span | Where it runs | What it does | Median (walk-large-weight, 1449 slices) |
| --- | --- | --- | --- |
| 1 | the timer trap's exit to kmain, `leave(1)` | settle; `switch(None)`, so fold and deschedule; reconcile | 158 µs |
| 2 | kmain's `pick` | settle; reconcile | 58 µs |
| 3 | the switch ecall's exit to the picked thread, `leave(pid)`, after `switch_to` (`sched.rs:528`) | settle; `switch(Some)`, which folds nothing (`cur` is `None`); reconcile | 72 µs |

- 1449 of the 1540 slices have exactly 3 spans.
- The other 91 have 4 to 16. These are the launcher's system calls, wakes and the end-of-window
  entries, each of which is another exit, and so another `leave`.
- The mean is 4897 / 1541 = 3.18.

## Loop bounds, per call

**`Marks::settle`** (`libs/stride/src/marks.rs:85`) visits only the marked slots, `slots[..n]`.

- A process is marked only when its ready count changes (`ptable.rs:150`, `set_state`). At a slice
  end that is the preempted process, once, and the switched-to process, once.
- `ready_now` (`sched.rs:196`) per slot calls:
  - `ready_count()`, a 4-word `TidMask` popcount by `trailing_zeros` over the set bits
    (`bits.rs:31`);
  - one `budget_of` lookup, which is an array index.
- That is about 100 instructions per marked slot, and 0-1 slots per span. **Not the cost.**

**`Queue::reconcile`** (`libs/stride/src/lib.rs:289`):
- the `lost` loop and the `gained` loop are each 0-1 budgets at a slice end;
- it then calls `raise_floor` **twice, unconditionally**: once after the removals (`:321`) and once at
  the end (`:343`). It does this even when `lost` and `gained` are both empty, as they are in span 2
  of a plain slice end.

**`Queue::raise_floor`** (`lib.rs:244`) walks every queued budget and calls `bs.state(b)`:
- `MemoryManager::sched_state` (`budget.rs:383`) does `object_phys` (an ownership-table lookup plus
  an `assert!`), a `debug_assert` on MAGIC, and 7 `kframe::read`s;
- each `kframe::read` goes through `at()`, which has two `assert!`s (`kframe.rs:20-28`);
- these are `assert!`s, so they stay in the release build.

**`Cpu::switch`** (`lib.rs:457`) with a budget taken off calls:
- `Queue::fold`, which is `state`, `weight` (`free_weight_of`: object_phys plus 2 reads), `set_state`
  (7 `kframe::write`s plus an `assert!`; under `sched-trace` one more `sched_state` read), then
  `raise_floor`;
- `Queue::deschedule`, which is `state`, `contains` (a scan of `slots[..len]` with no frame reads),
  `set_state`, then `raise_floor`.

**`check_visited`** (`marks.rs:140`, checked only) covers the 0-2 visited budgets: `live`,
`state` and `ready` for each.

## Queue walks per slice end

Each walk reads the `State` of all L queued budgets.

| Where | `raise_floor` walks | Other walks |
| --- | --- | --- |
| span 1 | 4: fold, deschedule, and 2 in reconcile | |
| span 2 | 2, both in reconcile | |
| span 3 | 2, both in reconcile | |
| outside the spans: `cpu.pick` -> `Queue::pick` (`lib.rs:347`) | | 1 (`min_by_key` over `state`) |
| outside the spans: `next_thread` (`sched.rs:430`) | | 1 over the live PIDs (`budget_of` and `ready_threads` each; no frame reads) |

That is **9 walks over the queue per slice end**, so about 9·L `State` reads of 8 checked words each.
In large-weight, L = 9 (the server and 8 users), which is about 81 `State` reads per slice end.

## The traces agree: the cost is linear in L, and the span sizes follow the walk counts

**Span sizes** (large-weight, L = 9). The medians are 158 / 58 / 72 µs, and the walks per span are
4 / 2 / 2.
- Span 1 also does the fold and deschedule `set_state`s and the trace's `B`/`P`/`R` records.
- Spans 2 and 3 are each about 2 walks: about 29-36 µs per walk, so about 3.2-4 µs per `State` read
  (about 400-500 instructions at 125 per µs).

**Across the cases** (`decompose.py`, medians per slice end, checked build with sched-trace):

| Case | Queued budgets L | Interrupt to marks audit (span 1 and the trap) | Marks audit | Audit end to next interrupt, less 1 ms (spans 2 and 3, `pick`, `switch_to`, return) |
| --- | --- | --- | --- | --- |
| carve-inflation depth 1 | 3 | 103 µs | 30 µs | 124 µs |
| server-busy | 3-4 | 115 µs | 37 µs | 137 µs |
| carve-inflation depth 4 | 6 | 139 µs | 51 µs | 162 µs |
| large-weight | 9 | 175 µs | 76 µs | 200 µs |
| debt-lift | 17 | 259 µs | 129 µs | 289 µs |

- The first half grows by about 10.5-12 µs per queued budget and the second by about 11-12.7. That is
  about 23 µs per queued budget per slice end, about 2.6 µs per budget per walk.
- Extrapolated to L = 0, about 70 + 85 = about 155 µs per slice end is fixed in the checked build.
- The marks audit (stamped, outside the above) is linear in L too.

**Release build** (no checks, no trace, no audits; RESULTS.md):
- 0.134 ms per slice end with L = 1 (the calibration alone);
- 0.357 ms with L = 9 (rv64);
- that is about 28 µs per extra queued budget, the same slope as the checked build. This is expected,
  because the per-read checks are `assert!`s that stay in release.
- The fixed part, about 0.1 ms, is in release as well.

## What is not explained

- **The per-`State`-read constant.** Code reading gives about 200-250 instructions: 8 checked frame
  accesses plus `object_phys`. The traces imply about 330-500. The gap is within a factor of 2, and no
  boot isolated it.
- **The fixed part,** about 100 µs in release and about 155 µs checked at L = 0. It covers three
  traps, two RustSBI `set_timer` ecalls, `expire`, kmain's loop and the context switch. The
  code-reading estimate in `switch-cost-estimate.md` gave 8-15k instructions (65-120 µs) for this, so
  it is plausibly all of it. No boot split the SBI ecalls out.

## What would remove the L-dependence (for the Architect's ruling; nothing done)

- **Make `raise_floor` conditional.** The floor can only rise when the queue's minimum pass may have
  changed: after a fold of the minimum budget, or when a budget leaves. A reconcile with `lost` and
  `gained` both empty changes nothing, and could return before both walks.
- **Keep `(pass, tie)` in the queue's own slot array** beside the `BudgetRef`. `raise_floor` and
  `pick` would then read kernel memory, not 8 checked frame words per budget. The frame stays the
  record, and the slot is written wherever `set_state` writes a queued budget's pass.
- **Either change alone** takes the 9·L reads per slice end down to about 1·L, or to L cheap reads.
  At L = 9 that saves about 0.2 ms of the 0.36 ms per slice end (estimated from the slope above).

## Per step, for the Architect's package (added after the "After the release measurement" ruling)

The walk-trace stamps whole spans, not steps. So the split below is **derived** from two things:
- the span medians at L = 9 (large-weight, walk-trace);
- the per-budget slope across L = 3..17 in the five traces, about 2.6 µs (about 330 instructions) per
  queued budget per `raise_floor` walk.

At 125 instructions per µs:

| Step | Per call | Span 2 (pick; 58 µs, about 7.2k) | Span 3 (exit to thread; 72 µs, about 9k) | Span 1 (exit to kmain; 158 µs, about 19.7k) |
| --- | --- | --- | --- | --- |
| `raise_floor` (the floor), L State reads | about 2.6 µs x L | 2 walks, about 47 µs (81%) | 2 walks, about 47 µs (65%) | 4 walks, about 94 µs (60%) |
| `Marks::settle` (`ready_now` per marked slot, `set_ready`) | about 1-2 µs per slot | 0-1 slot | 1-2 slots | 1 slot |
| `mm.ready` per lost/gained budget, requeue/wake `set_state` and its trace record | about 2-4 µs per budget | about 0 | 1 wake | fold and deschedule (2 `set_state`, `B`/`P`/`R` records) |
| `check_visited` (checked only), `trace::entry`, the walk records | about 2-5 µs | yes | yes | yes |
| Rest of the span (all steps but the floor) | | about 11 µs | about 25 µs | about 64 µs |

- So `mm.ready` and `set_ready` are small. Most of each reconcile is the floor: two unconditional
  walks of the whole queue, plus the walk in each fold and deschedule.
- The release slope is the same, so this is what release does too. The K22 claim ("marks, not a walk
  of every process") holds for processes, but **not for queued budgets**.
- Exact per-step instruction counts need step stamps, or a QEMU instruction-count plugin around each
  step. That is for the reconcile package.
- Note on the ruling's premise: the span the ruling calls "the reconcile" in `leave` also contains
  `Cpu::switch` (fold and deschedule). Of the 3.2 spans per slice, only spans 2 and 3 are a pure
  settle and reconcile.
