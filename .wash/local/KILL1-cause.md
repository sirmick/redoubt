# KILL1 phase 2: the cause. The victim is killed by ITS OWN deadline, which the guest's clock jumped to

Branches: `wp-KILL1` (f2eadc401, on origin/main ddafcad7d: the model contract) and the probe
branch `kill1-probe` (0237ceb2a: kernel feature `kill-probe`, a scratch case
`tests/lease-code-load-kill-probe.toml`, rv64 two harts). Logs under `.tmp/KILL1/`: `probe1/`,
`probe2/`, `probe3/` (console and session of three boots), `probe-boot-N.log` (the make).

## (1) The kernel model cannot reach it, by construction

`model/tests/destroy_contracts.rs` → `common::contracts::sibling_destroy_spares_neighbours`:
sixteen children of one parent under `users`, a process each reporting to init's endpoint E,
each ending as a lease ending by exit does (its process exits 0, init takes the notice, then
`budget_destroy`), in the gate's three orders, then newest first, then interleaved, the rest in
start order after; after every end each child still running holds its budget counting its one
process, and its own end is still its own exit. Passes (`.tmp/KILL1/model-siblings-3.log`). The
model's objects are maps keyed by never-reused ids and a destroy's kills are `p.budget == x`
lookups (model/src/kernel.rs `destroy_budget`): it has no frames, slots or chains to match
wrongly, so it can neither reach nor test a walk leaving a subtree. It stays as a contract of the
rule, not as this bug's test.

## (2) The probe: every kill is a deadline destruction of the victim's own budget

Kernel feature `kill-probe` (test-only, as `sched-trace`): at `mark_dying` the top, its links,
its parent's child list before the unlink and every process object the subtree counts (frame,
pid, alive, counted_in frame:id, creator, the budget it runs in); at each kill the victim with
the budget it runs in, the caller and whether the destruction is a deadline's; at `new_budget`
the id, frame, parent, deadline and `now`; at `expire_due`'s deadline branch the deadline, id,
frame, `now`, the timer hint and the first due timeout.

Three boots of the gate (rv64, two harts, `icount = "shift=3"`, seed 3), the kill in every one:

| boot | ends before the kill | the kill | the probe's line |
| --- | --- | --- | --- |
| 1 | 1–11, 15, 12 | agent-13 | `destroy of budget 175 frame 98265 kills pid 372 running in Some((175, 98265)) (caller Some(372), deadline true)`; the chain of 175 holds one object, pid 372, `counted_in 175:98265 runs_in Some(98265)` |
| 2 | 1–12, 15, 14 | agent-13 | `deadline 1912083218 of budget 175 frame 99733 due at now 1912083242 (hint 1912083218, timer None)`; budget 175 was created `deadline 1912083218 at now 112084737` |
| 3 | 1–12, (15 exited), (14 exited) | agent-13, then 14, 15, 16 | the same shape four times, each lease at its own deadline, 1.4 to 4 s apart |

So the walk never leaves the destroyed subtree: the destroyed top IS the victim's budget, its
counted chain holds exactly the victim's process object, counted and running there, and the
parent's child list is intact (the probe prints it at every destroy, by id and frame). The
destruction is R10 at a **deadline** (`deadline_since` set; the caller is whichever process's
kernel entry the expiry ran at: the victim's own in boot 1, another lease's in boots 2 and 3).
R10 and R39 hold as written: the lease ended at its deadline.

## (3) The deadline was 1.8 × 10⁹ µs after the lease's start, and the clock jumped to it

Each lease's budget is created with `deadline = now + 1_800_000_000` (the session's
`start_agent(c, 1_800_000_000, …)`; boot 2's creation lines: 1912083218 − 112084737 =
1.8 × 10⁹ exactly). The steward's audit before the kill is stamped `at 575133051` (boot 2) and
`at 657897367` (boot 1); the expiry that kills reads `now 1912083242`, 24 µs past the deadline,
and the steward's audit of the kill is stamped `at 1910270727` (boot 3, whose console runs on
past the kill): the guest's `time` CSR (`arch/riscv/timer_sbi.rs`, `rdtime` since boot) went
from ~575 s to ~1912 s between two console lines, and stayed there. Wall time of boot 2: 295 s
for the whole case (`probe-boot-2.log`). Not a steward or kernel arithmetic: the kernel reads the
counter, the steward reads `time_now`, both agree before and after.

The jump lands on the earliest armed timer. At that instant the kernel's timer hint for budgets
was this deadline (`hint 1912083218`) and no thread wait with a timeout was due even at the new
`now` (`timer None`: `collect_due` found nothing ≤ 1912 s), so nothing nearer was armed: every
hart idle, no disk request in flight (blkd's waits carry its 10 s request timeout), no slice,
no VM timer. QEMU `-icount` skips an idle guest to its next timer deadline; the bench runs this
case with `icount = "shift=3"` (sleep on), which docs/testbench.md says keeps idle time the
host's. QEMU 10.2.1's `icount_start_warp_timer` (accel/tcg/icount-common.c, fetched to
`.tmp/KILL1/qemu-src/`) jumps the bias by the whole deadline only with sleep off; with sleep on
it arms a real-time warp timer and `icount_warp_rt` adds the real time elapsed since the warp
started, uncapped at a fixed shift. The observed jump is instantaneous and exact (deadline +
24 µs), which neither branch explains by itself; I have not identified QEMU's path and do not
need to: the measurement is the guest counter's.

Why the lowest lease in start order: it has the earliest deadline (leases start 3.4 to 7.6 s
apart with one term), and `expire_due` destroys the earliest due one. Why right after a
neighbour's clean end: that is when the guest falls fully idle with no timer armed but the lease
deadlines. Why one per gate boot: the forbid stopped the case at the first; with it lifted (boot
3) the remaining leases die at their deadlines in turn, 1.4 to 4 s of guest time apart, none
getting the ~75 s its load still needed. rv64/1 in gate10 (the SSH connection closed by the
guest after the sixteenth launch, the shell idle) fits a clock jump as well.

## (4) What is open: was the idle instant benign, or a wedge?

A loading VM is runnable or waits on a 9P reply; erofsd, verityd and blkd are runnable or wait on
the disk with a timed wait. An instant with every hart idle and no timer but the lease deadlines
means the three remaining VMs were blocked untimed with no server holding their request, or a
host-side completion was in flight at the moment QEMU judged the guest idle. Boot 4
(`probe-boot-4.log`, `probe4/`: the same case on the host clock, `icount` removed) separates
them: **PASS in 78 s of wall time, all sixteen `exited 0`** (1–13, 16, 14, 15), no deadline
expiry (the probe's deadline line never prints), the session's gate line answered. So the
instant is benign (a disk completion or an interrupt in flight on the host side while every
vCPU was idle), and the whole finding is the bench's virtual clock: under `icount` the idle guest
is skipped to its next timer deadline, which with nothing else armed is the oldest lease's.

## (5) What this means for KILL1's remedy (for the Architect)

- The kernel's destruction walk is bounded to its subtree; nothing in `destroy_subtree`,
  `subtree_next`, the counted chains or the process-budget match is wrong. No kernel fix.
- The steward's arms were excluded in phase 1 and stay excluded.
- `lease-code-load`'s verdict channel saw a lease end at its deadline because the bench's guest
  clock reached it. The case cannot hold "no lease ends" under a clock that can jump to the next
  lease deadline whenever the guest idles. The remedy is the case's (and the page's claim about
  `sleep` on): the host clock (boot 4: 78 s, repeatable closely, not exactly), or leases with no
  deadline the clock can jump to, or a guest that is never timer-less while a lease runs; which
  one, and whether docs/testbench.md's "with sleep on, idle time is the host's" is withdrawn or
  qualified (the jump was seen with sleep on, on QEMU 10.2.1, at an instant with no timer nearer
  than the lease deadline), is the Architect's call. The same clock reaches every icount case
  that holds a lease or a far deadline across an idle instant: steward-latency's and the burst
  cases' leases die by deadline by design, but a lease meant to outlive the case cannot be
  relied on under icount. The probe is removed before the merge unless kept behind its feature
  per the kernel's convention; the model contract stays as R10's sibling contract.
