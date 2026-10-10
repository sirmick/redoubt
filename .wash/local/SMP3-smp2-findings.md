# SMP3: the whole bench at `--smp 2`, rv64 (head 0da4c37d2 on main 91256b4d0)

Run: `q run --cores 8 -- target/prebuilt/testbench --prebuilt target/prebuilt --arch rv64 --smp 2`.
241 PASS, 40 FAIL. One FAIL was mine and is fixed (formatting of case.rs, uncommitted). rv32 is
running now.

## Baseline: the same failures on main + the `--smp` option alone (1bb8d20c2)

Each failing case rerun at `--smp 2` on 1bb8d20c2 (SMP1's one-runner kernel), but the three long
ones (boot-profile, boot-profile-unverified, kernel-containment: 15-30 min timeouts on the branch).

**Fail on main too (30), so not SMP3's:**

- `init: refused the boot: <steward|beamlet-session>: could not make a fresh connection for`:
  beamlet-launch, beamlet-natives, beamlet-natives-attack, beamlet-serve, verity-flipped-tree,
  verity-wrong-root (both widths for the beamlet four). A real 2-hart bug on main, not attacked
  until now: init's 9P new_connection through a minted badge fails at boot.
- The scheduling oracle and the shares, judged on one hart (R12 across harts is SMP2's):
  endpoint-destroy-full, sched-budget-churn, sched-carve-return, sched-debt-lift,
  sched-exit-churn, sched-latency, sched-latency-tcg, sched-lift-delay, sched-timer-flood,
  sched-wake-no-preempt (oracle: "rank clauses put budget N first" — the oracle replays one runner);
  sched-cluster, sched-destroy-billing, sched-idle-gap, sched-large-weight(-release), sched-share(-release),
  sched-sleep-gaming, sched-ties, deadline-flood-billed (shares/orders on one hart).
- Timing bounds and attacks: budget-deadline (timeout), expiry-deadline-then-timeout,
  redoubt-ipc-attack (timeout), scan-bounds.

**Pass on main, fail on the branch every time (5), SMP3's:**

| case | what fails | cause |
| --- | --- | --- |
| bench-poweroff-missing | the 3 s self-check never reaches `IPC TEST PASSED` | log-server and ipc-client share the `system` budget; at 2 harts they now ping-pong across harts (a reply wakes the client, the IPI wakes the idle hart, which spins on the lock). Under icount (harts take turns on one host thread; a spinning hart spends its turn) the same run takes 8.2 s against 1.3 s on main. Without icount `ipc` at 2 harts takes 2.5 s on both. |
| map-anon-search-bound | "a refused search over every page table" over 12 ms | its sleeper thread now blocks on the other hart; its timer fires there mid-search and that hart spins on the lock; under icount the spin advances guest time, which the search's own `time_now` window counts |
| receive-bad-record | the first receive returns Ok after 13 ms, not InvalidArgument | the call is abandoned (40 ms timeout) before the main thread unmaps the record: the server thread took ~27 ms from taking the call to blocking, because its harts contend the one lock with the main thread's 1 ms sleeps (each idle runs the checked build's IPC-list audit) |
| timeouts (icount), timeouts-tcg | rdtime "linear in time_now" off by 2-30 % | the `read_time()`/`time_now()` pairs are not atomic: the syscall now waits for the lock held by the other hart, which is handling the same wake |

All five are measurements a sibling thread on another hart now perturbs: the kernel answers
correctly, the cases measured one hart's timing. Two options per case: mark `keep_smp` with that
reason, or make the case robust (receive-bad-record: block-before-unmap handshake or a longer call
timeout; timeouts: retry a pair until it is tight; map-anon: exit the sleeper before the timed
search). bench-poweroff-missing is a bench self-check of `poweroff`, not about harts: `keep_smp`.

## A cost the sweep shows

Synchronous IPC between two processes of one budget now crosses harts at every reply, with an IPI
and a lock handoff. Free on mttcg (2.5 s both), sixfold under icount. A wake-affinity rule (no IPI
when the waker will block next) would be a policy change to the brief's "a wake sends the idle hart
an IPI, also for a budget running elsewhere", so it is not made here.
