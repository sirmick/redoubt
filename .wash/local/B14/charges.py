#!/usr/bin/env python3
"""Per-budget charges from a sched-trace log, inside each SHARE window (analysis aid, not shipped).

A budget's pass rises by ticks * STRIDE / weight; with the weight w, rise * w / STRIDE is ticks
(10 ticks/us). Rises into a W (a floor lift) or across G/L groups are not counted.
Usage: charges.py LOG WEIGHT"""
import sys

STRIDE = 1 << 20
log, weight = sys.argv[1], int(sys.argv[2])
recs, shares = [], []
for line in open(log, errors="replace"):
    f = line.split()
    if len(f) == 6 and f[0] == "SCHED-TRACE":
        recs.append((int(f[1]), int(f[2]), f[3], int(f[4]), int(f[5], 16)))
    elif f and f[0] == "SHARE":
        shares.append((f[1], int(f[2]), int(f[3]), int(f[4])))
# time of each record: carry the last stamp forward
stamp, times = 0, []
for r in recs:
    if r[2] in "IUVXYTtMm":
        stamp = r[4]
    times.append(stamp)
for name, start, end, cpu in shares:
    last, charged, slices, timer_b = {}, {}, {}, {}
    audits, i = 0, 0
    u = None
    while i < len(recs):
        seq, entry, kind, bid, p = recs[i]
        inside = start <= times[i] <= end
        if kind in "GL":
            # skip the group; restart counts for the budgets it names
            n = 6 if kind == "G" else 9
            for r in recs[i:i + n]:
                last.pop(r[3], None)
            i += n
            continue
        if kind == "U":
            u = p
        if kind == "V" and u is not None and inside:
            audits += p - u
        if kind == "B" and inside:
            timer_b[bid] = timer_b.get(bid, 0) + p
        if kind in "WKRDP":
            before = last.get(bid)
            last[bid] = p
            if kind == "K" and inside:
                slices[bid] = slices.get(bid, 0) + 1
            if before is not None and kind != "W" and inside and p > before:
                charged[bid] = charged.get(bid, 0) + (p - before) * weight // STRIDE
        i += 1
    total = sum(charged.values())
    print(f"{name} [{start},{end}] window {end-start} us, audits {audits} us, program cpu {cpu} us")
    for b in sorted(charged, key=lambda b: -charged[b]):
        c = charged[b] / 10
        print(f"  budget {b}: charged {c:.0f} us ({1000*charged[b]//max(total,1)} of 1000 charged), "
              f"picks {slices.get(b,0)}, timer-entry charges {timer_b.get(b,0)/10:.0f} us")
    print(f"  all charged {total/10:.0f} us; window net of audits {end-start-audits} us")
