# STEWARD13 finding: rv32 one hart, N = 1, the decision wake at 30 ms with the launch off the loop

Every gate passes on both widths but one row: `steward-latency` on rv32 at one hart, round
N = 1, `decision_wake` net p50 30,376 µs (p99 33,376) against the 25 ms p50 target. Run alone
(`.worktrees/STEWARD13/.tmp/STEWARD13/rerun2/B.log`; console kept at
`.worktrees/STEWARD13/target/jobs/rv32-steward-latency.smp1.log`); under the eight-QEMU gate set
it was 32,314 / 34,419. N = 2 and N = 3 in the same boot: 1,202 / 1,440 and 969 / 2,299, as
before. rv64 at one and two harts: every round met. The page's table had the rv32 N = 1 cell at
24,235 / 28,493, 0.8 ms under the target.

## The bisect (one extra boot)

The same tree with the launch run inline on the loop (the thread started and idle, the reply
tokens, the parked calls, the held events and the install lock all in; a temporary six-line
switch in `Machine::launch`, not committed; `.tmp/STEWARD13/exp1/`): N = 1 decision wake
23,300 / 31,843 (met), the 20 ms timer wake 828 / 1,486 where the threaded run gives
2,173–4,119 (p50 about 2.2 ms), N = 2 932 / 40,617. So the shift is the launch running off the
loop, and only at one spinner.

## What the window looks like

Both forms are pathological at N = 1: a 30 ms receive timeout sleeps 53 to 62 ms while one lease
VM runs (the probe's own, just launched, loading its modules and then spinning), and beside a
second VM it wakes 1 to 3 ms late. In the console the sequence is identical at N = 1 and N = 2:
`reply_wait`, `timer_wake` (2–4 ms late), `decision_wake`; nothing of the steward's runs between
(the probe arms the timer at the loop's top and waits). What differs with the thread: the VM's
`process_start` is the thread's call, and the loop's lines (the record, the usage line, `served`,
the launch window) are written while the thread's report rendezvous completes, so the loop's
receive begins about 10 ms earlier relative to the VM's first run (`reply_wait` 105 µs where the
inline form gives 8.9–12.2 ms).

## What I can and cannot do

The quantity is the kernel's wake of a sleeping receive against one running VM: the steward
does nothing in the window, and the lateness is about the whole timeout in both forms, which
reads as a slice or timer-programming effect to find in the trace, not steward work. Options:

- (a) the rv32 one-hart N = 1 decision row is recorded, not judged, pending a kernel node on the
  one-runnable-VM timeout wake (as the rv32 two-hart timer wake already is), the page stating
  the measurement before and after and the bisect;
- (b) bisect further at one 12-minute boot each: `process_start` from the loop with the reads on
  the thread; the loop's lines written after the reply instead of before; a sleep of the loop
  before arming;
- (c) something I am missing.
