# STEWARD9 report (steward9-implementer-2)

Branch `wp-STEWARD9` on origin/main `dfbf2eeba` (train 76, STEWARD11 in). Worktree
`/home/mcloonan/redoubt/.worktrees/STEWARD9`. Tier A (a few lines of the steward's test-only
latency probe), reviewer steward-red-5. Head: see the end (filled at completion).

## What the node does

STEWARD8 found the latency case's kernel-deadline lease (5 s, twice an older 2.5 s launch) dying
inside its own launch (6.64 s on rv32 at two harts), so the rv32 rows that completed could not be
judged. Delivered: every round's deadline leases are derived from the measured launch and run one
at a time; the probe ends its window on the case's done marker; the five rounds are three cases,
each on its own trace ring; the table on scheduling.md is filled from one run per column at seed 3,
with the two rv32 findings stated with their numbers.

## Delivered, by path

- `servers/steward/src/bin/steward.rs` (`latency-probe` only): the window ends on the
  `latency-done` marker the case writes after its last round (`LATENCY-END`, then the exit for the
  reboot); the compile-time `LATENCY_ROUNDS = 5` is gone, so the case owns its round count and one
  build serves a three-round boot and a one-round boot alike. No kernel, target, D or ring change.
- `tests/steward-latency.toml` (N = 1, 2, 3; both widths), `tests/steward-latency-n4.toml` (N = 4;
  both widths), `tests/steward-latency-n16.toml` (N = 16; `arch = ["rv64"]`). Per-round deadline
  lease = 2 × that round's worst launch over both widths, rounded up to a second: 8 / 10 / 11 / 14 /
  14 s at N = 1 / 2 / 3 / 4 / 16 (N = 2 is 10, not 9: 9 s is the probe's own-lease sentinel); the
  sleep after each is lease + 1 s (the margin over R10's ≤ 30 ms destroy), guest clock. The
  derivations, the per-notice rationale and the ring arithmetic are in the toml comments.
- `tools/testbench/src/case.rs`: `only_steward_latency_builds_the_latency_probe` names the three
  cases (sorted) as the only tomls building the probe.
- `docs/kernel/scheduling.md` "With the real steward": the parameters, the three boots and why,
  the burst and the launch-in-flight residuals, the two rv32 findings, the table; the
  Responsiveness and R12 status lines add the two companion cases (`docs/SECURITY.md`'s R12 row
  matches). `docs/servers/steward.md`: the probe is built by the three cases (the host test's
  claim); the cleanup bullet's parenthesis no longer calls the case's deadlines "clustered".

## Why one at a time, three boots (the Architect's rulings, thread STEWARD8-bound)

A cluster of D deadlines queues D `Exited` events at the steward, dequeued serially (~70 ms
each), so the last notice is ~D × 70 ms late (1.12–1.22 s p99 at D = 16): the steward's serial
dequeue, not the per-notice quantity `deadline_notice` names. One at a time it is ~10 ms. The
sequential shape fills the per-boot trace ring (8,388,608 records) past its end at two harts, so
the rounds are three boots: N = 1–3, N = 4 (3.1 M records alone), N = 16 (rv64 only).

## The six findings

1. **The burst → STEWARD12.** D leases expiring together cost the last sponsor notice ~D × 70 ms
   at the steward's loop (per-exit cleanup, `LeaseEnded` audit, console lines come before the
   next notice). Evidence `.tmp/STEWARD9/cluster-deadline-notice-finding.md` (rv64, both hart
   counts, net p99 1.12–1.22 s at D = 16; the sequential N = 16 round 10 ms). Stated as a
   residual on scheduling.md until STEWARD12 lands.
2. **A launch holds the loop → STEWARD13.** A deadline firing while the steward is inside a
   synchronous `start_agent` (2–6 s) waits behind the launch: `served start_agent in 3926451 us`
   immediately followed by `deadline_notice … 3449138`. Evidence
   `.tmp/STEWARD9/sequential-ring-finding.md` ("Launch-blocks-notice mechanism"). Stated as the
   residual under load on scheduling.md until STEWARD13 lands.
3. **The spinner death at N = 16 on rv32 → READ1.** Beside fifteen spinners one spinner VM
   `exited 1` before the round's first deadline notice (its module reads at the system volume
   past `FILE_WAIT_US`, 10 s, behind the other fourteen's), so the samples group at N = 15.
   Evidence `.tmp/STEWARD9/rv32-dense-smp1-N15.log` line 530 (`lease alice/agent-36 … exited 1`),
   lines 735–743 (the N = 15 counts). `steward-latency-n16` is rv64-only; the rv32 N = 16 column
   states the cause.
4. **rv32 two-hart N = 2 timer_wake → SCHED5.** p50 31,855 µs against 15,000 (p99 35,692, met);
   rerun alone 31,835: deterministic. N = 1 5,643, N = 3 5,159. Every other target of the boot
   met, `dropped 0`, lock waits 249 of 1000. Evidence `.tmp/STEWARD9/rv32-14-FAIL-timerwake.log`,
   `.tmp/STEWARD9/rv32-14-rerun.log`. The rv32 two-hart rows carry their numbers; rv32 is gated at
   one hart until SCHED5 lands (BENCH1's per-width smp).
5. **The probe's length sentinel** (noted follow-up, not a node): the probe knows its own lease by
   `LATENCY_LEASE_US = 9 s`, so no derived lease may round to 9 s (N = 2's 8.94 s → 10 s). A
   distinct token on the probe's lease would remove the fragility; for the next latency-probe
   touch.
6. **A session cannot observe its own lease's end** (noted follow-up, M5 harness; owner Q5):
   `lease_ended` goes to the shell's notices listener, not session code, so the case's spacing is
   derived (lease + 1 s, guest clock) rather than event-ordered.

Plus the WrongObject wrong-destruction the cluster exposed → STEWARD10 (done; STEWARD11 the
Scope arm). Evidence `.tmp/STEWARD9/cluster-wrongobject-finding.md`.

## Runs (seed 3, one run per column; the oracle's lines in the job logs named)

FILLED AT COMPLETION.

## Gates

FILLED AT COMPLETION.

## Documentation check

- `docs/kernel/scheduling.md` "Responsiveness"/"With the real steward": updated (above).
- `docs/servers/steward.md`: the probe's builders (three cases), the cleanup bullet's wording.
- `docs/SECURITY.md` R12 row: the two companion cases, matching the R12 status line as a set.
- `docs/kernel/budgets.md` "Deadlines": cited, unchanged. `docs/testbench.md` (the oracle's
  `LATENCY-END` paragraph names `steward-latency` as the real steward's case): still true,
  unchanged. `docs/userland/beamlet.md` (`MAX_CALL_MS`, `FILE_WAIT_US`), `docs/servers/erofsd.md`,
  `bootfsd.md`, `littlefsd.md`, `verityd.md` (the latency manifest's arithmetic): unchanged, the
  manifest is unchanged. README.md, GETTING-STARTED.md, the milestone pages: no claim about the
  case's shape; unchanged. No dates, no package ids on pages or in case files.

## Open

- BENCH1's per-width `smp` (rv64 [1, 2], rv32 [1]) goes into the three tomls once its hash is
  given; until then the tomls carry `smp = [1, 2]` with the gate stated in their comments, and
  `rv32/steward-latency` fails at two harts on finding 4 by design.
