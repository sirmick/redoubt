# K17: a destruction feeds no receiver it is about to kill

Tier A (kernel), size S. Needs K13 (merged, e48232cb4): it wrote `kernel/src/message.rs`, a
hotspot with one writer at a time. K16 needs this package, so nothing else writes `message.rs`
until you are done. If INIT1 merges while you work, rebase and write the case in the form main
then has (a tester in `init`'s place).

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from `/home/mcloonan/redoubt/.worktrees/k17`.

## What it closes

- `docs/todo/destruction-feeds-the-doomed.md`. Its "Done when" is the contract. Delete the page
  when you are done.
- The ruling it implements: QA `K13-doomed-takes-call` (`.wash/qa/K13-doomed-takes-call.md`).

## The rule

**A thread of a process whose budget is dying takes nothing from a pump**: no abandoned-call
notice, no exit notice, no message. What the pump skips stays pending for the next receiver
outside the destruction.

- `budget_destroy` marks the whole subtree dying (`MemoryManager::mark_dying`) before
  `destroy_subtree` kills the first process, and `MemoryManager::process_is_doomed` already
  answers the question. Use it; add no new mark.
- One check, where `pump` picks a receiver. `pump` has three `find_thread` closures today (the
  abandoned-call notice, the exit notice, the message), each testing
  `s.wait == Wait::Receive && s.endpoint == Some(e)`. Make that test one private helper that
  also refuses a doomed process, and use it in all three. Test the wait and the endpoint first,
  so the budget is read only for a thread that is actually receiving on `e`.
- No gather across the destruction, and no deferred pump at its end. A skipped item needs none:
  a live receiver already waiting is served by the same pump, and one that arrives later is
  served by its own `receive`. The set a gather would need is unbounded (up to
  `MAX_PROCESS_COUNT` x `MAX_THREADS`).
- The outcome matches the model, which settles each endpoint once after the whole operation
  (`settle`, `to_pump`), when the doomed receivers are already gone. The order in which live
  receivers are served is not a rule (QA `K12-pump-once`). **Do not edit the model.** If it
  disagrees with the kernel, report that as a finding.
- An abandoned-call notice owed to a doomed holder stays flagged until the holder's process
  ends and its open calls are freed. Check that nothing on that path asserts the flag is clear
  (`end_thread`, the checked-build audits). If something does, stop and report it at the
  checkpoint.

## Reading list, in order

1. One example of the work: K13's commits `3d748c856` (the case first) and `74a89e999` (the
   kernel change with its page delta). Then `tests/ending-pumps-once.toml` and its program, the
   closest case. It knows R is receiving from a `READY` message plus a short `wait_ms`. Order
   yours on events instead, not on the clock (B4 removed cases that raced the host's clock). A
   wake never preempts (`docs/kernel/scheduling.md`, `bench:sched-wake-no-preempt`), so a child
   that sends its signal and then calls `receive` is blocked in `receive` before the tester,
   woken by the signal, runs again. Say at the checkpoint whether that held on both widths.
2. The todo page and the QA thread above.
3. `docs/kernel/ipc.md`: R4b, "What `receive` returns", and "Residual risks" (the last bullet,
   "A destruction can feed a receiver it is about to kill").
4. `docs/kernel/processes.md`, "Exit notices": the "Delivered" bullet.
5. Code:
   - `kernel/src/message.rs`: `pump` (~929) and `find_thread`; `process_ending` (~1525) and
     `end_thread` (~1488), as K13 left them.
   - `kernel/src/budget.rs`: `mark_dying` (~896), `process_is_doomed` (~1032), and
     `destroy_subtree` (~1198), which kills the doomed PIDs in PID order and the caller last.
   - `kernel/src/process.rs`: `killed`, `pending_notice`.
6. For comparison only: `model/src/kernel.rs`, `settle` (~1369), `end_process` (~1945) and
   `destroy_budget` (~2104).

## Owned paths

- `kernel/src/message.rs`: `pump` and one new private helper.
- New: `tests/destroy-keeps-notices.toml` and
  `tests/programs/src/bin/destroy-keeps-notices.rs`, plus the test-programs manifest entry if
  bins are listed there.
- Docs:
  - `docs/kernel/ipc.md`: the rule beside R4b (one or two sentences in its body), R4b's status
    line, and the residual bullet removed;
  - `docs/kernel/processes.md`: "Exit notices", the "Delivered" bullet says a thread of a
    process in a budget being destroyed takes no notice;
  - `docs/SECURITY.md`: the R4b row's tests;
  - `docs/SUMMARY.md`: the todo entry removed;
  - the todo page, deleted.

Anything else is a question to the orchestrator first.

## Deliverables

1. **The bug, shown first: `destroy-keeps-notices`.** It runs on rv64 and rv32, as a checked
   build (`debug_assertions = true`).
   - The tester makes a budget B and an exit endpoint E in its own budget, outside B.
   - It creates two processes, P1 and P2, in B. Each is its child, so both exit notices go to E,
     and each gets E's handle.
   - Both block in `receive` on E. Each signals the tester first and then calls `receive`, so
     the wake rule puts both in `receive` before the tester runs again.
   - The tester destroys B. On its way out, the first child to end has its notice pumped to the
     other, which is still receiving.
   - The tester then receives on E twice, the second with a timeout. It must get two notices,
     one for each child's PID, both with cause `killed`. Today it gets one, and the second
     receive times out.
   - The verdict is the kernel's: the PIDs `process_create` returned and the notices `receive`
     returns. Print each as an `ok:` line, as `ending-pumps-once` does.

   Commit the case alone first, and record its failure on main in the commit message.
2. **The fix**, per the rule above.
3. **Pages**, in the same commit as the fix: the rule beside R4b, processes.md's delivery,
   `bench:destroy-keeps-notices` on R4b's status line and the SECURITY.md row, the residual
   bullet and the todo page and its SUMMARY entry gone.

No new mutation is required: the model already never feeds a doomed receiver.

## Cases to run (both widths)

- `destroy-keeps-notices`: new.
- R4b and notices: `ending-pumps-once`, `redoubt-dead`, `process-lifecycle`, `budget-deadline`,
  `pid-pinning-attack`, `pid-reuse-authority`, `receive-bad-record`.
- R10: `budget-destroy-kills`, `budget-destroy-attack`, `endpoint-destroy-full`,
  `endpoint-destroy-open-calls`, `budget-destroy-growth`, `sched-latency` (seed 3). If GATE1
  has merged by then, also `kernel-containment`.

## Acceptance

1. Each case above by name, on both widths.
2. `cargo testbench`: the whole bench, which carries rv32, the unsafe ratchet, the size budget
   and the docs checker.
3. R10's numbers do not move. `endpoint-destroy-full`, `budget-destroy-growth` and
   `sched-latency` at seed 3, and the gate if it is on main, stay within noise of what K13
   recorded on budgets.md ("Residual risks", the measured list). The new check is one budget
   read per receiving thread a pump considers; report the numbers either way.

## Early checkpoint

Stop and report (member_update, at most 2000 bytes) once the case is committed and fails on
main on both widths. Include:
- the failing line, and what the tester received;
- which child took the other's notice, if the kernel's lines show it;
- whether anything on the doomed holder's path asserts on a flagged notice.

Wait for the go-ahead before you change the kernel.
