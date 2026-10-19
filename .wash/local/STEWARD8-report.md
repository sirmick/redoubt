# STEWARD8 report (steward8-implementer)

Branch `wp-STEWARD8` on origin/main `aaa0e75e2` (train 72), head `1f089f5eb`: `2c2ff8735`, `4060543e8`,
`1f089f5eb`; worktree `/home/mcloonan/redoubt/.worktrees/STEWARD8`. Reviewers: steward-red-5 (the
package), kernel-red-3 (the ceiling commit: OK with its P2 folded).
Evidence under `.worktrees/STEWARD8/.tmp/STEWARD8/`: `notes.md` (the working notes, every
figure with its log), `measure/` (the measurement runs), `verify/` (the verification runs and the
oracle judged by hand), `served.awk` and `launches.awk` (the extraction), the gate logs. Head: see
the end.

## Rulings built on

- F4 (`.wash/local/wave-2026-10-18-rulings.md`), its refinements on thread `STEWARD8-bound`
  (the served line is the source; `launch_timeout/0` beside `timeout/0`; agents.md's harness
  clause; the per-message wire-table default deferred) and "F4 — the VM's call ceiling"
  (`MAX_CALL_MS` becomes the launch bound, one number with the client's; async `start_agent` the
  durable follow-up, recorded by the Architect).
- The orchestrator: the served line is STEWARD8's own first commit, the node re-tiered to Tier A;
  the native change its own commit, reviewed by kernel-red-3 (its P2 on `pool.rs:13` folded);
  measure on wp-STEWARD7's head so the bound covers the N = 16 round; the bound is derived once
  and never re-derived over a later run.

## Three commits

1. `2c2ff8735` steward: `served_op` names opcode 10, so `steward: served start_agent in N us`
   is printed per launch, one queued console line; steward.md's console-lines bullet says so.
   No other steward change; `tests/size-budget.toml` servers/steward 3539 → 3540 for the line.
2. `4060543e8` client: `Redoubt.Wire.Client.launch_timeout/0` (12 000 ms) beside `timeout/0`
   (5 000), the arithmetic in its comment; the latency case's session passes it as
   `start_agent`'s third argument; `client_test.ex` gains two checks (the bound reaches the
   native as passed; the platform's `{:error, :timeout}` comes back as is). Pages: steward.md
   "Residual risks" (the launch's cost beside `end_lease`'s 589 ms, the bound's arithmetic,
   `SERVER_US`'s "under every caller's own bound" with 12 s), agents.md "Leases" (how long a
   launch takes under load, the bound) and "The agent harness" (`Agent.start` uses the launch
   bound), wire.md "Generated clients" (the timeout argument, the deferred per-message default),
   scheduling.md "With the real steward" (below), `tests/steward-latency.toml`'s header.
3. `1f089f5eb` beamlet: `MAX_CALL_MS` 5 000 → 12 000 in `userland/otp/vm/src/bif/system.rs`
   with the derivation; `natives.erl` makes one call at the ceiling (`call/0`) and raises on one
   past it (`wrong_types/0`, 12001), `natives.beam` rebuilt with the pinned erlc, `tests/system.rs`
   asserts the ceiling call reached the platform; beamlet.md's constants row and the natives'
   paragraph state the derivation and the cost (one of the VM's own call threads held up to 12 s,
   inside its own trust domain); `pool.rs`'s comment names the constant. `MAX_CALL_MS` had no
   other dependent (grep); the pool takes the microseconds the native passes.

## What was measured (served `start_agent`, worst per group, one run at seed 3)

Measurement runs on `0d8d9d463` (wp-STEWARD7 `99c0e1f32` + the served line; `measure/`):

| | alone | beside 1 | beside 2 | beside 3 | beside 15 |
| --- | --- | --- | --- | --- | --- |
| rv64, 1 hart | 1.702 | 2.251 | 2.427 | 3.052 | 3.132 |
| rv64, 2 harts | 2.672 | 2.463 | 3.038 | 3.620 | 3.716 |
| rv32, 1 hart | 2.281 | 3.533 | 3.862 | **5.851** (the 13th of 13; the twelve before 2.76–3.93) | not reached |
| rv32, 2 harts | 3.861 | 4.515 | 5.151 (the 4th of 4) | not reached | not reached |

The 5 s bound ended the rv32 rounds at N = 4 (one hart, the 5.851 s launch) and N = 3 (two harts,
the 5.151 s launch), as STEWARD6 and B88 saw. The bound: 2 × 5.851 s = 11.702 s → **12 s**. The
13th launch of the rv32 one-hart round against the twelve before it: the steward's lines show no
difference (no parked step, no cleanup fate, no restart; the same end-of-the-previous-lease
sequence), so the extra seconds are inside the batch's calls at the shared servers, which the
lines do not itemize (STEWARD6's removed instrument had the carve at walfsd at 1.66 s in that
round): B88's compute finding at one hart, a spread of 2.76–5.85 s over thirteen launches.

B88's gap measure (AgentStarted gaps less the sleep) beside the served time: on STEWARD7's rv64
one-hart run it gave 3.78 s beside three and 6.83 s beside fifteen where the served time is 3.05
and 3.13 s: the shell VM's own share and its sleep's lag, which is why the served line was needed
(`launches.awk`; stated on the page).

Verification runs on the ceiling commit as first made, `33a3e2ae6` (only comments and the size-budget line moved since; `verify/`; rv64 PASS 654 s and 789 s): rv64 one hart
1.698 / 2.228 / 2.561 / 2.996 / 3.127 s, two harts 2.672 / 2.888 / 3.648 / 3.947 / 3.769 s; rv32 one
hart 2.273 / 3.530 / 3.904 / 4.971 / 5.198 s, two harts 3.842 / 4.469 / 5.091 / **6.615** / 6.256 s.
Every launch within the bound; the rounds it ends no longer end. The worst launch now measured,
6.615 s beside three on rv32 at two harts, is within 12 s at 1.8×; the bound is not re-derived
(the orchestrator's ruling: that would be the tuning the rule forbids), and the page says which
rounds the derivation covers.

## The rows

- rv64, both hart counts: every round N = 1 to 16 complete, every target met, 0 `parked after`
  lines in any run (finding 4 of STEWARD7 closed: the 29th-launch refusal behind parked
  disconnects does not recur with the cleanup thread; at one hart the N = 16 timer wake now
  meets its targets, 1.1 ms p50/p99 against 54.5/57.3 before the thread). Cells from
  `verify/rv64-job.log`.
- rv32, one hart: N = 1 to 4 complete, every target met (the oracle run by hand on the kept log,
  `verify/rv32-smp1-oracle.log`; the boot's own verdict FAIL on `expect_after` N=16).
- rv32, two harts: N = 1 to 4 complete for the first time (16 samples each), launches within the
  bound; **not judged**: the boot ended in the N = 16 round before the reset's trace dump (below),
  and the oracle refuses a log without `SCHED-TRACE-END`. Stated so on the page, with the cause.
- The N = 16 round on rv32, either hart count: not reached, cause now measured. Beside fifteen
  spinners a launch there is 5.2 s (one hart) and up to 6.3 s (two), longer than the case's
  five-second leases ("twice the two-hart launch" when that was 2.5 s). One hart: the deadline
  leases die inside their launch (no `deadline_notice` line; probes 5 of 8), one spinner VM
  (`agent-135`) exited 1 mid-round (its module reads at the system volume behind the others:
  STEWARD7's finding), after which the samples group at N = 15, the deadline notice's p99 there
  45.1 ms over 8 samples: missed. Two harts: the 10th launch beside fifteen, served 5.68 s,
  failed its `Launch` step `NotPermitted` (the lease's deadline passed before its process was
  made), a forbidden line, the boot ended. A deadline passing inside the launch is B82's
  territory: a kernel case pinned what a deadline does between a budget's creation and its
  process's start (the deadline runs from the request), so the steward's `NotPermitted` here is
  the stated behaviour and the forbidden line the case's own; the fix is the case's lease
  length derived from the measured launch, the orchestrator's (A), ruled by the Architect as
  STEWARD9 (the deadline lease and its sleep from the same launch measurement; Tier B unless the
  deadline class turns out to be a probe constant in the steward binary).

## Finding 5 check (the Architect's, via the orchestrator)

No liveness bug by the need-against-share test at any supported count. Facts: erofsd's bucket
caps `pages 2`; a share may hold `max(1, 2 / (others + 2))` = one page always (`admit.rs:362`);
what pins a page is a T-message longer than the 24-byte words (`WORDS_BYTES = 3 * 8` on both
widths): a `Tread` (23 bytes) pins none, a module's `Twalk` (`Elixir.Inspect.List.beam`: 45 bytes)
pins one until answered. So the need is one page per waiting module walk and the share always
allows one; the account's ceiling is the bucket's total, two page-carrying requests per
(account, label set) per volume at once, a third `busy` and retried by the hub until its 10 s
give-up. That is a drain-rate matter: it held for four VMs of one account on rv32 at one hart
(the N = 4 round's module loads) and failed for seventeen behind fifteen spinners; where between
four and the image's nine it would bite on rv32 at one hart is not measured; on rv64 seventeen
were served. The term that would scale it is `pages`/`requests` gaining the VM-count term as
`state` did (the Architect's deferred package).

## Gates (every command through `scripts/q run` / `scripts/jobs.mk`; logs in `.tmp/STEWARD8/`)

| gate | result |
| --- | --- |
| steward built with `latency-probe`, both widths (`build1.log`, `build2.log`) | rc=0 |
| steward host tests, `cargo test -p redoubt-steward-server` (`host1.log`) | 73 passed, rc=0 |
| `elixir-oracles` rv64 (`target/jobs`), and `run-vectors` alone (`vectors1.log`): the two new client checks `ok` | PASS, rc=0 |
| `beamlet-vm` system tests (17) and `beamlet-vm` + `beamlet-redoubt` whole (`vmhost1.log`, `vmhost2.log`) | rc=0 |
| `steward-latency` measurement, rv64 both hart counts (`measure/`) | PASS, PASS |
| `steward-latency` measurement, rv32 (`measure/`): the one-hart N = 4 and two-hart N = 3 loops died on the 5 s bound as expected; the wedged boots killed by hand after their figures were read | FAIL (expected, the finding) |
| `steward-latency` verification on the ceiling, rv64 both hart counts (`verify/`) | PASS 654.2 s, PASS 788.9 s |
| `steward-latency` verification, rv32 one hart (`verify/`) | FAIL: `expect_after` N=16 (the lease-vs-launch finding); rounds 1–4 judged by hand, every target met |
| `steward-latency` verification, rv32 two harts (`verify/`) | FAIL: forbidden `a step failed` in the N = 16 round (the same finding); rounds 1–4 complete, not judged |
| `set CASES="formatting docs size-budget unsafe-budget steward-lease-ends steward-end-lease agent-login shell-compile beamlet-footprint"` both widths (`gates2.log`; `gates1.log` was the first try: formatting failed on `pool.rs`'s wrap and size-budget on servers/steward 3540 of 3539, both folded) | 14 of 14 PASS, rc=0; `Size budget: servers/steward: 3539 -> 3540` in the served-line commit |
| `./test-shell` under `q` (`test-shell1.log`) | every stage passed, rc=0 |

## Documentation check (summaries searched for the changed claims)

- README.md, GETTING-STARTED.md, SECURITY.md, docs/plan/README.md, docs/plan/m3-agents.md
  (`start_agent` "runs on the machine", still true), docs/servers/README.md, docs/userland/shell.md:
  no claim about the call bound, the launch's time or `MAX_CALL_MS`; unchanged.
- docs/testbench.md: names `steward-latency` as a case; no figure of the launch; unchanged.
- docs/servers/erofsd.md, verityd.md: the finding-5 arithmetic is the report's, not a page's
  (the Architect's ruling: stated unreached, a node only on the owner's decision); unchanged.
- No new R<n>; SECURITY.md unchanged. No dates or package ids on the pages or in the case.

## Open risks and follow-ups

- The case's deadline-lease length (5 s) is shorter than a launch beside fifteen spinners on
  rv32; the rv32 N = 16 round cannot be measured until the lease is derived from the launch.
- The rv32 two-hart N = 1 to 4 rows are complete but unjudged until a run reaches the reset.
- `MAX_CALL_MS` at 12 s lets one call hold one of a VM's two call threads 12 s; the Architect's
  (b) refinement (a per-call ceiling the launch path elects) if interactivity measurement shows it.
- Async `start_agent` (the launch's outcome by a notice) is the durable shape, recorded by the
  Architect as the follow-up.

Head: `1f089f5eb` (`wp-STEWARD8`), clean tree, on origin/main `aaa0e75e2`.
