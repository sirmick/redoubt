# K31: where a timer entry's instructions go (measurement, 2026-10-09)

Base 58326f13c (wp-K31 fresh from main; no code changed yet). Release kernel (`launch --system
--print-only --smp 1`, no debug assertions: no audits), both widths, QEMU 10.2.1 with
`-icount shift=3,sleep=on -rtc clock=vm`, one hart. In alice's console session:
`spawn(fn -> ... Enum.reduce(1..5_000_000, 0, &max/2) ... end)`, the loop alone on the hart (the
driver quiet: the line was short and logging began 4 s after the loop started). QEMU's
`log exec,nochain,int` switched on through the monitor for 1.5 s of host time; every executed
block weighted by its length from the kernel's and the firmware's disassembly; entries split at
the trap (the `int` lines) and at the first user block after it. A call stack is tracked from the
blocks that end in a call. Tools and raw data: /home/mcloonan/redoubt/.tmp/K31/ (tools/,
m64a/, m32a/: exec.log, entries.txt, tl50.txt / tl20.txt = one entry's timeline).

## Result

Every timer entry in the window is the same entry: the slice ended, the VM's thread is preempted,
`kmain` picks the same thread again and switches back to it. It is deterministic:

| | rv64 | rv32 |
| --- | ---: | ---: |
| timer entries in the window | 134 | 55 |
| kernel instructions / entry (mean, median) | 15,151 / 15,151 | 20,883 / 20,897 |
| firmware (SBI `set_timer`) / entry | 156 | 171 |
| SBI `set_timer` calls / entry | 3.0 | 3.0 |
| system calls in the window | 8 | 4 |

At a 1 ms slice and 125 M instructions/s of guest time (shift=3), that is 12 % (rv64) and 17 %
(rv32) of the hart for a thread that does nothing but compute.

## The entry, in order (rv64, entry #50, 15,307 instructions)

| phase | instructions |
| --- | ---: |
| trap save, lock, `mem::entered`, `from_user`, expiry check, billing, `slice_over`, `on_interrupt`'s rearm (SBI #1) | 1,145 |
| `sched::preempt`: `activate_process_thread` to `kmain` (satp, contexts, ready counts) | 733 |
| `resume_current` -> `leave(kmain)`: close billing, settle, deschedule the VM's budget (fold, requeue), reconcile, `set_slice_end(NEVER)` -> rearm (SBI #2) | 3,584 |
| `kmain`: pause/resume billing, `expire_due` (hint check), `pick`: settle, reconcile, queue pick, `next_thread`, cursor store | 6,671 |
| `kmain`'s switch: the S-mode `ecall` trap, `switch` (activate), `leave(VM)`: switch the budget back on, reconcile, slice end -> rearm (SBI #3), the return | 3,174 |

## By kind (self instructions per entry)

| kind | rv64 | rv32 |
| --- | ---: | ---: |
| checked word access: `kframe::at` (a call and two asserts per word) and `object_phys` | 4,228 | 3,995 |
| whole-budget decode/encode: `MemoryManager::budget`/`store` (~40 words each) and their memcpy/memset | 2,217 | 4,619 |
| 64-bit software division (`ticks_to_us`, `us_to_ticks`: 15 a entry) | - | 1,756 |
| the pick's walk of every live PID (`next_thread`: `live_pids`, `budget_of` each) | 669 | 863 |
| timer rearm (three a entry), slice end, `now_us`, firmware | 696 | 1,174 |
| stride queue and marks (settle, reconcile, fold, state, ready counts) | 2,569 | 2,915 |
| billing | 654 | 816 |
| process/thread switch (activate, set_state, contexts) | 1,177 | 1,174 |
| `current_pid` (24 calls) | 462 | 462 |
| trap asm, handler bodies, lock | 897 | 980 |
| `kmain` and `sched::leave` bodies | 1,512 | 1,865 |

## What is proportional to nothing the entry does

1. **`kframe::at` is a call with two runtime asserts for every word** (185 calls a entry, ~20
   instructions each, plus `object_phys`'s ownership-table assert per object access). The offsets
   are constants at every caller; inlined, the offset check folds away and the frame check is a
   compare. ~28 % of the entry on rv64.
2. **The pick reads and writes the whole budget object to move one word, the round-robin cursor.**
   `next_thread` decodes all ~40 words of the budget (`mm.budget(b.frame).cursor`) and `pick`
   decodes them again and encodes all of them back (`budget` + `store`) to set `cursor`. Two
   decodes and one encode of ~40 checked words each: ~2,200 (rv64), ~4,600 (rv32, where the
   decoded `Budget` is copied by a byte-wise memcpy).
3. **Three SBI `set_timer` calls per preemption, two of them for nothing:** `on_interrupt` re-arms
   for the slice end that just passed (the leave that follows re-arms anyway), and `leave(kmain)`
   arms for `kmain`'s `NEVER`-slice although `kmain` picks at once and `leave(VM)` arms again.
   Each is a `us_to_ticks` (a 64-bit division on rv32) and an ecall into the firmware.
4. **rv32's µs/tick conversions** (`ticks_to_us` twice a division, 15 divisions a entry, ~1,750).
5. Structural, not cut here: a preemption is a round trip through `kmain` (preempt, leave to
   `kmain` with a deschedule, a pick, a second trap for the switch, a second leave with the budget
   switched back on), about 7,000 of the 15,000. When the preempted thread's budget is the one the
   queue picks again, the deschedule/requeue and switch-on are both done. Changing that touches
   the scheduler's wiring (`redoubt-stride` `Harts::switch`, the model); not in this package unless
   asked.

## Plan for the fix

Cut 1-4 without changing any rule: inline `kframe::at` (and the `object_phys` check folded to the
ownership read already there); give the cursor its own word accessors (as `sched_state` has) and
use them in `next_thread`/`pick`; drop the two redundant re-arms (`on_interrupt` when the slice is
over and the entry preempts, `leave(kmain)` defers its arm to `kmain`'s idle and its expiry);
avoid rv32's 64-bit divisions on the hot path where a compare in ticks suffices. Then the gate: a
bound on kernel ticks per timer entry from the sched trace in the oracle, net of audits, on both
widths; beamlet-reduction-rate's rate up; the sched-* set at 1 and 2 harts, both widths.

# K31: the fix (completion)

Base 58326f13c. Head a43c4b872 on wp-K31:
- 4c3f716e1 kernel: a slice's end pays for its work, not for checked calls, whole budgets and
  three timer arms (kernel/src/{kframe,mem,budget,sched,time,main,bits,ptable}.rs,
  arch/riscv/{irq,timer_sbi}.rs; docs/kernel/timer.md, invariants.md; tests/size-budget.toml
  kernel ceiling 10595 -> 10625 with its `Size budget:` line)
- a43c4b872 tests, docs: sched-timer-entry bounds what a slice's end costs the kernel
  (tests/programs/src/bin/sched-timer-entry.rs, tests/programs/Cargo.toml,
  tests/sched-timer-entry.toml, tools/testbench/src/sched_oracle.rs; docs/kernel/scheduling.md,
  docs/testbench.md, docs/userland/beamlet.md)

## What changed (no rule changes)

1. `kframe::{at,read,write,fill}` `#[inline(always)]`, `object_phys` `#[inline]`: the offset
   checks fold at constant offsets; the frame check stays.
2. The round-robin cursor gets `sched_cursor`/`set_sched_cursor` (one word, as `sched_state`);
   `next_thread` and `pick` no longer decode/encode the whole budget.
3. The timer is armed only on the way to where its interrupt is taken: `leave` for a return to
   user mode, `kmain` before `arch::idle`, and `on_interrupt(from_user = false)` for a timer
   interrupt taken in the idle window (it returns there with the interrupt pending: without it
   the first version stormed in the idle window, found in the exec log and fixed before any
   bench run). `set_slice_end`, `note_timeout`, `note_budget_deadline` and `expire_due` only
   record. One SBI call a preemption instead of three.
4. `ticks_to_us`/`us_to_ticks`: one 64-bit division while the product fits (three weeks at
   10 MHz), the old split as fallback; equal to the old functions on 20M random and boundary
   inputs over 8 timebases (scratch check, .tmp/K31/eq/eq.rs).
5. `Process::ready_count` counts the mask in place (`Bits::count`), no copy.

## Measured (release, smp 1, icount shift=3, exec log; .tmp/K31/m64d, m32d)

| per timer entry | rv64 | rv32 |
| --- | ---: | ---: |
| kernel instructions, before | 15,151 | 20,883 |
| after | 8,953 | 13,209 |
| SBI set_timer calls | 3 -> 1 | 3 -> 1 |

What is left (rv64): stride queue and marks ~2,550, kmain/leave bodies ~1,570, process switch
~1,180, trap asm/handler ~900, the live-PID walk in `next_thread` ~660, billing ~650,
current_pid ~460, rearm+SBI ~390. About half of the entry is the round trip through `kmain` itself
(deschedule and requeue of the budget the pick takes straight back): structural, a change to the
stride wiring (`Harts::switch`) and the model; not done here. Suggested follow-up.

beamlet-reduction-rate (seed 1, fastest of 30), before = main + this branch's test commit only:
rv64 52,736 -> 55,465 (+5.2 %), rv32 47,119 -> 50,106 (+6.3 %). Floor 42,000 unchanged.

## The gate

`sched_oracle timer_section_max_ticks=5000 gate_harts=1` in the new `sched-timer-entry` (checked
build, `hold-trace`, smp [1, 2], both widths): the longest lock section a timer interrupt (cause
0x205) began, net of audits; judged at one hart, recorded at two (icount's one clock counts the
other hart's instructions in a section). Ticks are 100 ns = 12.5 instructions.

| | rv64 smp1 | rv32 smp1 | rv64 smp2 (recorded) | rv32 smp2 (recorded) |
| --- | ---: | ---: | ---: | ---: |
| before, p50/max | 2,254/5,827 | 2,992/6,848 | 4,271/7,621 | 6,556/9,931 |
| after, p50/max | 1,085/3,450 | 1,589/4,261 | 2,985/5,575 | 4,891/9,694 |

At one hart every section is identical but the window's first. The before-tree fails 5,000 on
both widths at one hart. Host tests: `kernel_sections_say_what_the_lock_waits_waited_behind`
extended (bound met/missed, gated/recorded, no timer section).

## Gates (all exit 0, head a43c4b872 unless noted)

- `make set CASES="<all 29 sched-*> beamlet-reduction-rate"` on the kernel commit's code
  (before the history rebuild, which changed one comment in timer_sbi.rs and docs only): rc 0,
  68 PASS, each case at its own hart counts (1, 2, 3, 4), both widths.
- `make rv64/beamlet-reduction-rate` rerun: PASS (rate 55,465).
- `make set CASES="formatting size-budget unsafe-budget docs sched-timer-entry"`: rc 0, all PASS.
- Smoke: `make set CASES="userland-boot init-boot bench-net-peer ipc-outcomes sum-clear
  lend-untouched-page no-cruft" build-rv64 build-rv32`: rc 0, all PASS (lend-untouched-page at
  1 and 4 harts).
- `q run --cores 8 -- cargo test -q -p testbench`: 187 passed.
- Not run: the whole bench (the train's); `--smp 2` sweep of the sched-* cases whose own count
  is 1 (they ran at their own counts; sched-latency, -timer-entry at 1 and 2; capped at 2-4).
- No new `unsafe`.

## Summaries checked

- docs/kernel/timer.md "The hart timer": where the timer is armed (updated; status + case);
  "Expiry" re-arm sentence (updated).
- docs/kernel/invariants.md I13 "Kept in" `rearm` (updated).
- docs/kernel/scheduling.md "Preemption points": the slice's end's cost and the bound (added,
  status + case); R23's trace-case list and the `hold-trace` feature list (case added).
- docs/testbench.md "The scheduler oracle": the section bounds (paragraph said "judges nothing",
  stale since fault_section_max_ticks; now names both bounds; status + case + host test).
- docs/userland/beamlet.md "How fast compiled code runs": table and the kernel-share sentence
  ("about 12,000 a interrupt") updated with the new rates and per-entry numbers.
- docs/kernel/README.md (unsafe counts, kframe's role): no change, no new unsafe, role same.
- README.md, GETTING-STARTED.md, docs/plan/m2-usable-shell.md: no claim about kernel entry cost;
  no change. docs/testbench.md "Which cases run in guest time" counts: stale before this branch
  (BEAM19 noted it), not updated.

# K31 fix round (kernel-red P2, rebase onto main without SMP6)

Base 04b7fa4d5 (main rebuilt without SMP6). Head b6c4b566c:
- 5c6762009 kernel: a slice's end pays for its work, ... (code unchanged from a43c4b872's
  parent commit 4c3f716e1 but for two comment rewraps in time.rs; message numbers re-measured;
  kernel ceiling 10076 -> 10106, main's own ceiling plus the same 30 lines)
- b6c4b566c tests, docs: sched-timer-entry bounds ... (P2: the p99 bound added; numbers)
Range-diff: git range-diff 58326f13c..a43c4b872 04b7fa4d5..b6c4b566c
(.tmp/K31/range-diff.txt).

## Rebase: what differed without SMP6

`git rebase --onto 90fe594b5 4b84112f2 wp-K31` (the branch had been rebased onto 4b84112f2 in
this round), then `--onto 04b7fa4d5 90fe594b5` (README-only delta). One conflict: the kernel
ceiling (main 10,076 without SMP6): set to 10,106, main's plus this branch's 30 code lines
(size-budget passes). Every kernel hunk applied to identical code: `irq.rs`'s timer arm,
`sched::leave`'s rearm, `kmain`'s idle, `time.rs`, `kframe`, the cursor. Main's pre-SMP6
`arch::idle` has the same interrupt window (set_sie/clear_sie after the lock), so the
idle-window re-arm (`on_interrupt(false)`) covers it as before. The only text difference: a doc
comment in time.rs's `expire_due` the merge left over 100 columns, rewrapped.

## Re-measured on the new base (release, smp 1, exec log; .tmp/K31/m64p,m32p,m64n,m32n)

| per timer entry | rv64 | rv32 |
| --- | ---: | ---: |
| kernel instructions, main 90fe594b5 | 15,022 | 20,692 |
| this branch | 8,846 | 12,869 |
| SBI calls | 3 -> 1 | 3 -> 1 |

beamlet-reduction-rate (main + test commit vs head): rv64 52,936 -> 55,674 (+5.2 %), rv32
46,959 -> 49,928 (+6.3 %).

## P2: the steady-state bound

The trace has two cold timer sections at one hart, not one: the program's first preemption
after its calibration and the window's first. So "max after the first" would still hold a cold
one; the steady entry is bounded by the p99 instead, beside the max:
`sched_oracle timer_section_p99_ticks=2000 timer_section_max_ticks=5000 gate_harts=1`. One p99
bound on both widths, not 2,000/2,500: 2,500 would let the before-tree's rv64 (2,241) pass.

| one hart, ticks net of audits | rv64 p99 | rv32 p99 | rv64 max | rv32 max |
| --- | ---: | ---: | ---: | ---: |
| head | 1,073 | 1,597 | 3,431 | 4,234 |
| before-tree (main + test commit) | 2,241 | 3,006 | 5,807 | 6,831 |
| bound | 2,000 | 2,000 | 5,000 | 5,000 |

The before-tree run failed on both widths ("their p99 held the lock 2241 / 3006 ticks ...
above 2000"); its max is over 5,000 too. Cold sections on the head: calibration 1,998 / 2,524,
window 3,431 / 4,234. Margins: p99 rv32 20 %, max rv32 15 %. Oracle host tests: p99 met and
missed, max met and missed, gated/recorded; 55 pass. testbench.md, scheduling.md and the case
comment say both bounds and why the p99.

## Gates

On the identical code at 7508d7c90 (base 90fe594b5; differs from the head by README cells from
main and comment/number rewrites only), `make -k set CASES="sched-timer-entry sched-latency
sched-timer-flood beamlet-reduction-rate userland-boot init-boot bench-net-peer ipc-outcomes
sum-clear lend-untouched-page docs size-budget formatting"`: every case PASS on both widths
(sched-timer-entry at 1 and 2 harts, sched-latency at 1 and 2, lend-untouched-page at 1 and 4)
but `docs`, which failed on main 90fe594b5's README TCB cell (C13), the one 04b7fa4d5 corrects.
On 04b7fa4d5's head: `sched-timer-entry` both widths at 1 and 2 harts PASS,
`rv64/beamlet-reduction-rate` PASS; final head b6c4b566c: `formatting`, `size-budget`, `docs`
rc 0, PASS. `cargo test -p testbench sched_oracle`: 55 passed. No unsafe changed.

# Train 20: rv32 sched-capped smp=2 fails on K31 (2026-10-10, k31-implementer-2)

Finding only; nothing changed in the branch (head bab03e8de). Traces and scripts:
.tmp/K31/an (fail-k31-rv32-smp2.log = K31 rerun, deterministic, same 449; pass-smp6-rv32-smp2.log
= SMP6 f590f70c8; mttcg.log, shift4.log = probes), oracle replay .tmp/K31/replay.

Clause: HART-SHARE late-a, 449 < 450 (expected 500). late-b 275, late-c 274 met. SMP6: 465.

Not a hand-over: in the window budget 31 (w 900) is picked on hart 0 at 836/836 of its slice
ends, no light budget ever runs there (SMP6: 724/724). b and c alternate on hart 1 (418/418).

Per-slice accounting, hart 0 (period between its I's = charge to 31 + nobody's time):
- K31: period 23,880 ticks = charge ~19,770 + audit + y + ~2,600 unbilled every slice.
- hart 1 (K31) and both harts (SMP6): unbilled ~0.
- Audit spans (U..V, µs): K31 hart 0 bimodal 81/327; every hart-0 audit run in leave() to kmain
  is long (456/458), none on hart 1 are; SMP6 118-123 on both. Between hart 0's U and V there
  are no hart-1 records: hart 1 runs user code (~30k instr = ~250 µs) inside hart 0's audit.
  When hart 0's audit falls instead at leave() to user (376 slices), the same ~2,600 ticks land in
  another nobody's span (kmain's expiry walk, pause_billing).
- Whole run: nobody 71/1000 (SMP6 30), lock waits 216 (220).

Mechanism: under icount the harts take turns on one host thread, and time_now counts both. Each
slice end on hart 0, QEMU ends hart 0's turn at the same point, right after leave() to kmain
begins, and hart 1 runs ~250 µs. That span is billed to nobody on hart 0 (audit, or kmain's
expiry walk), while every hart-1 span holding hart 0's turns is billed to b/c. So 31 loses ~10%
of its hart's clock, b+c ~3%. K31 moves where the turn ends: the timer is no longer re-armed in
the timer entry (expire_due/on_interrupt) or in leave() to kmain (no SBI set_timer before the
pick), and the entry is ~8k instructions shorter. On SMP6 the turn landed in billed time on both
harts. Rules for billing are unchanged by K31 (audits and kmain's expiry walk are nobody's on
both).

Probes (rv32 smp=2, K31 kernel, temporary tomls, deleted):
- no icount (harts at once, quiet): late-a 502, late-b 249, late-c 248, uncap 818/90/90,
  spread 500/499: PASS.
- icount shift=4: late-a 434, spread-a 561, spread-b 438: FAIL. Moving the turns swings the
  shares ~60/1000 either way.

Proposal (needs kernel-red's agreement, LAT4's precedent): the 2-hart shares are judged where the
harts run at once, a sched-capped-mttcg case (smp 2, quiet, both widths, same program and
oracle; measured late-a 502); the icount case keeps its 3- and 4-hart judgements and records the
2-hart ones; scheduling.md's "A woken hart waits for QEMU's turns" residual gets the share case.
Not the tolerance. No kernel change: K31 bills and arms by rule; re-adding an early re-arm would
only move QEMU's turn back. Risk: mttcg is host time; one run so far, repeat runs needed.
