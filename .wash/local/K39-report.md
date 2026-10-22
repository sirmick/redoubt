# K39 report

Branch wp-K39 in .worktrees/K39, base main b6d28a6b6 (K35's bounded expiry included, so "red on
main" and "red on K35's head" are one check).

## Design note (2026-10-22, before any code)

### What is wrong today

- The slice is wall time: `leave` sets `slice_end = now + SLICE_US` at the return after a pick, and
  a timer interrupt decides `slice_end <= now` (`sched::slice_over`) *after* its expiry
  (`irq.rs`, `expire_at_entry` first, decision second).
- Every kernel entry from user mode answers the expiry first. Its time is billed to the payers of
  what it ends (K20/R12), but it still passes on the wall clock, so it comes out of the slice.
  The same holds for a device interrupt's handling, which is billed to the device's owner.
- K24 already keeps the pick's exit work out of the slice: the slice starts at the return.
  Audits are kept out too: `sched::audit` moves `slice_end` by their length.

### Why a share check alone can't show it

Across budgets, stride makes up the loss. A victim whose slice is cut is charged only its user
time, so it stays the lowest pass and is picked again. Totals do not move.

What a cut slice loses is the thread's turn among its budget's threads. Each slice end moves the
budget's round-robin cursor to the next ready thread. That is what K35's trace shows: the poller
and the starved waiter were threads of one process and one budget.

So the case must have two equal threads in one budget, and the attacker's expiry must land in one
thread's slices only.

### The rule as I'd build it: the slice pauses

A slice is the thread's own time: its user time and its own calls.

- **Pause.** At every entry from user mode, right after `from_user` has accrued the user time,
  the hart records `(end, at)`: the slice's end and now, in µs. `at` is the moment the thread
  left user mode as accounting counts it, so a lock wait stays user time, as today.
- **Resume.** The slice resumes with `left = end - at`, setting `slice_end = now + left`:
  - a call or an exception resumes right after the entry's expiry, so the call's own handling
    counts, as today;
  - an interrupt (timer, device, IPI) resumes at its return, in `leave`, before `rearm`. No part
    of an interrupt is the interrupted thread's.
- **Decision.** A timer interrupt decides from the pause: the slice is over iff `end <= at`,
  that is, it had ended before the thread trapped. The trace's `=` records `(end, at)`, so the
  oracle's `end <= now` check needs no code change; its doc comment and the page sentence
  "a slice that ends during the entry's own expiry counts as ended" change.
- **Ended slices.** A slice that ended meanwhile is not resumed: `repick` or `kmain` set
  `slice_end` to NEVER, and `leave` drops the pause.
- **Audits inside a pause** move `slice_end` as before, but the resume overwrites that, so they
  are not counted twice. Audits outside a pause are unchanged.
- **Billing is untouched.** Nothing is charged twice: each interval keeps its one payer, and only
  the slice's end moves.
- **Deadlines.** Only a deadline due now preempts (R12 unchanged): `expired.destroyed` still
  calls `preempt()`.
- **Bound.** An attacker extends another thread's slice only by work billed to the attacker, or
  by device work billed to `system`. Others wait for it in wall time, and stride charges it.
- **Residual.** The steps of a return after the timer is armed (the close and the `sret`) still
  count against the slice. That is constant per entry, a few µs, and I'll measure it.
  Cost: two `time` reads per entry.

### Question: payer granularity

The ruling says "billed to another payer", which I read as another budget. The rule above pauses
for an interrupt or expiry whoever pays, the running thread's own budget included.

Why I recommend the pause:
- It is the ruling's first clause literally ("a slice is the thread's own time, its user time
  and its own calls"; "resumes with what it had").
- It covers K35's own trace. That trace was within one budget, which a budget-level rule leaves
  as it is.
- It is simpler. A budget-level rule needs a hook in `Sched::bill`. It double-counts audits
  inside explicit expiry bills, and it must skip the bill made at the return after `rearm`, or
  the timer fires early with a stale interrupt.

Its only extra effect: the claim and return of a device interrupt are billed to the interrupted
budget but no longer counted in its slice. If the Architect wants the budget-level rule, I build
that instead.

### Model

The model already keeps the rule: `bill_timer` does not touch `slice_left`. I'd add
`mutation:R12SliceCountsTimerWork` (`bill_timer` takes the work from the running thread's
`slice_left`). It is caught by a contract in model/tests/common/contracts.rs mirroring K24's
exit-work one. No `gen.rs` change, so no new draws.

### Case (name TBD, `sched-slice-own-time`): rv64 + rv32, one hart, icount, checked, `sched-trace`

- **Victim budget V:** threads T and C of one process, the same counting loop.
- **Attacker budget A:** equal weight. One process with N threads. Each time they run, they
  re-arm to one shared absolute deadline a little after their re-arm ends.
- **Why it hits T:** A's due waits land in whichever V thread runs after A's turn. The expiry
  X ≈ 1-2 ms takes that slice, and the cursor hands V's next turn to the other thread. N is
  sized from worst-walk, 250 due ≈ 27 ms, so N ≈ 10-20. I'll measure it.
- **Expected:** on main T's count collapses against C's. Under the rule T ≈ C.
- **Verdict:** the victim's count share T/(T+C) ≥ a bound from its round-robin share (half),
  with the margin per the bench's rule. The program counts, but the counts are the victim's,
  not the attacker's. The oracle also checks billing (A pays every expiry), so the share is
  not bought by mis-billing.
- **Red on main:** I'll run the same program on a scratch worktree of main (rv64 and rv32) and
  report the numbers.
- **Witness:** I'll require at least M expiries inside T's slices, so the case can't pass for
  lack of hits.

### Pages

- scheduling.md:
  - Preemption points: the slice's end item; the oracle sentence about the expiry.
  - R12's paragraph: the residual K35 added ("A timer interrupt's expiry inside a slice does
    count against it ... is the remedy") becomes the rule.
  - R12's status: the case and the mutation.
  - Charging: unchanged billing, checked.
- timer.md: "Expiry walks the processes" and R12 for timer work, the sentence that the expiry
  "falls inside the interrupted thread's slice".
- SECURITY.md: R12's Tested-by (C7).
- testbench.md: the C14 counts (+1 boot, icount) and the oracle's `=` description.
- The nobody-share and latency figures get re-recorded if they move.

### Touches (for K37 coordination)

- kernel/src/sched.rs: a per-hart pause in `Billing`, `slice_over`, `leave`, the trace's `=`.
- kernel/src/arch/riscv/irq.rs: the pause at entry, the resume after a call's expiry.
- model/src/{sched,mutation}.rs, model/tests/common/contracts.rs.
- The new case, tests/programs/Cargo.toml.
- scheduling.md: Preemption points and R12's slice paragraph only, plus R12's status.
- timer.md, SECURITY.md, testbench.md.

### Runs

- Single cases and small sets myself.
- The sched-*/smp-* count check and any figures re-recording are a load run: I'll ask for a slot.

## Build (2026-10-22): ruled thread-level pause, conditions (i)-(v)

Branch wp-K39, base b6d28a6b6, head **05e6ebaed**:
- 0fa131482 kernel: the pause, the model mutation and contract, size ceilings raised
  (`Size budget: kernel` +19, `model` +7);
- 0bdad28ad bench: the `DueWaits` and `CountPair` roles, the `expiries_in` oracle argument and its
  host test, and the case `sched-slice-own-time`;
- 05e6ebaed docs.

### Kernel

- `sched::pause_slice` runs at every entry from user mode, after `from_user`, and records
  `(end, at)`.
- `sched::resume_slice`:
  - for a call or an exception, right after the expiry (`irq.rs`);
  - for an interrupt, in `leave` before `rearm`. It is not resumed if the slice ended
    (`slice_end` NEVER).
- `slice_over` decides `end <= at`. The trace's `=` records `(end, at)`; the oracle's code is
  unchanged.

### (i) The bound

Stated in scheduling.md, Preemption points: a budget extends a slice only by work billed to it
or to `system`. Stride charges that work and R12's per-entry bound limits it.

### (ii) The residual, measured

- **Method:** a scratch build instrumented from `resume_slice` to the return's start of user time,
  over interrupts returning to their thread, in `sched-slice-own-time` (checked build), at
  10 ticks/µs.
- **rv64:** mean 6.6 µs, max 6.7 µs (n = 200).
- **rv32:** mean 11.9 µs, max 12.6 µs (n = 500).
- **Arm to back:** 0.3 µs, so the SBI arming is most of it. Stated on the page.

### (iii) Model

- `R12SliceCountsTimerWork` is in the model, listed on R12, and SECURITY.md's row is equal (C7
  passes).
- It is caught by `sched_contracts`: "the expiry of another budget's timeouts came out of the
  interrupted slice".
- No change to `gen.rs`.

### (iv) The case `sched-slice-own-time`

**Layout:** the attacker A is a sibling process in the victim's budget, by the orchestrator's
answer `k39-sibling-ok`.

**Why not cross-budget** (prototype, two budgets, `low` share of 1000):

| | N 8–31, lead 0.6–1.2 ms |
|---|---|
| main rv64 | 489–499 |
| main rv32 | 468–499 |
| K39 | 498–499 |

**Lead sweep**, A in V's budget, N = 31, T's share of 1000:

| lead | main rv64 | main rv32 | K39 rv64 | K39 rv32 |
|---|---|---|---|---|
| 0.8 ms | 451 | 437 | 494 | 492 |
| 1.0 ms | 417 | 415 | 495 | 495 |
| 1.2 ms | 365 | 338 | 496 | 495 |
| 1.5 ms | 368 | 413 | 495 | 493 |
| 2.0 ms | 535 | 423 | 503 | 505 |
| 2.5 ms | 495 | 429 | 499 | 505 |

**Final case, lead 1.2 ms:**
- K39: 494 (rv64) and 495 (rv32), PASS.
- main: 367 and 330, FAIL.

**Margin:** the worst shortfall on K39 is 8 → 16 → 20, so the floor is 480.

**Witness:** `expiries_in=2:60`, against 127 (rv64) and 164 (rv32) measured on K39.

**A pays every expiry:** cannot be told apart in one budget. The oracle's per-interrupt billing
check stands in, as stated in the case comment and on the page.

### Gates on the final tree, exit codes

Short gate, all PASS on rv64 and rv32:
- unsafe-budget, size-budget (after the raises), no-cruft, docs, formatting;
- host-tests (testbench, including `expiries_inside_a_budgets_time_have_a_floor`),
  stride-host-tests, model-host-tests;
- `cargo test -p redoubt-model --test mutations` with R12SliceCountsTimerWork: caught;
- smoke set: userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear,
  lend-untouched-page (smp 1 and 4);
- own case: sched-slice-own-time;
- focused: sched-wake-no-preempt, sched-irq-wake, sched-deadline-preempt, budget-deadline,
  sched-timer-entry (smp 1 and 2), sched-timer-flood, timeout-hint-exit, sched-round-robin,
  worst-walk, sched-exit-churn.

The kernel builds on both widths; every boot case built it.

Not run:
- the whole sched-*/smp-* classes, the -mttcg and quiet cases (`sched-wake-no-preempt-harts*`);
- the latency cases (steward-latency*, sched-latency);
- the nobody-share and latency figures for (v).

All of these are a load run, awaiting a slot.

### Summaries checked

- **scheduling.md:**
  - Preemption points: the list item, a new paragraph on the pause and the case, the oracle
    sentence;
  - R12: the slice paragraph's residual replaced, status +2.
- **timer.md:** the slice end bullet, the expiry paragraph, "Expiry walks the processes".
- **model.md:** 183 mutations, 30 of 36 variants, the table row.
- **testbench.md:** the oracle's `=` and `expiries_in`; C14 counts 358 boot, 189 icount, 182 so.
- **SECURITY.md:** R12 row.
- **No change needed:**
  - README.md and GETTING-STARTED.md: no slice claims;
  - Charging (billing unchanged);
  - the Responsiveness tables, pending the load run;
  - timer.md's "within 1 ms of any user code": still true, since user time counts in full.

## Load run and round 1 fold (2026-10-22)

Head **468052c93**:
- 2138f7b19 kernel;
- ea52a271a bench;
- 468052c93 docs.

Each commit passes the docs gate on its own; the two earlier commits were checked in scratch
worktrees.

### Load run on 05e6ebaed

- 49 cases × widths × smp, all of sched-*, smp-*, steward-latency*, sched-latency: **115 jobs,
  115 PASS** (prebuilt, `make -k -j8 set`).
- Logs are in .tmp/K39/load/.
- kernel-containment, run on the fold: PASS on both widths.

### (v) Nobody shares (of 1000), K39 against main b6d28a6b6, measured the same way

| case | rv64 K39 | rv64 main | rv32 K39 | rv32 main |
|---|---|---|---|---|
| sched-exit-churn | 35 | 25 | 61 | 42 |
| sched-latency, 1 hart | 76 | 70 | 117 | 108 |
| sched-timer-flood | 29 | 23 | 61 | 44 |
| sched-budget-churn | 71 | 66 | 110 | 102 |
| kernel-containment | 116 | — | 162 | — |

- **Cause:** a slice's end now takes a timer interrupt of its own, whose walk finds nothing.
  sched-exit-churn takes 3,773 timer interrupts on rv64 against 3,142 on main, 594 of them
  expiring against 926.
- **The page was older than main:** 14/26, 27/44, 17/35, 29/47, 61/84. Re-recorded with K39's
  figures; the before-figures are in the docs commit message.

### Latency

steward-latency, -n4 and -n16, and sched-latency: every target met. The p99s move both ways
against main at 16 samples (rv64 N=1 decision wake 1732 → 1906; N=3 1642 → 3246; others lower),
with no systematic shift.

sched-latency rv32 decision wake p99 moved:
- 1 hart: 4506 → 9536 µs;
- 2 harts: 9231 → 12039 µs.

Its target is 125,000 µs. The page's responsiveness table is the seed-3 sweep, not these cases,
so it is not re-recorded.

### Fold

- **Simplifier P2, kept.** Audits do run outside a pause: every call's IPC-lists audit
  (`redoubt::handle`, after the call's resume), the process, budget and index audits in a call's
  own handling, and `audit_marks` at a call's return. For those, `audit`'s move is what keeps the
  audit out of the slice. Its doc comment now says so.
- **Simplifier P3, done:** `sum.timer_expiring` is dropped; the summary reads
  `timer_expiring_in.len()`.
- **Editor P2, done:** the history words are gone from scheduling.md and the case comment; the
  before-figures are in the commit messages.
- **Editor P2, commit independence, done:**
  - kernel commit: model.md, SECURITY's mutation entry, R12's and Preemption points' status
    mutation entries (71 and 15), testbench.md's `=`;
  - bench commit: the case's status entries (72 and 16), SECURITY's case entry, testbench.md's
    `expiries_in` and C14 counts.
- **Editor P3, done:** "of a slice per interrupt". The 360/189 combined counts are the
  orchestrator's at the merge.
- **Red P3, done:** the bound clause now covers unbilled work. A timer interrupt that finds
  nothing comes only for something armed on its hart; another hart's interrupt comes only for
  that hart's work (a shootdown), inside its call's or destruction's bound.

### Gates on the final tree

- PASS: docs, formatting, size-budget, host-tests.
- sched-slice-own-time: PASS on both widths (code unchanged since; the run was on the fold's
  tree).
- kernel-containment: PASS on both widths.
