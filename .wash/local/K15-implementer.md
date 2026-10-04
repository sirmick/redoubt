# K15: the latency targets exclude the checked build's audits

Tier A (the kernel's trace, the bench's oracle), size S. Needs K12. GATE1 resumes on K15's merge.

## The package

The latency case and the containment gate run checked builds, because the scheduler trace needs
one. A checked build runs full audit scans that a release build does not have:
- `check_object_indexes`, after each destruction's Y record;
- `check_process_index`, at each process-object free.

These scans held the gate's deadline notice at 53.7 / 66.9 ms against 40 ms. With them removed,
it is 21.6 / 23.2 ms.

The owner chose that targets count only the kernel a release build runs. K15 does three things:
- stamps each audit in the trace, as test-only records;
- has the oracle subtract audit time inside every window a target judges;
- reports the audit total.

The audits stay full and stay where they are. No target moves.

Every cargo and bench command runs as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>` from `.worktrees/k15`.

## Rules

- `docs/kernel/scheduling.md#responsiveness`: "Targets exclude the checked build's audits".
- `docs/testbench.md#checked-builds`: its audit paragraph.
- R23 (no test channels): every new record is under `sched-trace`, and a default build compiles
  none of it.
- Rule F (`docs/testbench.md`): the subtraction is the bench's, from the kernel's trace. Nothing
  comes from a program's own claim.
- The decision and its evidence: QA `GATE1-notice-late`, and
  `.wash/local/GATE1-notice-late-architect.md` section 7.

## Reading list

1. `docs/todo/latency-excludes-audits.md`. Its "Done when" is the contract; delete the page at
   the end.
2. `kernel/src/sched.rs`, `pub mod trace`: the record kinds, `record`, and `r10`.
3. `kernel/src/budget.rs`, `destroy_subtree`: X, Y, and the audit after Y.
4. `kernel/src/process.rs`: `index_process` and `check_object_indexes`.
5. `tools/testbench/src/sched_oracle.rs`: the X/Y handling at ~225 and its host tests at ~570.
6. How each measure is judged:
   - `tests/programs/src/sched.rs`: `Stats`, `take_notices`, and the latency stand-in;
   - `tests/sched-latency.toml`;
   - GATE1's `tests/kernel-containment.toml` on `wp-gate1`.
7. An example of the work: commit `760eb9b63`, a measure added across the program, the oracle and
   the pages.

## Owned paths

- `kernel/src/sched.rs`: the `trace` module only.
- The audit call sites:
  - `budget.rs`: `destroy_subtree`;
  - `process.rs`: `index_process`. K14 owns `process_map` in the same file, so touch nothing else
    there.
- `tools/testbench/src/sched_oracle.rs` and the post-check's parsing.
- `tests/programs/src/sched.rs`: only the reporting of the windows the oracle needs, such as the
  deadline and the receipt time of each notice.
- Pages:
  - `docs/kernel/scheduling.md`: the measured numbers, and dropping its residual bullet;
  - `docs/testbench.md`;
  - `docs/SUMMARY.md`;
  - the todo page.

Not `kernel/src/message.rs`, which is IPC2's and K13's.

## Deliverables, in order

1. **Records.** Two record kinds, an audit's begin and end, under `sched-trace`, around both
   audit calls.
2. **The oracle.**
   - Parse the records, and sum the audit time inside each window.
   - Subtract that sum before each target applies:
     - the deadline notice, from the deadline to the receipt;
     - the driver wake and the steward's timer and decision wakes;
     - R10 (Y is already before the audit, so this should be zero; assert it);
     - a lease's end.
   - Print the audit total per measure beside the target.
   - The deadline notice's window has its ends from the program's `time_now`. The program prints
     both ends per sample, so the oracle can subtract within them. The target's check moves to the
     post-check, which owns the verdict.
   - Host tests:
     - a window with an audit inside it;
     - a window with an audit straddling its edge;
     - a window with no audit;
     - an unmatched begin or end, which is a failure, not zero.
3. **Can it fail?** A test-only kernel feature (call it `audit-unstamped`) leaves one audit
   unstamped. A recorded run of `kernel-containment` with it must miss the deadline notice. It is
   a negative run, recorded like `sched-inject-tie-fault`.
4. **Measure.**
   - `sched-latency` and `kernel-containment` (on `wp-gate1`, or after GATE1 rebases) pass on rv64
     and rv32 at seed 3, with the audits in.
   - Record the gross and net numbers and the audit totals on scheduling.md.
   - Drop the residual bullet, delete the todo page and its SUMMARY entry.

## Gates

`cargo testbench`, the full bench. It carries:
- rv32;
- the unsafe ratchet;
- the size budget: the release kernel's size must not change, since the records are `sched-trace`
  only;
- `cargo fmt --check`;
- `cargo run -q -p redoubt-doccheck`, clean.

## Early checkpoint

After deliverable 2's host tests pass, report (member_update, at most 2000 bytes):
- the record kinds;
- the window list;
- how the deadline notice's window reaches the oracle.

Wait for the go-ahead before running the gate.
