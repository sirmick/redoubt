# SMP4 status, 2026-10-08 (smp4-implementer)

Branch `wp-SMP4` in `.worktrees/SMP4`, on main 3206c43b4 (not yet on IRQ1). Commits (WIP ones fold
before review):

1. `kernel, testbench: a traced kernel records each section of its lock, and the oracle says what a
   lock wait waited behind` — `hold-trace` (records `h`, `j`), the oracle's sections line, both
   contention cases carry it. Q4.
2. `kernel: a frame is zeroed in one checked store loop, not a check a word` — P0. Zeroing at two
   harts: sched-budget-churn 354 → 64 ms (rv64), sched-exit-churn 616 → 122 ms (rv32); `map_anon`'s
   median section 2.1 → 0.9 ms. Unsafe count unchanged (the one block `write` had).
3. `kernel: every scan of a whole table a checked build makes is an audit` — P1: the frame owners'
   RAM scan and the live PIDs at a process's end, the live PIDs at an account's making, the IRQ index
   at its change; inside a destruction they defer to the destruction's audit, which now also checks
   the live PIDs. Audit id 5.
4. WIP x3: P2, a hart's wait behind another hart's audit is billed to nobody (checked builds only):
   each hart marks its wait (raw time it came) before drawing; an audit's end gives each marked hart
   `min(audit, time since it came)`; the waiter, having waited (`held`), takes it off the user time
   it closes and moves its slice's end by it, as the auditor does; a `y` record after the `Q` (or its
   `k`); the oracle subtracts only the billed part of a wait and checks `y` follows a wait of its hart
   and does not exceed it. Feature `audit-wait-billed` for the negative (not yet run). Two bugs found
   by containment and fixed: an excuse with no wait (the lock was free by the draw), and harts' clocks
   differing by a few ticks under `icount` (excuse clamped to the waiter's own clock).
5. `kernel, testbench: a program's fault prints one line, not its address space's map` — P3, a real
   bug in every build: a user fault printed the registers and the map, a line per mapped page, holding
   the lock. A 4096-page process's fault held it 837 ms (rv64) / 865 ms (rv32); now 7.5 ms (its
   teardown). New case `fault-report-bound` (oracle `fault_section_max_ticks`, forbids the map and
   register lines), both widths pass; the old report fails it (recorded negative).

## The five kept lines at two harts (main base, P0-P3; IRQ1 not in)

| Line | Before (SMP2) | Now | Verdict |
| --- | --- | --- | --- |
| sched-exit-churn threads-exit (`@1`) | rv32 448, rv64 482 | rv64 482, rv32 480 | **un-keepable**: drop `@1` |
| sched-budget-churn-shell | 326 / 406 | rv64 404, rv32 509 (p012c); negative `audit-wait-billed`: 329 / 339 | rv32 passes, rv64 misses |
| deadline-flood-billed-traced | 407..884 | rv64 413/404, rv32 513/515 (p012c) | rv64 misses |
| sched-wake-no-preempt | no proof | no proof, both widths | not lock waits any more (below) |
| kernel-containment bystander (`@1`) | rv32 445 | rv64 464, rv32 442 (p012c); every other verdict passes at 2 harts | rv32 misses by 8 |

Of the victims' waits, 85-89 % are now excused (behind other harts' audits): sched-budget-churn-shell
rv64 waits 1849 ms, 1597 excused. Sections net of audits are at most ~1.9 ms in these cases.

### Why sched-budget-churn-shell rv64 still misses (trace)

- During the shell's 96 destruction audits (20 ms each), the victim's hart waits (excused): the stall
  is symmetric, not the cause.
- Outside the audits the shell holds both harts about 60 % of the time (hart 0: shell 306 ms, victim
  148; hart 1: shell 271, victim 182), and the gross charges differ (victim 2.97M, shell 5.41M
  ticks). So the picks favour the shell: the victim is lifted out of the cap set (`u`, a lift to the
  floor, no charge) 80 times on rv64 against 45 on rv32, while the shell is reweighed 240 times and
  lifted 80 (its child create/destroy cycle). That is the cap set and the lift rules at two harts
  under a shell that carves and destroys continuously, not lock waits. I have not proved the
  mechanism; it is where the trace points.

### Why sched-wake-no-preempt fails at two harts

The sleeper's own blocking section (`receive`, ~0.5 ms in a checked build) outlasts its 300 µs nap,
and at two harts the nap's timeout is answered at whichever entry comes first after it is due: 11 of
20 at entries that are not a timer interrupt (another hart's system call or slice end), 7 at a slice
end (R in the same interval), 2 mid-slice. The oracle's proof needs all 20 taken mid-slice by a timer
interrupt. A 700 µs nap still gives 2 of 20. No lock wait is in any of those intervals now.

## Not yet done

- Before/after table on IRQ1 (wp-IRQ1 has commits; my branch conflicts with it in the oracle's kind
  list, the contention tomls, size-budget and scheduling.md; IRQ1 uses `x`/`c`, I moved to `y`).
- The `audit-wait-billed` negative run; docs (scheduling.md keep table, the blind-spot sentence,
  R78/residuals, testbench.md checked builds), the un-keeps themselves, the fold.

## Question C's experiment: shares judged gross of the billed waits (computed from the oracle lines)

Gross = (the budget's ticks + its billed waits) / (all ticks + all billed waits), audits excused as P2.

| Case, line | rv64 net / gross | rv32 net / gross |
| --- | --- | --- |
| sched-timer-flood sleepers / +deadlines / cancelled | 586/500, 583/498, 522/499 | 532/499, 527/498, 554/499 |
| sched-share a / b / c | 245/250, 244/250, 509/498 | 238/242, 238/242, 523/514 |
| sched-budget-churn blocking / spinning / deadline / fresh | 639/579, 581/531, 995/994, 969/957 | 644/589, 612/538, 995/993, 965/948 |
| sched-exit-churn threads / processes / faults | 505/459, 538/516, 509/503 | 478/458, 571/505, 568/505 |
| sched-budget-churn-shell | 404/367 | 509/461 |
| deadline-flood-billed-traced 16 / 64 | 413/347, 404/342 | 513/370, 515/367 |

Gross sits nearer water-filling wherever the waits are short (timer-flood, share): net over-credits
the waiter. Every line that passes net passes gross but deadline-flood rv32; budget-churn-shell rv64
and deadline-flood miss either way.
