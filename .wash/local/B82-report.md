# B82: the cause, from the trace and two reproductions

Worktree /home/mcloonan/redoubt/.worktrees/B82, branch wp-B82 on main 2b0c897c9. No commit: the
cause is not a kernel bug, and the deliverable as written (a kernel fix and a two-hart kernel race
case) does not apply. Tree clean (`git status --short | wc -l` = 0).

## The cause

The lease's own kernel deadline passes while the steward is still launching its VM. The steward
computes a lease's deadline at the start of its lease batch (`libs/steward/src/effects.rs`,
`carve_lease`: `deadline = cx.now() + l.lease`, into `Step::CreateBudget`), and the batch's `Launch`
step (reading the 1 MB beamlet image from erofsd:system in batches, mapping it into the child,
then `process_start`) takes about 1.25 s of guest time at one hart and about 2.46 s at two (under
icount the virtual clock counts both harts' instructions). The latency case's kernel-deadline
leases are `start_agent(c, 2_000_000)`: 2 s. At two harts the deadline falls between
`process_create` and `process_start`; the next kernel entry (R10: "a budget whose deadline has
passed is destroyed before any operation that enters the kernel") destroys the lease's budget,
step 2 kills the child, which has no thread yet (`[!] Terminating process with PID 280`), and the
launcher's next `process_map`/`process_start` finds `!p.alive()` and answers `NotPermitted`
(kernel/src/process.rs:526/617). The kernel does exactly what budgets.md R10 and processes.md say.

### Evidence in the two-hart log (.tmp/B82/latency-rv64-smp2-notpermitted.log)

- Every probe lease's `AgentStarted { .. deadline }` is audited after its launch: `deadline − at`
  = 6 566 413, 6 556 350, 6 536 085, 6 538 324 µs for a 9 000 000 µs lease, so the launch took
  2.43–2.46 s at two harts. In the one-hart logs (.tmp/steward4-logs/latency-rv64-smp1-full.log)
  the same difference is 7 763 769, 7 760 642, 7 729 823: a 1.24–1.27 s launch.
- Lease 17 is the first `start_agent(c, 2_000_000)` of round 1 (tests/steward-latency.toml line
  90: `for _ <- 1..d, do: (start.(9_000_000) ...` then `for _ <- 1..d, do: (start.(2_000_000)`),
  so 16 probe leases pass and the 17th dies in its launch, both times: 2.46 s > 2 s, deterministic.
- `[!] Terminating process with PID 280` has no earlier line for PID 280 (no PID reuse), no
  `shootdown:` line (no other hart ran it), and the launcher's cleanup (`process.close()`,
  handle.rs:373) prints nothing: the line is the kill, not the clean-up.

### Reproductions (rv64; .tmp/B82/b82-repro.toml, a temporary case, not committed)

Alice's session alternates a 600-s lease ended by `end_lease` and a short lease the kernel ends,
8 times, at 1 and 2 harts, with two temporary kernel diagnostics (reverted): a line at
`destroy_subtree` (top budget id, deadline or call, caller, hart) and one at `end_process`
(victim's thread count and budget).
- Short lease 2 s: PASS at smp=1 (26.8 s) and smp=2 (27.5 s): in this image a launch takes
  0.16 s (`deadline − at` = 1 846 906 for 2 000 000), so 2 s is ample. Log
  .tmp/B82/repro-2s-rv64-smp2-pass.log.
- Short lease 100 ms (below the 0.16 s launch): FAIL at smp=1 and smp=2 with exactly the
  latency case's failure. .tmp/B82/repro-100ms-rv64-smp1-notpermitted.log lines 74–77:
  `[B82] destroy top id=130 deadline=true caller=Some(340) hart=0` (the lease's own budget, by its
  deadline, in the steward's (PID 340) own kernel entry), `[B82] end_process pid=155 threads=0`
  (a never-started child), `[!] Terminating process with PID 155`, then `steward: a step failed
  (NotPermitted): Launch { .. }`.

### The node's three candidates

- A PID or slot reused while its termination is in flight on the other hart: ruled out. PID 280
  was never terminated before; the kill is by a destruction, and the only kill paths for a
  thread-less process are `destroy_subtree`'s (budget.rs:1390, process.rs free_charged).
- A destruction's subtree walk or boundary pumps reaching a sibling's process: ruled out. The
  dying top is the lease's own budget (`top id=130`, the budget the child counts in), and the
  failure needs no second hart at all.
- The stub's map/start racing a kill of its own child: this is it, but the killer is the lease's
  own deadline, run by the kernel at the steward's own entry, not a race between harts. Two harts
  only make the launch slower in guest time.

## What follows (not mine to decide; options)

1. The case (STEWARD4's): the kernel-deadline leases' term covers the launch at two harts with
   margin, e.g. 5 s, and the two-hart row is recorded. No design or kernel change. Recommended.
2. The steward's design: a lease's term runs from the agent's start (deadline set after
   `Launch`), which needs a kernel call to set or move a budget deadline (none exists), or the
   steward refuses a lease shorter than a launch floor. A design question for the architect on
   steward.md "Leases" ("Its budget has a deadline at most MAX_LEASE away"), and STEWARD4's code.
3. A kernel case pinning the behaviour (a deadline between `process_create` and `process_start`
   kills the child; map/start answer NotPermitted; the parent's usage is restored) at one hart:
   cheap, but pins what R10 already states; two harts add nothing.

Documentation: no kernel page changes. steward.md "Leases" could say the term is counted from the
request, the launch inside it (1.3 s at one hart, 2.5 s at two in the checked build): the
steward's page, STEWARD4's.

## Delivered (orchestrator's answer: option 3; option 1 is STEWARD4's, option 2 is thread B82-launch-race to architect-2, not blocking)

- `tests/deadline-in-launch.toml`, `tests/programs/src/bin/deadline-in-launch.rs` (registered in
  `tests/programs/Cargo.toml`): a lease of 300 ms carved from `system`; a child created in it and
  given its image (12 pages) inside the term; the term passes; the launcher's `process_map` and
  `process_start` answer `NotPermitted`; the lease's handle is `BadHandle`; the killed notice
  comes with cause `killed`, code 0, blaming nobody; a witness started in another budget while
  the notice is pending draws another PID (R20); after the notice and the witness's budget,
  the child's handle is gone and `system`'s usage equals what it was before the lease (I10).
  Verdicts are the kernel's answers, the notice and `budget_usage`, never the tester's own
  state (rule F). Console (rv64, 2 harts): `[!] Terminating process with PID 273` between the
  first and second `ok` lines, the kill of the thread-less child.
- `docs/kernel/budgets.md` "Deadlines": one sentence naming the window, and the case in the
  section's status list (5 → 6). No kernel code change; no SECURITY.md row (no new claim).

Gates (all through jobs.mk / q): `set CASES=deadline-in-launch`: PASS rv64 smp=1 0.9 s, smp=2
0.9 s; rv32 smp=1 0.6 s, smp=2 0.6 s. `docs` PASS 6.2 s. `rv64/formatting` PASS 25.3 s (after
`rustfmt +nightly` on the new file). The smp set, size-budget and unsafe-budget were not run: the
kernel does not move. Summaries checked: README.md, GETTING-STARTED.md, docs/kernel/README.md,
processes.md "Creating and starting" (its `NotPermitted` for "a process that has ended" already
covers the launcher's answer): no change needed, no status claim moves.
