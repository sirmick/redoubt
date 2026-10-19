# STEWARD9 report (steward9-implementer-2)

Branch `wp-STEWARD9`, head `a633a84d4`, three commits on origin/main `e740361ae` (train 77, BENCH1 in). Worktree
`/home/mcloonan/redoubt/.worktrees/STEWARD9`. Tier A (a few lines of the steward's test-only
latency probe), reviewer steward-red-5. Commits: `bfb4b73a8` steward (the probe), `f2eeacae0` testbench (the three cases, the host test), `a633a84d4` docs.

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

## Runs (seed 3, one run per column; every boot `SCHED-TRACE-END … dropped 0`)

All through `scripts/q` (jobs.mk targets; the rv32 two-hart boots `testbench --exact --arch rv32 <case> --smp 2`
on the prebuilt). The run quoted by the table is the head's binaries and tomls (tree `db395cc63`, which differs
from the head only in scheduling.md's text): logs `.worktrees/STEWARD9/.tmp/STEWARD9/final2/` (`rv64-make.log`,
`rv32-make.log`, the `rv*-steward-latency*.log` job logs, `rv32-*-smp2.log`, `kept/` the failing rv32 two-hart
boot's console). An earlier full set on the same shape (before the probe's stat fix) is in `final1/`, and the set
that found the stat regression in `final/`.

| case | rv64 1 hart | rv64 2 harts | rv32 1 hart | rv32 2 harts (`--smp 2`, recorded) |
| --- | --- | --- | --- | --- |
| steward-latency (N = 1, 2, 3) | PASS 540.9 s, 4,841,622 records (42 % margin) | PASS 589.9 s, 5,525,840 (34 %) | PASS 937.1 s, 5,005,142 (40 %) | FAIL 711.3 s: N = 2 timer_wake p50 31,831 µs (finding 4); everything else met; 5,979,365 (29 %) |
| steward-latency-n4 (N = 4) | PASS 368.9 s, 2,876,371 (66 %) | PASS 454.5 s, 3,711,105 (56 %) | PASS 530.7 s, 2,963,667 (65 %) | PASS 409.2 s, 3,932,389 (53 %) |
| steward-latency-n16 (N = 16, rv64 only) | PASS 155.8 s, 1,022,850 (88 %) | PASS 187.7 s, 1,363,142 (84 %) | not run (finding 3) | not run |

deadline_notice p99 met in every round on every column (8.3 to 15.9 ms against 40); the table on scheduling.md has
every row. The rv32 two-hart N = 2 timer_wake p50 reproduced five times over the day within 30 µs (31,855, 31,835,
31,825, 31,832, 31,831 µs).

**Regression found and fixed on the branch (not on main):** the marker-end probe's first form looked for the
`latency-done` marker (a `stat` at the home) on every idle loop turn; that call landed inside each round's first
probe lease's `reply_wait` window, a 170 to 570 ms sample against ~9 ms for the rest (both widths; `final/`
logs, `reply_wait` p99 166–381 ms). The look is now gated on a flushed round with no lease live, where no sample's
window is open; `reply_wait` p99 is back to 8.7–15 ms at one hart (the two-hart p50 of ~43 ms at N = 1 was in
STEWARD8's table too). This is why the five cases ran twice today.

## Gates (all through q; exit codes from the make summaries)

- `set CASES=formatting` PASS, `set CASES=docs` PASS — on the head (`.tmp/STEWARD9/gates2/head-docs-fmt.log`), after a
  first formatting FAIL on the probe's long `if` line (fixed; `gates2/gates-make.log` has the second set).
- `size-budget` PASS, `unsafe-budget` PASS (no `unsafe` touched; the probe lost a constant and gained a condition).
- `host-tests` PASS (the testbench's own tests, `only_steward_latency_builds_the_latency_probe` with the three
  cases), `steward-host-tests` PASS.
- The steward set, 39 cases both widths (every `tests/steward-*.toml` but the three latency cases and
  steward-host-tests): 77 targets rc=0, `set: 39 cases` (`gates2/steward-set-make.log`; one case is host-only).
- Release checks: `cargo check --release -p redoubt-steward-server --bin steward` for riscv64gc and riscv32imac,
  with and without `--features latency-probe`: rc=0 all four (`gates/release-checks.log`).
- The three latency cases as gated (above).
- Not run: the whole bench (the train's); the sweep of seeds (the page's sweep table is STEWARD4's and unchanged).

The boot cases and the steward set ran on tree `db395cc63`; the head differs from it only in
`docs/kernel/scheduling.md` (the table's numbers from that run), so the guest binaries are those runs'.

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

- The per-width `smp = { rv64 = [1, 2], rv32 = [1] }` is in steward-latency and steward-latency-n4
  (BENCH1's form); steward-latency-n16 is `arch = ["rv64"]` with `smp = [1, 2]`.
- Reading: the three tomls were read in full; steward.rs, case.rs, scheduling.md, steward.md and SECURITY.md by
  their changed regions and the whole diff of each commit, not cover to cover.
- The four tiny differences between this run and the previous one (e.g. rv32 one-hart N = 2 decision_wake p99
  40.5 against 2.2 ms the run before, one sample of 16, met) are the host side's ssh timing; the targets held in
  both.
