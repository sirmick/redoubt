SMP2 -> SMP4 handoff (smp2-implementer-3). SMP2 is merged to main as 19e3439b9. Nothing of mine is open. The detail is in .wash/local/SMP2-report.md and the pages: docs/kernel/scheduling.md ("Residual risks": "Fair kernel entry is bounded by count", "The console has two writers", "Some cases keep one hart" with its table); docs/testbench.md ("The scheduler oracle", "Checked builds"); docs/kernel/README.md#containment.

## What step 5 must un-keep (each is written on scheduling.md with its numbers)
keep_smp because lock waits decide the share (remove keep_smp, rerun at 2 harts both widths):
- sched-budget-churn-shell: the victim keeps 326 (rv64) / 406 (rv32) of 1000 at 2 harts; it waits 848 ms of 2 s. Want 500, `+`, tolerance 50.
- deadline-flood-billed-traced: the victim swings 407 to 884 at 2 harts.
- sched-wake-no-preempt: at 2 harts no nap ended mid-slice (the timeout's hart waited for the lock past its slice end; shortest delay 955 µs).
Lines judged at one hart only (`@1`; drop the `@1` to judge at 2):
- kernel-containment's bystander, `CHARGED-SHARE bystander ... 50@1 2:1 3`: rv32 445 on 12 of 16 seeds (the 543-546 mode passes); rv64 478-515. Its hart waits 27.5M ticks against 16.7M on the other hart. The program's call is `(TOLERANCE, "@1")` in tests/programs/src/bin/kernel-containment.rs; also edit the toml expect and its comment, plus README "The run" and the 2-hart paragraph.
- sched-exit-churn threads-exit (`50+@1`): rv32 448 at 2 harts (rv64 482); the victim's hart takes 4.6M of 8.4M ticks of waits.
Also step 5's to remove: the "blind spot" sentence in "Fair kernel entry". Shares are judged NET of lock waits: the oracle subtracts each Q wait from the budget the hart's H record names, so a wait billed to the wrong budget passes. When waits are short, judge shares gross (drop the subtraction in check_hart_share and judge_across_harts in tools/testbench/src/sched_oracle.rs, or add a gross check beside it) and delete that sentence.
Not lock-wait keeps (leave them): sched-large-weight (audit placement; rv32 572 at 2), the release twins (counts), sched-lift-delay, sched-carve-return, sched-cluster x2 (one queue's rounds), sched-latency-tcg (console, CONW1), bench-poweroff-missing.

## What to measure (before and after, at 2 harts, both widths)
- `lock waits N of 1000` on each sched_oracle summary line. At 2 harts the share cases sit at 84-210 (exit-churn 146/139, timer-flood 185/194, budget-churn 131/125, share 130/195), containment 229/280, sched-capped at 3/4 harts 300-400. Each HART-SHARE line also prints "its lock waits X of Y ticks".
- sched-lock-contention (2 harts) and -4 (4 harts): the driver wake is recorded, not gated. At 2 harts p50/p99 is 18.5/19.2 ms rv64 and 18.3/19.1 rv32; at 4 harts 4.3/59.3 and 7.1/86.9. Two 9.6 ms searches per wake: that part is IRQ1's (interrupts reach only the boot hart), not yours, but your lock-hold times move it. The oracle line "lock order: N waits ... waits in ticks p50/p99/max" gives the wait lengths.
- sched-latency at 4 harts (recorded): rv32 N=16 driver p50 17.2 ms against 15.

## lock-trace tooling
- Kernel feature `lock-trace` (implies sched-trace): after each contended Q record it writes `k` (id = ticket, pass = sections ahead at the draw), from cell.rs `TicketLock::acquire_ticket()` -> irq.rs -> sched.rs `trace::lock_wait(came, ticket, ahead)`.
- Oracle: `lock_order()` requires the k tickets to rise in trace order (the records are written holding the lock) and ahead < harts. It reports wait lengths. Its test is lock_waits_take_the_lock_in_ticket_order.
- Hold times are NOT recorded: Q gives each wait's start and end only. If you need section lengths, add a record at release (feature-gated, like k).
- `gate_harts=N` post_check argument: targets are judged at <= N harts and reported above. `@N` on a HART-SHARE / CHARGED-SHARE tolerance: judged only at N harts.
- Negative-case pattern (kernel red asked for it): sched-capped-holds-floor is a must_fail twin. The break is a stride feature plus a kernel feature, plus a boot line the case expects (no-cruft needs a cfg user in the kernel). The break's audit must read the same broken rule, or the checked kernel's audit catches it first.

## Traps
- Always run through scripts/q. q may grant fewer cores than asked: use `--jobs $(nproc)` inside `sh -c`, never a fixed --jobs.
- Never edit the worktree while a non-prebuilt cargo testbench is compiling. I ran cases from detached snapshot worktrees under .tmp (git worktree add --detach <commit>) and moved them only between builds. Each cargo testbench run builds in its own run dir.
- Console logs in target/testbench/run-* are pruned by later runs: copy them at once.
- origin/main moves under you (refs are shared): rebase -i onto origin/main may land on a newer main. Compare trees per file: `git diff <old> HEAD -- $(git diff --name-only origin/main HEAD)`.
- Size budget: every kernel/stride/model line change needs `Size budget: <crate>: a to b, reason` in the commit that raises it, and the ceilings chain commit to commit. Check every commit after any rebase.
- Under icount a count is the machine's instructions, not a hart's time: judge shares from charges only. The reconcile walk's max in worst-walk moves ~1 ms with unrelated code (phase); its bound is now a sweep's worst plus a tenth (10000 µs).
- Kernel console lines interleave with program UART lines on several harts (CONW1). Under icount it is rare; under plain TCG it is every run.
- The fold script approach (.tmp/SMP2/fold.py, strip later families from the final tree) worked for splitting WIP into families. Autosquash conflicts mostly hit status-count lines and the keep table's closing note.

What consumed my context: the gate runs and their sweeps, the rulings round-trips, two refolds and the repeated rebases onto a moving main.
