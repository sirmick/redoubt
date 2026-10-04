# K13 progress

## Early checkpoint (2026-10-01)

Branch wp-k13, base main 4d42be45f. Commit 59566c175: the bug case alone
(`tests/ending-pumps-once.toml`, `tests/programs/src/bin/ending-pumps-once.rs`, manifest entry in
`tests/programs/Cargo.toml`). No kernel change yet.

### The case

The judge (first program) makes E and spawns P (budget 800 pages) with E's receive right.
- P tid 1 receives on E until it has taken `MAX_OPEN_CALLS` (64) messages. The judge fills it
  with one thread: calls with a 2 ms timeout, so each call is taken, times out, is abandoned and
  stays open (P's notices for them are consumed by tid 1's receive loop). R4a then holds.
- The judge starts S, which receives on E; P tid 2 (C) calls E; S takes it and holds it.
- P tid 3 (R) receives on E.
- The judge's thread Q calls E: queued (R4a blocks R; no judge thread is receiving).
- `destroy(P's budget)`. Then: Q must still be waiting; a new judge thread receives on E and
  must take Q's call and reply; S then receives until a 50 ms timeout and must see exactly one
  `Abandoned(C's id)` and nothing else.

### Result on main, both widths

```
/home/mcloonan/redoubt/.wash/local/in-dev cargo testbench ending-pumps-once   -> exit 1
FAIL  ending-pumps-once [rv64, smp=1]  ... [ending-pumps-once] FAIL: Q is still waiting after the kill (got 2)
FAIL  ending-pumps-once [rv32, smp=1]  ... [ending-pumps-once] FAIL: Q is still waiting after the kill (got 2)
```
Got 2 = Q's call returned `Err(Dead)`. The earlier checks ("S holds C's call, and R is receiving
on E", "Q's call is queued before the kill") pass on both widths, so the setup is as designed.
The run stops at the first forbidden line, so the later two checks are not reached on main.

Also: `cargo +nightly fmt --all --check` clean, `cargo run -q -p redoubt-doccheck` clean (exit 0).

### Pumps reached inside process_ending (main)

`process_ending` -> `thread_ending` per thread. Inside:
- `unwind`: `Send` -> `give_buffer_back` (takes `&ProcessTable`, cannot pump); `Reply` ->
  `abandon` (`&ProcessTable`, no pump). Returns the served endpoint.
- `finish_served`: `return_lend` / `free_abandoned_lend` (`&ProcessTable`), `close_call`
  (no `ss`), `wake` (ready list + result only). None pumps.
- `thread_ending`'s own `pump(ss, mm, e)` on the served endpoint (message.rs:1491): **the only
  pump on the path.**
Outside `process_ending` but on the kill path: `kill_process` -> `destroy_quarantined_devices`,
and `process::settle_notice` -> `pump_endpoint` on the exit endpoint (after `process_ending`).
`fail_wait`'s pump (message.rs:554) is not reached from `process_ending`.

### Next (after go-ahead)

Fix in `thread_ending`/`process_ending` through one helper returning the served endpoint;
gather into `[Option<EndpointRef>; MAX_THREADS]`, pump each once after the last thread. Then the
case list on both widths, the remeasure, pages.

## k13-implementer-2 (2026-10-02)

Rebased onto main 0a82e2090 (case now d27d0d2ab). Branch wp-k13:
- d27d0d2ab testbench: the case (unchanged).
- 7410f41e6 kernel: an ending process pumps each endpoint once, after its threads. message.rs:
  new private `end_thread` (unwind, W_WAIT := None, drop_open_call + finish_served per open
  call; returns the served endpoint, no pump). `thread_ending` = end_thread + pump at once.
  `process_ending` ends every thread, gathers served endpoints into
  `[Option<EndpointRef>; MAX_THREADS]` each once, then pumps each once. Pages: ipc.md R4b status
  + residual bullet gone; SECURITY.md R4b row; SUMMARY.md entry; todo page deleted;
  plan/m1-separation.md follow-up bullet (orchestrator OK'd).
- 03a1c0b65 kernel: a traced build times a process's threads' ending inside a destruction.
  sched.rs trace: PHASE_BEGIN 'T' / PHASE_END 't', PHASE_THREADS = 1, guard `trace::phase()`
  (sched-trace only), around process_ending. sched_oracle.rs: admits T/t, pairs them (unpaired /
  nested fail), sums phase-1 time inside each X..Y, reports "their threads' ending p50/p99/max";
  unit test `a_destructions_threads_ending_is_timed_inside_it`. budgets.md numbers.

### Cases (both widths, exit 0 each)
ending-pumps-once, redoubt-dead, process-lifecycle, budget-deadline, redoubt-ipc (+attack),
redoubt-revoke, receive-bad-record, timeouts (+tcg), process-attack, pid-pinning-attack,
pid-reuse-authority, page-table-reclaim, lender-touches-lent, endpoint-destroy-full,
budget-destroy-growth, sched-latency (+tcg). `cargo test -p testbench sched_oracle`: 12 pass.

### Remeasure (containment gate, seed 3, medians of 9 destructions per slot)
Method: scratch worktrees (removed), gate files copied uncommitted from wp-gate1 at 5fdfeb298,
scratch-only marker (deadline destructions' X/Y id | 1<<40) to split H/D; script
.wash/local/K13-phases.py over target/testbench/kernel-containment-<arch>-smp1.log. Baseline =
same tree with the old process_ending plus the same T/t records.

| | R10 H | R10 D | threads H | threads D |
|---|---|---|---|---|
| rv32 before | 24548 | 21231 | 6713 | 5135 |
| rv32 after  | 20826 | 18694 | 2988 | 2594 |
| rv64 before | 24263 | 21242 | 6488 | 4977 |
| rv64 after  | 20672 | 18784 | 2897 | 2519 |
Gate PASS both widths, both kernels. K12's page said 23.6/20.5 rv32: main drifted ~+0.9 ms H
since K12 (base already 24.5); unattributed.

No regression vs the same baseline: endpoint-destroy-full R10 11070/11090 µs (base
11068/11089); budget-destroy-growth filled 1215/1388 µs (base 1214/1388); sched-latency seed 3
R10 p99 5562/5830 (base 5561/5828). (rv64/rv32.) Page's K12 figures are ~+90 µs, +4 µs, +100 µs
below these: drift on main, present in the baseline.

### Finding (from reading the code, no case run; not fixed, outside the brief): a destruction still pumps between processes
budget::destroy_subtree kills each doomed process in turn (process::killed -> process_ending),
and each process_ending pumps its endpoints before the next doomed process ends. `pump` does not
skip doomed processes. So if a dying budget holds P1 (waiting for a reply through an endpoint E
owned outside) and P2 (receiving on E), P1's end pumps E, P2 can take a queued call (or an exit
notice) and then be killed holding it: the caller gets Dead, against R4b. The model settles
once after the whole operation (`to_pump` / `settle`), so the model and kernel disagree here.
Options: gather across the destruction's kills and pump after the last; or have pump skip
doomed processes' receivers. Needs a case (two processes in one dying budget).

### Gates at tip 7052dd761 (branch wp-k13 on main 0a82e2090)
- `in-dev cargo testbench --allow-skip`: exit 0, 274 PASS, 1 SKIP (bench-ssh-loopback-openssh),
  both widths (rv32 builds included); size-budget (kernel 7718 of 7752, no raise), unsafe-budget
  (no unsafe added), formatting, docs all PASS.
- `in-dev cargo +nightly fmt --all --check`: exit 0. `in-dev cargo run -q -p redoubt-doccheck`: exit 0.
- First full run (before amend) had 2 FAILs: docs (C4: a commit hash on budgets.md; removed,
  the gate tip is in the commit message and here) and client-host-tests (redoubt-client
  `console` test aborted with a non-unwinding panic; host-only, no kernel code; passed 3/3 on
  rerun and in the second full run). Flaky, reported, not touched.
- Gate tip used for the remeasure: wp-gate1 at 5fdfeb298.
- Model: end_process ends every thread then settle() pumps once per operation (to_pump), so it
  agrees with the kernel for one process's end; it disagrees for a destruction of several
  processes (Finding above).

### Fix round 1 (tips 006a42c42 case, 48d3647be fix, 43f2f57bc trace; on main 0a82e2090)
- Simplifier 3: trace module now one span: THREADS_BEGIN 'T' / THREADS_END 't', `trace::threads()`
  guard, id 0; oracle pairs one begin/end (no id test), one test.
- Editor: ipc.md R4b status in details form (4); invariants.md I15 cites bench:ending-pumps-once
  (11), and SECURITY.md's I15 row to match (doccheck C7 required it); budgets.md baseline stated
  as "pumping after each thread", no history; scheduling.md names the threads' time in the
  oracle's destruction report (seed-3 p99 32 / 36 µs); ipc.md residual "A destruction's
  processes end one by one" as narrowed. Declined: the thread name K13-doomed-takes-call on the
  page (doccheck C4 bans package IDs); written "Open: the design is not yet ruled."
- Red: the case's comment notes C's tid < R's is the order that shows the bug, and that the
  threads' span covers the pumps.
- Checks: ending-pumps-once PASS rv64+rv32; sched-latency (+tcg) PASS both, R10 p99
  5562/5830 unchanged; oracle host tests 12 pass; fmt 0; doccheck 0; docs case PASS. Whole
  bench not rerun (kernel diff changed only in the trace module and its guard line).

### Fix round 1, changed (rebased onto main 94ab7073d)
Tips: f09b12526 case, eb39a7ae7 fix, c469f83e9 trace. My destruction-order residual removed
(the Architect's own residual and todo page are on main, 94ab7073d); ipc.md now only drops the
old residual and folds R4b's status line; the fix commit's message no longer mentions it.
Main's delta since 0a82e2090 is docs only (SUMMARY, ipc.md, the new todo page). Rechecked:
doccheck 0, fmt 0, docs case PASS, ending-pumps-once PASS rv64+rv32. sched-latency and the
oracle tests stand from the round (no code change since).
