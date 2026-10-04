# GATE1 two-lease notice split (rv64 seed 4, wp-gate1 e9ecc5ea5, 2026-10-02)

Run: `in-dev cargo testbench kernel-containment --arch rv64`, exit 1, 219 s, unchanged code.
Stdout: GATE1-two-lease-rv64.out. Console and trace: GATE1-two-lease-rv64.console.
Script: GATE1-two-lease-split.py, a reading only.

Post-check:
- R10 18 destructions, p50/p99/max 21050/25456/25456 µs, met (30000)
- lease end 7757 + 25456 = 33213, met (125000)
- driver wake net 8823/10710, met
- timer wake 8054/9674, met
- decision wake 7755/7757, met
- deadline notice net 26155/57053, MISSED (40000)
- 230 audits, 2563513 µs total

Terms per D lease, µs, net of the U/V audits inside each. Deadlines are end - gross of the
lease's two samples. X is the first X at or after the deadline, Y the next Y. r1 and r2 are the
two receipts, in order.

```
d | a | b | c1 | c2 | gross a b c1 c2 | X id
147763459 | 283 | 25335 | 457 | 20684 | 283 25335 59325 31545 | 4157
252858339 | 349 | 25171 | 565 | 10886 | 349 25171 59468 21764 | 12359
357959969 | 283 | 25171 | 566 | 10885 | 283 25171 59469 21763 | 20561
463063861 | 283 | 25297 | 418 | 21211 | 283 25297 59321 32089 | 28763
567892958 | 283 | 24992 | 527 | 10682 | 283 24992 59430 21560 | 36965
672721206 | 283 | 25171 | 527 | 10723 | 283 25171 59430 21601 | 45167
777549340 | 283 | 24972 | 457 | 31341 | 283 24972 59359 42219 | 53369
882661329 | 283 | 24897 | 525 | 10760 | 283 24897 59428 21638 | 61571
987758868 | 283 | 25456 | 416 | 21086 | 283 25456 59318 31964 | 69773
a p50/p99 283 349
b p50/p99 25171 25456
c1 p50/p99 525 566
c2 p50/p99 10886 31341
----
```

## Readings
- **a, expiry:** 283-349 µs. Expiry is on time.
- **b, the walks:** 24.9-25.5 ms, against about 20.8 ms with H destroyed first (seed 3 split).
  It grows about 4 ms with a second full lease live, and stays within R10 (p99 25456 <= 30000).
- **c1:** net about 0.5 ms. Its gross is 59 ms, almost all one audit (U1/V1, about 58.9 ms)
  straight after Y, which is subtracted. The stand-in (budget 42, as mapped before) is woken at
  the floor and is the first pick after Y.
- **c2, every lease:** the pattern is the same in all 9. K42 takes receipt 1. Then a short audit
  (U2/V2, about 10.8 ms) runs inside the stand-in's slice. The stand-in is then requeued (R42),
  never blocked: there is no D42 between its two receipts. Its pass rose by about 12 ms of charged
  time at weight 1000, the audit's included. Next the victim (43) is picked at the floor, then
  one H budget (two in lease 53369) at the floor, for a full slice each. Only then K42 takes
  receipt 2. Net c2 is 10.7-31.3 ms, which is whole H slices.
  - The H budgets are picked at a pass at or below the stand-in's: F against F + delta. No higher
    pass runs first, and no receive entry is long. That is R12 as designed.
  - Its trigger is the checked build. The audit is subtracted from the window, but it uses up the
    stand-in's slice, and the requeue that follows is not subtracted. Without audits the stand-in
    would run about 1 ms and take both notices in one slice.
  - The stand-in does no fixture work between its two receives. The code is time_now, the
    notice's checks and receive. The trace has no D, call or victim_control by 42 between them.

## Summary per term

| Term | Reading |
| --- | --- |
| a | not late |
| b | +4 ms of walks with H live; within R10 |
| c | R12 as designed, at or below the stand-in's pass, set off by the checked build's audit spending the stand-in's slice |
| fixture | none |
| kernel defect | none in this run |
