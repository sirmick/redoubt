# STEWARD10 report: a destroy by handle only while the deadline the steward minted is ahead

Branch `wp-STEWARD10`, worktree `.worktrees/STEWARD10`, on origin/main `41090437b` (train 73).
Head: `f22eabed4`, one commit, 6 files, +234/-58 (its message states the cause and carries the `Size budget:` line). Reviewer steward-red-5. Ruling: rulings 'STEWARD10' (thread STEWARD8-bound);
the shape as built and why it departs from the ruled mechanism:
`.worktrees/STEWARD10/.tmp/STEWARD10/shape.md`; accepted by the Architect on the thread (the
deadline as the identity witness; the brief amended to it), with two refinements answered there:
only the at-or-past-deadline guard is safety, and the before-deadline `BadHandle` branch stays a
failure because every budget the steward records hangs under a fixed carve (`Parent::Sub`,
`Parent::Users`), never under another record, and every destroy the core asks for is a
`DestroyBudget` step or a `forget`, both removing the record inline: no legitimate path leaves
a record past its budget. Logs under `.worktrees/STEWARD10/.tmp/STEWARD10/` (`gate-fmt-docs.log`,
`gate-fmt2.log`, `gate-set.log`, `check-*.log`) and `.worktrees/STEWARD10/target/jobs/`.

## The cause, as found

`drive.rs` `Step::DestroyBudget` destroyed the lease's budget by the handle the steward kept in
`made`, tolerating `BadHandle` when the owner's process was already gone. A lease's deadline
(R10) destroys the budget and revokes that handle at the kernel's first entry at or after the
deadline, freeing the steward's table index; the next handle the steward installs takes the
lowest free index (`install_handle`). Serially, each deadline lease's index was freed and the
destroy found it free (`BadHandle`, tolerated). With deadlines firing together (STEWARD9's
cluster), a newer object took the index before the exit notice was served: a non-budget gave
`WrongObject` (the loud form, a failed step and a forbidden line); a newer budget (a new lease,
the common case) would have been destroyed with no error (the silent form; `budget_at`'s I1 only
checks the slot's own consistency). The same destroy-by-handle ran in an `end_lease`'s end batch
served after the deadline but before the notice, and in `forget()` for a crossing's
`CROSSING_LIFE` budget and a failed batch's leftover, with the error ignored.

## Why the ruled mechanism does not map

- `DestroyBudget` is no step of STEWARD5's cleanup queue (that holds connections to give back);
  it runs inline in `step()`. Nothing to cancel under the queue lock.
- The steward never sees the kernel's `BudgetRef.id`: `Machine::budget_id` returns the handle
  index; no rt call reads a budget's id through a handle. Its identity for a budget is its own
  record plus the deadline it minted into the spec.

## The rule (steward-only, no kernel change)

`Made::Budget(handle, deadline)`. `Steward::destroy(kernel, b, deadline)`: read `kernel.now()`
right before the call; at or past the deadline make no call (the kernel took the budget at its
first entry at or after the deadline, `kernel/src/time.rs`: expiry runs first at every entry);
ahead of it, call `destroy`; a `BadHandle` with the clock re-read at or past the deadline is the
deadline passing inside the window (benign: the kernel's expiry ran at the call's entry, and the
index was still free); any other error, `WrongObject` included, is a failure. The
`BadHandle if !running` tolerance is gone. Used by the step and by `forget()`.

Race-free because: (1) nothing but a deadline revokes a budget handle the steward holds (its own
destroys drop the record as they go; the fixed carves are never destroyed); (2) the kernel never
takes a budget before its deadline; (3) only the loop initiates installs in the steward's table:
the cleanup thread's `disconnect` installs nothing, and a watcher's or reader's receive installs
only the handles the loop itself sent it, delivered inside the loop's blocking send; the clock
read and the destroy are the loop's two calls back to back, so no install can land between.

## Delivered (paths)

- `servers/steward/src/drive.rs`: `Made::Budget` carries its deadline; `Steward::destroy`; the
  step and `forget` use it; comments rewritten to state the rule.
- `servers/steward/src/bin/steward.rs`: the exit line carries the object's id
  (`steward: lease alice/agent-19 <id> of 1001/{} was killed 0`), the fold-in. STEWARD9's doubled
  `agent-19 was killed 0` lines: the lookup keys on the id already (`lease_name(object.id)`,
  lease numbers are monotonic per domain, `effects.rs route`); the page documents that a console
  write the console took but did not answer within the bound is written again, "so a line may
  appear twice", and the run's `start_agent` was served in 3.2 s under load, so that is the
  likely cause. No judged quantity reads the line: the oracle judges `LATENCY-SAMPLE`/`COUNT`
  lines and audit ids. With the id on the line, the pair is unambiguous whichever the cause.
- `tests/session-tcp-loopback.toml`: its `expect_after` regex for the exit line takes the id. A
  crossing's line is unchanged (`named()` gives its id as the name already): the first form of
  the change repeated the id there and timed out `steward-declassify`, `steward-confined-push`
  and `steward-push` on rv64 (30 min each; the rv32 three stopped by pid); fixed, rerun.
- `tests/size-budget.toml`: servers/steward 3540 -> 3549 (the guard, its doc comment, the
  display line), by the orchestrator's sum rule: the words stay, the number moves.
- `servers/steward/tests/steward.rs`: the Recorder gets a handle table for what the deadline
  frees (`deadline_takes`: the budget and its children leave the live set, their indices are
  freed, `make()` reuses the lowest; `destroy` answers as the kernel does: `BadHandle` for a free
  index, `WrongObject` for a non-budget, Ok for a budget, which leaves the live set), `lost` for
  a handle the steward lost before its deadline, `budgets` the live set (the verdict). Tests:
  - `a_leases_deadline_ends_it_without_a_failed_step` (reworked: the clock passes the deadline,
    the fake's R10 fires, the notice: no destroy call at all, no failed step, the sponsor told;
    a handle lost before its deadline fails the step);
  - `a_leases_index_recycled_for_a_new_budget_before_its_notice_is_not_destroyed_as_it`: the
    new lease's budget takes the freed index; after the old lease's notice the new budget is
    live in the fake kernel (the verdict), no destroy of it; its own `end_lease` destroys it;
  - `a_leases_index_recycled_for_a_connection_fails_no_step`: a session's `rebind` after a
    `reconnected` takes the index; no `WrongObject`, no destroy, the connection untouched;
  - `an_end_lease_after_the_deadline_makes_no_destroy_by_the_revoked_handle`: the end batch
    makes no destroy; the late notice gets its line and no event.
  All four fail on the old `drive.rs` (run with the fix's patch reverted) and pass with it.
- `docs/servers/steward.md`: "Leases" bullet on a deadline-taken lease rewritten (the rule, why
  it holds, what came before, the `end_lease` window, a lost handle) and its status line names
  the three new tests; "Failure and restart" gains a bullet (a budget the deadline took is
  destroyed once, by the kernel; the steward's `DestroyBudget` makes no call by that handle).

## Gates (exit codes)

- Host tests: `scripts/q run -- cargo test -p redoubt-steward-server`: 65 passed, rc 0.
- `make -f scripts/jobs.mk set CASES=formatting`: PASS (after `cargo +nightly fmt`), rc 0.
  `set CASES=docs`: PASS, rc 0.
- Release checks of the binary, `cargo check --release -p redoubt-steward-server --bin steward
  --target <triple> [--features latency-probe]`: rv64 and rv32, with and without the probe, rc 0.
- The steward set (every `steward-*` case but `steward-latency`, STEWARD9's slot), `size-budget`,
  `unsafe-budget`, `session-tcp-loopback` (its regex changed), both widths, `set CASES=... -k`:
  86 verdicts (`gate-set.log`): 75 PASS on the first run, with FAIL on `size-budget` (9 lines
  over, see below) and on the three crossing cases on rv64 (`steward-declassify`,
  `steward-confined-push`, `steward-push`, 1800 s timeouts: my first exit-line form repeated a
  crossing's id, which the cases' regex `steward: crossing [0-9]+ of` misses; their rv32 runs
  were stopped by pid, 7 verdicts short). Fixed (a crossing's line unchanged, the id only beside
  a lease's name) and rerun (`gate-set2.log`): formatting, docs, steward-host-tests,
  steward-declassify, steward-confined-push, steward-push, session-tcp-loopback,
  steward-lease-ends, steward-end-lease, both widths: 15 PASS; size-budget FAIL only for the
  raise not being committed yet. On the final commit (`gate-set3.log`): size-budget,
  unsafe-budget, docs PASS, rc 0. Every other steward case passed on the WIP commit
  `35b46262c`, which differs from the head only in the exit line's crossing form, the page's
  bullet wording and the ceiling.
- Not run: `steward-latency` (STEWARD9's slot, by the brief); the kernel is untouched.

## Documentation check (summaries)

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, the milestone pages, `servers/steward`'s
  own README (none): no claim on how a deadline-taken lease's destroy is made; no change.
- `docs/servers/steward.md`: the owning page, as above. R41's section: unchanged (it states the
  scope and the cutoff, not the handle rule). `docs/SECURITY.md` R10/R41 rows: cite no test this
  package changed or added; unchanged (the docs gate passes).
- `docs/userland/agents.md` "A lease's end is the kernel's": already states the kernel destroys
  at the first entry at or after the deadline and revokes every handle; now linked from the
  steward's page; no change.
- `docs/kernel/budgets.md` R10, `docs/kernel/objects.md` (lowest free index): unchanged, the facts
  the rule stands on.

## Open risks and notes

- The rule relies on (1)-(3) above being true of the steward binary; (3) is stated in the
  `destroy` doc comment. A future thread that receives handles from outside the loop's own sends
  would weaken it: the reviewer may want that stated in `bin/steward.rs`'s module doc too.
- A budget whose deadline passed is dropped from `made` without a call; if the kernel's expiry
  is late by a timer tick the budget lives until that tick, which R10 guarantees comes. No leak.
- The Recorder does not free an index on the steward's own destroys (the machine does), so the
  tests' handle numbers stay the order of making; stated in its field comment.
