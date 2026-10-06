# SCHED1: five scheduling cases red under the 1 ms slice (architect-15, 2026-10-06)

Head `a787bd3d2` on `wp-SCHED1`, both widths, deterministic under icount: `sched-ties`,
`sched-large-weight` (410 of 1000, the fixture wants 555), `sched-server-busy`,
`sched-carve-inflation` (409), `sched-debt-lift` (31,695 µs against 19,000).

## (1) Whose commitment the slice is

The page states the slice as a number today: docs/kernel/scheduling.md line 4 ("for a slice of
at most 10 ms") and line 70 ("its slice ends: `SLICE_US` (10,000 µs) from the pick"). SCHED1's
layout changes `SLICE_US` and the model's `SLICE` to 1,000 µs and restates those lines
(`.wash/local/SCHED1-layout.md`, the first paragraph); the Architect's design checkpoint
accepted the 1 ms slice *for evaluation* and the owner's decision was "fix the scheduler before
IPC3 merges, targets unchanged" (QA `IPC3-wake-latency`). So the slice is SCHED1's mechanism,
on trial, not a page commitment the cases guard, and the page's number follows the package that
lands.

Nothing R-numbered names the slice's length. R12's text is slice-free: "a budget gets at least
its weight's share of the CPU the runnable budgets share. No pattern of spinning, sleeping and
waking, exiting or faulting, creating, carving and destroying budgets, or arming timeouts and
deadlines gets it more" (scheduling.md, R12's first paragraph); the four rank clauses and the
charging rule name no slice. The cluster construction (v3) ruled its positive-lead category on
the 1 ms arithmetic (`SCHED1-coverage-ruling.md`) and its oracle's clauses are slice-free. The
one place the page ties a number to the slice is the Responsiveness arithmetic ("one 10 ms slice
is 1,250,000" instructions, line 470, and the decision wake "late by whole slices", line 859):
SCHED1 restates those with its measurements, as its brief says.

## (2) Which cases are accidents and which are findings

A case is an *accident of the old slice* when what it printed depended on a thread keeping the
CPU for longer than any rule promises. A case is a *finding* when it checks a clause the page
states without a slice and the number moved.

- **`sched-ties`: an accident, in its guest-side half only.** Its claim is the oracle's: "checked
  pick by pick against the four rank clauses by the bench's independent oracle" (the case's
  description), and that half still passes. Its guest-side lines ("wakers of one entry ran lowest
  id first", "a later entry's wakers ran first") assumed the judge's three sends land in one
  kernel entry, which held only because a 10 ms slice outlasted the three sends; nothing on the
  page promises a thread the CPU across three calls ("A wake never preempts" is the only
  guarantee, line 377, and it is about the woken, not the sender). Rank clause 2 is about the
  *kernel entry* that woke each budget, so the fixture must make the three wakes one entry by
  construction, not by timing: three waiters with the same absolute timeout, expired by one timer
  entry (the page's own example of a multi-wake entry: "30 sleepers a microsecond apart", line
  742, and the timer's expiry walk), or one destruction waking several (`A` already is). Rule: the
  guest-side expectations stay only if the fixture is rebuilt so; otherwise they go and the
  oracle's verdict is the case's whole claim. Sleeping first, calibrating, or any pass lead gives
  no right to the CPU under any slice.
- **`sched-large-weight`, `sched-server-busy`, `sched-carve-inflation`: findings.** Each checks a
  share R12 states slice-free ("within 50 per thousand", line 740: a weight-1000 server against
  eight users of 100 gets 1000/1800; a busy server's work stays within its share; a carving
  subtree gets at most half). The 1 ms slice changes how often a budget is picked, not what it
  is owed over a 16 s window. A drop from 555 to 410 per thousand is therefore one of two
  things, and the trace says which: a real R12 regression the slice exposes (per-pick cost
  charged to the heavy budget: `MIN_CHARGE` applied at ten times the rate, kernel time of the
  pick or a reconcile billed to the picked budget, the remainder arithmetic at small charges), or
  a measurement that counts what the page excludes (the checked build's audits, which B5 made
  every share net of, line 437; if the fixture's count is gross, the case, not the kernel, is
  wrong, and it was wrong before too, hidden by ten times fewer picks). Either way the fixture's
  bound does not move: SCHED1's brief stops on a "fairness/budget/lease regression" (line 51)
  and this is the first candidate. Required: for each case, from the trace in the window, the
  pick count, the charge per pick to each budget, `MIN_CHARGE` events, audit time inside the
  window, and the share recomputed net of audits; then the mechanism named.
- **`sched-debt-lift`: a finding, and a suspicious one.** "A sibling created after a weight-1
  grandchild's lineage is destroyed runs within one round" (the description; the residual on line
  ~868: "at most one round, decaying once the floor passes the parent's pass"). A round is the
  runnable budgets' slices in turn, so a 1 ms slice makes a round shorter, and the bound (19 ms,
  set from 10 ms rounds) should be met with room; 31.7 ms is longer than before. That is not the
  slice doing what it says: either the lifted debt is now expressed in more rounds (the lift's
  normalization, `libs/stride::lift`, is in pass units: a 1 ms charge at weight 1 is a tenth of
  the old lead, so the sibling should wait less), or something else delays the sibling (a
  reconcile cadence, a timer, the destruction's audit). Attribute from the trace before anything
  changes; the bound stays.

## (3) What the ties case owes

The clause it names ("a later entry's wakers rank first", clause 2) is a statement about kernel
entries, so the evidence is: the trace shows the three wakes of `B` in one kernel entry (one
`W` group between two `K`s) and `A`'s in a later one, and the oracle's clause-2 check passes on
those records. The fixture earns that by construction: the three `B` waiters wait with the same
absolute timeout and are woken by one timer expiry, or by one destruction. A fixture that sends
three messages and hopes to stay on the CPU proves nothing about ranks under any slice; the old
pass was the accident. If the construction cannot be made (one entry cannot wake three in the
current kernel), the guest-side lines are deleted and the oracle carries the claim alone, with
the case's description restated.

## What changes nothing

No target, bound or tolerance moves. No fixture is "adapted" to the slice. The slice stays until
the findings are attributed: if (2) shows a real regression the slice causes and no fix within
SCHED1's scope, that is the owner's decision (the slice, or the targets), brought with the
numbers; if it shows measurement counting what the page excludes, the fixtures are corrected
and the pages say what they measure.

## Addendum after the diagnosis (same day)

The diagnosis (consoles lost to rotation; figures in `.wash/local/evidence/SCHED1/diag-1ms/README.md`):
1591 picks in rank order, picks 767 : 8 × 81 (9.5:1 for 10:1 weights), so no pass or accounting
regression; the median gap between slice-end interrupts is 1.45 ms per 1 ms slice (about 0.1 ms
audits, about 0.35 ms kernel entry + reconcile + pick + switch) in the checked, traced build:
about 30 % of the CPU per switch, 3.5 % at 10 ms. The fixtures read it as share loss (555 × 0.70).

1. **The per-switch cost is a finding of its own, against a gap in the pages.** R12 bounds a
   *call's* kernel time ("a constant plus a term linear in the pages it maps or the objects it
   names"); a slice end is not a call: timer entry, charge, reconcile, pick, switch, and the timer
   re-armed for "the earlier of its slice's end, the next timeout and the next budget deadline"
   (scheduling.md:70-76). The Responsiveness arithmetic (:470) treats that cost as negligible,
   which was true at 3.5 % and is false at 30 %. One page predicts the likely culprit:
   fpga-platform.md's Sstc line, "setting the timer is a CSR write, not a trap into the firmware
   on almost every dispatch": without Sstc every slice end is an SBI ecall into RustSBI, plus the
   trace's ring writes and K22's reconcile. Required: the slice end's kernel time measured in the
   release build (no trace, no audits), both widths, decomposed in the checked build (SBI timer
   call, trace, reconcile, switch), and stated on scheduling.md ("Charging" or a residual: "a
   slice end costs about N µs of kernel time"). A rule or an efficiency package (arm the timer only
   when the deadline moves; Sstc on hardware) follows only if the release number is material.
2. **Shares, net.** As ruled: ratio of counts (R12's relative claim; the oracle's share cases are
   already net of audits), with useful work per window against the 10 ms build reported as the
   switching cost. **debt-lift** is not explained by the switch cost: about 8 runnable budgets ×
   1.45 ms is about 11.6 ms per round, and the sibling first ran at 31.7 ms, about 2.7 rounds
   against "within one round"; attribute from the trace.
3. **The slice is decided on the release number, not the checked build's.** Shares and targets
   are judged net of audits (B5) and the trace itself costs. If the release cost is small, 1 ms
   stands and the checked build's 30 % is a bench residual to state; if material, the owner
   decides with the trade written: what 1 ms bought (the cluster: driver p99 13.9 ms gross at
   1 ms against 96 ms at 10 ms) against what it costs (useful work per window, release). A
   middle value re-runs the cluster and the sweep. No fixture adapts; no target moves.

## After the release measurement (same day)

Release build, no checks, trace or audits (`.wash/local/evidence/SCHED1/five-cases/RESULTS.md`):
about 0.357 ms per switch on rv64 (44.6 k instructions at shift 3), 0.449 ms on rv32 (56 k);
useful work at 1 ms against 10 ms 0.762 / 0.718; a re-pick of the same budget 0.13 / 0.19 ms per
slice. The checked walk-trace puts the bulk in the reconcile: about 3.2 per slice at a median
72 µs (about 9 k instructions each, "more than its loop bounds explain"), about 0.25 ms of a
0.46 ms per-slice kernel time; audits about 75 µs; traps, SBI, pick and switch about 0.1 ms.
`large-weight`'s useful-work share is 553/1000: R12's relative claim holds; the miss is throughput.

Ruling:
1. **The cost is inherent and material, and now explained to its component: the reconcile.**
   The layout's stop rule is met and discharged: the loss is not unexplained. The release
   reconcile (`sched.rs` `reconcile`: `marks.changed()`, `Cpu::reconcile` over the lost and
   gained budgets with `mm.ready`, `marks.clear()`; `Marks::settle` over the marked slots only) has
   no loop over every process in release (the full walk is `debug_assertions` only), so 9 k
   instructions for a handful of budgets is a defect in what those steps do per budget, not in
   the slice: a finding against K22's claim ("Reconcile follows the budgets that changed: marks,
   not a walk of every process") and R12's kernel-time text. The successor attributes it with the
   walk-trace: instructions per `Cpu::reconcile` call by step (`mm.ready` per budget, the queue's
   requeue and floor, `set_ready`), and why 3.2 reconciles per slice (every kernel entry ends in
   one; which entries). That attribution is a package of its own, cut from the numbers (a cheaper
   reconcile, or fewer per slice), Tier A kernel; SCHED1 does not fix it.
2. **The slice decision is the owner's, with the trade written.** At 1 ms the release build
   loses about 24 % (rv64) and 28 % (rv32) of useful work under this load, against the cluster's
   driver p99 13.9 ms gross at 1 ms where 10 ms gave 96 ms; a fixed reconcile would move the
   first number, not the second. The decision_request offers: keep 1 ms and cut the reconcile
   package before the merge (the owner's "fix the scheduler" with the throughput restored by
   the fix, not by a tolerance); keep 1 ms and merge now with the loss stated as a residual and
   the reconcile package next; or a middle slice (2 to 4 ms) chosen by re-running the cluster and
   the sweep, with both numbers re-measured. Recommended: the first.
3. **The fixtures:** as ruled, shares by ratio of counts (useful-work share), the throughput
   reported beside them; `debt-lift`, `server-busy`, `carve-inflation` attributed before any line
   changes.

## The reconcile's cost, scope and sequence (same day, the orchestrator's three questions)

1. **Fix the reconcile first, as its own package, then re-measure, then ask the owner.** The
   owner's decision must rest on the cost the slice has, not on a defect's; today's 0.36 /
   0.45 ms per switch is about two-thirds reconcile, so the numbers would be wrong. The fix is
   not SCHED1's: its layout changes the slice and nothing else ("No tolerance, trace RAM, ABI,
   stride arithmetic or IPC change is proposed"), and a reconcile commit must be reviewable
   alone. Cut **K23** (Tier A, the kernel, size S): the reconcile's cost bounded by its stated
   loop bounds; the first candidate is `ready_count` walking thread slots per marked process
   (keep a count, or popcount the TID mask words), and the trace says the rest; with a host test
   on the count and the walk-trace's instructions per reconcile before and after. **Not** "one
   reconcile per slice end": a reconcile at every kernel entry is K22's design, and a wake must be
   queued before the pick that follows its entry; the count per slice (about 3.2: the slice end,
   and the calls and replies the budgets make) is the load's, not a defect. K23 branches from
   `main` (K22 is there), merges in a train before SCHED1; SCHED1 rebases, re-measures the
   per-switch cost and useful work at 1 ms against 10 ms in release, both widths, and then the
   owner is asked with those numbers. Until then the slice is on trial as before.
2. **The 276 µs billed to the descheduled budget is what Charging says.** scheduling.md:203 ("when
   it ends that budget's slice, and nobody's otherwise") and :228 ("with neither, only the budget
   it interrupted, when it ends that budget's slice"): the slice-end interrupt, and the kernel
   work it does before the next pick, is the ending budget's. No rule is violated; the amount is
   the defect's, and shrinks with K23. It is also why the heavy server's useful-work share stayed
   at 553: it pays its own slice ends in proportion to being picked, as every budget does.
3. **The sentence for scheduling.md now,** in "Charging" after the slice-end billing sentence
   (or a residual, SCHED1's choice), with numbers as a measurement, no package names: "A slice
   end costs about 0.36 ms of kernel time on rv64 and 0.45 ms on rv32 in the release build under
   QEMU (about 45,000 and 56,000 instructions), most of it the reconcile that follows the entry;
   at a 1 ms slice under nine runnable budgets that is about a quarter of the CPU, billed to the
   budgets whose slices end, so relative shares hold while useful work falls to about three
   quarters of the 10 ms build's (release build: 0.762 and 0.718). This is today's cost; a later
   measurement replaces it." (The editor's wording, adopted: a page states no promise of a fix.)
   The number is replaced, not amended, after RECON1.

## debt-lift (red round 5, P1): the bound no boot exercises

The finding (`.wash/local/evidence/SCHED1/r5-debt-lift-finding.md`): no construction tried
exercises "a lift delays a sibling by up to a round" deterministically; under `users` (weight
about 747,300) the lift is about 1/7,000 of a slice, and on a weight-100 parent the floor had
passed it before the sibling existed. The oracle's `check_lift` recomputes every lift's
arithmetic exactly, phase-independent.

1. **(c):** (a) now, (b) as its own node. The case judges the sibling's first wake by
   no-other-budget-picked-twice (which catches the grandchild's raw debt, about six rounds, and
   the red's P2); the one-round bound is carried by the oracle's arithmetic; the case's
   description and the page say the bound itself is not exercised by a boot.
2. **The page softens to what is proven.** "At most one round" is a derived bound whose
   derivation the page does not give and no boot exercises; the oracle proves the lift's
   amount, not the delay. Replace the residual's text (scheduling.md:884-889) with: "**A
   destroyed lineage's debt is carried onto its siblings as the parent's lead.** Debt lifted onto
   a shared parent is normalized to the parent's weight, and a sibling created under it enters at
   the parent's pass; the oracle recomputes every lift. How long that lead delays the sibling is
   the parent's lead over the floor against the sibling's weight: about a round when the parent is
   small and the lift fresh, nothing measurable under a parent as wide as `users`, where the lift
   is a few thousandths of a slice. No boot case yet puts a fresh lift on a small parent, so the
   delay's bound rests on the lift's arithmetic, not on a measurement; `bench:sched-debt-lift`
   checks that a sibling created after a weight-1 lineage's destruction runs before any other
   budget is picked twice, which its raw debt, unlifted, would have cost about six rounds.
   Rounding loses under one pass unit per destroyed budget, and the loss falls on the budget that
   churns." The R12 status line keeps `bench:sched-debt-lift`.
3. **(b) is a plan node,** not a residual only: a stated bound the pages carried must be either
   exercised or dropped. Node LIFT1 (Tier A, the bench and the fixtures, size S; needs SCHED1):
   a construction that puts a fresh lift of about a round on a small shared parent and observes
   the sibling's first run against it, both widths; if it finds "at most one round" false for
   small parents, the page's sentence changes to what it finds, which is the finding's value.
   The page's sentence above carries no package name; its "No boot case yet" is the pointer.
