# SCHED1: per-switch cost, read from the code (host-only estimate, 2026-10-06)

These are estimates from reading the path, not measurements. Under icount shift 3, 1 µs is 125
instructions, so 0.35 ms is about 43,750 instructions per switch.

## The slice-end path, with nothing due

1. **The timer trap.** `from_user` runs. `expire_due` returns early, because both hints are in the
   future (`time.rs:133`), so there is no expiry walk.
2. **The slice ends.** `begin_billing` and `slice_over`, then `preempt`.
3. **`leave(kmain)`.**
   - close billing;
   - settle the marks;
   - `cpu.switch`: fold the runtime and requeue;
   - reconcile, with the debug `check_visited`;
   - `audit_due`: the marks audit, stamped `U4`, about 27 µs and left out of this count;
   - `rearm`: an SBI `set_timer` ecall.
4. **`kmain` picks.**
   - `expire`, which returns early;
   - `pick()`: settle, reconcile again, `cpu.pick`;
   - `next_thread`, which walks every live PID with a `budget_of` lookup each (about 15-20 here);
   - `set_slice_end`, then `switch_to`, an S-mode ecall trap.
5. **`leave(pid)`.** Reconcile a third time, `audit_due`, `rearm` (a second SBI ecall), then the
   return to user.

## Estimates per part

| Part | Instructions |
| --- | --- |
| 3 traps, with full context save and restore | about 1k |
| 2 SBI ecalls into RustSBI (M-mode trap and handler) | about 2-4k |
| 3 reconciles with debug checks, over about 10 queued budgets | about 2k |
| `next_thread` over the live PIDs | about 1-3k |
| about 8 trace records, of 4 frame writes each | about 1k |
| overflow checks and debug asserts across the lot | roughly 1.5x the above |

The total is about 8-15k instructions, which is 65-120 µs. That explains a third of the 0.35 ms at
most. The rest is not explained by reading the path, so the evidence can't yet say whether the
cost is inherent, checked-build or trace overhead.

## ties: the judge's time before its first send

The judge was picked at about 377.8 ms, and its first send's IPC-list audit (`U3`) began at
379.17 ms. That is about 1.35 ms in which the trace records nothing. Each later turn shows about
1.2 ms the same way. From the code, the send's delivery path is small, so this gap is not explained
by reading either. It is consistent with the same unexplained per-switch cost, multiplied over the
trap, return and send path.

## The measurement that settles it

The traced five-cases boots give, for each slice, the `I` time, the `U4`/`V4` times and the next
`I` time. Those split the gap into entry-to-leave, audit, and the rest (pick, switch, return). One
checked boot with `walk-trace` gives the reconcile's own time.

## Every checked-build check on the slice-end path (code read 2026-10-06)

The audit is listed if it is stamped `U`/`V` (and so already outside the 0.35 ms), and costed by
its loop bounds otherwise.

| Check | Where | When | Loop bound | Stamped | Estimate |
| --- | --- | --- | --- | --- | --- |
| `arch::mem::audit::returning()` | `irq.rs:21`, `syscall.rs:23` | every return to user or `kmain` | none (reads the head of a 16-entry log) | no | about 20 instructions, x2 per switch |
| `Marks::check_visited` | `sched.rs:180`, in every reconcile | 3 reconciles per switch | the budgets visited this entry (1-3) | no | about 50-100 instructions each |
| `audit_marks` (`Marks::audit`) | `sched.rs:381`, `:412`, through `audit_due` | at most once a slice, after a reconcile that visited a budget, so about every switch at 1 ms | every live PID with a budget, and the queue | yes (`U4`) | measured about 27 µs in the ties trace; outside the 0.35 ms |
| `message::audit` (IPC lists, `check_lists`) | end of an entry that changed a list | only when `CHANGED`: not at a plain slice end, yes at every send or wake | every thread on the lists | yes (`U3`) | measured about 0.5 ms per send in the ties trace; outside |
| `check_live_pids` | `budget.rs:639`, `:657` | only at process start and end | all `MAX_PROCESS_COUNT` accounts | no | not on the slice-end path |
| `check_process_index`, `check_all` | `process.rs:247` | a process object's change or free | all object frames | yes (`U2`) | not on the slice-end path |
| `check_globals` | `arch/riscv/mem.rs` | kernel-half mapping changes only | 512 or 1024 root entries | no | not on the slice-end path |
| `debug_assert!`s and overflow checks | throughout | everywhere | none | no | inside each part's estimate above (about 1.5x) |
| the trace ring's bounds checks | `sched.rs::trace::record` | about 8 records per switch | none | no | about 50-100 instructions per record, inside "trace records" |

None of the unstamped checks on the slice-end path has a loop over a growing set. By their loop
bounds they add a few hundred instructions per switch, not tens of thousands. So the reading does
not put the unexplained part of the 0.35 ms (about 30k instructions) in a checked-build check. The
release boots (no checks, no trace, no audits) and the walk-trace boot (reconcile time) measure it.
