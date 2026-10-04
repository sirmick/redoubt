# K22: reconcile follows the budgets that changed

Tier A (the scheduler), size S-M. Needs K16 merged; start from main after it. Runs beside IPC3
(which owns `message.rs`). Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## Context rules (read these first)

- **Don't read whole files.** `grep -n`, then Read a range. You need: `Runnable` and its `fill`,
  `reconcile`, `leave`, `pick` in `kernel/src/sched.rs`; `Queue::reconcile`, `insert`,
  `remove_at`, `contains` in `libs/stride/src/lib.rs`; the `ProcessState` transitions in
  `kernel/src/ptable.rs` (search `ProcessState::Ready`).
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/K22-report.md`.

## Reading list (only these)

- `docs/kernel/scheduling.md`: "Charging", the reconcile and wake paragraphs, R12 and its bound
  paragraph ("A system call's kernel time is bounded by…").
- `.wash/local/K16-walks-ahead.md`, "If `Runnable::fill` on every entry still dominates".

## The problem

K16's worst-walk case measured reconcile at 0.32 s when 250 budgets woke in one kernel entry.
Every entry rebuilds the runnable list from every live process (`Runnable::fill`, with a linear
`contains` per process), and `Queue::reconcile` then finds the next budget to wake by scanning
the whole runnable list, with a linear `contains` on the queue, once per budget it wakes:
O(R^2 x N) for R wakers. The cost follows every live process and the square of the wakers, not
what changed.

## The settled design: marks

1. **A budget is marked** when one of its threads becomes ready or stops being ready: every
   `ProcessState` transition in `ptable.rs` that changes a ready mask, a process's creation and
   its end. The mark is a flag in the scheduler's per-budget state and an entry in a marked list
   (at most one per live process, so `MAX_PROCESS_COUNT` entries, kept in `SCHED` like `Runnable`
   today). Each budget also keeps a count of its ready threads, so "is it runnable" is one read.
2. **Reconcile visits only the marked budgets.** A marked budget that is queued, not running and
   has no ready thread leaves; one that has a ready thread and is not queued wakes. The wakes are
   sorted by descending id once (at most the marked count), then inserted, so the lowest id still
   ranks first. Marks clear at the end of the entry.
3. **The same events in the same order.** One reconcile per kernel entry, the same leaves and
   wakes, wakes in descending id. If today's leave order (queue slot order) is visible to the
   trace, the oracle or the model, keep it, or show it is not and say so. Never change what the
   model or the oracle sees.
4. **`Runnable::fill` goes** with its per-entry walk. `next_thread`'s walk of the budget's
   processes stays (bounded by `MAX_PROCESS_COUNT`, R12's constants clause); name it in the report.
5. **A checked build audits** the marks: the runnable set the marks imply equals a walk of every
   live process, as `fill` computes it today. The audit is the checked build's only and excluded
   from the targets as K15's are. **Ruled (architect-14, 2026-10-03, second ruling):**
   - **Every reconcile** checks the budgets it visited: each one is queued or running exactly
     when its ready count is above zero. This is a `debug_assert`, O(visited), unstamped, like
     the checked build's other assertions.
   - **The full walk** is the stamped audit. It runs at the first reconcile that visits a budget
     once a slice's length (the scheduler's slice constant, named, not a literal) has passed
     since the last full walk. It also runs, unconditionally, at a pick that finds nothing to run
     before the hart idles.
   - A full walk at every reconcile overflowed the trace ring at worst-walk's setup (about 130K
     reconciles at 509 live slots) and pushed its held replies past their deadline.
   - A missed mark is found at the next full walk, and a hart never idles past one.
   - A miss that a later, marked change undoes before then is not seen. Its effect is a budget's
     turn late or lost, a fairness error, never a wrong thread run.

## The cases

1. **K16's worst-walk case**: reconcile at 250 wakers, before and after, both widths; its p99
   and max at the gate's full fill do not regress.
2. **The stride differential and the model**: `the_crate_and_the_model_agree`,
   `a_broken_model_disagrees`, `scheduler_fairness`, `scheduler_contracts_hold`, and every R12
   mutation pass unchanged. If `Queue::reconcile`'s signature changes, the differential drives the
   new one with the same events.
3. **Host tests**: a mark missed (a ready-mask change with no mark) trips the audit at the next
   reconcile that visits a budget; a lone missed mark, followed only by a pick with nothing
   queued, trips it at that pick.
4. **The whole bench, both widths**, and the scheduler cases (`sched-*`) by name.

## Page lines (exact text in the report)

- **scheduling.md**: where it says reconcile takes every budget with a runnable thread, say it
  visits the budgets whose runnable state changed in the entry, with the same wakes and leaves in
  the same order. If the bound paragraph or a residual names reconcile's cost, it follows.
- **budgets.md**, the measured list: K16's reconcile number at 250 wakers becomes the new one.

## Owned paths

- `kernel/src/sched.rs` (`Runnable`, `reconcile`, the marks), `kernel/src/ptable.rs` (the mark
  at each ready-mask change), `libs/stride/src/lib.rs` (`Queue::reconcile`) and its tests.
- The page lines above.

**Not yours:** `message.rs` (IPC3), the pick's rule, the stride rules, the model.

**Hotspots:**
- IPC3 runs beside you in `message.rs`. Its wakes call into the scheduler; keep the mark inside
  `ptable.rs`'s transitions so neither package edits the other's file.
- SMP1 follows both and owns `sched.rs`'s pick, the reschedule IPI and the trace's hart field. Keep
  the marks out of the pick.

## Gates

- The whole bench on both widths, alone.
- The kernel's host tests, the stride crate's tests and differential, the model's tests.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, reconcile's numbers before and after, the leave-order
finding, and each page line as written.
