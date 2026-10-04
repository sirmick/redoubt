# K16 c5: the gate and sched-latency at the new values (impl-4)

Tree: WIP c5 (values, TidMask) + 1 MiB region + SERVED static + cases/pages/model. Focused run,
`cargo testbench --allow-skip <filter>` via in-dev, both widths (.k16/foc5a-summary.txt).

## Misses
kernel-containment (qemu seed 13, bench's pick), FAIL both widths:
- rv64: bystander share 752/1000 (floor 783); budget_destroy call-to-return p50/p99 62,891/63,175 us
  (R10 p99 target 30,000); driver wake 9,652/21,766/31,729; deadline notice p50 91,555.
- rv32: share 723/1000; budget_destroy 64,620/64,823; driver wake 9,990/26,818/28,144.
- Before c5 (gate-after-s3): R10 oracle p50/p99 17,113/20,210 us, target met; share passed.

sched-latency, FAIL both widths, one target: N=16 decision_wake p50 42,613 us (<= 25,000), p99 64,678.
Before c5 (sl-head-s3, seed 3): N=16 decision_wake p50 5,342. N=1 driver_wake p50 8,235 -> 10,493.
Others met (driver/timer wake p50 <= 15 ms, deadline notice p99 <= 40 ms).

## Not yet attributed
Suspect: per-entry walks over every PID slot, now 512 not 64 (sched.rs Runnable::fill each kernel
entry, pids() walks, stride queue scan of 512 slots: brief item 7). Cheapest attribution: rerun the
gate rv64 with MAX_PROCESS_COUNT=64 and everything else new (~6 min), then with 512 and the old
thread/call values.

## Passing
thread-limit (255), ending-pumps-once, endpoint-destroy*, stub-launch, pid-*, ipc-* but redoubt-ipc
(fixed since, not rerun), process* but process-lifecycle (fixed, not rerun), budget* but budget and
budget-syscall-attack (fixed, not rerun) and size-budget (7,880/7,875; c5 raises with a line).
Host: rt and ipd fail before, pass after (orchestrator's grant); model coverage passes (1.55 s).
