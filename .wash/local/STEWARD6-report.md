# STEWARD6 report: the steward's calls to shared servers inside a batch are bounded

Branch `wp-STEWARD6`, worktree `.worktrees/STEWARD6`, rebased onto origin/main `5eccd3acb`
(train 65). Head: `c2c12acde`, one commit, every gate below run on it. Reviewer steward-red-4.
Evidence under `.worktrees/STEWARD6/.tmp/STEWARD6/` (`measure/` the measurement logs and the
instrumentation patch, `gates/` the gate logs, `notes.md` the working notes).

## Delivered

`servers/steward/src/bin/steward.rs`:
- `SERVER_US = 4_000_000` (beside `HELLO_US`/`RELAY_US`, the precedent), documented with the
  measurement and the arithmetic.
- Every connection to a shared server is created `within` it: `server()`
  (`Connection::attach_within`), `unattached()` (`Nine::within`), `carve()`'s kept handle
  (`Nine::within`), `home()` (`attach_within`), `Console::new`'s attached connection (the
  console session's mint at `consoled`), and `ipd`'s `grant` (`typed::call_within`). So every
  later call on the kept connections (`servers`, `carved`, `homes`) is bounded too.
- `called(e)` maps the file client's `Sys(Timeout)` to `Error::Timeout` (as `minted` does for
  the 9P client), used by `server`, `home`, `read_program`, the grant, the console mint,
  `home_stat`, `home_read`, `home_write`; `read_program` and `home_write` now tell a failed call
  from a short read/write. Nothing retries; the batch's failure path is unchanged (a failed step
  is said as `steward: a step failed (Timeout): ...`).
- The module doc gains one paragraph ("The calls out inside a batch").
- Not touched: `libs/steward`, `libs/rt`, `libs/client`, `cleanup.rs`, `console.rs`,
  `drive.rs`. Nothing in `bin/steward.rs` outside the connection constructors and the error
  mapping, so STEWARD5 rebases over it; STEWARD7's `LATENCY_ROUNDS` line is untouched.

`servers/steward/tests/steward.rs`: the Recorder gains `stalled` (servers that take no call of
the steward's within its bound: `connect` there, `launch` when `bootfsd` is named, `home_stat`
and `home_read` when `walfsd:data` is) and three tests:
- `a_mint_the_server_does_not_take_in_time_fails_the_step_and_the_next_caller_is_served`: with
  bob's step parked at `erofsd:system`, alice's login mints there and gets `Timeout`: `Failed`,
  one failed step `(_, Timeout)`, the session budget destroyed (its scope with it), the parked
  step ran once before the mint and is not woken by the timeout, bob's next login is served and
  its answered mint runs the parked step (pending 0, parked 0).
- `a_launch_whose_image_read_is_not_taken_in_time_fails_start_agent`: `Failed`, the lease's
  budget destroyed, the next `start_agent` served.
- `a_push_whose_source_server_does_not_answer_in_time_answers_failed`: `submit` answers
  `Failed` before any budget, no failed step; the same push served when the server answers.

`tests/size-budget.toml`: servers/steward 3399 -> 3414 (+15), the `Size budget:` line in the
commit.

Pages: `docs/servers/steward.md` "Residual risks" (new bullet beside the relay's: the bound, the
measurement, the arithmetic, the Architect's wording of the residual), "Authentication and
sessions" (one clause under Login; status list gains the mint test, count 9 -> 10);
`docs/userland/agents.md` "Leases" (one clause after "`start_agent` answers `failed`").

## The measurement (the bound is measured, not guessed)

Instrument: `.tmp/STEWARD6/measure/instrument.patch`, a `timed(server, op, f)` wrapper in
`Machine` saying `steward: MEASURE <server> <op> took N us` on each new worst per (server, op);
ops: attach, mint, carve, mint-below, open, read, close, stat, grant (a `stat` is walk+stat+clunk,
three calls; an `open` two). Removed before the commit. Runs: `make -f scripts/jobs.mk
rv64/steward-latency` then `rv32/steward-latency` (seed 3, both hart counts, `--keep-failed`),
one at a time; logs `.tmp/STEWARD6/measure/{rv64,rv32}-steward-latency.smp{1,2}.log`.

Worst per (server, op), µs, rv64 smp1 / smp2 / rv32 smp1 / smp2:

    walfsd:data mint-below (1 call)   641873 / 1061809 / 1659369 / 1496589
    walfsd:data carve      (1 call)   464712 /  736717 /  422667 /  718446
    erofsd:system mint     (1 call)   134819 /  167520 /  164660 /   89041
    bootfsd read           (1 call)    44692 /   74843 /   79203 /   51058
    bootfsd open           (2 calls)   34144 /   63274 /   67246 /   35834
    home stat              (3 calls) 1079533 / 1616556 / in-round 1506155 / (see below)
    the rest (attach, bootfsd mint, ipd grant, consoled mint) under 25 ms everywhere.

Worst single call inside a completed round: 1.66 s (rv32, one hart, round 4 beside three
spinners, `walfsd:data mint-below`: a lease's `/project` carved below the home). Arithmetic:
2 × 1.66 s = 3.32 s, rounded up to a second: **SERVER_US = 4 s** (QA thread `STEWARD6-bound`,
the Architect: reading (a) stands; ruling F3 appended).

Outside the rounds: on rv32 both boots' loops died on the shell's 5 s `start_agent` timeout
(smp2: round 2 after spinner agent-33; smp1: round 4 after agent-113, beside three spinners);
after that, with the spinners (10-minute leases) running unopposed, the probe's `home_stat`
polls for the round marker took 4.2, 4.3, 5.1, 5.7, 6.2, 6.6, 8.1, 10.1 s (smp1) and 4.5 to
7.7 s (smp2) until the case's 3 h timeout. Stated on the page as the residual, not the bound's
source (the Architect's ruling).

Case verdicts of the measurement runs (instrumented tree, before the bound): rv64 smp1 and
smp2 FAIL on the known timer-wake p99 misses at N = 3, 4 (lease-end targets met, N = 1 rows
pass); rv32 smp1 FAIL "timed out waiting for {:round, 4, :done}" at 10800 s; rv32 smp2 FAIL
"timed out waiting for {:round, 2, :done}" at 10800 s. None is this change's.

## Gates (exit codes; every command under scripts/q or jobs.mk, from `.worktrees/STEWARD6`)

- Host: `q run --cores 4 -- cargo test -p redoubt-steward-server`: 58 passed in the server's
  test, every other target ok, exit 0 (on c2c12acde).
- Release builds, rv64 and rv32, with and without `latency-probe`: exit 0 x 4 (on c2c12acde).
- `make -f scripts/jobs.mk set CASES="formatting docs size-budget unsafe-budget"`: all rc=0 on
  c2c12acde (the first pass failed formatting, the docs status count and the size ceiling;
  fixed and amended: `gates/set1-make.log` vs the rerun).
- Steward set both widths (steward-ssh-two-principals, steward-lease-ends, steward-end-lease,
  steward-declassify, steward-push, steward-server-restart, steward-blame-lockout, init-boot):
  16/16 rc=0 on c2c12acde (`gates/set2-make.log`, logs `gates/set2/`); also 16/16 on 7f6c12e96
  before fmt's reflow (`gates/set1/`).
- steward-latency rv64 on c2c12acde (`make -f scripts/jobs.mk rv64/steward-latency`, rc=2 as
  before: the known timer-wake p99 misses at N = 3, 4 on both hart counts, and at one hart a
  decision-wake p99 miss at N = 4, 101 ms against 95 ms, one sample of sixteen, where the
  measurement run had 23 ms and the two-hart row 75 ms: STEWARD5's rows, the cleanup step's
  100 ms attempt in a sample's window; no timeout was reached, so not this bound's doing).
  N = 1, every target met, figures beside the
  measurement run's (before the bound), net p50/p99 µs:
  one hart: timer_wake 2018/3290 (was 1597/3233), decision_wake 1056/11545 (566/1270),
  deadline_notice 8186/11701 (7538/10885), reply_wait 6471/6494 (6493/13843);
  two harts: timer_wake 7954/11524 (2982/26268), decision_wake 1883/3910 (10480/17454),
  deadline_notice 9257/12217 (9012/9850), reply_wait 17625/43824 (44107/66035).
  No `a step failed` and no `Timeout` in either console log: the bound is not reached.
  Logs `gates/lat/rv64-steward-latency.log`, `gates/lat/steward-latency-rv64-smp{1,2}.log`.
- steward-latency rv32 on c2c12acde (`rv32/steward-latency`, rc=2): both boots end at the
  case's 3 h timeout as the measurement run did (one hart: "timed out waiting for {:round, 4,
  :done}", rounds 1-3 counted; two harts: "{:round, 3, :done}", rounds 1-2 counted: B88's
  wedge), so the oracle prints no rows; the N = 1 rows from the console's `LATENCY-SAMPLE`
  lines (gross, 16 samples, p50 / max µs; the measurement run's in brackets):
  one hart: timer_wake 2264/4373 (2266/4489), decision_wake 25592/25901 (25285/25719),
  deadline_notice 304385/517480 (302960/514957), reply_wait 17890/17941 (17906/32828);
  two harts: timer_wake 5332/8747 (5180/10830), decision_wake 4906/9884 (4019/13714),
  deadline_notice 305577/464181 (299614/338846), reply_wait 84607/85804 (84401/100000).
  Unmoved within the run's own spread. No `a step failed` and no `Timeout` in either console
  log. Logs `gates/lat/rv32-steward-latency.log`, `gates/lat/steward-latency-rv32-smp{1,2}*.log`.
- Not run: the whole bench (the train's); the steward-latency rows at N >= 2 are STEWARD5's and
  STEWARD8's.

## Documentation check

- docs/servers/steward.md: "Residual risks" and "Authentication and sessions" as above.
- docs/userland/agents.md "Leases": one clause.
- README.md, GETTING-STARTED.md, docs/servers/README.md, docs/plan/m3-agents.md: searched for
  claims about the steward's waits/bounds (`without bound`, `unbounded`, `forever`, `wait`):
  none; unchanged. docs/kernel/scheduling.md "With the real steward": its launch figures and
  cause are F4's; unchanged.
- No new R<n>; SECURITY.md unchanged. No dates, package ids or decision ids on the pages.

## For other packages

- F4 / STEWARD8: on rv32 at **one hart** the N = 4 round's loop died on the shell's 5 s
  `start_agent` bound beside three spinners (B88 measured 5.1 s there, borderline); log
  `.worktrees/STEWARD6/.tmp/STEWARD6/measure/rv32-steward-latency.smp1.log`, session log
  `...smp1-alice.ssh.log` (`{:error, :timeout}` after agent-113). At two harts round 2 died
  (B88's gate completed rounds 1 and 2): `...smp2*.log`. The Architect will fold the one-hart
  rows into STEWARD8's brief.
- Under icount `sleep=off`, a guest idle on host I/O sees its clock race; other boots on the
  host may move these rows (a suspicion, not shown).

## Open risks

- The bound is per call; a `stat` is three calls, so an operation may hold the loop up to
  3 × SERVER_US before its step fails. The page states the per-call bound.
- STEWARD7 changes `LATENCY_ROUNDS` and admission caps in the same file: a trivial rebase
  either way.
