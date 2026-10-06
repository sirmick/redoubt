# K19 implementer-2 handoff (second checkpoint: B18/B19 bench work)

## Branch state
- Worktree /home/mcloonan/redoubt/.worktrees/K19, branch wp-K19, head 0795e6b54 on 14aceaa63 (SMP1). NOT rebased onto fa08fe2c8 or later main. Worktree clean. No commits by me. Never pushed.
- All my runs are stopped: q jobs, QEMU, model suite.

## Assignment 938b6b293b968b4fda3904b91ca2d3a9: cause confirmed, fix outside K19
Detail: .wash/local/K19-report.md, the sections "worst-walk hang on 0795e6b54" and "Confirmed".
- The hang is a scheduler livelock that SMP1 introduced. kmain's pick sets slice_end = now + 1 ms. The exit path's non-audit work (reconcile, raise_floor over ~250 queued budgets, the `waiting` queue walk in leave) takes more than 1 ms of icount time. The picked thread is preempted at its first user instruction, and this repeats forever.
- main 14aceaa63 alone hangs the same way: nothing for 17 minutes after '250 holders waited'.
- K19's head with `slice-10ms` added to worst-walk's kernel_features PASSes rv64 (468.6 s). R10 p50/p99 16371/16383 µs.
- Both results were sent to the orchestrator as progress, with fix options (a) start the slice at the return to user, (b) guarantee forward progress, (c) O(changed) per-exit work. These need an R12 ruling and live in sched.rs/stride, not K19's files.

## Next
1. Wait for the orchestrator's ruling on the R12 livelock and on who fixes it.
2. Once it is fixed on main: rebase wp-K19 (`--signoff`; recount the size ceilings as in the first handoff's recipe). Rerun worst-walk on both widths through jobs.mk / q, and the full model suite: `q run --cores 8 -- cargo test -p redoubt-model --release`. It was interrupted inside mutations_are_caught; every binary before it was ok.
3. Report with member_update assignment_results.

## Traps
- Since the resume, q lives at /home/mcloonan/redoubt/scripts/q and jobs.mk at /home/mcloonan/redoubt/scripts/jobs.mk. The resume note may change this again after B18/B19.
- Do not pkill by a pattern matching your own shell's command line: that killed my session's shell once.
- Exports /tmp/k19-main (main 14aceaa63) and /tmp/k19-exp (head + slice-10ms) are scratch; delete them when done. Scripts are /tmp/k19-{main,exp,modelq}.sh.
- gdb debugging: add `-gdb tcp::PORT` to QEMU by hand. Use interrupt-only batch gdb scripts; killing gdb mid-continue wedges the stub.
