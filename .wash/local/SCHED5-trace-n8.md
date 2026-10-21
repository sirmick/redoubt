# SCHED5: the n = 8 trace (rv64, two harts, one boot)

Head ab926d21d (HAMMER_PAGES 8), `make -f scripts/jobs.mk rv64/sched-lock-contention`, rc=1
(positive misses, as before). Console: `.tmp/SCHED5/trace-n8/run-1151751-1792538265314467103/`.
Search alone p50 35,647 µs; the hammer's search sections 35,285 µs (130 over 10 ms).

## The samples are bimodal, and split by the hammer's window

Driver wakes, gross, in order (S = over 10 ms, f = under):

    105 x S (35.1 to 37.8 ms), then 95 x f (1.8 to 2.7 ms)

The hammer made 105 searches. Every sample taken while it hammers waits one whole search; the
95 after its last search are unloaded. The p50 lands in the upper mode only because 105 > 100.
The samples are spaced 38.08 ms apart in the loaded part: one wake per search, phase-locked.

## One loaded period (sample 2, due 12,659,806 µs, wake 12,695,486)

| µs | hart 0 (hammer, budget 37) | hart 1 (driver, budget 34) |
| --- | --- | --- |
| 12,657,308-384 | `x` (the RTC line, already claimed: finds nothing), 76 µs section, back to user | waits 94 µs for that section (its `time_now`) |
| 12,657,393 | re-enters 9 µs after its return: draws its ticket for the next search | |
| 12,657,396-474 | | `time_now`, 78 µs |
| 12,657,486 | | the driver arms the RTC (MMIO) and calls `receive`: draws its ticket **behind** the hammer's |
| 12,657,489-12,692,774 | the search, 35,285 µs, interrupts off | waits for the lock, 35,301 µs |
| ~12,659,806 | | the alarm is due: no hart can take it (one holds the lock, the other waits for it) |
| 12,692,785-12,694,193 | | `receive`, 1,408 µs (970 of it audits): the driver blocks (`D`) |
| 12,694,4xx | | hart 1, now idle, claims the alarm, wakes the driver (`W`) and picks it on itself (`K 34`) |

## The Architect's questions

- **Which hart takes the RTC interrupt:** hart 1, the idle one (claims by hart [92, 109]; hart 0's
  105 `x` entries find nothing). IRQ1 works.
- **Where the driver is placed:** on the hart that took the interrupt (hart 1). No pick on the busy
  hart while another idles.
- **What it waits for:** the kernel lock, in ticket order, behind exactly one section (R78's
  bound at two harts). It is not waiting to be woken: its own `receive`, the call that waits for the
  alarm, is queued behind the search, so the alarm is due before the driver even waits for it.
- **Why a whole section, not half:** the phase is not uniform. The driver arms its next alarm just
  after its wake, which is served at a search's end; between its two calls (`time_now`, then
  `receive`) the hammer, back in user mode for 9 µs, has already drawn the next ticket. So every
  loaded period puts the driver's `receive` behind a whole search: one wake per search.
- **The wait's parts:** 94 µs (`time_now` behind the hammer's return), 35,301 µs (`receive` behind
  the search), 1,408 µs (`receive` itself, 970 µs of it audits), then the wake.

## Reading

(b): the full-section wait is inherent to a fair ticket lock and the driver's shape (two calls per
wake, the second drawn behind the hammer's next ticket), not a scheduler fault. Two things the trace
shows beyond the question:
1. The hammer's window covers 105 of the 200 driver samples at n = 8, so the gate's p50 judges a
   mixture of loaded and unloaded wakes.
2. One trace cannot say why n = 16's positive met its p50 (1.2 ms): the lock-step forms whenever a
   wake is served at a search's end, so at n = 16 it either never formed or covered fewer than half
   the samples. Not traced (the effort was bounded to this boot).
