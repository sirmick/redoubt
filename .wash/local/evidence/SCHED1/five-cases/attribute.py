"""SCHED1 five-cases attribution from a sched-trace console (host-only, read-only).

usage: attribute.py <console> <weight-by-budget-id as id=w,id=w,...>

Per budget, inside the window in which every named budget is runnable (from the first to the
last timer interrupt that interrupted one of them):
- picks (`K`);
- the charge of each run, in ticks: the pass change from its `K` to its next `R`/`D`, times
  its weight, over STRIDE (2^20); remainders are ignored, so this is within one tick;
- one-tick charges (MIN_CHARGE);
- charged ticks and their share of all named budgets' charged ticks.
Also: the audit time (`U`..`V`) inside the window, and the kernel time per slice (the gaps
between consecutive timer interrupts, less 1 ms).
"""
import collections
import statistics
import sys

STRIDE = 1 << 20
path, weights_arg = sys.argv[1], sys.argv[2]
weights = {int(k): int(v) for k, v in (kv.split('=') for kv in weights_arg.split(','))}
recs = []
for line in open(path, errors='replace'):
    if line.startswith('SCHED-TRACE '):
        f = line.split()
        recs.append((int(f[1]), int(f[2]), f[3], int(f[4]), int(f[5], 16)))

interrupts = [(i, r[4]) for i, r in enumerate(recs) if r[2] == 'I' and r[3] in weights]
first_i, t0 = interrupts[0]
last_i, t1 = interrupts[-1]
picks = collections.Counter()
charges = collections.defaultdict(list)
open_pick = {}
for i in range(first_i, last_i + 1):
    _, _, kind, bid, val = recs[i]
    if bid not in weights:
        continue
    if kind == 'K':
        picks[bid] += 1
        open_pick[bid] = val
    elif kind in 'RD' and bid in open_pick:
        charges[bid].append((val - open_pick.pop(bid)) * weights[bid] // STRIDE)
audit = 0
open_u = None
for _, _, kind, bid, val in recs[first_i:last_i + 1]:
    if kind == 'U':
        open_u = val
    elif kind == 'V' and open_u is not None:
        audit += val - open_u
        open_u = None
window = t1 - t0
total = sum(sum(c) for c in charges.values())
print(f'window {t0}..{t1} us = {window} us; audits inside {audit} us ({100 * audit / window:.1f}%)')
gaps = [b[1] - a[1] for a, b in zip(interrupts, interrupts[1:])]
print(f'timer interrupts {len(interrupts)}; median gap {statistics.median(gaps)} us')
for bid in sorted(weights):
    c = charges[bid]
    ticks = sum(c)
    print(
        f'budget {bid} w={weights[bid]}: picks {picks[bid]}, charged runs {len(c)}, '
        f'median charge {statistics.median(c) if c else 0} ticks, one-tick charges '
        f'{sum(1 for x in c if x <= 1)}, charged {ticks} ticks = {ticks / 10:.0f} us, '
        f'share of charged {1000 * ticks / total if total else 0:.0f}/1000, '
        f'of the window {1000 * ticks / 10 / window:.0f}/1000, '
        f'of the window net of audits {1000 * ticks / 10 / (window - audit):.0f}/1000'
    )
