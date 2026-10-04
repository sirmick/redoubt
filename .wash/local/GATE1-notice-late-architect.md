# GATE1-notice-late: the Architect's answer, in full

## 1. Who owns the 40 ms, and what it is made of

`docs/kernel/scheduling.md#responsiveness` owns the target, in the table row "deadline notice:
the lease's `killed` notice received, less the lease's deadline, p99 <= 40 ms".
`docs/kernel/README.md#containment` adopts it ("the targets are the ones in responsiveness, and
the gate adds none").

It is a measured target, set from sweeps. It is not a sum the page derives. The row gives no
decomposition, and there is no per-term budget. From the rules, the measure is three terms in a
row:

| Term | Span | Rule | Expected |
| --- | --- | --- | --- |
| a. expiry | deadline -> the destruction starts (trace X) | budgets.md#deadlines: the hart timer is armed for the earliest deadline; expiry runs first at every kernel entry; a spinner is stopped by the timer interrupt, and the tight loop cannot put it off | about 0 |
| b. R10 | X -> Y, the destruction's kernel time, non-preemptible | budgets.md#r10-destruction; scheduling.md target p99 <= 30 ms | gate: 23.6 ms, met |
| c. the receiver's wake | Y -> the stand-in's `time_now` after `receive` returns the notice | R12; "a wake waits out the running thread's slice, a waking budget keeps a pass above the floor if it has one" | a timer-wake-like 8-10 ms |

Term c assumes the receiver is already blocked in `receive` on the exit endpoint when the
deadline passes. The workload that set 40 ms meets that assumption: `sched.rs` ~713-731, the
latency stand-in's by-deadline loop, does nothing else while it waits. 23.6 + ~9 = ~33 ms, which
fits the sweeps' worst of 37.5 ms. GATE1 measures 53-67 ms, so 30-43 ms falls outside a+b+c as
the latency case composes them.

## 2. The order is settled

budgets.md#deadlines and the R10 steps settle it:
1. At the first kernel entry at or after the deadline, expiry runs first.
2. The whole destruction then runs as one non-preemptible kernel operation (steps 1-9).
3. The `killed` notices are made inside it, at step 2 (Kill).
4. No user thread runs until it returns.

So the order is: deadline, then destruction (notices created), then notice received. "Notice
first" cannot happen. Nothing is open here.

## 3. The gate composes something the latency case never did

The gate's steward stand-in (`.worktrees/gate1/tests/programs/src/sched.rs`, `containment()`,
~1505-1557) is one thread. Each round it runs in this order:
1. it makes D and waits for D to arm;
2. it takes H's decision timeout (5 ms);
3. it calls `budget_destroy(H)` (110 ms call to return);
4. it takes H's notices, runs the `gone` checks and `victim_control`;
5. **only then** it enters `take_notices(exit[0], D)`.

D's deadline is `created + max(4 x h_arm, 200 ms) + 200 ms`. Nothing ensures it falls after
step 4. Two ways this inflates term c, neither of them a defect shown in the kernel:

- **(F) Fixture serialization.** D's deadline passes while the stand-in is still in steps 2-4.
  The notice waits in its queue for the stand-in's own work. That is not kernel latency.
- **(S) Billing carried into the wake.** The stand-in was blocked in `receive` in time, but it
  was billed for destroying H (R10: `budget_destroy` bills the caller, 23 ms). The 110 ms call
  to return is R10's 30 ms plus slices, exactly as the table's row predicts. If D's deadline
  falls while that lead is still above the floor, the steward's wake lands behind several 10 ms
  slices: 23 + 3-4 slices is 53-63 ms. This is the scheduler as designed. Whether the target or
  the billing gives way is a design question.

Candidate (1) in the question is term a: a deadline fired late. It would break
budgets.md#deadlines outright. The trace can rule it in or out.

## 4. The split that decides it

This is GATE1's implementer's work: a fixture and trace reading only, with no kernel change.
The kernel's existing `sched-trace` records hold everything needed. For each of the 9 D leases:

- print `d_deadline`, and the stand-in's `time_now` when it enters `take_notices(exit[0], D)`;
- from the trace: the destruction's X and Y, and the steward budget's first pick after Y;
- from that: a = X - deadline, b = Y - X, c = receipt - Y;
- and whether the stand-in entered `take_notices(D)` before or after X.

What each outcome means:

| Outcome | Owner | Action |
| --- | --- | --- |
| a large | kernel, breaks budgets.md#deadlines | K15, Tier A, kernel (timer and expiry at trap entry). I cut it on the evidence. |
| c large, the stand-in entered `take_notices(D)` after X (F) | GATE1's fixture | Fix in GATE1: the stand-in takes each slot's notices on its own thread, blocked in `receive` on that slot's exit endpoint (both leases stay live and H's destroy stays concurrent). This neither widens a target nor weakens the attacker. I then add one sentence to the scheduling.md row: the notice is measured by a receiver already waiting. |
| c large, the stand-in was waiting (S) | a design choice | I take it to the owner with a recommendation (decision_request). The choice is between billing a hand destruction to the caller and the 40 ms target. |

## 5. The bisect instrument

K12's "T records" are not on main and not on any branch: no commit or kernel feature carries
them, and `kernel/Cargo.toml`'s features are `sched-trace` and the fault injectors only. They
are not needed for this question: R10 is met, and terms a/b/c come from `sched-trace`'s X/Y and
pick records. If K15 is cut for term a, its brief re-adds a test-only phase record as a feature
under `test-only`, never in a default build (R23).

## 6. On the split (GATE1-notice-split.md): S is not confirmed

- **Units.** The "entry - deadline" column is in µs: -32,617,487 µs is **32.6 s** before the
  deadline, not 32.6 ms (r0: 66 s). So H's destroy, and the lead billed to the stand-in for it,
  came about 32 s before X.
- **No lead at Y.** A budget blocked that long wakes at the floor (scheduling.md, "Running while
  carved down" and "The lead follows the weight": at or below the floor it owes only its
  remainder, and a wake lifts it to the floor). It carries no lead from H at Y. So it is not a,
  not F and not S.
- **What remains** is the plain R12 wake under the gate's load. A 1000-weight system budget,
  woken at the floor at Y, still runs 24.4 ms later. That is about 2.4 slices, and near-constant
  across leases. The sub-agent's notice comes about 8 ms after the first.
- **Why that may still be design:** the page says a wake "waits out the running thread's slice"
  and "several budgets can tie at the floor". The 40 ms was measured under sched-latency's load,
  which has fewer runnable budgets at the floor.

### Mapping the stand-in's budget, with no bench change

The trace's X record carries the destroyed top's id. The 9 X records that do not match a D
deadline are the stand-in's own `budget_destroy(H)` calls. So the last K (pick) before each of
them is the stand-in's budget id.

Then, for each D lease, from Y to the stand-in's first K:
- the stand-in's W record and its pass, against the floor (the lowest pass among those queued);
- every K in between: whose it is (system 42/2/43/45, `sessions`, a lease, the bystander), with
  entry counts;
- any R (requeue) of the stand-in between its two notices (the 8 ms).

### What follows

| What the mapping shows | What it is | Next |
| --- | --- | --- |
| The stand-in waits behind ties at the floor, or behind a running slice it cannot cut | R12 as designed, meeting a target set on a lighter load | decision_request to the owner: a kernel change to how a floor wake competes, weighed against the 40 ms, with a recommendation |
| A budget with a higher pass runs ahead of the stand-in | a kernel defect against R12 | K15, cut from the evidence |

## 7. On the wake-after-Y table: the gap is the checked build's audit

### Where Y is, and what runs after it in the same entry

`kernel/src/budget.rs`, `destroy_subtree` (~1197-1262):
- Y is stamped after `end_destruction()`, which ends step 9.
- Then, in the **same non-preemptible kernel entry**, it runs:

  ```rust
  // The audit, off the measured walk: ...
  #[cfg(debug_assertions)]
  MemoryManager::with(|mm| mm.check_object_indexes());
  ```

`check_object_indexes` (`process.rs` ~279) runs four checks:
- `check_process_index` and `check_irq_index`: scans over `0..=high_frame`, up to 17,488 object
  frames in this run;
- `check_frame_owners`: a scan of every RAM frame;
- `check_handle_chains`: every slot of every **live** table, which includes H's full table of
  4,091 handles and the bystander's.

These are fixed walks, sized by RAM, object frames and live handles. Nothing else ran in that
window, so the result is constant to the µs across leases. That is c's first 24.4 ms.

### The second notice's +8 ms

Taking an exit notice frees the process object (R20). `index_process` (`process.rs` ~222) then
runs `check_process_index` under `debug_assertions`, another scan of `0..=high_frame`, in the
stand-in's own `receive` entry. The record order matches: the stand-in's two receives come
within a few entries of each other, with no R between them.

### Ruling on (1): where R10 ends

R10 is the destruction: steps 1-9 of budgets.md#r10-destruction. Y belongs after step 9, where
it is. budgets.md, Residual risks, already places the audit outside R10: the checks "run once
off the destruction walk, never scaling it". So Y is not early, and K12's 23.6 ms stands.

The audit is not production kernel time. A default build compiles none of it. But it does hold
the hart, and every latency target is measured in a checked build: `sched-trace` requires
`test-only`, which requires debug assertions (R23). So every user-visible wake measure since the
sweeps carries the audits. sched-latency's tables are small, so there it was a few ms. The gate's
full tables make it about 24 ms.

### Confirming run before the owner question

One local run, uncommitted:
- remove the two `cfg(debug_assertions)` audit calls named above (after Y, and in
  `index_process`);
- run `kernel-containment --arch rv64` at seed 3.

The cause is confirmed if c drops to a few hundred µs, and the notice p99 to about 21 ms + c.

### The owner question, once confirmed (draft)

The deadline notice misses 40 ms only by the checked build's own audits. These are full scans,
run in the destruction's kernel entry and at each process-object free. The shipped kernel does
not run them. Every latency target is measured in a checked build, because the trace needs one.

**Recommendation (A): measure the kernel, not its audits.**
- The traced kernel stamps each audit's start and end, as test-only records. The oracle reports
  them, and subtracts the audit time inside each measured window (the deadline notice, and every
  wake measure) before the targets apply. It also states the audit total.
- The audits stay full and stay where they are. No target moves. The attacker is unchanged.
- Rules: scheduling.md#responsiveness, which says what a target counts, and
  testbench.md, "Checked builds". budgets.md's "off the walk" sentence gets its counterpart.
- Work: kernel trace records (test-only) plus the oracle and post-check. Tier A, size S, cut as
  K15. GATE1 then runs unchanged.

**Alternatives:**
- **(B)** Keep the measure gross, and make the audits fit. Scope the post-destruction audit to
  what the destruction touched, or run the full audit only at quiescent points, such as the
  gate's end. This is cheaper, but it weakens the checked build's audit exactly where the gate
  attacks it. The focused fault cases keep full audits.
- **(C)** Keep everything, and let the 40 ms target include checked-build audits. Then the gate
  needs the audits made fast enough. This is a kernel rewrite of `check_handle_chains` and the
  frame scans, and is unlikely to fit at two full tables.
- **(D)** Measure the latency targets on a build without debug assertions. This needs the trace
  split from debug assertions, which changes R23's guarantee that a release kernel carries no
  test channel. That is a tenet-level change, and I recommend against it.
