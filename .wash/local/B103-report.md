# B103: the deadline notice at two harts is judged where the harts run at once

Base main 328bcecff (train 97). Branch wp-B103, head 64c5d5c34: one commit, no WIP. Step 1's
investigation (the parts, the turns) is kept below under "Step 1".

## Delivered (ruling (a), .wash/local/wave-2026-10-18-rulings.md "B103: ...")

- `tests/steward-latency-mttcg.toml` (new). It runs steward-latency's rounds N = 1-3, manifest,
  probe and session without icount, on the host's clock. It is in the quiet class and out of the
  whole run, as steward-latency is. Both widths at [1, 2].
  - Judged: the deadline notice's p99 at two harts, 40 ms net of audits.
  - Recorded: the one-hart notice (`record=`); every other measure has no bound here.
  - Leases are 1 / 1 / 2 s: twice each round's worst launch on the host's clock (0.50 / 0.48 /
    0.60 s over the measuring and acceptance runs), rounded up to a second.
- `tests/steward-latency.toml`: records the two-hart notice on both widths
  (`record=rv64:2:N=k` and `rv32:2:N=k`, k = 1-3) with the turns reason in its comment. The one
  hart stays judged.
- `scripts/jobs.mk`: the twin is in the quiet list.
- `tools/testbench/src/case.rs`: the probe-builders host test lists the twin.
- `docs/kernel/scheduling.md`:
  - Responsiveness status: 27, with the twin.
  - "With the real steward": the table's rv64 two-hart cell is marked recorded. The finding
    paragraph is replaced by the parts, the cause, the twin's figures and the sensitivity.
  - Residual risks: the rv32 finding bullet is removed, and "A hart waits for QEMU's turns" gains
    the notice cell.
- `docs/testbench.md`: the host-clock cases name the twin.
- Dropped as instructed: the instrument commits, and the test-only late-notice steward.

## Acceptance runs

The twin, `q run --quiet`, each alone, all rc=0 PASS. Notice net p99 in ms, N = 1/2/3.

| boot | run 1 | run 2 | run 3 |
| --- | --- | --- | --- |
| rv32, 2 harts | 9.4/9.8/9.4 (rerun) | 9.2/8.0/11.7 | 9.1/9.7/9.1 |
| rv32, 1 hart (recorded) | 5.1/6.2/5.5 (rerun) | 5.0/6.0/6.7 | 5.5/5.8/6.5 |
| rv64, 2 harts | 8.4/8.8/8.5 (rerun) | 7.2/8.5/8.4 | 9.4/9.5/7.7 |
| rv64, 1 hart (recorded) | 5.4/6.4/6.6 | 5.1/6.3/6.3 | 5.3/6.3/6.4 |

B107's pinned host work overlapped the quiet cores until 07:05Z. The first runs of the three
boots marked "(rerun)" started before that, and were rerun at the end of the set; the table
judges the reruns. The overlapped runs read:

- rv32, 2 harts: 9.4/9.0/9.1
- rv32, 1 hart: 7.3/6.4/6.7
- rv64, 2 harts: 7.7/9.9/9.5

Spread on the page, two harts: rv32 9.1-9.4 / 8.0-9.8 / 9.1-11.7 and rv64 7.2-9.4 / 8.5-9.5 /
7.7-8.5. Worst launch per boot is at most 0.60 s (N = 3, rv32, 2 harts) and at most 0.50 s at
N = 1, against leases of 2 s and 1 s. Wall time is about 620 s a boot.

Measuring runs before the acceptance set (3 s leases): rv32 2h 10.1/10.3/9.4, 1h 6.3/6.9/7.4;
rv64 2h 8.6/8.2/9.1, 1h 6.6/7.6/9.2.

## Ruling condition 3: can the twin fail

No cheap negative misses. A steward that held each notice to the loop's next turn, after the
exit's effects and the console turn, was tested and not kept. The patch is in
.tmp/B103/late-notice.patch, applied on 2b60945b6, with the 3 s leases. At two
harts it read:

- rv32: 13.8 / 14.9 / 14.7 ms
- rv64: 12.9 / 13.5 / 14.5 ms

That is about 5 ms over, and it passed. So the twin sees a regression only once it adds about
30 ms of host time in the notice path, about 2.5x that in icount time on this host (one-hart
notice 5.8 ms under multi-threaded TCG against 14.2 under icount). This is stated on the page.

## Gates (on 64c5d5c34, prebuilt rc=0)

- `make -f scripts/jobs.mk set CASES="docs formatting host-tests size-budget"`: rc=0, all PASS.
- Earlier on the same content: `steward-host-tests` and `unsafe-budget` PASS. No steward code
  is in the final diff.
- `steward-latency` rv64 at two harts (icount) with the new `record=`: rc=0 PASS, the notice
  recorded at 34.9 / 33.4 / 31.9 ms (the page's table keeps the earlier 30.4 / 32.4 / 24.6 run).
- rv32 and the kernel are untouched. No unsafe, no size change.

## Summaries checked

- docs/kernel/scheduling.md: updated as above.
- docs/testbench.md host-clock list: updated. Its case counts (352/187/165) were already stale
  on main before this change. They are not recounted here, since the categories' definitions are
  not mine; this is a follow-up.
- docs/servers/steward.md Leases ("the per-notice target, 40 ms, is met one lease at a time"):
  still true, no change.
- docs/kernel/README.md:233 and timer.md:356 name the stand-in's notice, not this cell: no change.
- README.md and GETTING-STARTED.md do not name the notice.

## Open

- steward-latency-n4 and -n16 still judge the two-hart notice under icount (rv64 N = 16 2h
  29.9 ms). The same artefact applies, but the ruling named N = 1-3 only; this is left as is.
- I read in full the files I wrote: the twin and the page passages. For case.rs and
  steward-latency.toml I read the changed regions only.

## Step 1 (investigation, kept)

The term that grows is the steward's wake on the notice to its send returning.

| | 1 hart | 2 harts |
| --- | --- | --- |
| icount, rv32 | 7.8 | 29.0 ms |
| icount, rv64 | 9.2 | 20.3 ms |
| harts at once, both widths | 2.5 | 4.2 ms |

- The other hart runs verity:system and blkd:system in that term.
- The steward's hart reads 24.6 ms of user time, against 0.5 at one hart.
- Sections read about 5x longer at two harts under icount: timer int p50 92->470 us rv32,
  58->278 rv64.
- Lock: the kernel lock, behind the other hart's slice-end and channel-call sections.

Tools and consoles are under /home/mcloonan/redoubt/.tmp/B103/.
