# WRITE1 report

Branch `wp-WRITE1`, rebased on origin/main 24507bec1 (train 81). Head: (filled at the end).
Identification: `.wash/local/WRITE1-identification.md`; the Architect's ruling:
`.wash/local/wave-2026-10-18-rulings.md` "WRITE1 identification". Evidence logs:
`/home/mcloonan/redoubt/.tmp/WRITE1/` (`repro1..7`, `gate1`, `diag8`, `diag9`, `system-tests.log`,
`suite.log`, `old-giveup.log`).

## What was delivered

The rule, held by construction and stated, not a fix of a hang: every hub request ends in
exactly one answer to its caller within `FILE_WAIT_US` plus the reply margin, and a `File`
operation's bound is the sum over its requests.

1. **The hub** (`userland/otp/redoubt/src/files.rs`, Tier A): an operation that ends holding a
   walked fid (an open the server refused; a request given up on while parked for its retry,
   `ebusy`) hands its result to the caller first and clunks the fid as a closed file's is
   (`Owner::Close`, the fid given back on the answer), off the caller's path. Before, the
   operation sent the clunk as its own next request and the result waited for the `Rclunk`
   under a fresh deadline: the caller's bound was 2 × `FILE_WAIT_US` and, with the clunk answered
   `busy` by a held share, unbounded in principle. `give_up` and `go_on` say so; `close_handle`
   shares the new `clunk`.
2. **Host tests** (`userland/otp/redoubt/tests/system.rs`, the fixture sink gains `busy_opens`,
   `hold_writes`/`let_writes`):
   `a_give_up_on_a_walked_fid_answers_its_asker_before_the_clunk` (fails on the old code:
   "the asker waited past the bound, for the clunk", `.tmp/WRITE1/old-giveup.log`) and
   `an_operations_bound_is_the_sum_of_its_requests` (a two-piece write, the first answered after
   nine tenths of the bound, the second never: `etimedout` after ≥ 1.9 bounds).
3. **The agent** (`Redoubt.Agents.Echo`, Tier B): `loadwrite`, the case's agent (27 loads, a
   numbered line per load with the previous write's answer); and the input read asked again on
   `etimedout`, at most six times, before the refused-input idle — without it, under sixteen
   launches at once, one or two agents' first read of `/project/in` timed out at the hub's 10 s
   and they idled for the lease's life by Echo's own design (gate 1 both widths, boots 8 and 9:
   the "two of sixteen never return" was that, their badges' first served request `finish`'s
   create; no request was left out or unanswered in any VM).
4. **The case** `tests/lease-writes.toml`, both widths, one hart, host clock: sixteen writers,
   sixteen `exited 0` and no lease ended by its deadline or killed, every `:ok`-reported write
   present in its file. A write answered `etimedout` may still land (served late, the answer
   dropped after the caller's own) and is counted, not forbidden: that is the rule's shape, so
   "exactly the :ok lines" is stated as "every :ok line, and only the agent's lines".
5. **Pages**: files.md "Files over 9P" (the rule per request, the sum, the trailing clunk off
   the caller's path, the VM's own calls unbounded by design and why; three tests added, the
   status count 28 → 31); walfsd.md "Residual risks" (the hang sentence replaced by the
   correction and the rule; the throughput finding kept with 224/348 under twelve writers).

## What was not changed, and why

- The VM's own asker (`run`'s `io.wait(FOREVER)`) is as it was, per the ruling: the hub answers
  by construction; stated on files.md. Diagnostic boot 9 bounded it under a probe and no VM-own
  give-up occurred in sixteen agents (the exit digits), so it was not on the path either.
- walfsd: untouched (the probe lines were test-only and are gone).

## Tests and gates (exact commands, exit codes)

| what | command | result |
| --- | --- | --- |
| beamlet-redoubt host tests | `q run --cores 4 -- cargo test -p beamlet-redoubt --features fake -- --test-threads=1` (userland/otp) | rc 0; system 29/29, whole suite green (`suite.log`) |
| the new test on the old code | same, files.rs at the branch base | FAILED as intended (`old-giveup.log`) |
| docs | `make -f scripts/jobs.mk set CASES=docs` | rv64/docs rc=0 |
| formatting | `make -f scripts/jobs.mk set CASES=formatting` | rv64/formatting rc=0 (run before the agent's input retry; rerun below) |
| unsafe budget | `set CASES=unsafe-budget` | rv64/unsafe-budget rc=0 |
| size budget | `set CASES=size-budget` | rv64/size-budget rc=0 |
| lease-writes ×3 both widths | `make -f scripts/jobs.mk set CASES=lease-writes` ×3 | (filled) |
| the beamlet set | `scripts/shell-cases origin/main` minus `steward-latency*` (excluded by the assignment) | (filled) |

## Documentation check

Searched for the old claims (`never comes back`, `wait past that bound for good`, `FILE_WAIT_US`,
`give-up`): files.md and walfsd.md changed as above; beamlet.md's table row ("10 s a file
operation waits for each request's answer") and ipd.md's "a call is parked for strictly less
than its client waits" agree with the rule as stated and need no change; agents.md, README.md,
GETTING-STARTED.md and the milestone pages carry no claim about the hub's bound or the data
volume's writes. testbench.md's clock rule (train 81) is cited from walfsd.md.

## Open risks and findings for the orchestrator

- The data volume's throughput under sixteen writers (decision 3) stands: 224 of 348 writes
  served under twelve, the rest `etimedout`; an input read can take more than one 10 s request
  under the launch storm (hence the agent's retry). A write SLA would be its own node.
- Two sessions' ssh connections closed under the virtual clock at sixteen leases (runs 4, 5):
  the clock skip reaching a socket timer; on the host clock not seen. Not this package's.
- A notice can still be lost in a burst of ends (SHELLN1's residual); the case waits on the
  sixteen notices as lease-code-load does, so a lost one fails its step loudly.
