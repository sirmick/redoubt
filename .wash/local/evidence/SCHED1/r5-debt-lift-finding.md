# SCHED1 red round 5: debt-lift cannot exercise "delayed by up to a round" (impl-6, 2026-10-06)

## What the red asked

Make the round exercised, either:
- by requiring that the marked wake ranks behind the 16 spinners (`ahead >= 16`); or
- by construction: the spinners are at the floor when the window's `go` lands.

A lift late by one round must then fail.

## What the traces show

Consoles are in `gate-fixtures/r5/`.

1. **The marker moved before S is made** (its audit now comes before S starts).
   - rv64: S starts (W 86, entry 2052), runs and blocks (D, entry 2055).
   - The launcher is not picked next. It waits out a round behind the 15 spinners (K at entries
     2057..2099).
   - It then sends `go` at entry 2105.
   - By then every spinner has just been picked once, so S's window wake is picked after 0-1 others.
   - In the earlier run without the marker, the launcher happened to be picked straight after S
     blocked (entry 2046), so the `go` landed with the spinners at the floor: 16 ahead.
   - Which of these happens is the launcher's rank against the spinners at S's start. That is phase,
     not construction.
2. **The lift in this fixture is negligible.**
   - U is a child of `users`, whose free weight is about 747,300.
   - G (weight 1) ran one slice. Its work is about 10,000 ticks x 2^20.
   - Lifted into U at 100, that is about one spinner slice of pass (1.05e8).
   - Lifted from U into `users` at about 747,300, it is about 1.4e4 pass units: about 1/7,000 of
     a slice.
   - So S's entry, at `max(floor, users' pass)`, is at the floor.
   - The round S waited in the diagnostic run came from S's own start charge (its pass a little
     above the spinners' at its window wake), not from the lift.
   - So no phase of this fixture exercises "a lift delays a sibling by up to a round".
3. **Experiment (scratch, reverted):** U under a weight-100 intermediate P shared with S, so the
   lift is about one slice. Judging S's first wake as the new budget:
   - rv32: S was picked after 0 others.
   - The launcher's marker and S's creation came after the floor had passed P's lifted pass. That
     is the residual's own "decaying once the floor passes the parent's pass", and again phase.

## Already exact, independent of phase

- The oracle recomputes every lift from the trace by the rule (`check_lift`).
- A lift wrong by any amount, one round late included, fails the case there whatever the phase.
- The round check adds only the consequence for the sibling.

## Options

- **(a)** Judge S's first wake (the new budget's first record after the marker; red P2 covered),
  and require only that no other budget is picked twice before it.
  - The fixture's claim is restated as "not behind G's raw debt" (about 6 rounds, caught).
  - The one-round bound is carried by `check_lift`'s exact recompute.
  - The case and scheduling.md disclose that the bound itself is not exercised.
  - Small, and deterministic.
- **(b)** Redesign so that the lift is about one round and is still live when S is made.
  - U goes under a weight-100 parent P shared with S. P must not be decayed by the floor before S
    is made, which means no launcher deschedule between U's destruction and S's creation, and that
    is again phase.
  - It needs a construction study and several boots, and it may show the residual's "at most one
    round" is not true for small parents. That would be a finding.
- **(c)** Option (a) now, with (b) as a follow-up package.

## Recommendation

(c).

## Other round-5 items, done and uncommitted

- **check_round P2:** the first `W` after the marker must be a new budget's (no `W`, `R`, `D` or `K`
  before it; a waker's `P` comes just before its `W`). Host test added.
- **large-weight:** each user is within a tenth of the users' mean.
  - rv64: least 992 per 1000.
  - rv32: least 988 per 1000.
  - It passes on both widths.
- **server-busy:** scheduling.md's residual is restated to what the ratio checks, with the
  flooding user's own share disclosed as unchecked (its trace: 229 against 257 each). The ratio of
  counts cannot see the flooding user's CPU, so there is no check for it.
