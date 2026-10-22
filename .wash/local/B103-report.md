# B103 step 1: the rv32 two-hart deadline notice taken apart

Base main 2b60945b6; branch wp-B103 head 81522433a, one WIP commit only (two instrument cases,
`tests/steward-notice-trace.toml` and `tests/steward-notice-trace-mttcg.toml`: steward-latency's
N = 1 round alone, kernel `sched-trace-large` + `block-trace` + `hold-trace`; the twin the same
without `icount`). Dropped before any merge. No code or page change yet.

Scratch, tools and every console: `/home/mcloonan/redoubt/.tmp/B103/` (parts.py: the four parts
per sample; timeline.py: per-hart sections/user/lock waits; secstat.py: section lengths by cause).

## Runs (each alone; exit codes)

icount (shift=3, virtual time: what the case judges today), `boots.sh steward-notice-trace`:

| boot | rc | notice net p50/p99 (ms) |
| --- | --- | --- |
| rv32 2 harts | 0 (PASS, cell recorded) | 35.9 / 44.9 |
| rv32 1 hart | 0 | 14.0 / 16.7 |
| rv64 2 harts | 0 | 24.7 / 35.2 |
| rv64 1 hart | 0 | 18.5 / 19.2 |

Harts at once (multi-threaded TCG, no icount, host time, net of audits, `q run --quiet`),
`QUIET=--quiet boots.sh steward-notice-trace-mttcg`:

| boot | rc | notice net p50/p99 (ms) |
| --- | --- | --- |
| rv32 2 harts | 0 | 7.5 / 9.2 |
| rv32 2 harts (repeat) | 0 | 7.5 / 8.6 |
| rv32 1 hart | 0 | 5.8 / 6.6 |
| rv64 2 harts | 0 | 7.5 / 9.9 |
| rv64 1 hart | 0 | 5.7 / 6.2 |

Also re-read without a run: SCHED5's saved rv32 two-hart head boot (N = 1-3, 43.4 / 45.3 / 56.5)
and its one-hart boot: the same split as below.

## The parts (mean net of audits, ms, N = 1, 15-16 samples a boot)

Deadline -> destruction begins (X); R10 (X..Y); Y -> the steward's wake on the exit notice;
that wake -> the `lease_ended` send returned (the sample's end).

| boot | dl->X | R10 | Y->wake | wake->sent (max) | whole (max) |
| --- | --- | --- | --- | --- | --- |
| icount rv32 1 hart | 0.22 | 6.20 | 0.02 | 7.77 (10.1) | 14.2 (16.7) |
| icount rv32 2 harts | 0.43 | 5.23 | 0.08 | **29.04 (39.3)** | 34.8 (44.9) |
| icount rv64 1 hart | 0.25 | 8.38 | 0.01 | 9.20 (10.3) | 17.9 (19.2) |
| icount rv64 2 harts | 0.19 | 4.98 | 0.12 | **20.27 (29.9)** | 25.6 (35.2) |
| at once rv32 1 hart | 0.10 | 3.18 | 0.02 | 2.53 (3.2) | 5.8 (6.6) |
| at once rv32 2 harts | 0.11 | 3.32 | 0.02 | 4.24 (5.0) | 7.7 (9.2) |
| at once rv64 1 hart | 0.10 | 3.13 | 0.02 | 2.52 (2.7) | 5.8 (6.2) |
| at once rv64 2 harts | 0.09 | 3.28 | 0.02 | 4.29 (6.5) | 7.7 (9.9) |

(One sample per icount boot is left out of the means by the tool's search, not the run; the
oracle's p99 above covers all 16.)

**The term that grows is the last: the steward's wake on the notice to its send returning.**
Destruction, R10 and the notice's delivery do not grow. With the harts at once that term goes from
2.5 to 4.2 ms at two harts (x1.7, the same on both widths). Under icount it goes x3.7 on rv32 and
x2.2 on rv64.

## Why it grows under icount: QEMU's turns

Budgets read from the trace's `#` records: 86 is the steward, 66 blkd:system, 69 verity:system,
72 erofsd:system, 99/112 alice's sessions. In that term at two harts the other hart runs
verity:system and blkd:system (the system volume's reads); at one hart they run after the send.

- Worst rv32 two-hart sample, wake->sent: steward's hart 24.6 ms of "user" time between its
  sections, other hart 25.8 ms of verity:system user; at one hart the steward's user time in the
  same term is 0.5 ms. Under icount the harts take turns on one host thread, and both harts'
  instructions advance the one clock, so the other hart's hashing is counted inside the
  steward's path.
- The same kernel sections read about 5x longer at two harts (p50 net of audits, whole boot):
  timer interrupt 92 -> 470 us rv32, 58 -> 278 rv64; call 279 44 -> 121 rv32; kmain 70 -> 172.
- Lock: the kernel lock (one ticket lock). The steward's hart waits 15-40 ms per window,
  behind the other hart's timer slice ends and channel calls (272) of verity:system and
  blkd:system. A typical wait is about 0.73 ms behind a 0.5 ms slice-end section, at every slice
  end. That is the page's "A hart waits for QEMU's turns" shape (kernel/scheduling.md, residual
  risks).

With the harts at once, the real two-hart cost is +1.7 ms in that term on either width: lock
waits behind the other hart's sections (R78's count bound). Whole notice p99 at two harts is
8.6-9.9 ms against 40.

## Where the target is judged today

The cell is in steward-latency, `icount = "shift=3,sleep=off"`: virtual time. On rv32 at two
harts it is `record=`, not judged. Every other cell of the case is judged in virtual time.
Keep apart: in host time this host's TCG runs the guest about 2.5x faster than icount shift=3
(one hart 5.8 ms against 14.2 ms for the same notice), so a host-time figure is not comparable
to the icount figures above. Compare ratios, two harts against one in the same mode.

## Proposal (needs the Architect; no code before)

No kernel or steward term to fix: the growth beyond x1.7 is the measurement's (QEMU's turns).
Options:
(a) Recommended. Keep the icount cell recorded with this reason, and judge it where the harts run
at once, as sched-capped / sched-capped-mttcg do: a `steward-latency-mttcg` twin (rounds N = 1-3,
no icount, `--quiet` class, rv32 and rv64 at [1, 2]) judging the deadline notice's p99 at 40 ms
net, target unmoved. The other cells stay judged in the icount case. The page's finding and
residual go, replaced by the turns sentence with both sets of figures.
(b) Same twin, but judge a ratio to a same-boot yardstick so host speed cancels (for example the
notice's p99 over the boot's R10 p50), since 40 ms in host time is looser than in virtual time.
(c) State it as a bound of R78's shape (the +1.7 ms real cost) and leave the cell recorded. Weaker:
nothing judges the cell.
Cost of (a): one case file, its docs (scheduling.md "With the real steward", the residual bullet,
testbench.md's list of host-clock cases), a twin boot about 9 min per width/hart count on the
quiet cores. Risk: the round's lease lengths come from icount launches; N = 1 ran clean in host
time, N = 2-3 not yet tried.

## Summaries checked

kernel/scheduling.md ("With the real steward", the finding bullet in residual risks, "A hart
waits for QEMU's turns"), tests/steward-latency.toml's record= comment: no change in this step,
since the page edit follows the ruling. README.md and GETTING-STARTED.md do not name the notice.
docs/kernel/README.md:233, timer.md:356, budgets.md, memory.md and testbench.md:839/905 name the
deadline notice, but only for the stand-in (`sched-latency`) or a negative run, not this cell, so
they need no change. Under (a), testbench.md's host-clock case list gains the twin.
