# WRITE1 identification: the write whose completion never returns

Branch `wp-WRITE1` (rebased on origin/main 24507bec1, train 81), WIP commits by path. Evidence
under `/home/mcloonan/redoubt/.tmp/WRITE1/repro<N>/` (console and session logs copied aside; the run
directories are pruned).

## Result in one paragraph

The hang did not reproduce. Seven boots, 68 agent VMs of one account writing to `/project` beside
loads from the system volume, every agent under a watchdog that halts the VM with a status naming
the step and the hub's state if it reaches no new step for 120 s: no watchdog ever fired, every
`File.write` returned (`:ok` or `{:error, :etimedout}`), and every agent that was not ended by the
case's own clock artefact ended by its own return, `exited 0`. What the runs do show is the data
volume's throughput under the account's load (decision 3) in a sharper form, and that the hub's
bound is per request, not per `File.write`. READ1's one piece of evidence for the hang (diag4,
diag4b: one agent of four stopped after its second line, N = 4, rv64 one hart) was taken under the
virtual clock with READ1's admission fixes not yet in; its own inference "the file holds the lines
of the very write the agent never returned from" does not hold: the file ends at the line written
*before* the stalled step (the agent writes each line after its load, so the missing line is the
next step's), so the bytes of the stalled write never reached the volume, and the stall sits in
that write's walk/open/stat or in the write request itself — or, as the clock rule published with
train 81 now explains, in nothing at all: an idle guest's virtual clock skips to its next armed
deadline, and a VM waiting on a slow volume with no nearer timer is exactly such an idle.

## The probes (test-only, feature `write-probe`, off in every default build)

- **walfsd** (`servers/walfsd/src/server.rs`, `probe`): every `minted`, `read`, `create` and `write`
  served says on walfsd's console the caller's badge, the path, offset, length and answer, with
  the time: whether and when the server answered (the R-message goes into the parked completion
  call's lend in the same serving; a delivery that fails ends the session, which the hub sees as
  `Ended` → `Lost` → `eio`).
- **beamlet-redoubt** (`files.rs` `probe`, `lib.rs` `run`): a VM a process halts with a status of a
  thousand million or more exits with that status plus the file hub's state at the platform's
  end, as digits: thousands = operations with a request out and no result; ten-thousands =
  results in `finished` (a wake the VM never took); hundred-thousands = results waiting on their
  clunk; millions = give-ups (`give_up`: every `etimedout`/`ebusy`/`emfile`); ten-millions =
  operations parked for a retry; hundred-millions = requests out with no deadline. The steward's
  `lease ... exited <status>` line carries it, the one channel a lease's VM has.
- **The agent** (`Redoubt.Agents.Echo`): a watchdog process from `start`, before the input read,
  told each step (the input read, `finish`'s mkdir/write/read, and in the new `loadwrite` mode
  each of the 29 appends), stopped before an idle by design (`spin`, `keep`); on 120 s without a
  new step it reads the agent's own input (0 answered, 1 refused, 2 unanswered in 30 s) and halts
  with `1_000_000_000 + step + 100 × probe`. `loadwrite` is READ1's first-gate shape: the 27
  loads, a line appended to `/project/out/load-<born>` after each, every line carrying the
  previous write's result and duration. So for a stalled write: the file says whether its bytes
  reached the volume, walfsd's line whether it was served, the status digits whether the hub still
  holds the request (thousands), holds an answer the VM never took (ten-thousands), or gave up
  (millions): the three outcomes the ruling names.
- **The case** `tests/lease-writes.toml` (WIP): N writers of alice's, the latency manifest, rv64 one
  hart; the shell polls `ls` + `read` of every file every 10 s during the loads (the first gate's
  shape); then the notices and a print of every file's lines. Its step order is wrong as it stands
  (the notices land inside the polling step and the next step waits for them; the print never ran
  in run 7), and it must stay on the host clock (below); both for the phase-2 case.

## Runs (rv64, one hart, `qemu_seed = 3`)

| run | N | clock | shell polling | watchdog | result |
| --- | --- | --- | --- | --- | --- |
| 1 | 4 | virtual | no | `loadwrite` only | 4 `exited 0`; 112 writes served; 15 `etimedout` (10.4–39 s per `File.write`) |
| 2 | 4 | virtual | yes | `loadwrite` only | 4 `exited 0`; 70 served; ≥ 10 `etimedout` (21–47 s) |
| 3 | 16 | virtual | yes | `loadwrite` only | 5 `exited 0`, 11 `was killed 0` at the 1800 s deadline; 4 writes served in 500 s guest; the 11 had `wait("in: :etimedout")` (two left `result`) or were idle — the clock skip to the deadline (train 81's rule), not a hang; my print step lacked a `within` and the case ran to the kill |
| 4 | 16 | virtual | yes | whole life | the session's ssh closed by the peer 77 s in, right after the 16th launch (`holds 106770 pages`, the share full) with no guest line: the clock skip reaching a socket timer |
| 5 | 16 | virtual | yes | whole life | the same at 95 s |
| 6 | 12 | virtual | yes | whole life | killed by me when train 81's clock rule arrived |
| 7 | 12 | host | yes | whole life | 12 `exited 0`, no halt; 224 writes served of 348 attempted (3–26 per agent, 29 each), 2,861 reads (the shell's polling), walfsd refused nothing; 9 of 12 `done` lines landed; the writes ran from 6.6 s to 426 s |

In every run with the watchdog, no status above the floor was ever printed: no agent reached a
step and stayed in it 120 s. The `etimedout` writes are the hub's `give_up` answering its caller
at `FILE_WAIT_US`, as designed; a `File.write` is five to six hub requests (walk, create or open,
stat for append, the write, the clunk), each with its own 10 s bound, so one `File.write` took up
to 47 s before failing — the bound holds per request today, not per operation, which the rule's
wording ("every hub request ends in exactly one answer to its caller within FILE_WAIT_US plus the
reply margin") already says; the page's sentence should say that an operation is several.

## Hub reading (userland/otp/redoubt/src/files.rs, beamlet-vm vm.rs)

For an Erlang asker every request gets a deadline at its send (`go_on`, `get_or_insert`), and
`pump` fails it at the bound on every `dispatch`: the idle's wait is bounded by `retry_in`
(every deadline and retry), and `finished()` runs `take_completed` while requests are out. A
`send` the server does not take stays queued and goes again at `RETRY_US` from `Io::wait`, so a
saturated server costs the request its 10 s, answered `etimedout` (run 3: four writes served in
500 s with nearly every request timing out). One corner is unbounded as read: the VM's own asker
(`run`'s `io.wait(FOREVER)` loop), used by the code path's `find_in_code_path`, `read_whole_file`
and `list_dir` (bif/code.rs), waits for ever if its answer never comes, and in a VM without a
console nothing else arrives to run `dispatch`; a lease's `code_path` is empty, so none of these
is on the agent's path. A second corner is 2 × `FILE_WAIT_US`: an operation given up on while it
holds a walked fid clunks it with a fresh deadline before its result is handed over. `poll_files`
hands a finished asker to its process only in `io == Waiting`; the states that would drop one were
not found reachable. On the server side the mux's `expire`/`pump`/`fill` answer every pending
request or end its session; walfsd's `write` never parks.

## What I propose for the ruling

1. Treat the hang as unreproduced under today's code and clock rule: the identification is "none
   of the three; the first evidence is explained by the virtual clock's skip and by READ1's
   inference not holding", unless the Architect wants the diag4 conditions (READ1's branch before
   the admission fix, virtual clock) rebuilt and rerun to see the skip there.
2. Phase 2 as a hardening of what the reading found, by construction: (a) the VM's own asker
   bounded like every other (the `FOREVER` loop takes `retry_in`, and a lost answer fails the
   VM-own call `etimedout` rather than wedging the VM); (b) the give-up's trailing clunk no longer
   delays the caller's answer (the result handed over at the bound, the clunk owned as a close
   is); (c) walfsd.md's hang sentence replaced by the rule as stated per request, with "an
   operation is several requests" said; files.md's one-answer rule. (d) The boot case: N writers
   on the host clock with the notices waited before the print and the writes checked by their
   lines. (e) The host test: a VM-own call unanswered past `FILE_WAIT_US` fails `etimedout`, and
   an Erlang asker's give-up on a walked fid is answered at the bound. Whether (a)/(b) are in
   scope is the Architect's: both are departures from the rule as written, found by reading, not
   by the reproduction.
