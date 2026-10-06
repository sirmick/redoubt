# LIFT1: a fresh debt lift on a small shared parent, observed at the sibling's first run

Tier A (the bench's oracle, a fixture program and its case), size S. Needs SCHED1 (merged):
start from `main`. The page carries a derived bound that no boot exercises; this package
exercises it on both widths and the page then says what was measured, whichever way it goes.
Run everything natively on this host under the job pool's rules; a timing case runs alone
(docs/testbench.md, "On a shared host").

## Context rules (read these first)

- Read `docs/kernel/scheduling.md` "Inheritance" and "The lead follows the weight" (the lift's
  conversion, `W = (pass - floor)+ x w_old + rem; pass = floor + W / w_new`), and the residual
  bullet "A destroyed lineage's debt is carried onto its siblings as the parent's lead"; the
  ruling `.wash/local/SCHED1-five-cases-ruling.md` "debt-lift (red round 5, P1)" and the
  red's finding it cites; `tests/sched-debt-lift.toml` and its 52-line program;
  `tools/testbench/src/sched_oracle.rs` by function: the record kinds, `check_lift`,
  `check_round`, and one of the oracle's synthetic-trace host tests as the pattern (the file is
  3,000 lines: never whole).
- Reports under 1,900 bytes, detail in `.wash/local/LIFT1-report.md`.

## What the page claims, made exact

A destroyed child's work since it entered the queue, `W = (pass_c - floor)+ x w_c + rem`, is
its lead over the floor at the destruction, in runtime; for a child that was runnable
throughout, that is about one slice (a budget's pass stays within a stride of the floor), two
at most. It lands on the parent P at P's restored weight: P's lead over the floor becomes
`W / w_P`. A sibling S created under P enters at P's pass. The floor advances by `STRIDE /
w_peer` per round of runnable peers of weight `w_peer`, so S's first pick waits about

    (W / STRIDE) x (w_peer / w_P) rounds,

independent of S's own weight. With `w_P = w_peer` and `W` one slice that is one round, the
page's "about a round when the parent is small and the lift fresh"; with `w_P` a tenth of a
peer's it is ten rounds. The page's phrase "against the sibling's weight" is suspect under this
arithmetic. The construction decides; this derivation is the hypothesis, written here so that
the measurement can refute it.

## The construction: `tests/sched-lift-delay.toml` and `tests/programs/src/bin/sched-lift-delay.rs`

Under `users`, as `sched-debt-lift` does: sixteen spinners of weight 100 (`w_peer`), so a round
is sixteen slices and the floor moves at a known rate. Then three phases in one boot, each:
1. carve a parent P of weight `w_P` from `users`, holding no process; carve a child C from P
   (weight below `w_P`, say `w_P - 1`, with a spinner), let it run until it has completed at
   least one full slice (spin on `time_now` for two slices' worth, then stop it: its lead is then
   whatever the trace records, which is the point);
2. destroy C and, in the very next call, create the sibling S under P (weight at most `w_P -
   1`, a process that reads `time_now` first thing and prints `[lift-delay] phase N: S ran at
   T`); the destroy and the create back to back is what makes the lift fresh;
3. destroy S and P before the next phase.
Phases: `w_P` = 100 (one round expected), 50 (two), 10 (ten). The sibling's weight varies
across phases too (say 99, 25, 9), so the claim about it is tested, not assumed. The program
prints `[lift-delay] ok` after the three and `SCHED-LIFT-DELAY TEST PASSED`; the numbers are
the trace's, not the program's.

**Freshness and vacuity.** A phase whose lift the floor has already passed at S's wake proves
nothing (the red's finding on the weight-100 parent). The oracle fails a phase, not passes it,
when P's lead at S's wake is not above the floor; the program's back-to-back calls are how it
stays fresh, and if a width cannot keep it fresh (a slice end between the two calls), the
report says so and the case narrows to what it can do, stated in its description.

## The oracle: `check_lift_delay` in `sched_oracle.rs`, `post_check = "sched_oracle lift-delay"`

For each phase, found as `check_round` finds its marker (an empty marker budget destroyed just
before S is made works here too, or P's own records; say which): from the lift group of C
(`check_lift`'s records give `W` and `w_P` exactly), the floor and `w_peer`, compute the
expected rounds from the formula above; from the trace after S's wake, count the picks of other
budgets until S's first pick and the most any one budget was picked (the observed rounds, as
`check_round` counts them). Report `lift-delay: phase N: W=.. w_P=.. expected R.R rounds,
observed K picks, max M of one budget`, and pass when the observed rounds are within one of the
expected (the phase of the round is at most one slice either way; say in the report if a tighter
tolerance holds on both widths). `check_lift` keeps recomputing every lift beside it. Host tests
on synthetic traces: a phase at the expectation passes, one round over fails, a stale lift (P at
or below the floor at S's wake) fails, three phases are reported separately.

## The page, after the measurement

- `scheduling.md`'s residual bullet: "about a round when the parent is small and the lift
  fresh ... No boot case yet puts a fresh lift on a small parent, so the delay's bound rests on
  the lift's arithmetic, not on a measurement" becomes what was measured: the formula in words
  (the child's lead in slices times the peer's weight over the parent's), the three measured
  points on both widths, and whether the sibling's weight mattered; `bench:sched-lift-delay`
  named beside `bench:sched-debt-lift`. If the formula is refuted, the sentence says what the
  trace shows and the report says why the derivation was wrong.
- R12's status line gains the case; `testbench.md`'s oracle paragraph names `lift-delay` where
  it names `round`. No dates, package IDs or review history.

## Owned paths

`tests/sched-lift-delay.toml`, `tests/programs/src/bin/sched-lift-delay.rs` (and the programs
crate's registration of it), `tools/testbench/src/sched_oracle.rs` (the new check and its
tests; `check_round` untouched), `docs/kernel/scheduling.md` (the bullet and the status line),
`docs/testbench.md` (the oracle paragraph). **Not yours:** the kernel, `libs/stride`, the
model, the slice length, `sched-debt-lift` (unchanged), the trace format (if a record you need
is missing, stop and say which: a kernel change is a design question).

## The short gate

Both builds (rv64, rv32; the case needs the checked build with `sched-trace`); host tests of
`testbench` (the oracle's suite) and the programs crate; the docs checker, `cargo fmt
--check`, the size budget, the `unsafe` ratchet, the no-cruft gate; own cases on both widths:
`sched-lift-delay` and `sched-debt-lift`, each run alone; the smoke set (`userland-boot`,
`init-boot`, `bench-net-peer`, `ipc-outcomes`). The whole bench is the train's.

## Not here

The kernel and the slice (SCHED1 is decided, RECON1 owns the reconcile's cost); a change to
the lift's arithmetic (if the measurement says the arithmetic is wrong, that is a finding for a
new node, and the page says what is measured meanwhile); `sched-debt-lift`'s judgement.

## Checkpoint

After the first green phase on one width: one progress line with the three expected and
observed figures as the trace gives them, before the page is written.
