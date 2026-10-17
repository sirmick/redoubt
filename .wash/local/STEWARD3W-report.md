# STEWARD3W: a cleanup step a busy server did not take is parked, not dropped

Branch `wp-STEWARD3W`, rebased onto main 9f1cff36c (train 57, STEWARD3U's console queue), worktree
`/home/mcloonan/redoubt/.worktrees/STEWARD3W`. One commit (`steward: a cleanup step a busy server
did not take is parked after the fourth timeout ...`), amended as the gates fold; `git log -1` for
the head. Reviewer: steward-red-4. The rebase's conflicts: the machine's module doc (both
paragraphs kept), the constants (both), `clean`'s line (now `self.out.say`, 3U's queue), the test
file (both sides' tests appended), the status line (3U's plus my tests) and the size ceiling
(3280 + 121 = 3401). The ordering sentence on steward.md now reads: the idle timer's ends, then
the cleanup queue's turn (parked steps among the due ones), then the console lines, then the
receive. 55 host tests pass on the rebased tree; both widths check with and without the probe.

## What was delivered

- `servers/steward/src/cleanup.rs`: each step knows its server (`Option<String>`); a `parked`
  list beside the due steps; `run` parks a step at its `TRIES`th timeout instead of dropping it,
  and a step done wakes its server's parked steps (they run in the same turn); `wake(server)`
  (a mint answered), `run_parked(server, clean)` (before a mint there: stops at the first
  timeout), `forget(server)` (`reconnected`); `len` counts parked steps, `due` ignores them (no
  timer); `parked()`. `TRIES`/`RETRY_US` unchanged; the module doc states the rule and the bound.
- `servers/steward/src/lib.rs`: `Kernel::clean(handle, parks)` (the flag names the attempt that
  says the line; the record is never dropped on a timeout); new `Kernel::server_of(handle)` and
  `Kernel::server_at(owner, slot: Option<u16>)` (`None` = the domain's vault, a crossing's);
  `Steward::parked()`.
- `servers/steward/src/drive.rs`: `give_back` records the server; `Steward::minting` wraps every
  mint (`connect`, `rebind`, `cross`): parked steps at that server run first, and a mint answered
  (not `Timeout`) wakes what is still parked there; `reconnected` forgets the server's parked
  steps.
- `servers/steward/src/bin/steward.rs`: `clean` says `steward: <what> parked after 4 attempts:
  retried when the server answers` once and returns `Timeout`; `server_of` (handle name / the
  carve's server / `consoled` / `sshd`), `server_at` (the binding table's server; `consoled`
  for the UART console slot only when no channel console waits; the vault's handle for a
  crossing); `minted` maps a mint's `Sys(Timeout)` to `Timeout` (was `Refused`) so a server that
  answered nothing wakes nothing; `LATENCY_ROUNDS` = 3 for the probe.
- `servers/steward/tests/steward.rs`: the Recorder names each connection's server by the image's
  binding table (`at`), takes no step at a `busy` server, refuses a connect at a server's `cap`;
  tests renamed/added (below).
- `tests/steward-latency.toml`: N = 3 round (`round.(2, 16)`), `LATENCY-COUNT N=3` lines, the
  description; `whole_run` stays false. `tests/size-budget.toml`: servers/steward 3113 -> 3234.
- Pages: `docs/servers/steward.md` "Failure and restart" (the parking rule, the narrower residual,
  the order of the loop's turn-time work beside the console lines, the status line);
  `docs/servers/serving.md` residual; `docs/kernel/scheduling.md` "With the real steward" (N = 3
  column; figures below).

## Host tests (51 pass)

`a_cleanup_step_that_times_out_is_retried_then_parked` (was `..._then_dropped`),
`a_parked_step_runs_at_the_servers_next_answered_call`,
`a_mint_at_a_server_with_parked_steps_runs_them_first_and_succeeds_where_it_was_refused`,
`reconnected_forgets_a_servers_parked_steps`,
`a_never_answering_server_costs_no_bound_once_its_steps_are_parked`; the existing
`the_cleanup_queue_holds_each_released_connection_once` holds (pending counts parked or due).

Two first drafts of the new tests were wrong about the rule, not the code: a *refused* mint is an
answer and wakes the step (the ruling's "taken or refused"), and alice's login is itself a mint at
`erofsd:system`, so it runs bob's parked step before the connect. Both tests now say so.

## Design notes

- The answer signal is the library's: a step done at a server (in `run`) or a mint the library made
  there that returned anything but `Timeout` (`minting`). The machine tracks nothing; its only
  additions are the two name lookups and the `Timeout`/`Refused` distinction on a mint.
- A mint a server does not take within the client library's second is now `Timeout` to the batch
  (`a step failed (Timeout)`), where it was `Refused`: the rule needs the distinction.
- The channel console's `ended` parks under the name `sshd`: the next `ended` sshd takes wakes it.
  `give_back` of a notices send right or a relay's control connection has no server (nothing is
  told): its step is done at its first run, as before.
- 3U hotspot: the page now says of the loop's turn-time work that the cleanup queue runs after the
  idle timer's ends and before the receive, and that the loop's own console lines are written where
  they arise; 3U rewrites the second clause for its queue.

## steward-latency, N = 3 (seed 3; logs `.tmp/STEWARD3W/latency-logs/`)

- rv64 smp1, rv64 smp2, rv32 smp1: round 3 completes. `parked after 4 attempts` at
  `erofsd:system` 2/2/1 times per run, no `a step failed`, every N=3 count, LATENCY-END. The
  case FAILs its targets by design (`whole_run = false`, the console-line finding).
- Figures (net µs p50/p99, N=3): rv64/1 timer 26134/28359 (p50 missed), decision 6266/83747,
  notice p99 33231, destroy 6098/8337, lease end 83747+9679=93426 met; rv64/2 timer
  41047/61773, decision 21149/139080, notice 50132 missed, destroy 8721/10194, lease end
  139080+9759=148839 missed; rv32/1: from target/jobs/rv32-steward-latency.log when the case
  ends. The N=3 decision-wake p99 is one sample (max) whose window holds the cleanup turn
  retrying the other spinner-era disconnect at a busy erofsd: 100 ms, the queue's turn bound
  (3Y), the shape STEWARD4 attributed to a 206 ms timer p99; the parked step itself costs nothing.
- rv32 smp2 STALLED in round 3: after agent-68 ended, the shell's `start_agent` for agent-69
  returned `{:error, :timeout}` (libs/wire/elixir/lib/client.ex `@timeout 5000`, the platform's
  call bound) -> MatchError, prompt `(5)>`, the bench waited for `{:round, 3, :done}`. The
  steward printed no AgentStarted and no failed step for agent-69 but printed the spinners'
  deadline kills at 1241 s: it never took the call, so it was blocked >= 5 s outside a batch
  from ~661 s (the parked line) to 1241 s, waking when the kernel printed `[!] Terminating
  process`. The only unbounded call there is `say` (attach/open/write/close at consoled,
  FOREVER); consoled's write polls the UART (uart.rs `put`, TX_TRIES) and takes the kernel's
  console hold per chunk (server.rs `console_held`). Suspect: a consoled/console-hold wedge at
  two harts on rv32, surfaced by the steward's unbounded console writes (3U bounds them).
  Round-3 start_agent batches (AgentStarted `at` less the deadline less the lease): rv64/1
  1.42-2.16 s, rv64/2 1.84-3.07, rv32/1 2.24-4.20, rv32/2 4.94 (the one): at two harts on rv32
  the batch sits at the client's 5 s bound regardless.

## Gates (on the rebased tree; logs under `/home/mcloonan/redoubt/.tmp/STEWARD3W/`)

- host tests: `cargo test -p redoubt-steward-server --test steward` -> 55 passed, 0 failed
  (host-tests2.log; 51 on the pre-rebase tree, host-tests.log).
- `cargo check --bins` rv64/rv32, with and without `latency-probe` -> rc 0 (check*-2.log).
- `cargo +nightly fmt -p redoubt-steward-server -- --check` -> rc 0 (fmt-final.log).
- `make -f scripts/jobs.mk prebuilt` -> rc 0; `make -f scripts/jobs.mk -j3 -k set
  CASES='steward-latency steward-server-restart steward-blame-lockout steward-session-ends
  steward-lease-ends steward-end-lease steward-pending-cap init-boot docs size-budget
  unsafe-budget'` (gates-run.log, job logs in gates-logs/): every case PASS on rv64 and rv32
  (server-restart 17.0/17.1 s, blame-lockout 11.2/11.0, session-ends 55.4/55.2, lease-ends
  9.3/9.8, end-lease 12.7/12.9, pending-cap 13.3/13.8, init-boot 8.5/8.8; docs, size-budget,
  unsafe-budget PASS) except steward-latency, which runs by name (`whole_run = false`): rv64
  smp1 and smp2 complete all three rounds (FAIL on the N=3 timer-wake p99: one sample with a
  cleanup retry's bound, below); rv32 smp1 completes all three rounds and meets every target at
  every N; rv32 smp2 stalls in round 3 (below), which I ended by its QEMU (cwd) after the
  evidence was in, so the run finished (`guest exited during the SSH sessions`).
- After the page edits: docs PASS, size-budget PASS (servers/steward 3399 of 3399), unsafe-budget
  PASS (final-host-cases*.log); the final commit's gates logs in gates-logs-final/.
- No `parked after` line outlives its round in any completed run: every connect at
  `erofsd:system` after a parked step is minted (no `a step failed`), and the parked steps run at
  the next lease's mint there or its end's step.

## steward-latency N = 3 on the rebased tree (seed 3)

- rv64/1: timer 1947/3252, 795/1853, 1142/56360 (p99 missed at N=3); decision 1121/11418,
  17265/20798, 913/3094; notice p99 9183, 12087, 11602; destroy 6048/8700, 6077/6179,
  6055/8538; R10 9679; lease end 20798+9679=30477 met; reply_wait 6474/6497, 6555/6614,
  7703/122760. Parked lines: 2.
- rv32/1: timer 708/1317, 1194/1406, 773/1255; decision 23740/35167, 1233/40490, 1202/19442;
  notice 11327, 13769, 12339; destroy 6705/7853, 6733/13612, 5860/9305; R10 9650; lease end
  40490+9650=50140 met; reply_wait 6789/6836, 8199/8690, 8845/55943. Every target met. Parked: 1.
- rv64/2: timer 7987/8018, 2549/23488, 2809/147557 (p99 missed at N=3); decision 1949/9492,
  1973/22099, 7205/54354; notice 12268, 12899, 10501; destroy 7998/8627, 8529/9779, 7375/10131;
  R10 9761; lease end 54354+9761=64115 met; reply_wait 17911/42891, 17409/32193, 19807/112295.
  Parked: 4.
- The N=3 timer-wake p99 miss on rv64 is one sample (the next largest under 4 and 24 ms) whose
  window holds the loop's cleanup turn retrying a `disconnect` the busy erofsd did not take:
  100 ms, the queue's turn rule (3Y); the same retry shows in the round's reply_wait p99. The
  parked step itself costs nothing after. Stated on scheduling.md beside the table.
- rv32/2: stalled again in round 3 (the second run with 3U's console queue, so the console path
  is not the cause): after agent-70 ended, the shell's `start_agent` returned `{:error,
  :timeout}` (the client's 5 s call bound + 5 s for a call thread); no console lines were
  dropped (3U's counter); the steward had taken no call (no AgentStarted, no failed step) and
  its own disconnects at erofsd:system had timed out four times just before; nothing on the box
  printed for ~570 s until the spinners' leases ended at ~1211 s, then the steward served the
  exit notices as usual. Reading: erofsd:system takes nobody's call under two spinner VMs at two
  harts on rv32 (the shell VM's call threads sit in its code loads there, the steward's
  disconnects time out), recovering only when the spinners die. A server/kernel finding, not the
  steward's loop; the row says `not reached` with the cause. Logs: gates-logs/ and
  `target/testbench/run-4025928-*/steward-latency-rv32-smp2*.log` copied to latency-logs/ as
  `stall2-*` (the first stall's logs are `steward-latency-rv32-smp2*.log` there).

## Summaries checked

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`: no claim about the cleanup queue: no change.
- `docs/plan/m3-agents.md`: no mention of the cleanup or the dropped step: no change.
- `docs/SECURITY.md` R41 row: its tests (the destroy-before-release order) are unchanged: no
  change. The rows naming "parked" are the serving library's parked calls, unrelated.
- `docs/servers/init.md` "Residual risks", "Fresh connections per child": name no dropped step:
  no change.
- `docs/servers/serving.md` residual: changed (above).
- `docs/kernel/scheduling.md`: the N = 3 sentence and column: changed.

## Files read whole

cleanup.rs, lib.rs, drive.rs, bin/steward.rs, tests/steward.rs, tests/steward-latency.toml,
tests/size-budget.toml, docs/servers/steward.md, docs/servers/serving.md,
docs/kernel/scheduling.md.
