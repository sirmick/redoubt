# RECON1: the reconcile's cost bounded by its stated loop bounds

Tier A (the kernel's scheduler). Size S. Needs SCHED1 (merged: the owner's decision, 2026-10-06:
keep the 1 ms slice, merge SCHED1 with the loss a stated residual, this package next), SMP1
(merged: `libs/stride` now has per-hart runners over the one queue) and K19 (merging: the
budget frame's words 107 and 108 are its chain heads). Start from `main` once K19 is on it. The
attribution is in this brief's last-but-one section. Re-cut against `main` fa08fe2c8 and
wp-K19 0795e6b54 (architect-16): the code map below is that tip's.

**Why:** SCHED1's release measurement (`.wash/local/evidence/SCHED1/five-cases/RESULTS.md`): a
slice end costs about 0.357 ms of kernel time on rv64 (about 44,600 instructions at icount shift
3) and 0.449 ms on rv32 (about 56,000), under `sched-large-weight`'s nine runnable budgets.
About two-thirds of it is the reconcile: about 3.2 reconciles per slice at a median 72 µs, about
9,000 instructions each, for a handful of marked budgets. The page claims otherwise.

Run everything natively on this host under the job pool's rules (docs/testbench.md "On a shared
host"): the scheduling cases are guest-time cases and share the pool; the kernel's host tests
run alone only if they assert a time (they do not).

## Context rules (read these first)

- **Don't read whole files.** The code map after SMP1 (`libs/stride/src/lib.rs`, 728 lines):
  `Queue<B, N>` (:182; `slots: [Option<B>; N]`, `len`, `floor`, `front`, `back`),
  `raise_floor` (:245: `queued().map(|b| bs.state(b).pass).min()`, every queued budget's frame
  read), its callers `fold` (:252), `deschedule` (:262), `reconcile` (:291: twice, after the
  lost pass and after the gained pass), `destroy` (:396), and `pick` (:351: the same walk,
  `bs.state` per budget for `Rank`); `reweigh` (:362) and `destroy`'s lift change a queued
  budget's pass without a walk. Above the queue: `Runner` (:408), `Wiring` (:430: `switch`,
  `reconcile`, `pick` over the harts' runners, `settle`), `Cpu` (:547, one hart) and `Harts`
  (:643), all going through the one `Queue`. The kernel: `kernel/src/sched.rs` `SCHED:
  Harts` (:88), `impl Budgets for MemoryManager` (:99: `state` is `sched_state(frame)`, a
  checked frame read; `set_state` :102, with the trace's pass record for a queued budget),
  `Sched::reconcile` (:187, `marks.changed()` then `cpu.reconcile`), `leave` (:356), `pick`
  (:423). `libs/stride/src/marks.rs` whole (small). Nothing else.
- **Don't open `.wash/qa/*.md` or other packages' reports** but SCHED1's `RESULTS.md` and the
  attribution section below.
- **Measure before and after with the same method,** or the number means nothing.
- **Reports under 1900 bytes,** detail in `.wash/local/RECON1-report.md`.

## Reading list (only these)

- `docs/kernel/scheduling.md`: "The current minimum and ties" (:94; the reconcile paragraph at
  :127, "The reconcile visits only the budgets whose runnable state changed"), R12's
  kernel-time paragraph and status line (:796), "Charging" (:159; the slice-end cost sentences
  at :222-223: "... (0.762 on rv64 and 0.718 on rv32, both in the release build). The
  reconcile's cost is above its loop bounds. This is today's cost; a later measurement
  replaces it."). The residual "a delivery and a timer expiry walk every thread" is gone
  (IPC3): nothing to keep there.
- `.wash/local/evidence/SCHED1/five-cases/RESULTS.md`; this brief's attribution section.
- The code named in the context rules.

## The claim it corrects

K22's claim, on the page: "The reconcile visits only the budgets whose runnable state changed"
(:127); and R12's text that a call's kernel
time is "a constant plus a term linear in the pages it maps or the objects it names". The
attribution (`.wash/local/evidence/SCHED1/five-cases/reconcile-finding.md`) shows the reconcile
does visit only the marked budgets, and that the cost is elsewhere: **the queue's floor**.
`Queue::raise_floor` (`libs/stride/src/lib.rs:245`) reads every queued budget's seven-word
`State` through checked frame reads (`sched.rs:100` `state()` → `sched_state(frame)` → `kframe::read`),
about 2.6 µs per budget per walk; `Queue::reconcile` calls it twice unconditionally (:322, :349),
`fold` (:257) and `deschedule` (:282) once each, `destroy` (:404) once, and `Queue::pick` (:352)
walks the same way, so a slice
end makes four to nine walks: about 23 µs per queued budget checked, 28 µs release, over three
spans (the leave at the timer exit, kmain's pick, the leave after the switch). Kernel time per
slice end is therefore linear in the queued budgets L (3 to 17 in the traces), with a fixed
100 to 155 µs at L = 0 (traps and two SBI ecalls) that is not this package's. `settle` and
`ready_count` are not the cost (marked slots only; a four-word popcount), and the "276 µs billed
per run" was an analysis artefact: whole charges are 555 / 56 × 8.

## The fix

The finding's two sketches, both:
1. **`raise_floor` only when the floor can rise:** after a removal, a wake at the floor, or a
   charge of the queued minimum; not twice unconditionally per reconcile, and never when the
   queue did not change. The floor's rule ("it never falls"; the minimum queued pass) is
   unchanged, and the stride differential (`the_crate_and_the_model_agree`) proves it.
2. **`(pass, tie)` cached in the queue's own slot array** beside the `B` (`slots` becomes
   slots of budget and rank, or a parallel `ranks: [Rank; N]`), in `Queue`'s own memory, so
   `raise_floor` and `pick` read kernel memory, not seven checked frame words per budget. The
   cache is written at every place `Queue` or `Wiring` changes a queued budget's pass or tie:
   `fold`'s charge, `deschedule`'s requeue, `reconcile`'s wake, `reweigh`'s rescale and
   `destroy`'s lift of a queued parent (both change a queued pass with no walk today; the
   kernel's `set_state` already singles out "a pass that changes while the budget is queued"
   for the trace, the same condition). The frame stays the authority: the checked build audits
   the cache against `sched_state(frame)` for every queued budget at every reconcile. No new
   frame word: the budget frame's words 104-108 are `READY`, `QUEUED`, `TAKEN` and K19's
   `CHARGED`, `COUNTED` heads, and this package adds none.
   One queue serves every hart (SMP1: `Harts` over one `Queue` under the scheduler's lock), so
   the cache lives where the queue does and needs nothing per hart; `pick`'s `elsewhere`
   filter is unchanged.
Nothing else: the marks' rule, when a reconcile runs, the pick's rank, the charging arithmetic,
the queue's capacity and the per-hart runners are untouched. The bound this package states on
the page: a slice end's scheduler work is linear in the queued budgets at **well under a
microsecond per budget** (a cached rank compare), plus the floor's walk only when the floor can
have moved.

## The measurement that gates it

The slice-end gap method of SCHED1's `RESULTS.md` and the finding's per-span method: in the
release build (no checks, trace or audits), under `sched-large-weight`'s load, the median gap
between consecutive slice-end timer interrupts less the slice, both widths, before and after;
and the kernel time per slice end as a function of L, fitted over the five traces' L = 3..17
(the finding's method). The gate, restated from the finding: the per-budget term falls from
about 28 µs to under 3 µs per queued budget per slice end in release (one cached compare per
budget per walk, two to three walks), and the fixed term at L = 0 is unchanged (it is not this
package's); so under nine budgets a slice end costs about the floor (100 to 155 µs) plus under
30 µs, against about 0.36 ms today. State the numbers. If the measurement finds a cost that
is neither the floor's walks nor the fixed term, stop and report.

## The fixture that proves it

`sched-large-weight`'s program prints, beside its share by ratio of counts, the useful work over
the window ("useful work: the server X and all nine Y of 1000 of the window",
`sched-large-weight.rs:46`); SCHED1's release measurement put the 1 ms build's at 0.762 (rv64)
and 0.718 (rv32) of the 10 ms build's (`RESULTS.md` "Release build: per-switch cost and useful
work, 1 ms against 10 ms"). The case itself is a checked build, where audits move the number,
so the bound is judged in a sibling case **`sched-large-weight-release`** (the same program and
checks, `debug_assertions = false`, both widths): its "all nine" figure at or above **0.85** of
the 10 ms release figure `RESULTS.md` records for that width, the threshold pinned in the toml
with the arithmetic in its comment (the fixed 100 to 155 µs per 1 ms slice is 10 to 15 %, not
this package's to remove). The checked case is unchanged. The bound is written on
scheduling.md with the measurement; a miss is a finding. Neither case is a memory case, so the
bench's scan wait (B11) does not touch them. The host tests: the cache agrees with the frames
after every operation (a property test over random operation sequences, checked against
`sched_state`); `raise_floor` runs only when the floor can have moved (a counting `Budgets`
in the test); the stride differential (`the_crate_and_the_model_agree`) unchanged.

## Position relative to SCHED1's merge

The owner chose (b): SCHED1 merges first, on the 1 ms slice, with the per-switch cost a stated
residual in "Charging"; RECON1 follows in the next train on top of it, and its measurement
replaces SCHED1's sentence (replaced, not amended). SCHED1's re-measurement is therefore this
package's before-number; the after-number is the gate above.

## Page lines (exact text in the report)

- **scheduling.md** "The current minimum and ties" (:94): the floor's paragraph gains the bound
  ("the floor is raised only when it can have moved, from ranks the queue keeps beside its
  slots; a slice end's scheduler work is under a microsecond per queued budget"); "Charging"
  (:222-223): the three sentences from "(0.762 on rv64 ..." to "... a later measurement
  replaces it." are replaced, not amended, by the measured useful work after the fix and the
  per-slice-end cost, with no promise in them; R12's status line (:796) gains the host tests
  and `bench:sched-large-weight-release`.
- **SECURITY.md:** R12's row gains the tests.

## Owned paths

`libs/stride/src/lib.rs` (`Queue`: `raise_floor`, `reconcile`, `fold`, `deschedule`, `destroy`,
`reweigh`, `pick`, the slot array and its cache; `Wiring` only where it must pass the cache
through), `libs/stride/src/tests.rs`, `kernel/src/sched.rs` (the checked audit of the cache
against the frame, at `Sched::reconcile`), `tests/sched-large-weight-release.toml` (new; the
program is shared and unchanged unless the release case needs a line), the pages above.
**Not yours:** the slice (SCHED1), the marks' rule and `marks.rs`, the pick's rank, the
charging arithmetic, `Runner`/`Harts`/the per-hart billing (SMP1's), the budget frame's words
(K19's chains among them), the model, `tests/sched-large-weight.toml`'s checked expectations.

## Gates

The short gate (.wash/SWARM.md "Integration trains"): both builds; the kernel's and
`libs/stride`'s host tests; its own cases (`sched-large-weight`, `sched-large-weight-release`,
`sched-share`, `sched-ties`, `sched-server-busy`, `sched-carve-inflation`, `sched-debt-lift`,
`sched-cluster`, `sched-destroy-billing`, `kernel-containment`) on both widths, each timing
case alone; the smoke set; fmt, the unsafe ratchet, the size budget, the no-cruft gate,
doccheck. The whole bench in its train. Report each command with its exit code and the
before/after table.

## Not here

The slice; the number of reconciles per slice; the SBI timer re-arm (hardware's Sstc, or a
timer armed only when the deadline moves: its own package if the attribution names it); the
destruction's walks (K19's); the per-hart runners and the ticket lock (SMP1's); a second
queue per hart.

## Checkpoint

After the attribution is confirmed by your own measurement (the instruction count per step,
before any change): one progress line with the branch and the table.

## The attribution (SCHED1's successor, 2026-10-06)

`.wash/local/evidence/SCHED1/five-cases/reconcile-finding.md` and `RESULTS.md`'s new sections:
the cost is `raise_floor`'s walks (about 2.6 µs per queued budget per walk, checked; about 28 µs
per budget per slice end in release, L = 3..17), over three spans per slice end in 94 % of
slices (the leave at the timer exit, 158 µs and four walks; kmain's pick, 58 µs and two; the
leave after the switch, 72 µs and two); `mm.ready`, `set_ready` and the trace records are 2 to
4 µs per budget; a fixed 100 to 155 µs at L = 0 remains, unsplit (traps, two SBI ecalls).

---

## plan_set text for the node

```json
{"RECON1": {"title": "The reconcile's cost bounded by its stated loop bounds: a kept ready count, one floor raise, tens of instructions per visited budget",
 "template": "package", "parent": "M1", "needs": ["SCHED1"],
 "body": "SCHED1's release measurement (.wash/local/evidence/SCHED1/five-cases/RESULTS.md): a slice end costs ~0.36 ms rv64 / ~0.45 ms rv32 of kernel time, two-thirds of it ~3.2 reconciles per slice at ~9k instructions each for a handful of marked budgets, against docs/kernel/scheduling.md's claim that a reconcile visits only the budgets whose runnable state changed (K22) and R12's kernel-time text. Brief: .wash/local/RECON1-brief.md (architect-15). Tier A kernel, size S. Attributed (reconcile-finding.md): Queue::raise_floor's unconditional walks reading every queued budget's 7-word State through checked frame reads, 4-9 walks per slice end, ~28 us per queued budget in release; not settle/ready_count. Fix: raise_floor only when the floor can rise; (pass, tie) cached in the queue's slots, audited against the frame in the checked build. Gate: under 3 us per queued budget per slice end in release, the fixed L=0 term (100-155 us: traps, two SBI ecalls) unchanged; sched-large-weight's useful work at 1 ms within 0.85 of 10 ms both widths. Needs SCHED1 (the owner chose (b): SCHED1 merges first with the cost a residual; RECON1 next, replacing its sentence).",
 "state": "todo"}}
```
