# K16 items 1+2 (live-PID set, dense stride queue): the gate, rv64 seed 13, tip 10281b636

`TESTBENCH_QEMU_SEED=13 cargo testbench --arch rv64 kernel-containment`: PASS, exit 0, 378.4 s.
Logs: .k16/attr-e.log, .k16/attr-e-console.log.

| run | PIDs | share (floor 783) | budget_destroy call-to-return p50/p99 | R10 trace p50/p99 | decision_wake p50 | deadline_notice net p50/p99 |
| --- | --- | --- | --- | --- | --- | --- |
| pre-c5 | 64 | - | - | 17,113/20,210 | - | - |
| (a) new values | 64 | 821 PASS | 48,350/48,356 | 20,843/25,794 | 8,108 | 26,665/29,850 |
| full, before items | 512 | 752 FAIL | 62,891 | - | (42.6 ms) | - |
| items 1+2 (e) | 512 | **831 PASS** | 46,165/46,536 | **20,163/25,440** | 7,729 | 26,147/29,009 |

At 512 PIDs the walks now cost less than (a) at 64 PIDs. R10 p99 25.4 ms is under the 30 ms
stop-and-report line, and still 5.2 ms over pre-c5's 20.2 ms (like with like: same oracle).
The call-to-return gap (46 ms vs R10 20 ms) is still the constant to explain in step (2).

## Focused set, both widths, at 10281b636 (.k16/foc6-summary.txt, logs .k16/foc-<filter>.log)
`.k16/focused.sh sched-latency budget process ipc stride pid thread-limit sched-destroy-billing
sched-budget-churn` and `cargo testbench --arch rv32 kernel-containment` (PASS, exit 0, 117.9 s).
- PASS: sched-latency 4, process 12 (process-lifecycle included), stride-host-tests, pid 4,
  thread-limit 2, sched-destroy-billing 2, sched-budget-churn 2, budget 26 of 28 (budget and
  budget-syscall-attack included), gate rv32.
- FAIL, owed by c5: size-budget (kernel 7,912 > 7,875; the line goes in c5's body);
  unsafe-budget (kernel/src/bits.rs is in no budget: it holds no unsafe, so it is listed).
- FAIL, fixed: redoubt-ipc both widths, abandoned 213/218 of 256. The c5 WIP cut the client's
  attempts from 4x the bound (256 for 64) to 2x; with the filler's 128 and ~78 parked calls the
  Busy retries ran the loop out. Back to 4x (WIP 071146ee3 folded into c5): redoubt-ipc and
  redoubt-ipc-attack PASS both widths, exit 0, 256 abandoned, 256 notices.

## Step (2): the instrumented destroy run (rv64 seed 13, 512 PIDs, tip 071146ee3 + temporary kernel/src/dstat.rs)
Gate PASS exit 0 (instrumented). Per destruction, inclusive times; rows nest by number. visited/live are slots/items for walks counted. Raw: .k16/attr-f-console.log; table: .k16/dstat-table.txt.
```
destructions 18 handler total us p50/max 60521.0 81592
row                                      us p50   us max calls p50 visited p50  live p50   n
0 destroy_begin                             309      309         1           0         0   9
1 victims loop (live_pids)                10182    12889         1           8         2  18
1a killed                                 10022    12710         2           0         0  18
1b end_process                             7075     7720         2           0         0  18
1c settle_notice                           2875     4952         2           0         0  18
2 process::budgets_dying                    270      285         1           0         0  18
3 message::budgets_dying                   6742     7329         1           0         0  18
3a fail_all (endpoint waiters)              416      610         1          13         0  18
3b stamp/notice passes                      658     1037         0          13         0  18
3c endpoints_dying                          288      303         1           0         0  18
4 lift_dying                                294      344         1           0         0  18
5 destroy_marked                           4930     4938         1           0         0  18
5a close_dependents                         200      200         0           0         0  18
5b free_owned_endpoints                    4193     4193         1           0         0  18
5c free_deferred_frames                       0        1         1           0         0  18
5d dma_migrate_quarantine                    25       25         1           0         0  18
5e migrate_held_pids                        345      354         1         512         8  18
5f free_dying_budgets                        83       83         1           0         0  18
8a process_ending                          3604     4489         2           0         0  18
8b terminate                               3180     3182         2           0         0  18
8b0 println Terminating                     188      189         2           0         0  18
8b1 release_ipc_frames                       46       46         2           0         0  18
8b2 release_all_memory                     2508     2508         2           0         0  18
8b3 dma_release                              28       28         2           0         0  18
8b4 ArchProcess::destroy                      0        0         2           0         0  18
8c destroy_quarantined_devices                0        0         2           0         0  18
8d activate                                   1        1         2           0         0  18
8e process_ended                            388      389         2           0         0  18
8e1 close_all_handles                       164      164         2           0         0  18
8e2 CHECK check_live_pids                   131      131         2        1024         0  18
8e3 CHECK check_frame_owners                  0        0         2           0         0  18
9 AUDIT check_object_indexes              37891    55750         1           0         0  18
find_process (PID index)                    931     1062         6        2816        42  18
```

### The destroy constant, confirmed
The checked build's audit (`sched::audit(AUDIT_DESTRUCTION, check_object_indexes)`, after R10_END)
costs 37.9 ms p50, 55.8 ms max per destruction. Handler total minus the audit is ~22.6 ms, which
matches the R10 trace (20.2 ms p50). Within R10 the cost is live content: killed (two victims)
10.0 ms [end_process 7.1 (process_ending 3.6, release_all_memory 2.5), settle_notice 2.9],
message::budgets_dying 6.7 ms (most of it in the owner-chain device walk), and
free_owned_endpoints 4.2 ms.

## Step (3): table-size walks converted (WIP 3f4630826)
A PID set of process objects (`Objects::process_pids`) is kept in `index_process`, and the
checked build audits it against the index in `check_process_index`.

| walk | visited/live before | after | us p50 before/after (instrumented) |
| --- | --- | --- | --- |
| find_process (process::budgets_dying, endpoints_dying; 6 calls per destruction) | 2,816/42 | 42/42 | 931/771 |
| migrate_held_pids | 512/8 | 8/8 | 345/289 |
| holds_process (budget_create, not on R10) | 512 accounts | live PIDs | - |
The old migrate_held_pids also copied the 4 KiB PID index onto the kernel stack; it doesn't anymore.

These were not converted, and why:
- check_live_pids, check_process_index, check_handle_chains (`pids()`): checked-build audits, which
  scan by design (R12).
- random_free_pid (`pids()` twice per process_create, 113 calls in the gate): it draws from the
  *free* PIDs, and ProcessTable occupancy has no set. This is a question, not an improvisation.
- find_free_thread (`0..MAX_THREADS`): a search that ends at the first free TID.
- WAIT_CAP's count in send walks live threads (find_thread), so it grew with live content. Reported,
  not converted.
- Open-call, label and start-handle loops are bounded by their counts (ncalls, nlabels, count).

Uninstrumented gate, rv64 seed 13, at 3f4630826: PASS, exit 0, share 832. R10 p50/p99
20,027/25,221 us. budget_destroy call-to-return 45,995 us. decision_wake p50 7,699 us.
deadline_notice net p99 28,718 us. Logs: .k16/attr-h*.
Item 3 (marked reconcile): not needed, since the gate passes with the live set.

## c5's owed items, and the re-roll
- Reruns: redoubt-ipc (fixed: 4x attempts), process-lifecycle, budget and budget-syscall-attack
  all PASS on both widths (.k16/foc6-summary.txt).
- The model failed at the new values, which nobody had run: the three R4a mutations were uncaught
  (the flood's server had 200 pages, and 3..8 crowd accounts), and R2NoWaitCap and
  R4OverdrawOnDelivery replayed unnoticed (the traces test's 150-step sequences never got past a
  thread bomb of MAX_THREADS + 9 steps). Fixed in model/src/check.rs (server pages
  MAX_OPEN_CALLS + 136; crowd MAX_OPEN_CALLS/WAIT_CAP - 1 ..= +4, the same 3..=8 at 64/16) and
  model/tests/traces.rs (450 steps, as coverage.rs). model.md's flood paragraph follows.
  model-host-tests PASS in 427.6 s (526 s before).
- label.rs:30 now says MAX_LABELS (16) (granted).
- unsafe-budget: kernel/src/bits.rs listed in "kernel: core"; the count stays 19.
- Sizes (release, qemu-virt): rv64 text 115,434, rodata 25,256, data 32, bss 468,568,
  data+bss 468,600, headroom 579,976 of 1 MiB; rv32 text 131,886, rodata 19,948, data 32,
  bss 460,288, data+bss 460,320, headroom 588,256. MEMORY_MANAGER 410,112 B rv64 /
  405,984 B rv32, ~801 B a PID.
- Data-region ruling checks: the loader backs p_memsz only (loader/src/image.rs:51); the kernel's
  allowed range is KERNEL_AREA..usize::MAX (loader/src/main.rs:220); nothing in libs/layout bounds
  the data region.
- ipc.md's walk line now says a walk visits only the PIDs that have a process, through a set.
- The stride unit test that pinned slot order in reconcile was renamed and re-commented: same
  leaves (1 then 4), with 4 now read from 1's slot.
- Re-roll: c4f2fbb55 "stride: the queue is dense" (Size budget: libs/stride: 329 -> 332), then
  a9314dc92 "kernel: 512 processes of 255 threads, and every walk follows what exists" (the
  values, the TID/PID sets, the process-object set, the 1 MiB region, cases, pages, model;
  Size budget: kernel: 7,875 -> 7,927). Its tree is the WIP tip's (backup branch k16-wip-6),
  except for the stride test's name, the two ceilings, ptable.rs's import order (formatting) and
  check.rs's two lines folded to stay under the model's ceiling.
- On the tip: size-budget, unsafe-budget, formatting, docs, stride-host-tests,
  map-fixed-tables-rv32 PASS.

## Final tip 827dbb8b4 (c5 amended), on c4f2fbb55 (stride), on c3 a4d233181
More failures that the values caused and nobody had run, now fixed in c5:
- rt-host-tests: redoubt-sys's received_layout pinned 24 slots (now 32), and hostile's per-kind
  test filled 8 of the 16 label slots and never tried a count of 16.
- pid-reuse-authority rv32 timed out once: TRIES 1024 against 511 PIDs misses about 13% of the
  time (e^-2). Now 8,192 tries (e^-16) and timeout_secs 120. Four runs on each width PASS (rv64
  6-24 s, rv32 0.3-6 s).
Run on the tip's code (exit 0 unless noted): sched-latency 4, budget* 28, process* 12, ipc* 12,
stride, pid* 4, thread-limit, sched-destroy-billing, sched-budget-churn, every *-host-tests case
(model 427.6 s), every *-build case, docs, formatting, size-budget, unsafe-budget,
map-fixed-tables-rv32. Summaries: .k16/final-summary.txt, .k16/final2-summary.txt.
The gate numbers are from 3f4630826 (attr-h). Its kernel code is the tip's except for one import
line's order. The rv32 gate PASS is from 10281b636.
Not run: the whole bench (B7 holds the slot).
Question: random_free_pid still walks pids() twice per process_create.
