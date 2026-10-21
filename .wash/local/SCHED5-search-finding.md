# SCHED5: `map_anon`'s search is worse than the pages say (pre-existing)

Measured 2026-10-20 on the base kernel 3e6f29443: checked build, `icount` shift=3, scratch case
`.tmp/SCHED5/wt/tests/sched5-search.toml`. The case pins a page at the end of every n-page window
of the 256 MiB area, then times a refused `map_anon` of n pages (worst of 3).

| n | pages pinned | rv32 | rv64 |
| --- | --- | --- | --- |
| 16 | 4,096 | 16.4 ms | 22.1 ms |
| 8 | 8,192 | 23.7 ms | 35.2 ms |
| 4 | 16,384 | 38.4 ms | 61.3 ms |
| 2 | 32,768 | 67.8 ms | 113.6 ms |

## The cause

Each candidate start costs one walk (`first_occupied_page`). So a refusal's cost follows the
number of starts, 65,536 / n, and not only the pages read. "9.6 ms worst" (memory.md R22,
scheduling.md, SECURITY.md) was the 2 MiB-span shape, never the worst.

A process forces 65,536 / n starts by pinning 65,536 / n pages of its own budget:

- a 5,248-page lease: about 25-30 ms;
- a 128 MiB budget: about 114 ms on rv64.

Under the cap the 2 MiB shape cannot be built: a 512-page request is `TooLarge`. Any refused
search now needs a page in every 16-page window, or denser.

## What it touches in SCHED5

1. **The pages.** The exception's figure on memory.md R22, scheduling.md R12/R78 and
   SECURITY.md. K34 is its bound.
2. **`map-anon-search-bound`'s bounds.** The refusal bound is 12 ms and the wake bound 15 ms,
   both derived from the 9.6 ms shape. Under the cap the case's smallest refused shape (n = 16)
   reads 16,824 µs on rv32 and 22,425 µs on rv64, so it fails at the refusal assert.
   (`.tmp/SCHED5/set-memory/`, then a rerun with the time printed.)
3. **The contention cases.**
   - The hammer is n = 16 per the ruling, so a search is now about 22 ms.
   - The trace: `map_anon` sections up to 22.2 ms, 150 a window.
   - The two-hart case passes: driver net p99 1.3 ms on rv64, 1.6 ms on rv32.
   - The four-hart case: its three hammers pinned inside the window and made no refusals.
     Fixing: pin before the window.
   - `search_alone` runs in the first program's budget, which holds about 970 pins. Its
     "refusals" are the budget's, not the search's. I propose moving it into a child sized like
     a hammer.
   - `irq-boot-hart-only` passes where it must fail. To look at once the hammer pins before
     the window.
