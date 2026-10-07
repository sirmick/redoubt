#!/usr/bin/env python3
"""Charges a wake's floor lift absorbs: for each wake of BUDGET inside the SHARE window, the pass at
its leave (D), the charges folded while out (rises to the last P before the W) and the floor (the
W's pass). Absorbed = (max(D, floor) + charges) - max(D + charges, floor). (Analysis aid.)
Usage: absorbed.py LOG WEIGHT BUDGET SHARE-NAME"""
import sys

STRIDE = 1 << 20
log, weight, me, want = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
recs, win = [], None
for line in open(log, errors="replace"):
    f = line.split()
    if len(f) == 6 and f[0] == "SCHED-TRACE":
        recs.append((f[3], int(f[4]), int(f[5], 16)))
    elif f and f[0] == "SHARE" and f[1] == want:
        win = (int(f[2]), int(f[3]))
stamp, out_at, last, wakes, absorbed, folded = 0, None, None, 0, 0, 0
for kind, bid, p in recs:
    if kind in "IUVXYTtMm":
        stamp = p
    if bid != me or kind not in "WKRDP":
        continue
    if kind == "D":
        out_at = p
    elif kind == "W" and out_at is not None:
        c = last - out_at
        if win[0] <= stamp <= win[1]:
            wakes += 1
            folded += c
            absorbed += (max(out_at, p) + c) - max(out_at + c, p)
        out_at = None
    last = p
t = lambda x: x * weight / STRIDE / 10
print(f"{want}: {wakes} wakes of {me}; charged while out {t(folded):.0f} us; absorbed by the lift {t(absorbed):.0f} us")
