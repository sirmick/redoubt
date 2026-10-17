# B80 report — beamlet-schedulers-one-hart judged in guest time

Worktree /home/mcloonan/redoubt/.worktrees/B80, branch wp-B80 on main 68c4b2904. One commit
(`cases: beamlet-schedulers-one-hart-mttcg becomes beamlet-schedulers-one-hart, judged in guest
time ...`). Files: tests/beamlet-schedulers-one-hart.toml (renamed from -mttcg), scripts/jobs.mk,
docs/userland/beamlet.md, docs/testbench.md. Nothing in beamlet, the kernel or tools/testbench.

## What the case measured

The ratio is one scheduler's fastest run over two schedulers' fastest, in hundredths; under 90
fails. Train 48 read 82, 81 alone and 81 under load on rv32; rv64 passed each time.

Host: Core Ultra 9 285K, 8 P-cores (0-7, 5.5 GHz) + 16 E-cores (8-23, 4.7 GHz), powersave
governor. q's quiet set is E-cores 20-23. QEMU at smp = 1 has one vCPU thread: the two
scheduler threads never run at once on the host (the plan body's "two outruns one" is not what
happens; two is *slower* than one, and the ratio under 90 says by more than a tenth).

| run (case as it was, all ten pairs printed) | width | ratio | one, two (µs, fastest) | spread of the ten |
| --- | --- | --- | --- | --- |
| train 48: alone on main, alone on 3Y, under load | rv32 | 82, 81, 81 | 386,566 / 465,804 | — |
| jobs.mk quiet lease, alone | rv64 | 96 | 463,009 / 482,141 | — |
| jobs.mk quiet lease, alone | rv32 | 94 | 399,867 / 424,698 | — |
| q run --quiet, alone, ×3 | rv32 | 95, 90, 94 | 400,598 / 420,963; 405,909 / 448,815; 403,421 / 426,881 | one 400–443 ms, two 421–515 ms |
| q run --cores 1 (one P-core, every thread on it) | rv32 | 93 | 287,217 / 305,959 | one 287–313, two 306–327 |
| q run --cores 1 (one P-core) | rv64 | 96 | 405,277 / 420,958 | one 405–418, two 421–459 |
| icount shift=3 seed 1 (probe) | rv32 | 94 | 3,790,698 / 3,999,102 | ≤ 1.7 % |
| icount shift=3 seed 1 (probe) | rv64 | 96 | 3,459,590 / 3,601,614 | ≤ 1.2 % |

Sampled every ~150 ms from /proc (scratch: /home/mcloonan/redoubt/.tmp/b80/s*.txt): the vCPU
thread migrated among cores 20-23 13 to 28 times a run; the four cores' clocks read 0.8 to 4.7 GHz
during the runs (idle cores sit at 0.8 GHz and ramp when the thread lands). So within one boot the
ten host times of either side spread by a tenth, and min/min lands anywhere from 81 to 95. The
intrinsic cost in instructions is 5.5 % (rv32) and 4 % (rv64): the kernel's 1 ms slice ends
alternate the two threads where one scheduler keeps the hart. rv64 passed on the host's clock
because its margin over 90 was six points to rv32's four.

## The change

The case runs under `icount = "shift=3"`, `qemu_seed = 1` (a disk keeps sleep on; both timed runs
keep the hart busy), is renamed beamlet-schedulers-one-hart, leaves jobs.mk's quiet list for the
boot class, keeps the floor 90 (the page's nine tenths; four points under rv32's 94), and gets
`timeout_secs = 120` (4 × 28.5 s, the slowest alone; 34.3 s beside a 12-core load).

## Gates (all PASS)

| | rv64 | rv32 |
| --- | --- | --- |
| beamlet-schedulers-one-hart alone ×5 | 96, 96, 95, 95, 96 (one 3,459,56x; two 3,601,5xx / 3,603,83x) | 94 ×5 (one 3,790,70x; two 3,996,2xx / 3,999,0xx) |
| beside a 12-core spinning load ×2 | 96, 96 (34.3 s wall) | 94, 94 (33.3 s) |
| beamlet-schedulers-mttcg | 188 | 191 |
| beamlet-reduction-rate | 57,010 | 52,098 |
| docs, formatting, size-budget, unsafe-budget | PASS | — |

Logs: /home/mcloonan/redoubt/.tmp/b80/gates/ (per-run job logs), gates.out (summary).

## Summaries checked

- docs/userland/beamlet.md: status line renamed; "Several schedulers" table and floor text updated
  with the guest-time figures and the host-clock spread.
- docs/testbench.md: "Which cases run in guest time" list and the verdict-only-alone clause updated.
  The counts there (145 of 226, 68, 4) were already stale (390 case files) and are not touched.
- scripts/jobs.mk header comment: names no case; the quiet list loses the case.
- README.md, GETTING-STARTED.md, docs/plan/*.md: no mention of the case; no change.
- tests/beamlet-schedulers-mttcg.toml: does not name its twin; no change.
- .wash/plan.toml and .wash/qa/B57-split-line.md name the old case name as history; not mine to edit.
