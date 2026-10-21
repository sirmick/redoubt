# B101: launch-idle's drift, and why the idle rate grows with host load

Branch `wp-B101`, base 820d5e5ff, head 0dba59657:
- 4b78610a8 kernel: a release of the kernel lock wakes only the hart whose turn it serves
- 1375a23a5 docs: ipd's 8 s park costs an idle box a re-ask per parked request
- 0dba59657 docs: the machine at rest says what launch-idle measures now and why

Evidence notes: `.tmp/B101/notes.md`; raw windows `.tmp/B101/idle/{1,2,e1,e2,s3-*}/windows.txt`
(each run's case log and QEMU trap log beside it); tools `.tmp/B101/bin/` (`idle.sh` runner,
`attrib.py`/`agg.py` trap log by kind and epc function, `seq.py` what each hand-off halt follows,
`idlesec.py` the oracle's idle-section rule on a kept trace). Thread: B101-cause; ruling and its
correction in the rulings file under 'B101-cause'.

## 1. The drift (launch-idle alone, busiest hart, interrupts/s)

Slot 1 (20/20 PASS, no company), two rounds each, rv64 / rv32; user ecalls/s:

| point | rv64 | rv32 | user ecalls |
| --- | --- | --- | --- |
| t27 48023b53f (IDLE2's merge) | 2.1, 2.5 | 2.4, 2.4 | 6.1 |
| t42 6af2d6020 | 2.2, 2.6 | 2.3, 1.9 | 5.9-6.1 |
| t57 9f1cff36c | 2.4, 2.3 | 2.2, 2.2 | 5.9-6.1 |
| t73 41090437b | 2.5, 3.1 | 2.5, 2.5 | 8.1-8.3 |
| t88 3e6f29443 | 2.6, 3.0 | 2.2, 2.6 | 8.1-8.3 |

Slot 2 (16/16 PASS, no company): c87 87553025a (the ceiling's own commit) 2.4, 2.4 / 2.3, 2.4;
t27 2.7, 2.4 / 2.2, 2.2; t62 2.7, 2.5 / 2.1, 2.2; t63 2.7, 2.6 / 2.3, 2.8. User ecalls 6.1 through
t62, 8.3 at t63.

User-ecall split, rv64 one job each (guest-deterministic): t61 6.1, t62 6.0, t63 8.3, t64 8.0,
t70 8.3.

Reading:
- **No merge holds most of the rise.** The ceiling's own commit reads 2.3-2.4 today, against the
  1.6-1.9 it was set from; QEMU 10.2.1 (binary 2026-06-23), the host kernel (booted 2026-10-12)
  and the firmware (2026-09-30) are unchanged since. The 1.6-1.9 does not reproduce.
- **IPD1 (train 63, 4baab6710)**: ipd's park went 30/60 s -> 8 s; an idle box keeps sshd's reader
  and its listener's accept parked (servers/sshd/src/listener.rs: "the accept and each slot's
  parked read"), each now re-asked every 8 s: user ecalls 6.0 -> 8.3/s. Busiest-hart effect
  within the window spread (~+0.2). Stated on ipd.md, not changed (IPD1's ruled margin).
- **Mechanism by kind (trap log by epc)**: m_software is mostly the kernel lock's hand-off (a hart
  halted in `acquire_ticket`, woken by the releaser's `wake_halted`): 3.05/s at t27 against 4.00
  at t88 (rv64 round 2), with idle wakes 1.13 -> 1.42; timers flat (s_timer 1.70 / 1.67).

## 2. Why the rate grows with host load

B98's kept trap logs (ecaeae89f, kernel identical to 820d5e5ff): 20 windows beside the injected
writer, 5 alone. Means per second:

| | alone rv64 | loaded rv64 | alone rv32 | loaded rv32 |
| --- | --- | --- | --- | --- |
| busiest hart | 3.60-3.72 | 2.92-4.88 | 3.25-3.28 | 2.95-5.32 |
| m_software in acquire_ticket | 4.97 | 7.35 | 5.15 | 7.41 |
| m_software in idle | 1.57 | 1.47 | 1.48 | 1.38 |
| s_timer | 2.24 | 1.94 | 1.88 | 1.80 |
| SBI rearm | 5.52 | 4.99 | 5.18 | 4.87 |
| SBI wake_halted | 5.32 | 7.77 | 5.48 | 7.81 |
| user ecalls | 9.71 | 9.13 | 9.67 | 9.17 |

Only the lock hand-off multiplies; no timer is caught up in bursts, guest work is flat. A loaded
host stretches kernel sections in wall time, so more harts arrive while the lock is held. What
each hand-off halt followed on its own hart (rv64): another hand-off halt with nothing between
36 % alone / 41 % loaded (the release woke every marked hart; a ticket lock serves one); a user
ecall 31 / 32 %; an idle wake 22 / 15 %. Guest-side, fixed for the first (below); the second is
the price of queueing the woken hart (below).

## 3. The fix

(i) The release wakes only the marked hart whose ticket it serves (`libs/ticket`, redoubt-ticket,
linked into the kernel; `kernel/src/cell.rs`). The waiter records its ticket, marks (SeqCst),
re-reads `serving`; the release advances `serving` (SeqCst), reads the marks and tickets. Host
test `redoubt-ticket::no_hart_halts_past_its_turn_and_a_release_wakes_only_the_hart_it_serves`:
a deterministic scheduler over real threads runs a release beside a wait, a later ticket's hart
halted beside them, over all 863 orders of their steps; checks no lost wake-up, only the served
hart woken, a woken hart finds its turn. Recorded negative runs:
`cargo test -p redoubt-ticket --features read-before-mark` FAILED (run 65: hart 1 halted, its
turn come, no wake), `--features wake-none` FAILED (run 50, same).

(ii) (ruled, then dropped with the Architect) Sending wake_idle's interrupt at release failed
timer-flood-at-rest on both widths: 668 (rv64) and 666 (rv32) idle sections that found nothing
against 20. Trace: the waker retook the lock and ran the woken thread itself. Reverted; R78 says
why, with the numbers.

## 4. After (slot 3, every job alone; `.tmp/B101/idle/s3-*`, `s4`)

launch-idle, 3 rounds a width, alternating branch (head), base 820d5e5ff, and ka30 (the branch
with ipd's KEEP_ALIVE at 30 s, `.tmp/B101/bin/ka30.sh`). Busiest hart; machine-wide m_software;
by epc (agg.py) hand-off halts in acquire_ticket and idle wakes; user ecalls:

| | busiest hart | m_software | hand-off halts | idle wakes | user ecalls | verdicts |
| --- | --- | --- | --- | --- | --- | --- |
| branch rv64 | 3.0, 3.2, 3.1 | 5.0-5.2 | 3.47 | 1.52 | 9.7 | 3 PASS |
| branch rv32 | 3.0, 3.0, 3.0 | 5.0-5.1 | 3.42 | 1.59 | 9.7 | 3 PASS |
| base rv64 | 3.8, 3.6, 3.9 | 6.4-7.3 | 5.22 | 1.57 | 9.7 | 2 PASS, **1 FAIL (3.9, alone)** |
| base rv32 | 3.6, 3.3, 2.8 | 6.2-7.1 | 5.00 | 1.44 | 9.7 | 3 PASS |
| ka30 rv64 | 2.8, 2.8, 2.7 | 4.1-4.4 | 2.68 | 1.50 | 9.2 | 3 PASS |
| ka30 rv32 | 3.1, 2.6, 2.1 | 3.6-6.0 | 3.18 | 1.41 | 9.2 | 3 PASS |

Main failed launch-idle's 3.8 ceiling alone once (rv64, 3.9): the page's "a rise of about a
tenth" headroom was already gone. With the fix the busiest hart reads 3.0-3.2 and the ceiling
catches a rise of about a fifth. Host cores 0.004-0.005 all three; ceiling 0.01.

**B94's per-probe cost after** (branch minus ka30; four more probes a minute, so a probe = the
per-second difference x 15): m_software +0.87/s rv64 (about 13 a probe), +0.47/s rv32 (about 7,
noisier); SBI calls +1.27/s rv64 (19), +1.17/s rv32 (18); user ecalls +0.5/s (7.5). B94 read 66
interrupts and 84 SBI calls a probe (and +1.0 user ecall/s). The user-ecall difference here is
half B94's, so the probe-rate difference may not be B94's four a minute; read these as per
second first. K35 starts from them.

smp-lock-wait-4-mttcg, calls at once / in turn, 3 rounds a width: branch rv64 1.87, 1.76, 1.83;
rv32 1.77, 1.75, 1.79. Old lock (`.tmp/B101/wt-old4`: the branch with cell.rs and hart.rs as on
820d5e5ff) rv64 2.64, 2.68, 2.69; rv32 2.54, 2.57, 2.56. Bound set at 2.2 (tenths in the program).
Mini-slot on the bound: branch PASS 1.86 (rv64), 1.78 (rv32); **old lock FAIL** 2.66, 2.53 (the
recorded negative, `[lock-wait] FAIL ... (at most 2.2 times it)`).

sched-lock-contention-4-mttcg on the branch: PASS both widths.

## 5. Gates

On 561a3240c+97570d56e (the code as merged, before the bound and page commits), `make -f
scripts/jobs.mk set CASES=...` from `make prebuilt` (rc=0; 367 rv64 / 353 rv32 cases, 0 failed),
all rc=0 on both widths: timer-flood-at-rest (3 / 4 found-nothing sections, bound 20),
irq-boot-hart-only, smp-boot, smp-lock-wait, sched-lock-contention, sched-lock-contention-4,
userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page; rv64:
size-budget (kernel 10842 of 10842, libs/ticket 67 of 67), unsafe-budget, host-tests, docs,
formatting. Both builds: `make -f scripts/jobs.mk build-rv64 build-rv32` rc=0 (an earlier tree);
`make prebuilt` builds both widths on every head since.

On head 0dba59657 (prebuilt rc=0, 0 failed): smp-lock-wait rc=0 both widths (2 harts, 1.60 and
1.35 times against 3.0), docs, formatting, size-budget rc=0; quiet, alone: smp-lock-wait-4-mttcg
PASS both widths; launch-idle and sched-lock-contention-4-mttcg PASS both widths (slot 3, on the
same kernel and programs but the bound's tenths).

Not run: a whole bench (the train's); the steward/SSH idle cases (`steward-ssh-idle`,
`steward-context-idle`) carry no interrupt ceiling.

## 6. Summaries checked

- docs/kernel/scheduling.md R78: updated (status line, the wake, the argument, the idle-wake
  price with timer-flood-at-rest's numbers, hand-off halts at rest 3.4-3.5 against 5.0-5.2, the
  1.7-1.9 marked as when measured, smp-lock-wait-4-mttcg's bound and recorded negative).
- docs/SECURITY.md R78 row: Tested-by equals R78's status line (docs C7).
- docs/kernel/README.md: TCB row and file row for redoubt-ticket, total +323, forbid-unsafe list.
  Pre-existing staleness not refreshed: the kernel's 15,899 lines (16,346 at base) and the total.
- kernel/Cargo.toml comment: names redoubt-ticket among the host-tested pure parts.
- docs/servers/ipd.md: IPD1's cost at rest stated.
- docs/testbench.md: smp-lock-wait-4-mttcg in the host-time lists; 'The machine at rest': the
  ceiling's origin (1.6-1.9 does not reproduce at its own commit), IPD1's and the probes' costs,
  the range with the fix against base, and why the rate rises beside a host writer. The boot-case
  counts in 'Which cases run in guest time' are left to B104 (script handed over).
- tests/launch-idle.toml: its comment records the 2026-10-14 windows as history; the page carries
  today's range; no change.
- docs/kernel/timer.md, kernel/README.md (crate), README.md, GETTING-STARTED.md: no claim about
  the lock's wake (grep 'wake_halted\|each marked hart\|reschedule interrupt'); no change.
