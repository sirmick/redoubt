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
