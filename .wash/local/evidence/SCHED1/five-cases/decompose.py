"""SCHED1 five-cases: total charge per budget and the slice-end gap split (host-only, read-only).

usage: decompose.py <console> <weight-by-budget-id as id=w,id=w,...>

Inside the same window as attribute.py (first to last timer interrupt of a named budget):
- every pass rise of each named budget (`P` records, pass after the event), times its weight over
  STRIDE: the budget's whole charge, including the kernel time billed to it after it was
  descheduled (attribute.py counts only `K` to `R`/`D`, which leaves that out for a budget picked
  after another and puts it in for a re-pick of itself);
- per pair of consecutive timer interrupts with exactly one marks audit (`U 4`/`V 4`) between:
  interrupt to audit start, the audit, and audit end to the next interrupt less SLICE_US.
"""
import statistics
import sys

STRIDE = 1 << 20
SLICE_US = 1000
path, weights_arg = sys.argv[1], sys.argv[2]
weights = {int(k): int(v) for k, v in (kv.split('=') for kv in weights_arg.split(','))}
recs = []
for line in open(path, errors='replace'):
    if line.startswith('SCHED-TRACE '):
        f = line.split()
        recs.append((f[3], int(f[4]), int(f[5], 16)))

irq = [i for i, r in enumerate(recs) if r[0] == 'I' and r[1] in weights]
lo, hi = irq[0], irq[-1]
last = {}
for kind, bid, val in recs[:lo]:
    if bid in weights and kind in 'PKRDWNA':
        last[bid] = val
rise = dict.fromkeys(weights, 0)
for kind, bid, val in recs[lo:hi + 1]:
    if bid not in weights or kind not in 'PKRDWNA':
        continue
    if kind == 'P' and bid in last and val > last[bid]:
        rise[bid] += (val - last[bid]) * weights[bid] // STRIDE
    last[bid] = val
total = sum(rise.values())
window = recs[hi][2] - recs[lo][2]
print(f'window {window} us; all charges {total / 10:.0f} us ({1000 * total / 10 / window:.0f}/1000 of it)')
for bid in sorted(weights):
    print(f'budget {bid} w={weights[bid]}: all charges {rise[bid] / 10:.0f} us, '
          f'{1000 * rise[bid] / total if total else 0:.0f}/1000 of all charges')

to_u, audit, rest, gaps = [], [], [], []
for a, b in zip(irq, irq[1:]):
    us = [(i, r) for i, r in enumerate(recs[a:b], a) if r[0] in 'UV' and r[1] == 4]
    if len(us) != 2 or us[0][1][0] != 'U':
        continue
    t0, tu, tv, t1 = recs[a][2], us[0][1][2], us[1][1][2], recs[b][2]
    to_u.append(tu - t0)
    audit.append(tv - tu)
    rest.append(t1 - tv - SLICE_US)
    gaps.append(t1 - t0)
med = statistics.median
print(f'{len(gaps)} gaps with one marks audit: gap median {med(gaps)} us = interrupt to audit '
      f'{med(to_u)} + audit {med(audit)} + audit end to next interrupt less {SLICE_US} '
      f'{med(rest)} (medians)')
