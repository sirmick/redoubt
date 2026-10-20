# WRITE1 report

Branch `wp-WRITE1`, three commits on origin/main dc75a30eb (train 85), head **3fda9d68c**:
`175981635` beamlet (the hub), `616b762ad` shell + testbench (the agent, the case), `3fda9d68c` docs.
Identification: `.wash/local/WRITE1-identification.md`; the rulings: `wave-2026-10-18-rulings.md`
"WRITE1 identification" and "WRITE1's gate finding". Evidence logs under
`/home/mcloonan/redoubt/.tmp/WRITE1/` (`repro1..7`, `gate1`, `gate2`, `diag8..13`, `roundA..H-*`,
`system-*.log`, `suite-*.log`, `old-giveup.log`, `old-givenup.log`, `beamletset.make.log`).

## What was delivered

The rule, held by construction and stated, not a fix of a hang (the hang did not exist: the
Architect's correction, recorded): **every hub request ends in exactly one answer to its caller
within `FILE_WAIT_US` plus the reply margin, and every fid it walked is let go; a `File`
operation's bound is the sum over its requests.**

1. **The hub** (`userland/otp/redoubt/src/files.rs`, Tier A), two departures closed:
   - *The clunk before the answer.* An operation that ended holding a walked fid (an open the
     server refused; a request given up on while parked for its retry, `ebusy`) sent the clunk
     as its own next request and handed its result over only on the `Rclunk`, under a fresh
     deadline: the caller's bound was 2 × `FILE_WAIT_US`, unbounded while the clunk was answered
     `busy`. Now the result is the caller's at the bound and the clunk is a closed file's
     (`Owner::Close`), off the caller's path.
   - *The fid retired, never clunked* (found by the boot case). A request given up on in flight
     had its fid retired for good: one fid held at the server and one burnt in the connection's
     pool per `etimedout`; under sixteen agents' timeouts at the data volume (decision 3's
     throughput) the account's working pool at walfsd drained and the late starters' first read
     of `/project/in` was refused `too many open files` — the "two of sixteen never return" of
     the clean gate, named by the agent's exit status `104` (= `emfile`) once it could say so.
     The given-up request stays the hub's (`Owner::GivenUp`); at its late answer the hub drops
     the answer and clunks the fid it walked or held as a closed file's, or gives the number back
     if the walk failed — at the hub's moment, since the mux serves a connection's requests in
     order but past one the file server holds, so a queued clunk could overtake a held read.
   - Under the test-only `exit-kinds` build a halt exits with the status the process gave (13
     for every halt before), so an agent with no console can say why it ended.
2. **Host tests** (`userland/otp/redoubt/tests/system.rs`; the fixture sink gains `busy_opens`,
   `hold_writes`/`let_writes`): `a_give_up_on_a_walked_fid_answers_its_asker_before_the_clunk`
   (fails on the code before: "the asker waited past the bound, for the clunk");
   `a_fid_given_up_on_is_clunked_when_its_late_answer_comes` (fails on the code before: "the fid
   was not clunked"); `an_operations_bound_is_the_sum_of_its_requests` (a two-piece write, the
   first answered after nine tenths of the bound, the second never: `etimedout` after ≥ 1.9
   bounds).
3. **The agent** (`Redoubt.Agents.Echo`, Tier B): `loadwrite`, the case's agent (27 loads, a
   numbered line per load carrying the previous write's answer); and an input read that fails
   for a reason other than its absence is asked again a second apart, thirty times (five and a
   half minutes, each try the hub's bound), then the VM exits with a status naming the error
   (`100 + code`) rather than idling for the lease's life with nothing said. `enoent` and an
   unknown input keep the documented `wait`. Thirty, not six: in the beamlet set, beside the
   set's other boots on the host clock, the last-launched agent's input read was answered
   `etimedout` six times running on both widths (`exited 101`) — the data volume's throughput
   reaching the input read for over a minute — while alone it never took more than one.
4. **The case** `tests/lease-writes.toml`, both widths, one hart, host clock, in the whole run:
   sixteen writers of one account beside their loads; sixteen `exited 0` and no lease ended
   otherwise; every write an agent was told succeeded present in its file. A write answered
   `etimedout` may still land (served late, its answer dropped after the caller's own) and is
   counted, not forbidden: the rule's own shape, so the Architect's "exactly the :ok lines" is
   stated as "every :ok line, and only the agent's lines". Cost: 370–550 s per width alone;
   the first notice is waited for up to 1200 s and the case bounded at 2400 s, for a loaded host.
5. **Pages**: files.md "Files over 9P" (the rule per request, the sum, the clunk after the answer
   and at a given-up request's late answer, the VM's own calls unbounded by design and why;
   four tests added, the status count 28 → 32); walfsd.md "Residual risks" (the hang sentence
   replaced by the correction and the rule; the throughput finding kept, with 224/348 under
   twelve writers); beamlet.md (a halt's status under `exit-kinds`).

## What was not changed, and why

- The VM's own asker (`run`'s `io.wait(FOREVER)`) is as it was, per the ruling: the hub answers
  by construction; stated on files.md. Diagnostic boot 9 bounded it under a probe and no VM-own
  give-up occurred in sixteen agents.
- walfsd: untouched (its probe lines were test-only and are gone).

## Tests and gates (exact commands, exit codes)

| what | command | result |
| --- | --- | --- |
| beamlet-redoubt host tests | `q run --cores 4 -- cargo test -p beamlet-redoubt --features fake -- --test-threads=1` (userland/otp) | rc 0; 95 passed, 0 failed on the final tree (`suite-final4.log`); under B90's tick |
| the new tests on the old code | same, files.rs at the branch base, one test each | FAILED as intended (`old-giveup.log`, `old-givenup.log`) |
| mix format / compile | `mix format --check-formatted`, `MIX_ENV=prod mix compile` (userland/shell, pinned toolchain) | rc 0, no warnings |
| docs | `make -f scripts/jobs.mk set CASES=docs` | rv64/docs rc=0 on the final tree (round K) |
| formatting | `set CASES=formatting` | rv64/formatting rc=0 on the final tree (round K; the mix-format gate included) |
| unsafe budget | `set CASES=unsafe-budget` | rc=0 (on 88ce8464e; the hub unchanged since) |
| size budget | `set CASES=size-budget` | rc=0 (on 88ce8464e; the hub unchanged since) |
| lease-writes ×3 both widths | `set CASES=lease-writes` | rounds G, H on 88ce8464e (the hub as now): PASS rv64 546/538 s, rv32 543/536 s; round D on d48b3e51d (before the given-up clunk): PASS 380/368 s; in the beamlet set (beside its other boots): FAIL both widths, `agent-16 exited 101` (input read `etimedout` six times), the reason the agent now tries thirty times; round J on 7c7db483f: PASS rv64 551 s, rv32 513 s; round K on the final head 3fda9d68c (train 85, B92's q): PASS rv64 555 s, rv32 631 s. Six green rounds per width on the hub as it is, one failed round (beside the set's load) that the thirty tries answer |
| the beamlet set | `scripts/shell-cases origin/main` minus `steward-latency*` (excluded by my brief): 97 cases, `make -k set CASES=...` | 194 verdicts, 191 green; failed: lease-writes ×2 (above) and `rv32/beamlet-agent-footprint` once at 2.6 s — a race of that case's own (below), passing 3 of 3 rerun alone and in every recent train |

Rounds before the given-up clunk, for the record: A (d48b…'s predecessor) PASS both; B lost 1–2
of 16 on both widths; E rv32 `exited 104`. The rule's second half came from those.

## Documentation check

Searched for the old claims (`never comes back`, `wait past that bound for good`, `FILE_WAIT_US`,
`give-up`, `exit-kinds`): files.md, walfsd.md and beamlet.md changed as above; beamlet.md's
table row ("10 s a file operation waits for each request's answer") and ipd.md's "a call is
parked for strictly less than its client waits" agree with the rule as stated; agents.md,
README.md, GETTING-STARTED.md and the milestone pages carry no claim about the hub's bound or the
data volume's writes. testbench.md's clock rule (train 81) is cited from walfsd.md. Echo's
moduledoc states the new input behaviour.

## Open risks and findings for the orchestrator

- **beamlet-agent-footprint races its own agent's exit.** Its agent (`keep hello`) writes the
  result and then `Process.sleep(:infinity)`; with nothing in flight and no process reading the
  console the VM gives up (`beamlet: Deadlock`, exit 1), which the case's forbid on an `init`
  exit catches when the boot ends later than the scan (one boot of 2.6 s here; 4.3–6.4 s in
  twelve passing boots). Not this package's: the `keep` path is as it was; `wait` spins for
  that reason. A `keep` that spins, or a case that ends at the scan, closes it.

- The data volume's throughput under sixteen writers (decision 3) stands: most of an agent's
  writes are answered `etimedout` under the storm (files of 1–3 lines beside files of 26); a
  write SLA would be its own node.
- Two sessions' ssh connections closed under the virtual clock at sixteen leases (runs 4, 5):
  the clock skip reaching a socket timer; on the host clock not seen.
- A notice can still be lost in a burst of ends (SHELLN1's residual); the case waits on the
  sixteen notices as lease-code-load does, so a lost one fails its step loudly. None lost in
  seven host-clock boots.
- `walfsd` holds the fids of requests the hub gave up on only until their late answers; a
  request whose answer never comes (a session the server ended) is returned `Ended` to the hub
  and its number freed; nothing new for the server.
