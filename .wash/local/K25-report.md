# K25 — final report

Branch wp-K25, head **1afc7fe27**, on main 5218d0219 (B18, B17, B22; K24, K23, WFS2 included). Gate below ran on eb533c219; after the rebase sched-timer-flood, the smoke set and both builds were rerun on 6f0cd01c2 (all PASS); 1afc7fe27 adds only the residual-risk wording on two pages. Red team: OK
with notes on a7c79a592 (same content, earlier base); renews by diff.

| Commit | What |
| --- | --- |
| f0a4deecb | kernel: a fault the kernel takes on a user mapping is a kernel failure, never a stale retry (cause 1, sum-clear). Size budget kernel 9220 -> 9222 |
| a412f7c0c | kernel: the harts' account at system_reset reports a short count without calling it a failure (cause 2, lend-untouched-page smp=4); smp-boot forbids `^harts: [0-9]+ in the tree` |
| ec86fe488 | tests: sum-clear and lend-untouched-page run in guest time; docs/testbench.md 137 of 205 / 68 |
| 38185acf4 | kernel: an expiry's own handling and the pick and switch into a budget are billed to whoever caused them (cause 3); model bill_timer + R12TimerWorkUnbilled, bill_switch + R12SwitchBilledToPrevious; pages. Size budget kernel -> 9254, model 10296 -> 10325 |
| 700944106 | kernel: on one hart the stale mask costs nothing per page, and a hart's index is a shift (cause 4). Size budget kernel -> 9259 |
| eb533c219 | wash: the smoke set holds sum-clear and lend-untouched-page (sched-exit-churn joins when stable) |

## Causes

1. **sum-clear (both widths).** SMP1's `retry_stale` took the kernel's own S-mode load through a
   valid user leaf for a stale TLB entry and `resume_current` resumed the user thread with the S-mode
   trap's registers (`PROGRAM HALT: Instruction page fault of 0xffff...`). Only faults from user mode
   are retried; kernel/memory-layout.md `satp` says so.
2. **lend-untouched-page smp=4 (both widths).** The checked build's harts account at `system_reset`
   printed `harts: FAIL: 4 in the tree, 4 started, 1 ran user code` for any one-process boot.
3. **sched-exit-churn threads-exit (rv64 since SMP1; rv32 since SCHED1).** Not chaos (the delay sweep
   below): two kinds of kernel time landed on the victim or on nobody.
   - Each expiry bill measured its interval and then ran (trace record, fold, pass): the bill's own
     handling, ~50 us per expiry of the attacker's 1 ms poll (~589 per 2 s window rv64), fell to
     nobody. Intervals now meet; the last bill opens the budget billed last's billing
     (`sched::bill_from`); `bill_from_now`/`resume_billing` close what is open first.
   - kmain's pick and switch were the descheduled budget's: the victim paid the switch at each of its
     slice ends (~90 us at 1 ms, a tenth of its charge). Rule now (owner/orchestrator ruling, option
     b): the pick and switch into a budget are paid by the budget picked, whatever ended the run
     before; a block or exit by its actor; a timer entry's handling by the budget it served; a pick
     of nothing is nobody's (`Payer::Next`, owed time billed at the switch). An intermediate version
     (preemption -> next, block/exit -> actor) broke sched-lift-delay rv32 (non-uniform steps); (b)
     passes it.
   - The time still charged to nobody in the window is the expiry walk in kmain (by rule) and the
     timer entries that find nothing mid-slice (by rule), ~17-20 ms of 2 s (about 1%); the exit path
     itself is billed to its actor.
4. **sched-cluster-old-control rv32.** SMP1's per-page work on map/unmap in a checked build moved the
   cluster's setup ~16 ms later, so the control's fixed-time service read as less latency than its
   50 ms bar. One-hart fast path in the stale mask, power-of-two hart block (index a shift).

## Measurements

threads-exit net (floor 450), pure n-iteration delay at every trap entry, scratch trees at
f39990f3c (without) and 176a36b2b (with the first billing version):

| n | without rv64 | without rv32 | with rv64 | with rv32 |
| --- | --- | --- | --- | --- |
| 0 | 439 | 426 | 510 | 511 |
| 30 | lost to the q restart | 426 | lost to the q restart | 508 |
| 100 | 436 | 427 | 515 | 512 |

Under the final rule (b) at eb533c219: **492 rv64, 487 rv32**; processes-exit 494/495,
processes-fault 494/495. Outside the tolerance by ~40, not by luck.

map/unmap on rv32 old-control, ticks per call (unmap / map_device): pre-SMP1 174793 / 72236; main
214492 / 88113; with the trims 193822 / 79848 (old-control PASS); trims and no lock-holder assert
188243 / 77615 (3% more; assert kept).

## Gates on eb533c219 (jobs.mk from a fresh prebuilt, rc 0 unless noted)

- prebuilt rc=0.
- sched-* both widths: all PASS but **sched-timer-flood rv64 and rv32 FAIL on B17's clause only**
  (cancelled-waits: "0 finding another budget's wait ended early (none, but the case requires
  some)"; its shares met: 483/487 net).
- The four: sum-clear rv64/rv32 PASS; lend-untouched-page rv64/rv32 smp 1 and 4 PASS;
  sched-exit-churn rv64/rv32 PASS (492/487); sched-cluster-old-control rv64/rv32 PASS.
- smp-boot 2/4 and smp-evict 2, both widths PASS.
- Smoke set: userland-boot, init-boot, ipc-outcomes, bench-net-peer both widths PASS.
- build-rv64, build-rv32, docs, formatting, no-cruft, size-budget (kernel 9259, model 10325),
  unsafe-budget, stride-host-tests, model-host-tests (417 s) PASS; rv64/model-mutations (every
  mutation, 3374 s fanned) PASS, none uncaught.
- R12 mutations: all 25 caught (R12SwitchBilledToPrevious by
  sched_contracts seed 0, R12TimerWorkUnbilled by scheduler_fairness).
- icount deadlines by B19's rule (at least four times the slowest pass alone, rounded up to 10 s):
  sum-clear slowest 1.9 s -> >= 10 s, keeps 20 s; lend-untouched-page slowest 1.8 s (rv32 smp 4)
  -> >= 10 s, keeps 30 s.

## Documentation checked

docs/kernel/scheduling.md (Charging rule and numbers, R12 and Charging status lists),
docs/kernel/timer.md (R12 for timer work), docs/kernel/model.md (R12 row, contracts, differential
count 20 of 25), docs/SECURITY.md (R12 register row), docs/kernel/memory-layout.md (`satp`: no stale
retry for the kernel's own fault), docs/testbench.md (guest-time counts), .wash/SWARM.md (smoke set).
README/GETTING-STARTED and the milestone pages name none of these behaviours: no change.

## Residuals and follow-ups

- Two paths still bill a bill's own handling to nobody (now on scheduling.md, Residual risks; timer.md and the Charging rule qualified to a timeout's expiry): an interrupt billed to its device's owner
  (`bill_irq`, then `restart_billing`), and a deadline destruction's tail after its bill.
- sched-exit-churn stays out of the smoke set until the orchestrator calls it stable.
- Large committed files (kernel/src/sched.rs, kernel/src/arch/riscv/mem.rs, model/src/check.rs,
  docs/kernel/scheduling.md) were reviewed by diff and context, not read whole, under the context
  rules.
