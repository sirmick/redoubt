# SCHED1 1 ms slice diagnosis: two scratch boots, rv64, HEAD a787bd3d2 (2026-10-06)

The scratch case files are here (they were never committed). The run directories
(`run-3751388-...`, `run-3753703-...`) were pruned by the bench's run-directory rotation before
they could be copied. The figures below were read from those consoles before the pruning.

## diag-large-weight

`[large-weight] FAIL: the weight-1000 server got 412 of 1000, want 555`.

Oracle: 16917 records and 1591 picks in rank order; 1664 audits, 152009 µs.

| Measure | Value |
| --- | --- |
| Picks: server (budget 31) | 767 |
| Picks: each of the eight users | 80 or 81 |
| Picks: launcher | 177 |
| Slice-end timer interrupts (`I`) | 1553, from 2.6 ms to 2470 ms |
| Median gap between consecutive interrupts | 1450 µs (1388 µs for the launcher) |

## diag-ties

`first runs A [63363301, 63377191, 63391878], B [63028482, 63061987, 63097264]`, and the
`clause 2` FAIL.

Trace around the judge (budget 1) and its three sends:
- The judge woke at entry 849 (W) and was picked (K) at 851.
- U3 (the IPC-list audit) ran from 379169 to 379671 µs. Then b1 (34) woke (W), the marks audit
  (U4/V4) ran, and at 379765 the timer interrupted budget 1 (I 1), which was requeued (R 1).
  b1 was picked next (K 34) and blocked (D).
- The judge was picked again (K 1). The marks audit ran from 381250 to 381282, U3 from 382449 to
  383021, then b2 (38) woke, then I 1 and R 1.
- Third send: the same pattern, with U3 from 385914 to 386549, then b3 (42) woke, then I 1 and
  R 1, then K 42.
- After the third send: K 1, then X 31 (the destroy) at 388632.
