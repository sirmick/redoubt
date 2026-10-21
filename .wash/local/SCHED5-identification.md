# SCHED5 identification (2026-10-20): both rows are one kernel section, the stub's `map_fixed` of beamlet's text

Base 3e6f29443. Nothing built or committed; every boot below is an unmodified tree plus one
scratch case.

## The finding

The hypothesis does not hold. Neither row is the timer's arming or the slice: the timer fires at
the deadline, the expiry and the pick follow at once. **Each slow wake waits for one kernel lock
section: the `map_fixed` with which the stub maps beamlet's text segment into a lease VM just
launched** (`stub/src/main.rs`, one call per ELF segment). The kernel runs with interrupts off,
so on one hart nothing is answered until the section ends. On two harts the steward's hart waits
for the lock behind it. R12 permits this: "a system call's kernel time is bounded by a constant
plus a term linear in the pages it maps" (scheduling.md, R12). This call is about 835 pages
on rv32.

| | rv32 | rv64 |
| --- | --- | --- |
| beamlet text segment (R E LOAD) | 0x34293e B = 835 pages | 0x230356 B = 561 pages |
| its `map_fixed` section, one hart, checked build | 39,778–39,792 µs | 21,183–21,184 µs |
| per page | ~47.6 µs (~5,950 guest instructions) | ~37.8 µs (~4,700) |
| the read-only segment's `map_fixed` (156 / 206 pages) | 7,472 µs (47.9 µs/page) | — |

No audit runs inside these sections (hold-trace's audit ticks are 0).

## Row 2: rv32, one hart, N = 1 decision_wake (30 ms receive timeout)

Boot C: `steward-latency`'s round N = 1 alone, with `block-trace` and `hold-trace`, rv32 at one
hart, seed 3 (`.tmp/SCHED5/sched5-block-n1.toml`, run in a detached scratch worktree). It passed,
and the cell reproduced: decision_wake net p50/p99 32,468 / 33,312 µs (recorded, not judged),
timer_wake 653 / 657. In 15 of 16 samples the wake lies inside the probe lease's own text
`map_fixed`, which starts 7.5 or 9.4–9.8 ms before the deadline (two alternating phases) and
lasts 39.8 ms. The sixteenth sample, the round's first, misses it (2.6 ms). One lease, budget
133, traced (µs since boot):

- 88,159,971: the stub's first `map_fixed` (the read-only segment), 7,472 µs; then its copy in
  user mode, two `call`s, one `set_flags`, one `receive`.
- 88,266,207: the text `map_fixed`, held to 88,305,985 (39,778 µs).
- 88,273,742: the steward's 30 ms decision timeout falls due, inside the section.
- 88,308,067: the steward runs (gross late 34,325 µs). That is 2.1 ms after the section ends: the
  section's return, the pending timer interrupt's expiry, the slice decision and the pick.

Why "only with exactly one runnable VM": a second VM sharing the hart slows the new VM's stub,
so it reaches the text map after the wake (N = 2, N = 3: 1.2 and 1.0 ms). STEWARD13's thread
moved the loop's receive about 10 ms earlier against the VM's first run, which moved the
deadline from near the section's end (23 ms inline) to near its start (32 ms). Neither the
launch nor its thread is the cause.

## Row 1: rv32, two harts, N = 2 timer_wake (20 ms)

Taken from the trace of the earlier SCHED5 member's boot (`.tmp/SCHED5/A-rv32-smp2/`, run before
WEIGHT1; no `hold-trace`). In 12 of 16 samples (5 to 16) the steward's hart traps and waits
**40,445–40,458 µs** for the kernel lock (the trace's `Q`). The other hart holds it, running the
probe lease just launched (budgets 284, 288 … 328, one per lease, about 45 ms after the VM's
first wake), and writes no record and no audit in the section. The wait begins 14,821–14,828 µs
before the deadline in every sample: gross 46.8–47.0 ms, net 31.8 ms after the audits outside the
section. One sample, budget 284:

- h1 takes the lock at 441,399,626 (after a 2.5 ms wait of its own).
- h0, running the steward (86), waits from 441,399,622 to 441,440,071.
- The timer deadline is 441,414,447. The slice end h0 recorded (441,398,188) was passed by
  42 ms, because the hart was in the lock wait.

The same 40 ms wait accounts for the two-hart N = 3 decision-wake p99s (39,824 / 40,031 /
41,174 on the page's table): 10 of 16 samples, the wait 39,967–40,449 µs. N = 1 and the first
four N = 2 samples miss it by phase: the round's sequencing places the probe's timer away from
the VM's text map. That is "why N = 2 / one spinner": phase, not count.

## rv64 (why it passes)

Boot D, the same case on rv64 at one hart: the same section, 21,183 µs, in 15 of 16 samples. It
starts 12.1–15.0 ms before the deadline and ends 6–9 ms after, so decision_wake net p50/p99 is
6,597 / 9,429 µs. The rv64 text segment is two thirds the size, and each page costs 20 % less.

## Other long sections in the same rv32 trace (net of audits, over 2 ms)

`map_anon` 763, the longest 44.4 ms (a boot server's); `map_fixed` 79, the longest 39.8 ms;
`unmap` 102, the longest 23.5 ms; `budget_destroy` 17, the longest 40.3 ms (R10, stated). Every
memory call is linear in its pages at about 48 µs a page on rv32, so any VM's large heap map,
unmap or stub load holds a wake the same way. These two rows are where the case's phase puts a
sample on one.

## Tools and logs

`.tmp/SCHED5/tools/`: `extract.py` (console to trace TSV), `window.py`, `longwaits.py`,
`sections.py` (hold sections against samples), `bycause.py`.

Runs:

- `.tmp/SCHED5/C-block-n1/` (rv32 one hart);
- `.tmp/SCHED5/D-block-n1-rv64/`;
- `.tmp/SCHED5/A-rv32-smp2/` (the earlier member's two-hart boot).

The named STEWARD9 log (`.tmp/STEWARD9/rv32-14-FAIL-timerwake.log`) no longer exists.
