# K16 handoff from k16-implementer-4 to k16-implementer-5

Worktree `.worktrees/k16`, branch `wp-k16` on main 5f9f9d61c. Scratch is `.k16/` (never staged).

## Commits
- Final: 3cc54449f (c1), 1d7f7c221 (churn ruling), 388e9b9aa (c4), 41ad6a29d (c2), a4d233181 (c3).
- af98ed064: WIP c5, the values and TidMask.
- 2622a8070: WIP c5 part 2: 1 MiB region + assert, SERVED static, cases, every brief page, model
  coverage, rt/ipd tests (body of the commit lists them).
- 73f4883d9: WIP item 1, live-PID set (kernel/src/bits.rs; PidMask; live_pids()).
- 10281b636: WIP item 2, dense stride queue.
All four WIPs are re-rolled (walk commits before c5, or into it).

## c5 still owes
- Rerun redoubt-ipc, process-lifecycle, budget, budget-syscall-attack (fixed, not rerun).
- size-budget: kernel 7,880 > 7,875 at 2622a8070 (recount after items 1-2). The commit needs a
  `Size budget: kernel:` line.
- Model suite run time (was 526 s).
- libs/rt/src/server/label.rs:30 says MAX_LABELS (8). Not granted; ask.
- Sizes (pre items 1-2): rv64 bss 468,424, headroom 580,120; rv32 460,152/588,392.
- rt/ipd grant: admit.rs and ipd sizing.rs tests in c5, body says why (they pinned 48); ipd's
  budget binds before the bound (accepted).

## Re-measure not run
I stopped re-measure (d) and killed its container: items 1+2 are unmeasured. Run the gate first.

## The destroy constant (likely answer, orchestrator accepts)
The 31.6 ms of budget_destroy that walks no PID is most likely the checked build's audit,
check_object_indexes (budget.rs, after R10_END): it is excluded from R10 by K18/B5 and was
included by my handler timer. Confirm it within the one instrumented run: time the audit and
R10_START..END separately.

## Next, in order (Architect)
1. Items 1+2 focused cases, both widths (gate, sched-latency, budget*, process*, ipc*, stride).
2. ONE instrumented run, rv64 seed 13, 512 PIDs: per destruction, every destruction-path loop's
   visited vs live and time, the audit included. Draft: .k16/dstat.rs.
3. Convert any table-size walk (TID/open-call/handle-table/WAIT_CAP/labels/start-handles) to
   live sets, listing visited/live before and after. Live content that grew is reported, not
   converted. R10 30 ms p99 is the stop-and-report line.
4. Item 3 (marked reconcile) only if Runnable::fill still shows. Its own commit, size reported first.
5. If the gate still fails after this: name the walk and its cost per slot (owner decides 128/256).
   Never lower the value to pass.
Report R10 trace time like with like: (a) 64 PIDs, new values: 20,843/25,794 us; pre-c5
17,113/20,210 (detail .k16/report-c5-attribution.md).

## K21 rebase (carried forward)
- mem.rs allocate: keep K21's add_context_page unwind around one header page, set_header after
  the map, make_satp(root_phys).
- release_ipc_frames before release_owned_frames (K21's name) in terminate and the unwinds.
- FreeList all zero, so the memory manager stays .bss.
- boot_budgets comment: "saved contexts" -> "header page".

## Notes
Never cat .wash/qa/K16-limits.md. Benches serially.

## What consumed my context
The attribution runs, the cases' failures, the page sweep.
