# IRQ1 first measurement (wp-IRQ1 working tree, uncommitted; base 19e3439b9)

`q run --cores 8 -- cargo testbench sched-lock-contention`: 4 PASS (exit 0). Logs copied to
.worktrees/IRQ1/.tmp/lc1/. Driver wake from the post-check (net of audits), before = SMP2's on main.

| case | width | before net p50/p99 | after net p50/p99/max | after gross p50/p99 |
| --- | --- | --- | --- | --- |
| 2 harts | rv64 | 18.5 / 19.2 | 1.5 / 10.7 / 10.7 | 2.0 / 11.1 |
| 2 harts | rv32 | 18.3 / 19.1 | 1.7 / 10.6 / 10.6 | 2.3 / 11.2 |
| 4 harts | rv64 | 4.3 / 59.3 | 3.5 / 58.6 / 58.7 | 5.7 / 61.8 |
| 4 harts | rv32 | 7.1 / 86.9 | 5.4 / 58.6 / 58.7 | 8.4 / 61.9 |

Other lines (after): lock order held, at most 1 (2 harts) / 3 (4 harts) sections ahead; lock waits
126 / 134 of 1000 at 2 harts (rv64/rv32), 486 / 499 at 4; nobody 29 / 44 (2), 47 / 65 (4).
Claims by hart (checked-build count): 2 harts rv64 [12, 189], rv32 [0, 201]; 4 harts rv64
[15, 2, 184, 0], rv32 [176, 3, 22, 0]. Entries from user mode that claimed nothing: 170-384 a run,
all billed by the rule (oracle check new).

## 2 harts
Distribution (gross, rv64): min 1.2, p50 2.0, p65 10.3, max 11.2 ms: a wake waits at most the rest
of the one search in progress; the second section is gone.

## 4 harts: the tail is bimodal, and was there before IRQ1
Gross samples, rv64: 131 under 10 ms, 26 at 60-62 ms, the rest spread 20-50; rv32 similar (38 at
50-62 ms). From sample ~113 on, every other sample is ~61 ms (six 9.6 ms sections). On main the p99
was already 59.3 (rv64) / 86.9 (rv32), so IRQ1 leaves rv64's tail where it was and cuts rv32's.

What the trace shows in one slow sample (rv64, records 17228-17356): the driver blocks in `receive`
on hart 2 at 1707.7 ms (trace clock), hart 2 idles; no hart takes a device interrupt until about
1738.6 ms (no `x` entry from the hammers, hart 2 draws its ticket only after hart 1's at 1738.6);
then the wake (W31 at record 17356, ~1739.4 ms). So in that sample the idle hart was not woken for
~30 ms: either the alarm was raised late on the trace's clock, or hart 2's SEIP did not rise. I have
not yet explained it; the trace records neither the alarm's rise nor the idle hart's wfi exit. The
driver's `rtc::clear` comes after `receive` returns (sched.rs `driver`), so the RTC line is still high
when the kernel completes the claim; I have not ruled out a latch effect.
