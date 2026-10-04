# K13: an ending process pumps each endpoint once

Tier A (kernel), size S. Needs K12 (merged, 5986f0dd7) and IPC2: both write
`kernel/src/message.rs`, a hotspot with one writer at a time (.wash/SWARM.md, "The pages move
with the code"). Rebase on main after IPC2 merges, before you start.

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/dev.sh bash -c 'cd /work/.worktrees/k13 && <command>'`.

## What it closes

- `docs/todo/process-ending-pumps-once.md`. Its "Done when" is the contract. Delete the page when
  you're done.
- The ruling it implements: QA `K12-pump-once` (`.wash/qa/K12-pump-once.md`). Notice order is not
  a rule. The pump may run once per endpoint, after all of the process's threads have ended.

## Governing rules

- [R4b (a server dies)](../../docs/kernel/ipc.md#r4b-a-server-dies): queued senders keep waiting
  for the restarted server. This is the rule the code breaks today.
- [R3 (lends and abandoned calls)](../../docs/kernel/ipc.md#r3-lends-and-abandoned-calls) and
  [I15 (abandoned calls reported once)](../../docs/kernel/invariants.md#i15-abandoned-calls-reported-once):
  each notice goes exactly once, to the thread holding the call, in no promised order.
- R4a (in R4): a process at `MAX_OPEN_CALLS` takes no calls. This is the lever the bug case
  uses.
- [R10 (destruction)](../../docs/kernel/budgets.md#r10-destruction): the 30 ms target, and the
  budgets.md residual-risks line "the threads' teardown, 8.0 ms (8 ms)".

## Reading list, in order

1. One example of the work: commit `c9e8db6fc`. It is a message.rs teardown change that lands
   with its page delta. Then `tests/redoubt-dead.toml` and
   `tests/programs/src/bin/redoubt-dead.rs`, the existing R4b case. Model your case on it.
2. The todo page and the QA thread above.
3. `docs/kernel/ipc.md`: R3, R4, R4b, "What `receive` returns", and "Residual risks" (the
   bullet "An ending process's own threads can take a message").
4. `docs/kernel/budgets.md`, "Residual risks", item 4 and the measured list at the end.
5. `docs/testbench.md`: cases, checked builds, rule F.
6. Code in `kernel/src/message.rs`:
   - `process_ending` (~1503) and `thread_ending` (~1465), with `unwind` (it returns the served
     endpoint) and `finish_served`;
   - `pump` (~922) and `find_thread` (~426).

   Callers are in `kernel/src/ptable.rs`: `thread_ending` at ~743 (a lone thread's exit), and
   `process_ending` at ~779 and ~798.
7. For comparison only: `model/src/kernel.rs`, where `end_process` (~1919) and `settle` (~1366)
   already batch through `to_pump`. The kernel is coming into line with the model. **Do not edit
   the model.** If it disagrees with the kernel, report that as a finding.

## Owned paths

- `kernel/src/message.rs`: only `process_ending`, `thread_ending` and one new private helper
  they share.
- New: `tests/ending-pumps-once.toml` and `tests/programs/src/bin/ending-pumps-once.rs`, plus the
  test-programs manifest entry if bins are listed there.
- Docs:
  - `docs/kernel/ipc.md`: the R4b status line and the Residual-risks bullet;
  - `docs/kernel/budgets.md`: the measured numbers;
  - `docs/kernel/scheduling.md`, only where it quotes the 8 ms or the totals;
  - `docs/SECURITY.md`, the R4b row;
  - `docs/SUMMARY.md`, to drop the todo entry;
  - the todo page, deleted.

Anything else is a question to the orchestrator first.

## Deliverables

1. **The bug, shown first: `ending-pumps-once`.** It must run on rv64 and rv32, as a checked
   build (`debug_assertions = true`).

   The setup:
   - Process P has earlier threads holding `MAX_OPEN_CALLS` calls taken on endpoint E, so R4a
     keeps it from taking more.
   - A thread of P waits for a reply through E.
   - A later thread of P (a higher tid) is in `receive` on E.
   - Another process Q's call is queued on E.

   Kill P. Its holders' calls close, P drops under `MAX_OPEN_CALLS`, and today the served-endpoint
   pump hands Q's call to P's later thread, which ends holding it.

   The case must show:
   - Q is still waiting after the kill, not `Dead`;
   - a new receiver on E (a restarted server) then takes Q's call, and Q gets that server's reply;
   - each of P's abandoned calls gets exactly one notice.

   Commit the case alone first, and record its failure on main in the commit message.
2. **The fix.**
   - `process_ending` ends every thread with no pump in between. It gathers each thread's served
     endpoint into a fixed `[Option<EndpointRef>; MAX_THREADS]`, each endpoint once (no heap).
   - Only after the last thread has ended does it pump each gathered endpoint once.
   - `thread_ending` for a lone thread still pumps at once.
   - Share the body between the two through one helper that returns the served endpoint.
   - Check that nothing else on `process_ending`'s path pumps (`unwind`'s withdraw cases,
     `finish_served`, `wake`). If something does, stop and report it at the checkpoint.
3. **Remeasure** `process_ending` at the containment gate's full fill (see the R10 / full-fill
   cases below), on both widths: the median of nine destructions, with and without the other
   lease live, the way K12 split the phases with its T-record bisect.

   If that instrument is not in the tree, ask the orchestrator for it. Do not commit a new
   trace feature to get it.

   Replace budgets.md's "threads' teardown" line and the totals with the measured numbers. The
   page's own words say what the cost now follows: one pump per served endpoint, not one per
   parked call.
4. **Pages.**
   - ipc.md: drop the residual bullet, and add `bench:ending-pumps-once` to R4b's status line.
   - SECURITY.md agrees with that.
   - Delete the todo page and its SUMMARY entry.

No new mutation is required. The model already batches, and it belongs to IPC2's writer until
IPC2 merges.

## Cases to run (both widths)

- `ending-pumps-once`: new.
- R4b: `redoubt-dead`, `process-lifecycle`.
- R3/I15 notices: `cargo testbench --list` and grep for "abandon" and "notice"; run every hit.
- R10 / full fill: `endpoint-destroy-full`, `budget-destroy-growth`, `sched-latency`.

## Acceptance

1. `cargo testbench ending-pumps-once`, `cargo testbench redoubt-dead` and so on, each case
   above by name, on both widths.
2. `cargo testbench`: the full bench, which carries rv32, the unsafe ratchet, the size budget and
   the docs checker (docs/testbench.md).
3. The three R10 cases pass at seed 3 on both widths, and none regresses against K12's numbers on
   budgets.md.

## Early checkpoint

Stop and report (member_update, at most 2000 bytes) once the new case is committed and fails on
main on both widths. Include:
- the failing line;
- what Q got;
- your list of the pumps reached inside `process_ending`.

Wait for the go-ahead before you change the kernel.
