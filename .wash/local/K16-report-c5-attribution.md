# K16 c5: attribution of the gate regression (rv64, kernel-containment, qemu seed 13)

## The knob
| run | values | share (floor 783) | budget_destroy call-to-return p50 | driver wake p50 |
| --- | --- | --- | --- | --- |
| full | all new, 512 PIDs | 752 FAIL | 62,891 us | 9,652 |
| (a) | all new, MAX_PROCESS_COUNT 64 | 821 PASS | 48,350 | 8,717 |
| (b) | 512 PIDs, old threads/calls/caps/labels/depth | 758 FAIL | 60,896 | 9,899 |

The regression is the PID count alone. Logs: .k16/attr-{a,b,c}.log and -console.log.

## (c): walks per kernel entry, by function (instrumented build, all new values)
Temporary counters, now reverted (.k16/walkstat.patch). The bench counted 1,125,051 entries and
83,964 picks. "Slots" are PID slots visited, empty ones included.

| walk | calls | slots per call | per entry (avg) | max in one entry |
| --- | --- | --- | --- | --- |
| stride Queue scans (contains, queued, is_empty, take_out; reconcile) | - | 512 each | **3,789** | **17,408** |
| sched.rs:167 Runnable::fill | 1 per entry | 512 | **512** | 512 |
| sched.rs:388 next_thread | 1 per pick | 512 | 38 | 512 |
| message.rs:437 find_thread (R2 sender/receiver picks, fail_all) | 33,794 | ~508 | 15 | 5,383 |
| message.rs:1723 next_timeout | 21,697 | 512 | 9 | 2,048 |
| process.rs:302/309 random_free_pid | 113 each | 512 + nth | ~0 | 512 |
| budget.rs:1249 destroy_subtree victims; message.rs:1600 budgets_dying; handle.rs:452 check_handle_chains (checked builds only) | 18 each | 512 | ~0 | 512 |

- Per pick, the queue scan is all N = 512 slots (`pick` = `queued().min_by_key`). Reconcile adds
  one 512-slot `contains` scan per queued budget (the slot loop), plus one per runnable budget per
  insert pass. That is about 7.4 full queue scans per entry on average and 34 at the worst entry.
- So the per-entry cost is the dense-queue item (2) first and Runnable::fill (1) second. The
  find_thread and next_timeout walks matter only on IPC- and timer-heavy entries.

## budget_destroy split (instrumented; timed with the trap timer, 10 ticks/us)
Each of the 9 destroys took 50.9-51.4 ms in the handler:
- **per-PID sweeps: 19.2-19.7 ms.** These are destroy_subtree's victim loop (with its kills),
  budgets_dying's fail loop, sweep_handles and every find_thread under them. The figure includes
  the per-process work those loops do, not only the empty-slot passes.
- **the rest: 31.6 ms**, the same in every destroy. It is not a PID-slot walk. Under (a) at 64
  PIDs, call-to-return was still 48.4 ms, so a 30+ ms remainder is independent of the PID count.
  It is unattributed here (destroy_marked's object-frame pass over ~17.5k frames is the likely
  candidate). Before c5, the R10 oracle measured p50/p99 17.1/20.2 ms, a different measure.
- The instrumented call-to-return was 65.9 ms (the counters cost ~3 ms), and the share was 735.

## Next
Item 1 (the live-PID set) is starting. Item 2 (the dense stride queue) is likely the larger win
by these counts. I propose doing 2 before re-measuring, then 3 only if fill still dominates.
