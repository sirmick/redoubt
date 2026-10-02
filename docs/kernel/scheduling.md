# Scheduling

The kernel runs threads from one stride queue over every budget with a runnable thread. Each
budget has a **pass**; the lowest pass runs next, for a slice of at most 10 ms, and running
raises the pass in inverse proportion to the budget's free weight. There is no priority: `init`,
the steward and the drivers are scheduled by weight like everyone else. A waking budget joins at
no less than the queue's current minimum, a wake never preempts, and every run is charged,
however short it is and however it ends.

## Purpose

CPU is a resource like pages, and a hostile budget must not get more of it than its share, nor
delay anyone else beyond its weight. The scheduler has two jobs: hold every budget to its weight
whatever pattern of running, sleeping, exiting and budget churn it tries, and keep the steward
and the drivers responsive enough that a person can end a session under load. It does both with
one mechanism and no exceptions, so the rule that contains a hostile agent is the same rule that
serves the steward.

## Interface

### One flat stride queue

<details><summary>Status: built · partly tested: round-robin among one budget's threads is not attacked by a case · tested (8)</summary>

- bench:sched-share
- bench:sched-large-weight
- bench:sched-server-busy
- bench:sched-carve-inflation
- host:redoubt-stride::the_crate_and_the_model_agree
- mutation:R12PriorityById
- mutation:R12IgnoreWeight
- mutation:R12StrideWeightIsLimit

</details>

Every budget with a runnable thread is in one queue, whatever its class. There is no priority,
no second queue and no flag that jumps it. The kernel runs the queued budget with the lowest
pass, and running raises its pass by its runtime times `STRIDE` (2^20) divided by its weight. So
over any stretch in which budgets stay runnable, each gets CPU in proportion to its weight.

The weight the queue uses is the budget's **free weight**: its weight limit less what its
children carved ([R7 (carving)](budgets.md#r7-carving)). Carving moves share to the child and
never duplicates it. Weights are 32-bit and matter only relative to each other. At boot the
kernel gives `root` 1,000,000; `system` gets a quarter (250,000) and `users` the rest less
1,000 (`INIT_WEIGHT`), which `root` keeps so that a process in it has free weight
([budgets](budgets.md)). Below that, the budget that carves a child chooses the child's weight.

Class (`system` or `user`) decides trust, never order ([budgets](budgets.md)). A system-class
server doing a user's work waits its turn like the user.

Within a budget, threads take turns. Each pick runs the budget's next runnable thread after the
one it ran last, in (pid, tid) order, wrapping. The code is `kernel/src/sched.rs`, which keeps
each budget's scheduling state in the budget's own frame, and `libs/stride`, which holds the
arithmetic, the ranks and the order of steps. `libs/stride` is `#![forbid(unsafe_code)]` and has
no dependencies.

### Preemption points

<details><summary>Status: built · partly tested: that an interrupt's wake does not preempt, and that another budget's deadline does, are not attacked by a case · tested (5)</summary>

- bench:sched-wake-no-preempt
- bench:budget-deadline
- host:redoubt-model::scheduler_contracts_hold
- mutation:R12PreemptOnWake
- mutation:R12TimeoutWakePreempts

</details>

The running thread keeps the CPU until one of these:
- its slice ends: `SLICE_US` (10,000 µs) from the pick;
- it blocks, exits, faults or is killed;
- a budget deadline fires.

Nothing else takes the CPU. A timeout expiring or an interrupt firing only makes a thread
runnable; it runs when the queue next picks it. The hart timer is always armed for the earliest
of the slice end, the next timeout and the next budget deadline ([timer](timer.md)).

A budget deadline preempts whatever runs, because the destruction lifts passes and removes
budgets, so the queue must be picked again. A thread that entered the kernel just as the deadline
fired takes its trap again when it next runs.

The kernel itself is never preempted. It runs with interrupts off (`sstatus.SIE` clear) and takes
the timer and device interrupts only from user mode or while idle, so a system call or a
destruction runs to its end. Picking is done by `kmain`, the kernel's own loop (PID 1). It runs
its pick through a private S-mode `ecall` into the kernel's trap handler, which saves `kmain`'s
context as it saves a thread's.

### The current minimum and ties

<details><summary>Status: built · partly tested: wakers ahead of requeued budgets, and requeues in order, are checked on the target only when a run happens to produce such a tie; the host tests and the model attack them · tested (10)</summary>

- bench:sched-ties
- bench:sched-idle-gap
- host:redoubt-stride::ranks_follow_all_four_clauses
- host:redoubt-stride::the_floor_survives_an_empty_queue
- host:redoubt-stride::a_running_budget_stays_queued_and_counts_for_the_floor
- mutation:R12WakeBanksCredit
- mutation:R12NoFloorWhenIdle
- mutation:R12TieQueuedFirst
- mutation:R12RequeueAhead
- mutation:R12RequeueLifo

</details>

A budget **wakes** when it goes from no runnable thread to one. Its pass becomes
`max(own pass, floor)`. The **floor** is the current minimum: the lowest pass among queued
budgets, the running one included at the pass it was last charged. The floor only rises, and it
holds while the queue is empty. So a budget that slept while others ran, or through an idle gap,
comes back at the floor and not with credit it banked while away.

A pick takes the lowest **rank**, `(pass, tie, id)`. At an equal pass:
1. a waker ranks ahead of a budget that was requeued;
2. of two wakers, the later kernel entry's ranks first;
3. within one entry, the lower budget id first (ids are the kernel's, I12 (ids never reused));
4. requeued budgets run in the order they were requeued.

The tie key encodes this. Each wake takes `front - 1`, and one entry's wakes are processed in
descending id, so the lowest id ends frontmost. Each requeue of a still-runnable budget takes
`back + 1`. Both counters reset when the queue empties. Wakes are reconciled once per kernel
entry: as the kernel leaves, budgets that lost their last runnable thread leave the queue and
budgets that gained one wake. Ranking later wakers first starves nobody: a waker that has run is
charged, its pass rises above the floor, and it no longer ties.

`bench:sched-ties` runs a kernel built with the scheduling trace ([R23](#r23-no-test-channels)).
The bench's own oracle (`tools/testbench/src/sched_oracle.rs`) rebuilds the order from the trace's
events with its own reading of the four clauses and checks every pick, keeps its own floor, and
requires every pass never to fall but at a weight change, which it recomputes
([the lead follows the weight](#the-lead-follows-the-weight)).

```mermaid
flowchart TD
    K["kernel entry: a call, an interrupt,<br/>a timeout, a slice end, a deadline"] --> D{"does the running<br/>thread leave the CPU?"}
    D -- "no: a wake never preempts" --> REC
    D -- "yes: it blocked, exited, faulted,<br/>its slice ended or a deadline fired" --> F["charge: t = rem + ticks x STRIDE<br/>pass += t / w, rem = t mod w<br/>(at least 1 tick)"]
    F --> S{"still has a<br/>runnable thread?"}
    S -- yes --> RQ["requeue behind its equals<br/>(tie = back + 1)"]
    S -- no --> LV[leave the queue]
    RQ --> REC
    LV --> REC
    REC["reconcile: each budget that gained a<br/>runnable thread wakes, highest id first,<br/>pass = max(own, floor), tie = front - 1"] --> FL["floor = max(floor, lowest queued pass)"]
    FL --> R{"is the CPU<br/>with kmain?"}
    R -- no --> RUN[the running thread resumes]
    R -- yes --> P["kmain picks the lowest (pass, tie, id)<br/>and runs that budget's next thread<br/>after its cursor, for up to 10 ms"]
```
*Figure: the pass and wake rule at one kernel entry. `w` is the budget's free weight.*

### Charging

<details><summary>Status: built · partly tested: interrupt handling billed to the device's owner is not attacked by a case · tested (15)</summary>

- bench:sched-sleep-gaming
- bench:sched-exit-churn
- bench:sched-timer-flood
- bench:sched-server-busy
- bench:sched-destroy-billing
- bench:deadline-flood-billed
- host:redoubt-stride::a_split_charge_equals_the_whole
- host:redoubt-stride::every_charge_counts_at_any_weight
- host:redoubt-stride::a_deschedule_charges_at_least_one_unit_and_a_destroy_only_what_ran
- mutation:R12ShortRunsFree
- mutation:R12DropRemainder
- mutation:R12ExitRunsFree
- mutation:R12NoMinimumCharge
- mutation:R12FoldAtNewWeight
- mutation:R12DeadlineWorkUnbilled

</details>

Runtime is counted in timebase ticks at the trap boundary. There are two ways into user mode
(resuming a thread, returning from a call) and one way out (the trap handler). On every trap from
user mode the kernel adds the user time since the last return to the running budget's pending
runtime. So no path runs user code unaccounted, whatever ends the run.

Pending runtime is folded into the pass at a deschedule, before a weight change, at a
destruction, and when work is billed to a budget that is not running:

```
t = rem + ticks x STRIDE;   pass += t / w;   rem = t mod w      (w: the free weight)
```

- **The remainder is exact**, so a run split into pieces costs what one run would, at any weight.
  Without it a weight above `STRIDE` would round a one-tick run to nothing.
- **A deschedule charges at least `MIN_CHARGE`** (1 tick), so a run too short for the clock to
  see is not free.
- **One fold counts at most `RUNTIME_CAP`** (2^40 ticks), so the product fits 64 bits on rv32 and
  rv64 alike. The pass is a 128-bit number that is only added to and compared, so it never wraps.
- **A weight change** (a carve, or a carve returned) folds first, so runtime is charged at the
  weight it ran at, and then rescales what the budget owes to the new weight
  ([the lead follows the weight](#the-lead-follows-the-weight)).

Kernel time is billed as well:
- a system call's time is its caller's;
- an expired timeout is billed to its thread's budget, and a deadline's destruction as below;
  each walk that finds an expired item is billed with it, and the last walk, which finds
  nothing, is nobody's, so one entry does at most one walk nobody pays for;
- an interrupt's handling is billed to the owner of its device object
  ([R5 (interrupts)](devices.md#r5-interrupts)); one with no device object, to nobody;
- `kmain`'s pick and switch after a deschedule are the descheduled budget's;
- idle time is nobody's.

The top of a destruction returns its carve to its parent before any of the destruction's work is
billed. So the parent, often the caller of `budget_destroy`, pays for the destruction at the
weight it has once the child is gone, not at the sliver it kept while the child held the rest.
In `bench:sched-destroy-billing` a parent that kept 10 of 1000 destroys the child holding 990 and
is back on the CPU within twice the destruction's cost and four slices; billed at 10, it would wait
for seconds.

Every destruction's whole cost is billed to someone. For `budget_destroy` that is the caller, as
the call's own kernel time. For a deadline it is the top's parent, after its carve returns, or the
nearest ancestor with free weight above 0 if the parent has none; `root` always has. No part of a
destruction is billed to nobody ([R10 (destruction)](budgets.md#r10-destruction)). On a
deadline the kernel names the payer once the carve is back, and bills it after the subtree is
gone for everything from the walk that found the deadline. In `bench:deadline-flood-billed` a
creator floods its own budget with empty weight-0 budgets on short deadlines: its count falls as
the flood grows from 16 to 64 a round, and an equal-weight victim keeps its half. With the bill
planted out, the victim fell to 137 of 1000.

A server that works for a caller spends its own budget's CPU: no time is donated
([Residual risks](#residual-risks)). CPU charging is separate from page charging
([R6 (charging)](budgets.md#r6-charging)). The model charges runtime, and a deadline's
destruction as work billed to its payer (`R12DeadlineWorkUnbilled`); billing other kernel work is
the kernel's alone, and the boot cases are its only check.

### Inheritance

<details><summary>Status: built · tested (11)</summary>

- bench:sched-budget-churn
- bench:sched-debt-lift
- bench:sched-idle-gap
- host:redoubt-stride::create_then_destroy_without_a_run_moves_nothing
- host:redoubt-stride::a_churned_child_adds_to_a_leading_parent
- host:redoubt-stride::the_inherited_wait_is_not_counted_again
- mutation:R12DestroyDropsDebt
- mutation:R12CreateAtFloorOnly
- mutation:R12LiftByMax
- mutation:R12UnnormalizedLift
- mutation:R12LiftCountsEntryWait

</details>

A child budget enters at `e = max(floor, parent's pass)`, and keeps `e` as its **entry**. A
running parent is charged first. So a child never starts behind the parent it came from, and
making a child buys no turn.

When a budget is destroyed ([R10 (destruction)](budgets.md#r10-destruction): descendants first,
bottom-up), its own work since entry moves to its parent:

```
W = (pass - max(e, floor))+ x w_child + rem_child
parent's pass = max(parent's pass, floor) + W / w_parent      (the remainder carried)
```

`w_parent` is the parent's free weight after the child's carve has returned. A parent below the
floor starts from the floor, without its remainder: it cannot bank what it did not run. Only
work after entry counts, so creating and destroying a budget that never ran moves nothing. The
child's debt adds to the parent's own lead instead of hiding under it, so a parent that spins and
churns children pays for both.

`bench:sched-budget-churn` runs a churner that creates a weight-1 child, lets it run a slice and
destroys it, over and over (blocked, spinning, by a deadline, and through fresh intermediates),
against an equal-weight victim who keeps half; the oracle recomputes every lift from the trace.
Its last variant, a shell that keeps giving a child half its weight and taking it back with no
run between, leaves the victim neither more nor less than half.

### Running while carved down

<details><summary>Status: built · tested (8)</summary>

- bench:sched-carve-inflation
- bench:sched-carve-return
- bench:sched-budget-churn
- bench:sched-destroy-billing
- host:redoubt-stride::a_carve_and_its_return_leave_the_state
- mutation:R12StrideWeightIsLimit
- mutation:R12FoldAtNewWeight
- mutation:R12RescaleOnlyOnReturn

</details>

A budget that runs while most of its weight is carved away accrues its lead at the small weight
it kept, and owes that runtime at whatever weight it has later
([the lead follows the weight](#the-lead-follows-the-weight)). A destroyed child's work is lifted
at the child's weight at its destruction, whatever the child kept while it ran. That only
over-charges the budget that carved; it never under-charges, so no share is gained by carving. In
`bench:sched-carve-inflation` a spinning budget that carves spinning children, one deep and four
deep, gets at most half against an equal victim.

A budget that holds a process keeps free weight above 0. A carve that would take its last free
weight gets `InvalidArgument`, and so does `process_create` into a budget whose weight is all
carved ([R7](budgets.md#r7-carving)).

### The lead follows the weight

<details><summary>Status: built · tested (6)</summary>

- bench:sched-carve-return
- bench:sched-budget-churn
- host:redoubt-stride::a_carve_and_its_return_leave_the_state
- host:redoubt-stride::the_crate_and_the_model_agree
- host:testbench::weight_changes_are_recomputed
- mutation:R12RescaleOnlyOnReturn

</details>

What a budget owes is runtime, and a weight change keeps it exactly. Every weight change, a
carve and a carve returned alike, folds at the old weight and then converts the budget's lead
and remainder to the new weight, the same way a lift converts a child's work:

```
W = (pass - floor)+ x w_old + rem
pass = floor + W / w_new;   rem = W mod w_new
```

So a carve raises the lead by the ratio of the weights and its return lowers it by the same
ratio, and a carve returned with no run between leaves the budget where it was. If the carve
raised the floor (the budget had the lowest pass), the return converts from the higher floor and
the budget ends a little higher: over-charged, never ahead. Both directions
are needed: rescaling only on a return would let a budget carve just before a burst and return
just after, and its lead would shrink at the return without having grown at the carve. A budget
at or below the floor owes only its remainder, and waking would lift it to the floor anyway. At a
destruction the carve returns first, so the parent's lead is converted before the child's work
is lifted onto it at the parent's restored weight ([inheritance](#inheritance)). A budget at
weight 0 holds no process, and what it owes is stated at weight 1 meanwhile, so it is carried
exactly through 0: carving everything away and then getting back only part of it is the same as
the one carve from the old weight to the new.

`libs/stride`'s `rescale` is the conversion; the model makes the same one, and the bench's oracle
recomputes every weight change a traced kernel records, the one place it lets a pass fall. A host
test checks that a carve and its return leave pass and remainder unchanged. In
`bench:sched-carve-return` a budget of weight 1000 carves 999 away, runs 9 ms on the 1 it kept,
and takes the weight back: it then gets 492 to 494 of 1000 against an equal victim. With the
remainder alone rescaled, as before, it got 0.

### Responsiveness

<details><summary>Status: built · tested (7)</summary>

- bench:sched-latency
- bench:budget-destroy-growth
- bench:sched-budget-churn
- host:testbench::destructions_are_timed_and_bounded
- host:testbench::audits_are_subtracted_inside_each_window
- host:testbench::shares_are_judged_net_of_audits
- host:testbench::an_unmatched_audit_fails

</details>

No wake latency follows from weight. A wake waits out the running thread's slice, a waking
budget keeps a pass above the floor if it has one, and several budgets can tie at the floor. So
wakeup is prompt but not bounded, and responsiveness is a **measured target** under a named
workload, not a bound derived from the queue.

The workload (`tests/programs/src/bin/sched-latency.rs`): a driver stand-in (weight 1000, from
`system`) that waits for the goldfish RTC's alarm interrupt; a steward stand-in (1000, from
`system`) that sleeps on timeouts, destroys leases by hand after a timeout (its decision) and
waits for other leases' deadlines; and N spinning sessions of weight 100 from `users`, for
N = 1, 4 and 16. At N = 16 a spinning server of weight 1000 joins them. Each run takes 200 wakes
and 50 destructions of each kind, on QEMU, on rv64 and rv32.

**Targets are guest instructions.** The case runs under `-icount shift=3,sleep=off` with the RTC
on the same clock (`-rtc clock=vm`; [`tests/sched-latency.toml`](../../tests/sched-latency.toml)):
every guest instruction advances virtual time by 2^3 ns, and idle time skips to the next timer
deadline. So 1 ms is 125,000 instructions, and each target below is that many instructions; the
milliseconds are for reading. Host load does not change a result.

**Targets exclude the checked build's audits.** The case is a checked build, since the trace needs
one ([R23](#r23-no-test-channels)). A checked build runs full audit scans after each destruction
and at each process-object free, and a release build compiles none of them. Each target counts the
kernel a release build runs, a share as well as a window, so an audit neither fills a window nor
moves the schedule. The audit time inside each window, a share's included, is subtracted, from the
trace's audit records, and reported beside it ([checked builds](../testbench.md#checked-builds)).
The scheduler does not see an audit either: its time is charged to no budget, and the running
slice's end and the start of the kernel time being billed both move forward by its length, so the
thread that ran it is picked and preempted as in a release build (`sched::audit`). With two full
handle tables live, the audits are about 24 ms after a destruction and 8 ms at a free.

Measured at seed 3, p99 in µs, net / gross (audit time inside the windows): the audits hold the
deadline notice, which waits out the audit after its own destruction, and little else. The
trace's audit stamps shift the phases, so these seed-3 numbers differ from the sweeps', whose
figures stay as measured.

| Measure (p99, N = 1 / 4 / 16) | rv64 | rv32 |
| --- | --- | --- |
| deadline notice | 4233 / 5052 / 13896 net, 19754 / 21994 / 33535 gross | 4435 / 5143 / 13055 net, 19526 / 21718 / 31435 gross |
| driver wake | 9632 / 10815 / 32350, net = gross | 10045 / 11482 / 33939, net = gross |
| steward timer wake | 8543 / 9674 / 31740, net = gross | 8917 / 10274 / 32964, net = gross |
| steward decision wake | 5931 / 6143 / 39449, net = gross | 6169 / 6409 / 29490, net = gross |
| a lease's end (worst decision wake + R10) | 39449 + 6366 = 45815 | 29490 + 6577 = 36067 |

The run's 837 audits total 7.52 s (rv64) and 7.25 s (rv32) of the hart; R10 itself has none
inside it, which the oracle asserts. Beside each destruction's R10 time the oracle reports its
threads' time: the processes' threads ending inside it, their pumps included (the trace's `T` and
`t` records), at seed 3 a p99 of 32 µs on rv64 and 36 µs on rv32. In the containment gate, with two full handle tables live
(rv64, seed 3, its D leases' notices), the deadline notice is 23,185 µs net and 53,830 µs gross,
492,458 µs of audit inside its windows. With the audit after a destruction left unstamped
(`audit-unstamped`, the recorded negative run), it is 47,270 µs net: the target misses, since the
oracle subtracts only what the trace shows it. With two full leases live at a deadline's end
(seed 4), the deadline notice is 25,292 µs net on rv64 and 25,728 µs on rv32. With each audit's
time billed to the budget that ran it and counted against its slice (`audit-billed`, the second
recorded negative run), it is 44,343 and 45,283 µs net: the target misses. The steward stand-in
spends its slice on the audit at the first `killed` notice, is requeued, and the victim and a
hostile budget run a slice each before it takes the second. A share is judged the same way: in
`sched-budget-churn`, whose attacker destroys a budget each slice, the victim of the spinning
parent gets 494 of 1000 net of audits on rv64 (403 gross) and 495 on rv32 (416), and the shell's
victim 500 and 499. With the audits billed to the budget that ran them, as before, the shell
paid for scans a release build does not run, and its victim got 650 and 643 net: more than half.

**The gate runs one pinned seed.** The guest's boot RNG seed decides the PIDs the kernel draws,
which shift instruction counts and so the phase of every later event; with it pinned, a run
repeats exactly, and the case prints the seed so that a failure replays. One seed hides the
spread, so a **sweep** of about 16 seeds measures it. A target that the stride rank decides (how
many spinners' slices a waking budget with a lead lands behind) is the sweep's worst case plus a
stated margin. The sweep's seeds and worst case are recorded here with the target they set.

| Measure | Target |
| --- | --- |
| driver wake: the RTC's time when the driver runs, less the alarm it set | p50 <= 15 ms, p99 <= 50 ms |
| steward timer wake: `time_now` when it runs, less its timeout's deadline | p50 <= 15 ms, p99 <= 50 ms |
| steward decision wake: the same, for the timeout after which it destroys a lease | p50 <= 25 ms, p99 <= 95 ms (from the fourth sweep below; the third sweep set the same 25 and 95 ms) |
| deadline notice: the lease's `killed` notice received, less the lease's deadline | p99 <= 40 ms (back from 54 ms when destruction scanned every object frame; [budgets](budgets.md#residual-risks)) |
| R10 kernel time of one destruction, from the trace | p99 <= 30 ms (back from 39 ms when destruction scanned every object frame; [budgets](budgets.md#residual-risks)) |
| a lease's end from the steward's decision: the worst decision-wake p99 + R10's p99 | <= 95 + 30 = 125 ms, asserted as one sum by the post-check |
| `budget_destroy`, call to return | recorded against one round: R10's 30 ms plus (runnable budgets + 2) slices |
| the 1000-weight server's share of the spinning CPU at N = 16 | at least 384 less 30 per thousand |

In instructions: 15 ms is 1,875,000, 25 ms is 3,125,000, 30 ms is 3,750,000, 40 ms is 5,000,000,
50 ms is 6,250,000, 95 ms is 11,875,000, 125 ms is 15,625,000, and one 10 ms slice is 1,250,000.

The decision wake is measured by the stand-in itself (`time_now` against its own deadline), and
the post-check reads its sample windows, net of audits, while R10's time comes from the kernel's
trace: that half of the lease-end sum is the program's own report, not the kernel's.

**The sweep** (2026-09-27, seeds 1 to 16, on rv64 and rv32, `TESTBENCH_QEMU_SEED`; the gate runs
seed 3, this sweep's worst, and no gate sets the variable). The steward decision wake, p50 / p99 in µs at N = 1, 4 and 16, and a lease's end (the
worst decision-wake p99 plus R10's p99 from the trace):

| Seed | rv64 N=1 | rv64 N=4 | rv64 N=16 | rv64 lease end | rv32 N=1 | rv32 N=4 | rv32 N=16 | rv32 lease end |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 5919 / 5920 | 6130 / 6131 | 17903 / 82467 | 108846 | 6700 / 17101 | 6394 / 6396 | 7597 / 29468 | 56874 |
| 2 | 5919 / 5920 | 6130 / 6131 | 17903 / 82466 | 108845 | 6701 / 17099 | 6395 / 6397 | 7596 / 29466 | 56872 |
| 3 | 5918 / 5920 | 6130 / 16512 | 17902 / 103984 | 130363 | 6700 / 17097 | 6394 / 6398 | 7598 / 29476 | 56882 |
| 4 | 5919 / 5920 | 6130 / 16512 | 17902 / 82468 | 108847 | 6701 / 17097 | 6395 / 6397 | 7597 / 29469 | 56875 |
| 5 | 5919 / 5920 | 6130 / 16511 | 17902 / 86653 | 113032 | 6700 / 17098 | 6394 / 6396 | 7597 / 29464 | 56870 |
| 6 | 5919 / 5920 | 6130 / 16511 | 17904 / 82467 | 108846 | 6701 / 17099 | 6394 / 6396 | 7596 / 29469 | 56876 |
| 7 | 5919 / 5920 | 6130 / 6131 | 17902 / 82466 | 108845 | 6700 / 17099 | 6394 / 6396 | 7597 / 29465 | 56870 |
| 8 | 5919 / 5920 | 6130 / 6130 | 17903 / 60947 | 87326 | 6700 / 17098 | 6395 / 6397 | 7598 / 29469 | 56875 |
| 9 | 5919 / 5920 | 6130 / 16512 | 17901 / 82463 | 108842 | 6701 / 17098 | 6394 / 16883 | 7596 / 40403 | 67809 |
| 10 | 5919 / 5920 | 6130 / 16512 | 17900 / 94272 | 120651 | 6700 / 17100 | 6395 / 6397 | 7597 / 40399 | 67805 |
| 11 | 5919 / 5920 | 6130 / 6131 | 17902 / 93225 | 119604 | 6701 / 17095 | 6394 / 16873 | 7596 / 29461 | 56867 |
| 12 | 5919 / 5920 | 6130 / 6131 | 17904 / 82464 | 108843 | 6700 / 17100 | 6395 / 6397 | 7596 / 40407 | 67814 |
| 13 | 5919 / 5920 | 6130 / 16512 | 17901 / 94273 | 120652 | 6700 / 17099 | 6395 / 16882 | 7596 / 40401 | 67807 |
| 14 | 5919 / 5920 | 6130 / 26893 | 17902 / 93225 | 119604 | 6701 / 17098 | 6395 / 6397 | 7597 / 40396 | 67802 |
| 15 | 5919 / 5920 | 6130 / 16510 | 17903 / 93224 | 119603 | 6701 / 17097 | 6394 / 6396 | 7597 / 40403 | 67810 |
| 16 | 5919 / 5920 | 6130 / 16512 | 17902 / 82464 | 108843 | 6700 / 17096 | 6395 / 6397 | 7597 / 29466 | 56872 |

The worst case is rv64 at N = 16: decision-wake p50 17,904 µs (every seed within 4 µs of it) and
p99 103,984 µs (seed 3); R10's p99 is 26,379 µs (rv64) and 27,407 µs (rv32) on every seed. The
targets were 15 ms (p50) and 50 ms (p99) before the sweep, which rv64 missed on every seed; the
sweep sets them at the worst case plus a margin of a tenth, rounded up to 5 ms: p50 17.9 x 1.1 =
19.7, so 20 ms; p99 104 x 1.1 = 114.4, so 115 ms. Every other measure met its fixed target on every seed; their worst p99s
were driver wake 33.3 ms, steward timer wake 34.2 ms and deadline notice 23.7 ms (all rv32), and
the server's share never fell below 380 of 1000.

**The second sweep** (2026-09-29, the same seeds and widths) followed a change that found process
objects by index instead of scanning frames. The change made most destructions cheaper, which
moved the workload's timeline: the last lease destructions at N = 16 now overlap the live
sessions. There, taking a lease's exit notice inside R10 frees its process object, and that
object's handle sweep walks every live process's handle pages. The N = 16 deadline notice and
R10's kernel time, p99 in µs, and a lease's end:

| Seed | rv64 notice | rv64 R10 | rv64 lease end | rv32 notice | rv32 R10 | rv32 lease end |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 36195 | 33863 | 73287 | 37902 | 35107 | 86435 |
| 2 | 36316 | 33985 | 73409 | 38023 | 35235 | 86561 |
| 3 | 36287 | 33943 | 94886 | 37989 | 35190 | 86530 |
| 4 | 36331 | 34000 | 94941 | 38042 | 35251 | 97523 |
| 5 | 36156 | 33823 | 73245 | 37868 | 35069 | 75474 |
| 6 | 36495 | 34164 | 84347 | 38220 | 35421 | 86759 |
| 7 | 37940 | 34116 | 84297 | 38164 | 35370 | 75771 |
| 8 | 37836 | 34084 | 105786 | 38144 | 35337 | 97606 |
| 9 | 36174 | 33843 | 73264 | 37882 | 35089 | 86413 |
| 10 | 36198 | 33866 | 94809 | 39346 | 35113 | 86447 |
| 11 | 37919 | 34131 | 84313 | 38177 | 35386 | 75790 |
| 12 | 36486 | 34156 | 73581 | 38203 | 35412 | 86748 |
| 13 | 36432 | 34101 | 73522 | 38146 | 35355 | 75757 |
| 14 | 36177 | 33844 | 94786 | 37885 | 35090 | 86416 |
| 15 | 36291 | 33948 | 105650 | 48851 | 35196 | 75600 |
| 16 | 38661 | 33878 | 73301 | 37924 | 35124 | 86458 |

The worst are the deadline notice at 48,851 µs (rv32, seed 15) and R10 at 35,421 µs (rv32, seed
6). Their targets became the worst case plus a tenth, rounded up to 1 ms: 48.9 x 1.1 = 53.7, so
54 ms, and 35.4 x 1.1 = 39.0, so 39 ms. The lease end's bound moved with R10's, to 154 ms; its
worst is 105.8 ms. The difference from 30 ms was the handle-sweep term of budget destruction's
cost. A destruction now follows the dying subtree and folds that sweep into one pass, so the term
is gone and the targets are back at 30 and 40 ms above ([budgets](budgets.md#residual-risks)).
Every other measure met its target on every seed. The gate stays on seed 3.

**The third sweep** (2026-09-29, the same seeds and widths) followed a change that frees a page
table once it maps nothing. It found the first sweep's rv64 median was one phase, not a property
of rv64: the N = 16 decision wake's p50 is about 7 ms on most seeds and about 18 ms on some, on
either width (rv64 on seeds 6, 7, 11, 12 and 16, rv32 on seeds 5 and 9). The decision wake, p50 /
p99 in µs, and a lease's end:

| Seed | rv64 N=1 | rv64 N=4 | rv64 N=16 | rv64 lease end | rv32 N=1 | rv32 N=4 | rv32 N=16 | rv32 lease end |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 5919 / 5920 | 6130 / 16511 | 7143 / 39423 | 73286 | 6155 / 6157 | 6395 / 16883 | 7599 / 51328 | 86437 |
| 2 | 5919 / 5920 | 6130 / 16512 | 7142 / 50181 | 84166 | 6155 / 6157 | 6395 / 16883 | 7598 / 51335 | 86570 |
| 3 | 5919 / 5920 | 6130 / 16512 | 7143 / 50182 | 84125 | 6155 / 6157 | 6395 / 16884 | 7598 / 51340 | 86530 |
| 4 | 5919 / 5920 | 6130 / 16512 | 7142 / 60941 | 94941 | 6155 / 6157 | 6395 / 16881 | 7598 / 62271 | 97522 |
| 5 | 5919 / 5920 | 6130 / 16510 | 7143 / 39422 | 73246 | 6154 / 6157 | 6395 / 16883 | 18523 / 40403 | 75472 |
| 6 | 5919 / 5920 | 6130 / 16512 | 17899 / 50182 | 84346 | 6155 / 6157 | 6396 / 16884 | 7599 / 51336 | 86756 |
| 7 | 5919 / 5920 | 6130 / 16512 | 17900 / 50181 | 84297 | 6155 / 6157 | 6395 / 16881 | 7598 / 40400 | 75770 |
| 8 | 5919 / 5920 | 6130 / 16512 | 7143 / 71702 | 105786 | 6155 / 6157 | 6395 / 16881 | 7599 / 40402 | 75739 |
| 9 | 5919 / 5920 | 6130 / 16512 | 7142 / 39421 | 73264 | 6155 / 6158 | 6395 / 16881 | 18529 / 51325 | 86413 |
| 10 | 5919 / 5920 | 6130 / 16512 | 7143 / 39422 | 73288 | 6155 / 6157 | 6395 / 16883 | 7599 / 40403 | 75516 |
| 11 | 5919 / 5920 | 6130 / 16511 | 17900 / 50182 | 84313 | 6155 / 6157 | 6395 / 16881 | 7599 / 40404 | 75789 |
| 12 | 5919 / 5920 | 6130 / 16511 | 17900 / 50184 | 84340 | 6155 / 6157 | 6395 / 16883 | 7598 / 40407 | 75818 |
| 13 | 5919 / 5920 | 6130 / 16512 | 7141 / 82463 | 116564 | 6155 / 6158 | 6395 / 16882 | 7597 / 40398 | 75752 |
| 14 | 5919 / 5920 | 6130 / 16511 | 7143 / 60942 | 94786 | 6155 / 6157 | 6395 / 16885 | 7599 / 51342 | 86433 |
| 15 | 5919 / 5920 | 6130 / 16512 | 7143 / 71701 | 105649 | 6154 / 6157 | 6395 / 16883 | 7599 / 51332 | 86527 |
| 16 | 5919 / 5920 | 6130 / 16512 | 17899 / 39423 | 73301 | 6155 / 6157 | 6395 / 16883 | 7599 / 51334 | 86458 |

The worst are p50 18,529 µs (rv32, seed 9) and p99 82,463 µs (rv64, seed 13). By the same rule, a
tenth over the worst rounded up to 5 ms: p50 18.5 x 1.1 = 20.4, so 25 ms; p99 82.5 x 1.1 = 90.7,
so 95 ms. The lease end's bound follows, to 95 + 39 = 134 ms; its worst is 116.6 ms. Every other
measure met its target on every seed. The gate stays on seed 3, which is no longer the worst
seed; the targets come from the sweep, not from the seed the gate runs.

**The fourth sweep** (2026-09-30, the same seeds and widths) followed a destruction that walks what
it destroys instead of every object frame ([budgets](budgets.md#residual-risks)), which cheapens
R10 and moves the phase of every later event. R10 and the deadline notice return to the 30 ms and
40 ms they had before the second sweep. The decision wake's worst p50 is 18,541 µs (rv32, seed 11)
and its worst p99 82,497 µs (rv64, seed 6); a tenth over the worst rounded up to 5 ms sets the p50
at 25 ms and the p99 at 95 ms. A lease's end follows to 95 + 30 = 125 ms. Every other measure met
its target on every seed; the gate stays on seed 3. The decision wake, p50 / p99 in µs at N = 1, 4
and 16, and a lease's end (the worst decision-wake p99 plus R10's p99 from the trace):

| Seed | rv64 N=1 | rv64 N=4 | rv64 N=16 | rv64 lease end | rv32 N=1 | rv32 N=4 | rv32 N=16 | rv32 lease end |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 5926 / 5926 | 6137 / 16521 | 7149 / 60964 | 82485 | 6162 / 6165 | 6402 / 16894 | 7606 / 62296 | 84795 |
| 2 | 5926 / 5926 | 6137 / 16522 | 7149 / 50199 | 71841 | 6162 / 6164 | 6402 / 16894 | 18537 / 51367 | 73994 |
| 3 | 5926 / 5926 | 6136 / 16521 | 7149 / 60967 | 82567 | 6161 / 6164 | 6402 / 16893 | 7608 / 51364 | 73947 |
| 4 | 5926 / 5926 | 6137 / 16521 | 7149 / 82492 | 104149 | 6162 / 6164 | 6401 / 16892 | 18539 / 62295 | 84939 |
| 5 | 5926 / 5926 | 6137 / 16521 | 7149 / 50202 | 71683 | 6162 / 6164 | 6401 / 16894 | 7607 / 51363 | 73824 |
| 6 | 5926 / 5926 | 6137 / 16521 | 7149 / 82497 | 104318 | 6162 / 6164 | 6402 / 16893 | 18539 / 40424 | 63236 |
| 7 | 5926 / 5926 | 6137 / 16522 | 7149 / 39440 | 61213 | 6162 / 6164 | 6402 / 16894 | 18538 / 51355 | 74116 |
| 8 | 5925 / 5926 | 6137 / 16522 | 7149 / 50202 | 71943 | 6162 / 6164 | 6402 / 16894 | 18538 / 51364 | 74094 |
| 9 | 5926 / 5926 | 6137 / 16522 | 7149 / 50202 | 71702 | 6162 / 6164 | 6402 / 16894 | 7607 / 51357 | 73836 |
| 10 | 5926 / 5926 | 6137 / 16522 | 7149 / 39440 | 60962 | 6162 / 6164 | 6402 / 16893 | 7608 / 51357 | 73860 |
| 11 | 5926 / 5926 | 6136 / 16521 | 7148 / 7149 | 38309 | 6162 / 6165 | 6402 / 16892 | 18541 / 51360 | 74136 |
| 12 | 5926 / 5926 | 6137 / 16521 | 7149 / 82494 | 104307 | 6162 / 6164 | 6402 / 16894 | 18540 / 51357 | 74160 |
| 13 | 5926 / 5926 | 6137 / 16522 | 7149 / 60965 | 82723 | 6162 / 6164 | 6402 / 16894 | 18539 / 51355 | 74101 |
| 14 | 5926 / 5926 | 6137 / 16522 | 7149 / 50204 | 71705 | 6162 / 6164 | 6402 / 16892 | 7607 / 62294 | 84775 |
| 15 | 5925 / 5926 | 6137 / 16521 | 7149 / 50202 | 71807 | 6162 / 6164 | 6402 / 16892 | 18536 / 51353 | 73940 |
| 16 | 5926 / 5926 | 6137 / 16522 | 7149 / 50204 | 71739 | 6162 / 6164 | 6402 / 16893 | 18538 / 51353 | 73869 |

**The fifth sweep** (2026-09-30, the same seeds and widths) followed a destruction that closes
only the handles chained to what it destroys and frees the dying tables whole
([budgets](budgets.md#residual-risks)). R10's p99 is 5,462 µs on rv64 and 5,723 µs on rv32 at
most, on every seed, and the deadline notice's worst p99 is 27,089 µs (rv64, seed 7). The
decision wake's worst p50 is 7,606 µs (rv32, seed 5) and its worst p99 50,993 µs (rv64, seed 15);
a lease's worst end is 56,454 µs. The targets stay where the fourth sweep set them. The steward
now sets each by-deadline lease's deadline 300 ms ahead, not 150: a budget's deadline is fixed
at its creation, before the spawn, and at N = 16 the spawn takes about 150 ms, so on one seed
every lease ended before its process started and the notice took no sample. The lead is the
fixture's; the notice is measured from the deadline, and its target stays 40 ms. The decision
wake, p50 / p99 in µs at N = 1, 4 and 16, and a lease's end:

| Seed | rv64 N=1 | rv64 N=4 | rv64 N=16 | rv64 lease end | rv32 N=1 | rv32 N=4 | rv32 N=16 | rv32 lease end |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | 5926 / 5927 | 6137 / 16521 | 7149 / 28676 | 34137 | 6162 / 6165 | 7079 / 17572 | 7604 / 45854 | 51576 |
| 2 | 5926 / 5927 | 6137 / 16522 | 7149 / 28675 | 34136 | 6162 / 6165 | 7078 / 28060 | 7604 / 29479 | 35202 |
| 3 | 5926 / 5927 | 6137 / 16521 | 7150 / 39440 | 44901 | 6162 / 6164 | 7079 / 17571 | 7605 / 29479 | 35200 |
| 4 | 5926 / 5927 | 6137 / 16521 | 7149 / 28676 | 34137 | 6162 / 6164 | 7078 / 17570 | 7604 / 29483 | 35205 |
| 5 | 5926 / 5927 | 6137 / 16522 | 7149 / 39440 | 44902 | 6162 / 6165 | 7078 / 28061 | 7606 / 32510 | 38232 |
| 6 | 5926 / 5927 | 6138 / 16522 | 7150 / 50203 | 55664 | 6163 / 6164 | 7078 / 17571 | 7605 / 40422 | 46143 |
| 7 | 5926 / 5927 | 6138 / 6138 | 7149 / 28676 | 34137 | 6163 / 6165 | 7079 / 28060 | 7605 / 40419 | 46140 |
| 8 | 5926 / 5927 | 6137 / 16522 | 7150 / 28674 | 34135 | 6162 / 6164 | 7078 / 17570 | 7605 / 40418 | 46139 |
| 9 | 5926 / 5927 | 6137 / 16521 | 7149 / 28675 | 34136 | 6162 / 6165 | 7078 / 17570 | 7605 / 29478 | 35200 |
| 10 | 5926 / 5927 | 6137 / 16522 | 7150 / 28677 | 34138 | 6162 / 6165 | 7077 / 17569 | 7605 / 40418 | 46139 |
| 11 | 5926 / 5927 | 6137 / 16522 | 7148 / 17911 | 23372 | 6163 / 6165 | 7078 / 17571 | 7604 / 29475 | 35197 |
| 12 | 5926 / 5927 | 6138 / 16522 | 7149 / 17915 | 23376 | 6162 / 6164 | 7078 / 17571 | 7606 / 29482 | 35203 |
| 13 | 5926 / 5927 | 6137 / 16521 | 7149 / 28673 | 34134 | 6163 / 6164 | 7079 / 28060 | 7604 / 40423 | 46145 |
| 14 | 5926 / 5927 | 6137 / 16522 | 7149 / 39434 | 44895 | 6162 / 6165 | 7078 / 17570 | 7604 / 40419 | 46141 |
| 15 | 5926 / 5926 | 6138 / 6138 | 7149 / 50993 | 56454 | 6162 / 6165 | 7078 / 17570 | 7605 / 40423 | 46144 |
| 16 | 5926 / 5927 | 6137 / 16521 | 7149 / 7149 | 21982 | 6162 / 6165 | 7079 / 17572 | 7604 / 29479 | 35201 |

The worst are p50 18,541 µs (rv32, seed 11) and p99 82,497 µs (rv64, seed 6); R10's p99 is
22,812 µs (rv64, seed 6) and the deadline notice's is 37,450 µs (rv32, seed 15), both inside 30 and
40 ms. The lease end's worst is 104,318 µs, inside 125 ms.

The case fails on any `missed`. `bench:sched-latency-tcg` runs the same workload in host time and
only reports, with the oracle still checking every pick.

On hardware (the softcores) the same workload is measured in cycles, from `rdcycle` or `mtime`
at the stated clock rate. That is a characterisation of the board, not a gate.

## Authority

Status: built · tested: bench:sched-carve-inflation, bench:legacy-gone, host:redoubt-model::scheduler_contracts_hold

- **Weight is the only lever.** No call sets a pass, a rank, a priority or a slice, and there is
  no yield: a thread gives up the CPU by blocking, for instance in a `receive` with a timeout.
- **Weight is carved.** A budget's weight comes out of its parent's free weight at
  `budget_create`, by whoever holds a handle to the parent (R7), and goes back when the child is
  destroyed. A budget's free weight rises only when a child it carved is destroyed.
- **Ties are the kernel's.** The budget ids that break ties are assigned by the kernel and never
  reused; no caller chooses one.
- **`kmain`'s switch is not a call.** Its tag lies outside the call table; a user-mode `ecall`
  with it gets `InvalidArgument` like any unknown number ([ABI](abi.md)).
- **Passes are not readable.** No call returns another budget's pass or rank. The scheduling
  trace would, which is why the production kernel has none ([R23](#r23-no-test-channels)).

## Security properties

### R12 (scheduling)

<details><summary>Status: built · partly tested: the bound on a call's kernel time is attacked only for `map_anon`'s search, `map_fixed`'s range and `process_create` · tested (40)</summary>

- bench:sched-share
- bench:sched-sleep-gaming
- bench:sched-idle-gap
- bench:sched-exit-churn
- bench:sched-budget-churn
- bench:sched-carve-inflation
- bench:sched-debt-lift
- bench:sched-timer-flood
- bench:sched-server-busy
- bench:sched-large-weight
- bench:deadline-flood-billed
- bench:sched-carve-return
- bench:map-anon-search-bound
- bench:scan-bounds
- host:redoubt-stride::the_crate_and_the_model_agree
- host:redoubt-stride::a_broken_model_disagrees
- host:redoubt-model::scheduler_fairness
- host:redoubt-model::scheduler_contracts_hold
- mutation:R12PriorityById
- mutation:R12IgnoreWeight
- mutation:R12WakeBanksCredit
- mutation:R12TieQueuedFirst
- mutation:R12RequeueAhead
- mutation:R12RequeueLifo
- mutation:R12PreemptOnWake
- mutation:R12TimeoutWakePreempts
- mutation:R12NoFloorWhenIdle
- mutation:R12ShortRunsFree
- mutation:R12DropRemainder
- mutation:R12ExitRunsFree
- mutation:R12DestroyDropsDebt
- mutation:R12CreateAtFloorOnly
- mutation:R12LiftByMax
- mutation:R12StrideWeightIsLimit
- mutation:R12UnnormalizedLift
- mutation:R12LiftCountsEntryWait
- mutation:R12FoldAtNewWeight
- mutation:R12NoMinimumCharge
- mutation:R12DeadlineWorkUnbilled
- mutation:R12RescaleOnlyOnReturn

</details>

A budget's CPU follows its free weight, in one queue with no priority. While it has a runnable
thread, a budget gets at least its weight's share of the CPU the runnable budgets share. No
pattern of spinning, sleeping and waking, exiting or faulting, creating, carving and destroying
budgets, or arming timeouts and deadlines gets it more. The rule's parts are the sections above:
free weight, the preemption points, the wake rule and ranks, charging and inheritance.

A system call's kernel time is bounded by a constant plus a term linear in the pages it maps or
the objects it names. It never depends on the extent of an address area or on what other
processes hold. `map_anon`'s search is linear in the fixed-size area it searches, a constant,
and never in `len` (`bench:map-anon-search-bound`). A term linear in a fixed kernel constant (`MAX_PROCESS_COUNT`, the platform's
interrupt count, `MAX_DMA_DEVICES`, a fixed table size) is a constant. A term linear in RAM
frames or kernel-object frames is not. Billing it to the caller does not excuse it, because
every wake waits for it. R10 (destruction) walks only the dying subtree, its owner lists, the
dying processes' own tables and page tables, and the chains of the handles held outside it
([budgets](budgets.md#residual-risks)), so it is no exception. What a call looks up by PID or by interrupt number
it finds in an index the kernel keeps as objects are made and freed: a process object in one of
`MAX_PROCESS_COUNT` slots, an IRQ object in one of `MAX_IRQS` (1024, the PLIC's sources; a boot
naming a higher interrupt stops). So `process_create`'s PID draw looks at most at 63 slots, an
owed exit notice is sought among at most 63 process objects, and an interrupt finds its object
in one lookup; a checked build proves each index against a scan of every object frame. In
`bench:scan-bounds`, after one budget fills 20,000 pages with endpoints, `process_create` with
its exit notice and an interrupt take what they took on an empty system; with the old scans, the
first took 1.2 s against 22 ms.

It is attacked three ways:
- **Boot cases, in virtual time**, count each budget's work over a window and compare it with
  its weight's share, within 50 per thousand: spinners at 100, 100 and 300; near-slice, 20 µs
  and long-sleep bursts; a sleeper waking into an idle gap; threads and processes that exit or
  fault just before their slice ends; budget churn; carving; 30 sleepers a microsecond apart and
  64 staggered deadlines; a system server flooded by one user; a weight-1000 server among eight
  users of 100.
- **The differential** drives `libs/stride`, wired as the kernel wires it, and the model's
  scheduler through 3,000 random sequences of creations, destructions (leaf, on the CPU, and
  whole subtrees), wakes, blocks, runs and preemptions, and requires every pass, entry,
  remainder, tie, queue membership, floor and pick to agree after every step. A model with any of
  18 scheduling rules broken must disagree.
- **The model's mutations**: each `R12*` variant breaks one part, and `scheduler_fairness` (ten
  scenarios, each with an independent check) or the scheduler contracts must catch it
  ([model](model.md)).

### R23 (no test channels)

Status: built · partly tested: no case builds the production kernel and checks that it carries no trace, or that a test-only feature is refused without debug assertions

The production kernel carries no test-only diagnostic channel. The scheduling trace is one: a
record of every budget's id and pass at every wake, requeue, pass change, pick and lift, which
tells whoever reads the console who runs when. It exists only under the Cargo feature
`sched-trace`, and no default build enables it:
- the kernel's default features are `print-panics` alone, and `./build` adds only the board
  (`qemu-virt`);
- every trace site in `kernel/src/{sched,budget,mem,main,process,redoubt}.rs` sits under
  `#[cfg(feature = "sched-trace")]`, so with the feature off the ring, its records and its
  `SCHED-TRACE` console lines are not compiled;
- the bench turns it on per case (`kernel_features`), only for `sched-ties`,
  `sched-budget-churn`, `sched-latency` and `sched-latency-tcg`, whose `sched_oracle` post-check
  reads the trace printed at `system_reset`.

The other diagnostic features are off by default in the same way: `sched-inject-tie-fault`, a
debug-only break of the tie rule that implies the trace; `audit-unstamped`, which leaves the audit
after a destruction out of the trace, and `audit-billed`, which bills each audit's time to the
budget that ran it and counts it against its slice, each for one recorded negative run
([responsiveness](#responsiveness)); and `debug-print`, which prints every
pick's PID and thread and every trap. `dma-reset-deaf` is a test-only fault, not a channel
([devices](devices.md)), and so are `handle-chain-fault` and `process-chain-fault`, a handle
installed without its stamp entry or its process object entry for the chain audit to catch
([budgets](budgets.md#residual-risks)), `sum-probe`, a stray
kernel load that must fault ([R24 (SUM and MXR clear)](memory-layout.md#r24-sum-and-mxr-clear)),
and `panic-in-print`, a
panic inside `print!` ([boot](boot.md#failure-and-restart)). Each of these implies the feature
`test-only`, which the kernel refuses to compile without debug assertions, so the release build
`./build` makes cannot carry one; a checked build, as `./build --debug` makes, still can.

## Failure and restart

<details><summary>Status: built · partly tested: a picked thread that dies before the switch, and a full queue, are not attacked by a case · tested (6)</summary>

- bench:sched-exit-churn
- bench:budget-deadline
- bench:sched-idle-gap
- host:redoubt-stride::a_deschedule_charges_at_least_one_unit_and_a_destroy_only_what_ran
- host:redoubt-stride::the_floor_survives_an_empty_queue
- host:redoubt-model::scheduler_stays_fair_past_the_old_pass_saturation_boundary

</details>

- **A thread exits, faults or is killed on the CPU:** its budget is descheduled and charged what
  it ran, at least one tick, like any other deschedule.
- **A budget is destroyed:** bottom-up, each budget is charged what it ran, its work is lifted into
  its parent, and it leaves the queue. A budget destroyed on the CPU (its deadline, with nothing
  descheduling it first) is charged, and the CPU is free.
- **A deadline ends the entering process:** the trap handler resumes whatever is current and uses
  nothing of the dead process (I14 (no call panics the kernel)).
- **The picked thread died since the pick:** `kmain` does not run it and picks again.
- **Nothing is runnable:** `kmain` idles, billing stops, and the floor holds for the next waker.
- **Arithmetic cannot stop the kernel.** Passes are 128-bit and never wrap; a lift saturates
  rather than overflow. The queue has one slot per process, and a queued budget has a runnable
  thread, so it cannot fill; if it ever did, a budget would be left out rather than anything
  stopping (a debug assertion in checked builds).
- **A restarted server** in a fresh budget enters like any child, at `max(floor, parent's pass)`,
  and carries no credit from the one that died. Restarted in the same budget, it keeps that
  budget's pass.

## Residual risks

- **Server work is paid by the server's weight.** Work a server does for a user is paid by the
  server's weight, not the requester's; the steward's work, by the steward. No time is donated, so
  a user who floods a server takes that server's share away from the server's other callers,
  never more: in `bench:sched-server-busy` the server's work for one user stays within its
  weight's share and the other users keep theirs. Servers therefore bound the work of one
  request, and the steward, whose weight is large, bounds its per-request work and relies on its
  per-(account, label set) caps ([steward](../servers/steward.md)).
- **Wakeup is prompt but not bounded.** A wake waits out the running slice, may keep a larger
  pass, and may tie. Human control rests on a measured steward lease-termination latency, not a
  proven bound, until something needs a real-time rule ([TENETS](../TENETS.md#guarantees)).
- **The steward decision wake is late by whole slices.** A wake never preempts: at N = 16 the
  steward stand-in waits out the running slice, then the slices of any budgets that rank ahead of
  it. Its p50 is one or two slices late, depending on where its timeout lands against the running
  slice: about 7 ms or about 18 ms, and any seed on either width can show the 18 ms mode (the
  third sweep above). Its p99 is a wake that lands behind several of the sixteen budgets, so it
  falls in steps of one slice (about 10.8 ms), from about 39 ms to about 82 ms, about 8 slices
  (rv64, seed 6). The exact chain from a seed to its mode was not traced. The targets (25 and 95 ms) are set from the sweep, and
  a lease's end from the steward's decision is 125 ms, not 80. A pinned seed repeats one run; a
  change that moves the phase can land on a worse one than the sweep saw, which the margin covers
  and a new sweep re-measures.
- **The kernel is not preemptible.** A call's or a destruction's kernel time delays every wake
  on the machine, which is why R12 bounds a call's kernel time whoever pays for it. R10's time
  dominates lease termination and follows the dying subtree and the handles that depend on it, so its target
  and the deadline notice's are 30 and 40 ms, not the 39 and 54 ms a whole-frame scan had
  ([budgets](budgets.md)). Ending a DMA driver adds up to `RESET_US` (1 ms) of reset polling for each device it held, at most
  `MAX_DMA_DEVICES` (16) ([devices](devices.md)).
- **A destroyed lineage can delay one sibling by a round.** Debt lifted onto a shared parent (such
  as `users`) can delay one sibling created under it in the same round by at most one round,
  decaying once the floor passes the parent's pass. A lifted pass loses the wake-first tie to
  spinners still at the floor and waits out the round; `bench:sched-debt-lift` bounds a sibling's
  first run at (runnable budgets + 2) slices. Rounding loses under one pass unit per destroyed
  budget, and the loss falls on the budget that churns.
- **Some shares are judged gross of audits.** `sched-budget-churn`'s shares are judged by the
  post-check, net of the audits inside their windows. `sched-exit-churn`, `sched-destroy-billing`
  and `deadline-flood-billed` still judge theirs in the program, gross, so an audit-heavy variant
  could fail one on the audits alone. Follow-up: [todo](../todo/shares-judged-gross.md).
- **Scheduling is observable.** `rdtime` is readable in user mode, so a thread that times its own
  gaps learns how busy the machine is. Timing channels are out of scope
  ([TENETS](../TENETS.md#threat-model)).
- **Measured on QEMU, on one hart.** The targets are guest instructions under `icount`; no
  hardware run is measured, and a hardware run will characterise in cycles, not gate. A target
  set from a sweep holds for the seeds swept, not for every seed. The queue and its accounting
  drive one hart until M2 (usable shell) ([several harts](../plan/m2-usable-shell.md#several-harts)). The cases that read the trace run a
  kernel built with it, which has a record at every queue event and 32 MiB less RAM for the budget
  tree.

## Why

- **Weights, not priorities.** A server ahead of everyone would let one user make `fsd` or `keyd`
  do expensive work while no user budget runs. Large weights from the boot manifest
  ([init](../servers/init.md)) keep `init`, the steward and the drivers responsive without that; a driver that spins while others are runnable is a bug for
  the bench to find, not a mode to support.
- **Free weight.** Carving is how budgets delegate, so a child's share must come out of its
  parent's; counting the limit instead would let a tree of carves multiply its share.
- **The floor.** Without it a sleeper keeps a stale low pass and dominates when it wakes. With
  it, a waker is still prompt: it joins at the front of the current minimum.
- **Wake-first, deterministic ties.** A budget that just woke has usually waited, and one that was
  requeued has just run. Fixed clauses over kernel ids let an independent oracle check every pick.
- **No preemption on wake.** One preemption source, the timer, keeps accounting to a single point
  and makes a flood of wakes cost nothing extra. The price is that wake latency is measured, not
  derived.
- **Instructions, a pinned seed and a sweep.** Host time makes a gate depend on the machine it
  runs on; guest instructions do not. A pinned seed makes a failure replay exactly, and the sweep
  keeps the pinned seed from hiding a worse phase. The lease-end target stays the sum of its two
  parts, so a slower wake cannot hide inside the destruction's allowance, or the reverse.
- **Ticks, an exact remainder, one tick at least.** Whole microseconds would let a run under 1 µs
  go free, and a large weight would round short runs to nothing. Counting at the trap boundary
  means no exit path runs unaccounted.
- **Additive, normalized inheritance from entry.** Otherwise a budget sheds debt by destroying the
  child that ran, or a light grandchild's raw pass stalls an unrelated sibling, or an honest
  parent pays twice for the wait it gave the child.
- **No time donation.** A server thread running on its caller's budget could be stopped mid-call
  by that caller's lease and hold the server's locks for good. Servers pay for their own CPU
  until measured priority inversion says otherwise ([beyond](../beyond/scheduling-extensions.md)).
- **No trace in production.** A per-pick record of every budget is a cross-principal channel that
  the containment claims do not cover, so it exists only in test builds.
