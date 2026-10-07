# K29: R10's p99 grew ~6 ms with K19 (kernel-containment)

Branch wp-K29 from wp-K28's head 5e7c5170c, worktree /home/mcloonan/redoubt/.worktrees/K29.

## Numbers (kernel-containment, 20 destructions, ~9,338 object frames, qemu_seed 13, icount)

| kernel | rv64 p99 | rv32 p99 | threads' ending |
| --- | --- | --- | --- |
| main bdb38430e, pre-K19 (train-8) | 23,394 | 24,104 | 3.5 ms |
| K19 + K28's audit fix (5e7c5170c) | 29,500 | 30,576 | 2.5 ms |

rv32 fails the 30,000 µs bound; alone on `--quiet` the same 30,576 (deterministic).

## Phase split, rv64, K19 + K28 (measurement build: `println!("K29 <phase> <µs>")` at
## destroy_subtree's phase boundaries plus walk-trace; uncommitted; the prints cost ~0.5 ms)

A 30.0 ms destruction (the 29.5 ms one of the unstamped run):

| phase | ms |
| --- | --- |
| kills (step 2: `killed` for each process on the counted chains; T/t spans 2.5 ms of it) | 6.29 |
| process::budgets_dying (step 3: the charged chains' frees) | 0.13 |
| message::budgets_dying (step 4: endpoints and devices destroyed, messages reached) | 16.06 |
| lift_dying | 0.44 |
| destroy_marked (close_dependents, free endpoints/deferred frames, carve, PIDs, budgets) | 5.45 |
| pump_listed (two pumps: 0.66 + 0.74) | 1.55 |
| bill + Y | 0.08 |

Parser: /tmp/k29-phases.py (pairs the stamps with each X..Y by µs; the trace ring is dumped at
the end of the run, so the console order does not interleave).

Suspects checked: `unchain`'s `assert!(contains && contains)`: `List::contains` reads the
member's own links, O(1); not it.

## The same split on the pre-K19 kernel (worktree .worktrees/K29-base at bdb38430e, same stamps)

rv64, the longest window of each run:

| phase | pre-K19 | K19 + K28 | Δ |
| --- | --- | --- | --- |
| kills (step 2) | 8.04 (three pumps inside: 0.67 + 0.92 + 0.02) | 6.29 | −1.75 |
| process::budgets_dying (step 3) | 0.29 | 0.13 | −0.16 |
| message::budgets_dying (step 4) | 9.45 | 16.06 | **+6.61** |
| lift_dying | 0.44 | 0.44 | 0 |
| destroy_marked | 5.51 | 5.45 | −0.06 |
| end pumps | — | 1.55 | +1.55 (moved out of the kills) |
| whole | 23.90 | 30.00 | +6.10 |

The growth is step 4 alone: `message::budgets_dying` (the owner walk: endpoints' message reach
and devices' destruction, then the queued and taken chains). The pumps moved (1.6 ms in the
kills before, 1.55 at the end now), they did not grow.
