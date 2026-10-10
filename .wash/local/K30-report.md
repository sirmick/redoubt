# K30: two stalls at 2 harts on main (budget-deadline, redoubt-ipc-attack)

Branch wp-K30: head bcc38acc5, ONE commit on main 7a575c183 (worktree
/home/mcloonan/redoubt/.worktrees/K30): tests/programs/src/bin/budget-deadline.rs only (LEASE_US
= 500 ms for both leases; the go sent with a 2 s timeout, its result in the first verdict). No
kernel change. The red's verdict at 7d972e9a5 was OK with notes; its note dropped the
redoubt-ipc-attack timeout change from K30: main runs that case at one hart only, where its 20 s
guard (8x) must stay, so SMP3 takes the two-hart timeout instead (instruction 761c7a3f). Reproduction worktree /home/mcloonan/redoubt/.worktrees/K30-smp2: detached at
d0a19ac6c with SMP3's bench commit bad22440c (`--smp N`) cherry-picked UNCOMMITTED, plus local,
uncommitted kernel stamps (`println!("K30 H{hart} ...")`) and case-timeout edits for measurement.
Runs: `q run --cores 2 --tenant K30 -- target/prebuilt/testbench --prebuilt target/prebuilt
--exact --arch rv64 --smp 2 <case>`; logs in K30-smp2/target/jobs/k30-*.log and
K30-smp2/target/testbench/run-*/.

## Verdict: neither is a kernel stall

No lost wakeup, no lost IPI, no deadline left unarmed, no wrong result. Both are the cases' own
timing assumptions meeting the cost of two harts under one lock under icount: QEMU's
round-robin TCG runs one hart at a time, and a hart spinning on the kernel lock (`acquire`,
or an idle hart woken by a timer or IPI) spends its whole vCPU quantum of *virtual* time, so
every kernel entry that meets the other hart in the kernel costs tens of ms of guest time
(SMP3's finding: "a spinning hart spends its turn"). Both pass unchanged on one hart from the
same build (ipc-attack 10.6 s, budget-deadline 1.0 s).

### redoubt-ipc-attack: slow, not stuck

Unchanged case at `--smp 2` rv64: FAIL at its 20 s timeout after `buffers`. With
`timeout_secs = 180` and otherwise the same build: **PASS in 33.2 s**. The step after
`buffers` is the flood: 2,000 `call(E, .., timeout 0)`. On one hart the victim never runs
between the attacker's polls (same hart), so almost every poll times out at once and the flood
takes milliseconds. On two harts the victim runs on the other hart: each poll that lands wakes
it (`wake_idle` -> IPI), it replies and re-enters `receive`, so the next poll lands too: 2,000
cross-hart round trips, each an IPI and two lock handoffs. Stamped run (44 s guest): 1,034
attacker polls, 430 victim deliveries, 1,294 `wake_idle`, 436 IPIs, zero timer expiries.
Kernel PIDs are random (pid pinning); victim/attacker were 184/499 and 338/459 in two runs.

### budget-deadline: the lease dies before the test program is ready for it

Program flow: create lease `timed` with deadline now+100 ms, spawn two children into it (the
bystander, then the spinner, which waits for a `go` message), sleep 10 ms, `send(go, FOREVER)`,
then `receive(exit, 2 s)` x2 and the `[deadline]` lines. At one hart the two spawns take a few
ms; at two harts under icount they take ~85 ms (hundreds of kernel entries, each contending
with the other hart's startup and idle wakes). Three failure modes, one root, seen as the
stamping varied (the stamps themselves cost ~5 ms of guest time a line: the 16550 transmits at
baud rate under icount, so heavier stamping shifts the mode):

1. **The reported hang** (unstamped run; light stamps): main's `send(go)` traps at 382,436 us
   with the deadline at 381,348 us; the entry's expiry destroys the lease first (both children
   terminated, the spinner still blocked in `receive(go)`), preempts main (R12: a deadline is a
   preemption point) and main's re-executed `send(go_client, FOREVER)` queues on main's own
   live endpoint `go` whose only receiver is dead: `block 2:1 wait=Send deadline=NEVER`, for
   ever. No `[deadline]` line, nothing armed, both harts idle. The kernel is right: `go_client`
   is stamped with main's budget, not the lease's, so R10 does not fail the send.
2. **`[deadline] lateness LATE`** (minimal stamps, both widths, 0.8 s): the children are killed
   on time but the notices reach main 60,194 us (rv64) / 108,532 us (rv32) after the deadline,
   against "destroying by hand took 30,538 / 34,666 us; bound 1,000 us more". The late part is
   main being woken and picked across harts and the lock handoffs; a 1 ms margin is one hart's.
3. **Panic -> park** (heavy stamps): the deadline fires mid-spawn, the second
   `spawn(..).unwrap()` fails on the dead lease, the panic handler parks (`block 2:1
   wait=Sleep deadline=NEVER`). Same silence as 1.

Timeline evidence (minimal stamps, rv64): spinner `block 370:1 wait=Receive` 353,396;
main's 10 ms sleep expires 361,061; `expire deadline frame 609` 383,091; Terminating 370, 286.

## For SMP3 (orchestrator's answer 9a012426: lands in the commit that introduces `keep_smp`)

Add to tests/budget-deadline.toml, beside `timeout_secs`:

```toml
# The lateness clause (notices within LATE_US, 1 ms, of what destroying the lease by hand costs)
# is a one-hart target: on two harts the notices reach the program 95-146 ms after the deadline
# against 30-35 ms by hand, the cross-hart wake and the kernel-lock handoffs under icount (K30).
# SMP2 restates the target across harts.
keep_smp = true
```

## The fix (orchestrator's choice, answer 8c8b4c0a) and its gate

- redoubt-ipc-attack: no change in K30 (above); the measurement stands for SMP3: 28.7 s rv64 /
  25.1 s rv32 at two harts from the fixed tree, 20 s guard on one hart.
- budget-deadline: the leases get 500 ms (room for the setup on two harts), the go is sent with
  a 2 s timeout so a lease dead first fails the case with a line, not silence. The 1 ms lateness
  bound stays a one-hart target: `keep_smp = true` is to mark it, but main's case loader is
  `deny_unknown_fields` and `keep_smp` exists only in SMP3's unmerged bench commit (2b5d757c3
  after its rebase), so the key cannot land on a branch off main without failing every load of
  the case; asked the orchestrator (question 0baffd4d) whether it follows SMP3's merge.
- **For SMP2's list (plan body):** budget-deadline's lateness clause (`late <= cost + LATE_US`,
  LATE_US 1 ms) is a one-hart target; at two harts the notices reach the program 95-146 ms after
  the deadline (by-hand cost 30-35 ms), the cross-hart wake and lock handoffs under icount. SMP2
  restates it across harts.

Runs of the fixed cases from the scratch worktree (main 7a575c183 + SMP3's bench commit + the
fix, uncommitted; `q run --cores N -- target/prebuilt/testbench --prebuilt target/prebuilt
--exact --arch W --smp N <case>`), logs K30-smp2/target/jobs/k30-fix-*.log:

| case | rv64 smp 1 | rv64 smp 2 | rv32 smp 1 | rv32 smp 2 |
| --- | --- | --- | --- | --- |
| redoubt-ipc-attack | PASS 2.4 s | PASS 28.7 s | PASS 2.2 s | PASS 25.1 s |
| budget-deadline | PASS 1.1 s | FAIL 1.0 s: lateness LATE (146,399 us after; by hand 30,538) | PASS 0.7 s | FAIL 0.8 s: lateness LATE (95,120 us; by hand 34,666) |

budget-deadline at two harts now fails in under a second on the lateness line instead of
hanging 120 s: the hang is gone; the loud failure is the one-hart bound keep_smp covers.

Gates on wp-K30 7d972e9a5 (jobs.mk / q): formatting PASS 20.7 s; docs PASS 3.6 s; the smoke set on both widths all
PASS: userland-boot, init-boot, ipc-outcomes, sum-clear, lend-untouched-page (1 and 4 harts),
bench-net-peer; budget-deadline and redoubt-ipc-attack at 1 hart on both widths PASS
(logs K30/target/jobs/k30-smoke-*.log, summary k30-smoke.txt). Not run: the kernel package's
full gate (tests only, per the orchestrator), rv32 of the two cases at 2 harts beyond the
scratch runs above (same binaries).
