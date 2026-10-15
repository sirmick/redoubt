# B58 trials (k33-implementer, 2026-10-12), wp-B58 uncommitted

Kernel: the slice-end decision recorded (`=`: end and now, µs), sched-trace only; the trace's
slice_over set from the decision, not sampled at entry. Oracle: computes 'over' from `=` and
refuses an `O` that disagrees (host test the_oracle_decides_whether_a_timer_interrupt_ended_the_slice;
5 fixtures given the kernel's `=`; testbench 188 pass). Case: stepped naps base + 17k mod 1000,
calls 37 µs, 120 naps, `wake_mid_slice=5` (new oracle bound) beside `wake_witnesses=5`.

| base nap | rv64 mid-slice / at a call | rv32 mid-slice / at a call |
| --- | --- | --- |
| 130 µs (ruled) | 29 / 65 (1 run) | 0 / 43, 0 / 43 (2 runs) |
| 600 µs | 33/56, 31/52, 36/56, 43/46, 41/52 | 12/70, 11/69, 4/73 (FAIL), 13/70, 19/65 |
| 1300 µs | 8/47, 6/45, 6/52, 9/47, 5/49 | 13/60, 15/55, 13/57, 12/58, 16/56 |

rv32 at 130 µs: 79 timer interrupts returned to user mid-slice and none held a wake: each found
nothing due (no `E`), the timeout already answered at the other hart's call. Under icount the armed
hart takes its interrupt at its next turn, after the other hart's call. Mid-slice wakes on rv32 are
QEMU's turn order, not the kernel.
Traces: .tmp/B58/t3run (rv32 130 µs), analysis .tmp/B58/ana.py, ana2.py; outputs .tmp/B58/n600-*, n1300-*.
