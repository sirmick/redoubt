#!/usr/bin/env python3
# K13 measurement only: from a containment-gate console log whose scratch kernel marks a
# deadline destruction's X/Y id with bit 40, list each destruction's R10 time and its threads'
# ending (T/t phase 1 inside it), and the medians of the full-fill ones (R10 >= 5 ms) per slot.
import re, statistics, sys

recs = []
for line in open(sys.argv[1], errors="replace"):
    m = re.match(r"SCHED-TRACE (\d+) (\d+) (\S) (\d+) ([0-9a-f]+)\s*$", line)
    if m:
        recs.append((m.group(3), int(m.group(4)), int(m.group(5), 16)))

rows, open_x, open_t, inside = [], None, None, 0
for kind, ident, us in recs:
    if kind == "X":
        open_x, inside = (ident, us), 0
    elif kind == "Y":
        rows.append(("D" if open_x[0] >> 40 else "H", us - open_x[1], inside))
        open_x = None
    elif kind == "T":
        open_t = us
    elif kind == "t":
        if open_x is not None:
            inside += us - open_t
        open_t = None

big = [r for r in rows if r[1] >= 5000]
print(f"{len(rows)} destructions, {len(big)} at >= 5 ms")
for slot in "HD":
    s = [r for r in big if r[0] == slot]
    if s:
        print(f"{slot}: n={len(s)} R10 median {statistics.median(r[1] for r in s):.0f} us, "
              f"threads' ending median {statistics.median(r[2] for r in s):.0f} us; "
              f"each (R10, threads): {[(r[1], r[2]) for r in s]}")
