# K18: a checked build's audits do not move its schedule

Tier A (the kernel), size S. Closes `docs/todo/audits-billed.md`; read it first. Design: QA
GATE1-notice-two-leases. Evidence: `.wash/local/GATE1-two-lease-split.md` (rv64, seed 4).

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from your worktree.

## The rule (owner's decision A, applied in full)

Each latency target counts the kernel a release build runs. K15 took the audits' time out of
each window, but the scheduler still charges that time and counts it against the slice. So in a
checked build an audit moves the schedule: the thread that ran it is requeued where a release
build would let it run on. This package makes the audits invisible to the scheduler as well:

- an audit's time is charged to no budget, at no weight;
- it does not count against the running slice: the slice's end and the start of the pending
  charge both move forward by the audit's length.

The audits stay full and stay where they are. No target moves; the attacker is unchanged. A
release build compiles none of it.

## Where

- `kernel/src/sched.rs`: the pending charge (`ticks` since the slice began, the "ticks it has run
  and not yet been charged"), `pick`'s `set_slice_end`, and `trace::audit`'s guard.
- The two audits: `budget::destroy_subtree`, after `Y` (`check_object_indexes`), and
  `MemoryManager::index_process` (`check_process_index`).
- Measure each audit's length in the checked build itself (`cfg(debug_assertions)`), not only
  under `sched-trace`. A checked build without the trace must schedule the same way. Keep the
  `U`/`V` stamps as they are.
- One obvious place: a single wrapper that runs an audit, then shifts the slice end and the
  charge start. Both audit sites go through it.

## Tests

1. `kernel-containment` at seed 4, the two-lease fixture from wp-gate1 e9ecc5ea5, on both widths.
   Every target is met. Take the fixture from GATE1's branch only to measure; do not commit it
   (it is GATE1's). Report the deadline notice net and gross, and the split of c2.
2. `sched-latency` and `kernel-containment` at seed 3: no target regresses.
3. A recorded negative run, like `audit-unstamped`: a debug-only feature `audit-billed` keeps the
   old charge, and the two-lease run misses. Record it on scheduling.md beside the
   `audit-unstamped` run.
4. Whole `cargo testbench`.

## Pages (they move with the code)

- `docs/kernel/scheduling.md`:
  - "Targets exclude the checked build's audits": an audit neither fills a window nor moves
    the schedule;
  - remove the residual "A checked build's audits move its schedule";
  - re-measure the seed-3 table if its numbers move.
- `docs/testbench.md` "Checked builds": the same sentence.
- Delete `docs/todo/audits-billed.md` and its SUMMARY line.

## Owned paths

`kernel/src/sched.rs`, `kernel/src/budget.rs` and `kernel/src/process.rs` (the audit call sites
only), `kernel/Cargo.toml` (the feature), the pages above, `docs/SUMMARY.md`. Anything else is a
question to the orchestrator.

## Report

The before and after numbers for 1 and 2, the negative run, and the lines changed.
