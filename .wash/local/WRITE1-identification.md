# WRITE1 identification: the write whose completion never returns

Branch `wp-WRITE1` (from origin/main ddafcad7d). Evidence copied under
`/home/mcloonan/redoubt/.tmp/WRITE1/repro<N>/` (the run directories are pruned).

## The probes (test-only, feature `write-probe`, off in every default build)

- **walfsd** (`servers/walfsd/src/server.rs`, `write`): every write served says on walfsd's console
  `walfsd: write <path> off=<o> len=<n> -> Ok(n)|Err(..) at <us>`: whether and when the server
  answered the Twrite (the Rwrite goes into the parked completion call's lend in the same
  serving; a delivery that fails ends the session, which the hub sees as `Ended` → `Lost` → `eio`).
- **beamlet-redoubt** (`files.rs` `probe`, `lib.rs` `run`): a VM a process halts with a status of a
  thousand million or more exits with that status plus the file hub's state at the platform's
  end, as digits: thousands = operations with a request out and no result; ten-thousands =
  results in `finished` (the asker's wake not yet taken: the VM lost it); hundred-thousands =
  results waiting on a clunk; millions = give-ups (`give_up`, every `etimedout`/`ebusy`/`emfile`);
  ten-millions = operations parked for a retry; hundred-millions = requests out with no deadline.
  The steward's `lease ... exited <status>` line carries it (the one channel a lease's VM has).
- **The agent** (`Redoubt.Agents.Echo`, input `loadwrite`): READ1's first-gate shape: loads the 27
  modules and appends a line per load to `/project/out/load-<born>`, each line carrying the
  previous write's result and duration. A watchdog process halts the VM when no new step is
  reached for 120 s (12 × `FILE_WAIT_US`), status `1_000_000_000 + step + 100 × probe`, the probe
  a read of the agent's own input made meanwhile (0 answered, 1 refused, 2 unanswered in 30 s).
  So for a hung write: the file says whether its bytes reached the volume; walfsd's line whether
  it was served; the status digits whether the hub still holds the request (thousands), holds
  the answer the VM never took (ten-thousands), or gave up (millions) — the three outcomes.
- **The case** `tests/lease-writes.toml` (WIP name): N writers of alice's, rv64 one hart, the
  latency manifest; the shell polls `ls` + `read` of every file every 10 s during the loads, as
  the first gate did; then the sixteen notices and a print of every file's lines.

## Runs

| run | N | shell polling | result | writes served | `etimedout` | slowest `File.write` |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | 4 | no | 4 of 4 `exited 0`, no stall | 112 | 15 (10.4–39 s each) | 39.1 s |
| 2 | 4 | yes | 4 of 4 `exited 0`, no stall | 70 | ≥ 10 (21–47 s each) | 46.6 s |
| 3 | 16 | yes | (running) | | | |

No write hung in runs 1 and 2: every `File.write` returned, as `:ok` or `{:error, :etimedout}`.
What they do show, for the rule: a `File.write` is several hub requests (walk, create or open,
stat for append, the write, the clunk), each bounded by `FILE_WAIT_US` on its own, so one
`File.write` took up to 47 s before failing `etimedout` — "FILE_WAIT_US plus the reply margin"
holds per request today, not per operation.

## Where the first evidence came from

diag4 and diag4b (READ1's `.tmp`, N = 4, rv64 one hart) ran on READ1's branch while the fid caps
were still 32 per bucket with the roots in the share, so every agent met `too many open files`
and `busy` refusals constantly; one agent of four stopped after its second line in each. gate-rv64-1
(N = 16) shows no write at all: the shell's own read answered `eio` and no agent left a file —
the admission storm READ1 fixed, not the write path. Under today's admission the refusals are
rare at N = 4, which is why run 3 goes to N = 16.

## Hub reading (userland/otp/redoubt/src/files.rs, beamlet-vm vm.rs)

For an Erlang asker every request has a deadline from its send (`go_on`, `get_or_insert`) and
`pump` fails it at the bound, on every `dispatch` (idle's bounded wait, `finished()` while
requests are out). The one path with no bound is the VM's own asker (`run`'s `io.wait(FOREVER)`
loop): a VM-own file operation (the code path's `find_in_code_path`, `read_whole_file`) whose
answer never comes waits for ever, and in a VM with no console nothing else arrives to run
`dispatch`; a lease's `code_path` is empty, so no such call is on the agent's path as read.
`poll_files` drops a finished asker whose process is in the table with `io != Waiting`; the
states reachable there were not found to allow it. The identification therefore rests on the
reproduction, not on the reading.
