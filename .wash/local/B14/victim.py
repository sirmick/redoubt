#!/usr/bin/env python3
"""Break a budget's charges in a window down by who ran when they were made (analysis aid).
Usage: victim.py LOG WEIGHT BUDGET SHARE-NAME"""
import sys
from collections import Counter

STRIDE = 1 << 20
log, weight, me, want = sys.argv[1], int(sys.argv[2]), int(sys.argv[3]), sys.argv[4]
recs, win = [], None
for line in open(log, errors="replace"):
    f = line.split()
    if len(f) == 6 and f[0] == "SCHED-TRACE":
        recs.append((int(f[1]), int(f[2]), f[3], int(f[4]), int(f[5], 16)))
    elif f and f[0] == "SHARE" and f[1] == want:
        win = (int(f[2]), int(f[3]))
stamp, running, in_timer, last = 0, None, False, None
by = Counter()
for seq, entry, kind, bid, p in recs:
    if kind in "IUVXYTtMm":
        stamp = p
    inside = win[0] <= stamp <= win[1]
    if kind == "I":
        in_timer = True
    if kind == "O":
        in_timer = False
    if kind == "K":
        running_before, running = running, bid
    if bid == me and kind in "WKRDP":
        if last is not None and kind != "W" and p > last and inside:
            ctx = ("me running" if running == me else f"{running} running") + (", timer entry" if in_timer else "")
            by[(ctx, kind)] += (p - last) * weight // STRIDE
        last = p
for k, v in sorted(by.items(), key=lambda kv: -kv[1]):
    print(f"  {k}: {v/10:.0f} us")
