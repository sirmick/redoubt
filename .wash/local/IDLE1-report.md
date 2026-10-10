# IDLE1 report (idle1-implementer, 2026-10-09)

Owner's question: does the launched system properly idle? Yes, within its ceilings: at rest, with
alice at an SSH prompt for 60 s on launch's 4-hart machine, QEMU takes 0.002 host cores and each
hart takes 3 to 4 traps a second. One cause stands out (below): wake-ups of harts waiting for the
kernel lock, about three quarters of every hart's interrupts.

## Branch

- Base: main f5316e38c. Head: wp-IDLE1 aee5f940d, one commit (39040eb43 rebased, hunk folded).
- The uncommitted hunk: the `Dir` guard (each idle.rs test's directory removed however the test
  ends) was right and is folded in. Its `settle()` note was **not**: it said QEMU writes the log
  through a ~4 KiB stdio buffer that may be unflushed after 200 ms. QEMU 10.2.1 (this host) under
  `strace -e write` with `-d int -D` makes one `write` per log line (311 594 writes of 131 bytes in
  4 s), so nothing sits in a buffer; the note was dropped, `settle()` is unchanged from 39040eb43.
  The commit message is still true as written.

## Gates (all through scripts/q, env from launch-env-2026-10-09.md)

| gate | command | rc |
| --- | --- | --- |
| testbench host tests | `make -f scripts/jobs.mk set CASES="host-tests memory-host-tests"` | 0, 0 (PASS 54.2 s, 50.4 s) |
| idle tests, focused | `scripts/q run --cores 4 -- cargo test -p testbench -- idle` | 0 (5 passed); no testbench-idle-* dirs left |
| formatting | `make -f scripts/jobs.mk set CASES=formatting` | 0 |
| docs | `make -f scripts/jobs.mk docs` | 0 |
| prebuilt | `make -f scripts/jobs.mk prebuilt` | 0 |
| launch-idle rv64 | `scripts/q run --cores 4 --quiet -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch rv64 launch-idle` | 0, PASS 66.7 s |
| launch-idle rv32 | same, `--arch rv32` | 0, PASS 65.5 s |
| launch-system | `make -f scripts/jobs.mk set CASES=launch-system` | 0, 0 (PASS rv64 5.3 s, rv32 5.0 s, net class) |

launch-idle ran alone: rv64 then rv32 one after the other, each on the 4 quiet cores (20-23),
never beside another boot of mine; other members' jobs held general cores 0-19 meanwhile.

## Measured (one 60.0 s window each) against the case's ceilings

| | rv64 | rv32 | ceiling |
| --- | --- | --- | --- |
| interrupts/s, busiest hart | 3.9 (hart 0) | 4.1 (hart 0) | 9.0 |
| user ecalls/s | 5.9 | 7.0 | 18.0 |
| QEMU host cores | 0.002 | 0.002 | 0.01 |

Per hart, interrupts/s by cause:

| hart | rv64 | rv32 |
| --- | --- | --- |
| 0 | 3.9: m_software 2.9, s_software 0.1, s_timer 0.9 | 4.1: m_software 2.9, s_software 0.3, s_timer 0.9 |
| 1 | 3.7: m_software 2.7, s_software 0.1, s_timer 0.8 | 3.6: m_software 2.7, s_software 0.2, s_timer 0.7 |
| 2 | 3.6: m_software 2.6, s_timer 0.9 | 2.9: m_software 2.1, s_timer 0.8 |
| 3 | 3.1: m_software 2.3, s_timer 0.7 | 2.8: m_software 1.9, s_timer 0.8 |

(s_external 0.0 on harts 1-3 on both: present, under 0.05/s.) Exceptions/s, every hart's:
supervisor_ecall 19.5 rv64 / 19.2 rv32; user_ecall 5.9 / 7.0.

All within the ranges the commit's ceilings were set from (4.3, 8.7, 0.004 most); the ceilings
stand at 2.2x, 2.6x to 3x and 5x this run's values. Host cores is measured in 1/100 s ticks (0.002
cores over 60 s is 12 ticks), so 0.01 is not far above the measurement in its own resolution; no
ceiling change proposed.

## Where the traps come from (trap epc symbolised against the release kernel, nm)

rv64 window (rv32 the same shape, counts in parentheses):
- supervisor_ecall 1 173 (1 149): `TicketLock::release` 602 (541), `time::rearm` 501 (540),
  `sched::leave` 60 (63), `hart::shootdown` 5 (5).
- m_software 625 (574): in `TicketLock::acquire_ticket` 559 (508), in `arch::riscv::idle` 59
  (61), `hart::shootdown` 5 (5).

So: the kernel lock's release sends an SBI IPI to each hart halted waiting for it
(kernel/src/cell.rs, `release` -> `wake_halted`), about 10 a second at rest, and those wake-ups are
~75 % of every hart's interrupts and half the firmware calls. `rearm`'s set_timer runs ~8.5 a
second against ~3.3 timer interrupts a second over all harts. `sched::leave`'s ~1/s IPIs match the
~1/s wakes out of `idle`.

## Proposed follow-up node (not fixed; the kernel is not this package's)

**IDLE2: kernel-lock wake-ups at rest.** At rest ~10 lock hand-offs a second each cost an SBI
`send_ipi` and an M-mode software interrupt, three times the timer rate. A likely cause, to be
confirmed: every hart's timer is armed for the earliest timeout and budget deadline
(docs/kernel/timer.md: "the earliest timeout of any blocked thread; the earliest deadline of any
budget"), so one timeout wakes all four harts at once and three queue on the lock. Measure with
the sched/lock trace features; options are one hart taking a shared timeout, or a hart that finds
nothing to do dropping out without the lock. A second, smaller item for the same node: `rearm`
re-arms ~2.5 times per timer interrupt. launch-idle's interrupts-per-hart ceiling would then be
lowered with the measurement.

## Affected summaries checked

- docs/testbench.md "The machine at rest" (in the commit): matches the code: monitor switch,
  `logfile`/`log int`/`log none`, counts by hart and name, /proc times, the floors, the status
  list's 6 tests all exist and pass. No change.
- GETTING-STARTED.md: no mention of idling or launch-idle (grep idle / at rest); the case cites
  its "Try the system" section only as what it boots. No change.
- docs/kernel/timer.md: the measurement agrees with its arming rule (every hart's timer comes by
  the earliest timeout/deadline; one ecall per arming); the case measures, it changes no kernel
  rule. No change; the follow-up above cites it.
- README.md: no idle claim (grep). No change.

## Fix round (steward-red OK with notes on aee5f940d) -> head 0e8b68421

- P2 window end: the `idle` step now waits for the bench's answer on each edge
  (`Shared::idle_edge`, ssh.rs; `IdleEdge` = (on, ack sender)): the quiet starts once `log int`
  is typed, the next step once `log none` is. A bench that fails meanwhile aborts the wait; the
  deadline bounds it. The post-loop `idle_edges` in qemu.rs is gone (a finished session has had
  both edges answered).
- P2 docs/testbench.md "The machine at rest": the edge ordering; ceilings about twice the most of
  nine 60 s windows (five rv64, four rv32); host cores read in CLK_TCK ticks (100/s: 0.002 cores
  over 60 s ~12 ticks, 0.01 = 60).
- P3 commit message: nine windows, five rv64 and four rv32, as the case comment.

Gates on 0e8b68421 (base main f5316e38c), all rc=0: `cargo test -p testbench` via q (182
passed), formatting case, docs, prebuilt, launch-idle rv64 (PASS 65.6 s) and rv32 (PASS 65.6 s),
each alone, `q run --cores 4 --quiet`, one after the other.

| | rv64 | rv32 | ceiling |
| --- | --- | --- | --- |
| interrupts/s, busiest hart | 3.1 (m_software 2.1, s_timer 0.9, s_software 0.1) | 2.8 (m_software 1.9, s_timer 0.8, s_software 0.1) | 9.0 |
| user ecalls/s | 4.1 | 4.1 | 18.0 |
| supervisor ecalls/s | 14.5 | 14.1 | - |
| QEMU host cores | 0.002 | 0.002 | 0.01 |

User ecalls fell from 5.9/7.0 to 4.1 on both widths: the next step's command no longer lands in
the window, as the red predicted. The m_software share (~70 %) and the IDLE2 proposal stand.
Ceilings unchanged.

## Text fold (steward-red renewed OK with notes on 0e8b68421) -> head 1db22499c

P3 folded, text only: tests/launch-idle.toml's comment, docs/testbench.md "The machine at rest"
and the commit message say the nine windows the ceilings come from were measured before the
session's next step waited for `log none` (its command fell in them), that one window on each
width since measured 4.1 user ecalls/s, so `user_ecalls` (18) is loose by about 2x until the
kernel's lock wake-ups at rest are cut and the ceilings set again. No case rerun (text only).
Gates on 1db22499c: `make -f scripts/jobs.mk docs` rc=0; formatting case rc=0.
