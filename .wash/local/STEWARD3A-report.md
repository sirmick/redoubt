# STEWARD3A report: leases and agents in the steward server; the sponsored principal

**Head: 2d831801b**, ONE commit on STEWARD3Q's accepted head 794449d21 (over 3R and 3S; main's
plan commit is 6713d6ffa), tree clean, branch wp-STEWARD3A, worktree
/home/mcloonan/redoubt/.worktrees/STEWARD3A. The final gates, every case green on both widths
at this head, are in the last section ("Final gates at 2d831801b"); the sections between
describe the package's history (the first head f04dc870e, the rebase to 04b3be8cc, the review
folds) and still hold. Written by steward3a-implementer (to 98a166314), steward3a-implementer-2
(Q3, Q4, the re-measure, the exit line, the end-lease order, the rebase, the three review folds)
and steward3a-implementer-3 (the final gates and this report's last section).
steward-red-3 renews by range-diff: base 794449d21, previous head f04dc870e.

**The three lease cases are blocked on STEWARD3Q** (walfsd's `in_flight: 2` per account bucket
refuses the lease's home session while alice's console and SSH sessions hold both; Q4 wall 1).
Verified here with a stand-in, `in_flight: 4` in `servers/walfsd/src/server.rs`, **not
committed** (6, 3Q's derivation, fails `Limits::fits` at walfsd's 3 MiB budget, which 3Q
raises): at the committed tree plus that one hunk, rv64, `steward-lease-ends` PASS 9.7 s,
`steward-end-lease` PASS 13.8 s, `steward-sub-agent-dies-with-agent` PASS 25.8 s (logs
`.tmp/STEWARD3A/verify3/`). The steward's lines: `lease alice/agent-1 of 1001/{} exited 0`
(lease-ends), `was killed 0` after `LeaseEnded` (end-lease: the exit notice comes after the core
let the lease go; the driver keeps the name for it), `was killed 0` (sub-agent, the deadline).
Lease-end latency (`steward-end-lease`, alice's transcript): the notice `[lease alice/agent-1
ended]` matched 8.627 s after ssh started and the call's reply `{:ended, {:ok, %{}}}` 36 ms
later (8.663 s); the typed line itself is not stamped by the bench (it follows the
`others-refused` wait), so call-to-notice is bounded by the call's own round trip: under 40 ms
from the reply's print, through the steward's receive loop, the kernel's destruction and the
exit notice.

Rulings built from: `.wash/local/STEWARD3A-Q1-ruling.md` (the notice rides the `notices`
endpoint the launcher makes, (a2); `lease_ended` is opcode 17, a send) and
`.wash/local/STEWARD3A-Q2-ruling.md` ((A): a sponsored principal in the core; core, oracle and
model move together; the image folds agent's budget into alice's). Questions:
`.wash/local/STEWARD3A-questions.md`.

## What was delivered

### The server (`servers/steward`)

- `protocol.rs`: `start_agent` → `EventKind::StartAgent`, answered `StartAgentReply { lease,
  name }` from `Answer::Lease`; `end_lease` → `EventKind::EndLease`, answered empty; `lease_ended`
  is `Class::None` (malformed as a call on every badge); a caller of `Class::None` is refused
  before the class match; `protocol::ahead(caller, words)` names a sponsor's `end_lease` (the
  loop parks nothing, so it is answered as it arrives; the name is for a loop that one day parks).
- `drive.rs`: `Kernel::connect`/`launch` take the owner `Object` (a session's or a lease's
  binding table differs); `launch` returns the pid and, for a session, the send right of its
  `notices` endpoint, kept in `Steward::notices` and released at `Forget`; `Output::Notice {
  Session(badge), LeaseEnded }` is delivered at once through `Kernel::notify` with the name
  `<principal>/<name>` (`lease_name`: `alice/agent-3`, `agent/session-7`); an `Exited` event marks
  its object's pid gone before the core decides, so a `DestroyBudget` that finds the budget gone
  (`BadHandle`) after the kernel's deadline is done, not failed; on a budget whose process still
  runs it fails the batch (host test).
- `own.rs`: `Child::{Session{labelled}, Lease}`; a lease binds slot 0 `/boot`, slot 1 the
  sponsor's home's `project` directory at `/project` through the home's kept carve (`How::Carved`
  gains `below`), slot 5 the system volume; nothing else. `lease_args`: `principal=<sponsor>`,
  `home=/project`. `/boot` is one slot more than the questions file said: the sub-agent case
  launches `piped` from it (agents launch native programs in their own sub-budgets).
- `bin/steward.rs`: a lease's VM is beamlet on `Elixir.Redoubt.Agents.Echo` with the manifest's
  `agent` size (whole: no relay); a session's launch makes the notices endpoint pair, hands the VM
  the receive right as the named handle `notices`, keeps the send right (badge 1, no stamp: an
  endpoint of the steward's is under `system`), closes the receive right after the launch;
  `notify` is one page and one send bounded by `NOTICE_US` (100 ms).
- `lib.rs`: `start` iterates the core's carves (each named), not the manifest's principals: a
  sponsored principal has no carve.

### The core (`libs/steward`), the oracle, the model

- `manifest.rs`: `principal` line gains the optional `sponsor="NAME"` (the one optional field;
  written only when present, so every existing trace reads the same); `Fixed::new` resolves it to
  another, unsponsored principal (no chains); `Carve` gains `name`; `carves()` skips sponsored.
- `effects.rs`: `carve_session` parents a sponsored principal's session at the sponsor's
  unlabelled sub-budget with `deadline = now + MAX_LEASE`; `notify_sponsor` aims at the lease's
  domain's principal for a lease, at the sponsor's unlabelled domain for a sponsored session, at
  nobody for a plain session.
- `guards.rs`: `sponsor_of` (R39: a sponsored session is ended only from an unlabelled session
  of its sponsor), `sponsor_set` (a sponsored principal logs in to its unlabelled set alone:
  `Labelled`).
- `edges.rs`: `end_lease` dispatches by the id's kind: a lease as before; a session only if its
  principal is sponsored and the caller's account is the sponsor's, else `unknown`.
- `tables/session.md` (+9 rows; the generator's outputs regenerated): `Login !sponsor_set →
  refuse`; `Starting/Ending × EndLease (ahead) → refuse` (no `unreachable`: an id a caller
  guesses must not exit the steward); `Running/Detached × EndLease (ahead)`, `!sponsor_of → refuse`
  else `Ending` with `unroute, drop_requests, audit_lease_ended, notify_sponsor, reply_ok,
  destroy_budget`; every row leaving `Running`/`Detached` for `Ending` adds `notify_sponsor`.
  `gen/src/lib.rs`: the two guards registered (rule-carrying); its self-test now allows the
  session machine's `EndLease (ahead)`.
- `trace/traces/sponsored.trace` (39 events) takes every new row; `output.rs` and the oracle's
  `trace.ex` print `sponsor=<index>` only where there is one.
- Oracle (`elixir/`): `guards.ex` (`sponsor_of`, `sponsor_set`), `effects.ex` (`carve_session`,
  `notify_sponsor`), `steward.ex` (sponsors resolved at boot, carves filtered, `end_lease` by
  kind), `trace.ex`, `gen/session.ex` regenerated.
- Model: `Mutation::PolicySponsoredEndedByAnyone` (`sponsor_of = pass`) and
  `PolicySponsoredVaultLogin` (`sponsor_set = pass`), both R39; the test manifest's carol is
  sponsored by alice with owned `[10]`, sets `[[],[10]]` (the same principal count: no draw
  changes modulus but carol's set pick); P1 checks a sponsored session's budget under the
  sponsor's top with the sponsor's account, carved from the sponsor's unlabelled sub-budget, with
  a future deadline, and never labelled; P13 covers a sponsored session's `EndLease`; `Usage` of
  a sponsored principal is `sponsored`. Caught: `PolicySponsoredEndedByAnyone` by
  `steward_policy` seed 274, `PolicySponsoredVaultLogin` seed 6. Reach table re-recorded:
  `steward_policy` 146 items (was 142), last at seed 199 unchanged; `steward_noninterference`
  150 (was 145), last at 439 unchanged (`model/tests/steward_reach.rs`, `docs/kernel/model.md`).

### init, the wire, the image, the shell, the cases

- init: manifest field `sponsor`; a principal has a `budget` or a `sponsor`, never both
  (`check.rs` `budgets`); the sizes-within-share check skips sponsored principals; `holds` and
  `contexts_of` take the sponsor's budget for a sponsored principal; `steward_lines` writes
  `sponsor=` and `top=0,0,0`; the check refuses a sponsor that is not another, unsponsored,
  principal (`principals[i].sponsor`, `Why::Unknown`).
- `libs/wire/tables/steward.md`: opcode 17 `lease_ended` (`lease: u64`, `name: string`, no
  reply); generated Rust and Elixir regenerated (`cargo run -p redoubt-wire-gen`).
- `image/manifest.json` (and `tests/data/boot-profile/manifest-unverified.json`): `agent` has
  `sponsor: "alice"` and no budget; alice 81,920 pages / 64 processes; `sizes.agent` (filled from
  the measurement below). `image/README.md`'s arithmetic rewritten.
- Shell: `Redoubt.Shell.Notices` serves the `notices` handle from `Redoubt.Shell.start/0` and
  prints `[lease NAME ended]` through the printer; `Redoubt.Agents.Echo`, the lease's module
  (`echo` / `spin` / `sub MS`).
- Cases: `steward-lease-ends`, `steward-end-lease`, `steward-sub-agent-dies-with-agent`,
  `beamlet-agent-footprint`; `agent-login`'s expectations moved to `users/alice/{}` (30
  processes with the agent's session; no carve for agent).

### Q3: the platform without a console (ruling (a), `.wash/local/STEWARD3A-Q2-ruling.md`, Q3)

- `userland/otp/redoubt/src/lib.rs`: `Redoubt::new` opens `/dev/cons` only if the namespace
  holds it (`Namespace::lookup`); `console: Option<Arc<Console>>`, `cons: Option<ConsoleIo>`,
  one path: with none, `console_write` drops, `console_read` answers `Eof`, `console_size` is
  `None` (the shell's default for `unknown` is 80x24; it never runs there), `console_resized`
  never fires (no resize thread), `idle` with no deadline returns at once, the `Drop` flush has
  nothing to wait for, and `run` says its result line only with a console (the exit code is still
  the module's). A refused open of a console that is there is still the error it was.
- `fixture.rs`: `fixture::lease(volumes, args)` builds a startup block with no `/dev/cons`
  (`session_built` and it share `built`); `tests/console.rs`:
  `without_a_console_writes_are_dropped_reads_end_and_the_size_is_unknown` (platform-level, no
  waiter thread, no request) and `a_vm_without_a_console_runs_its_module_to_exit` (`console:size()`
  from the VM fixtures through `beamlet_redoubt::run`, exit 0).
- The steward's exit line is `steward: lease NAME of ACCOUNT/{} exited CODE` (`faulted`, `was
  killed`), NAME from `Steward::lease_name` (`alice/agent-1`; a session's `alice/session-2`; the
  kind and id if the core no longer knows the id). No case expected the old form.
- Pages: beamlet.md (the platform table's `console_write`/`console_read` row says `/dev/cons` is
  optional and what its absence means; the intro sentence; the status line names the two host
  tests), agents.md ("Leases": a lease has no console, output nowhere but `/project/out`, the
  steward's line says how its VM ended), steward.md (the console-lines sentence of "Two
  embedders" gains the exit line, named as the notice names the lease).
- `tests/size-budget.toml`: `libs/steward` 3942 → 4076 (the sponsor field, guards, effects and
  `end_lease` by kind; the commit carries the `Size budget:` line). `libs/wire` 3936 holds.

### Q4 (2a): the lease root's quota (ruling in `.wash/local/STEWARD3A-Q2-ruling.md`, Q4)

Found by probing the agent's VM on the machine (the questions file, Q4): its `/project` file
operations failed for two reasons in the pages' rules. Wall 1 (walfsd's `in_flight: 2` sessions
per account bucket) is STEWARD3Q's. Wall 2 is built here: a connection minted below the home
carve with quota 0 is a 0-byte live root in walfsd's ledger (R48), so the lease's root is now
carved with a quota of its own.
- Core (`libs/steward/src/manifest.rs`): `Sizes.agent_quota`; the `sizes` line's one optional
  field `agent_quota=B` (absent: 0, so every trace reads and writes as before); `lines()` writes
  it when non-zero; `tests/lines.rs` round-trips it and refuses a second one. The trace output's
  `fixed` line and the oracle's `trace.ex` (parse and print) follow; `run-traces`: every trace
  `ok` (`.tmp/STEWARD3A/oracle-run2.log`). `model/src/policy.rs`, `tests/core.rs`: the field at 0.
- init: `sizes.agent` gains the optional `quota` (string, bytes); `check.rs` refuses a quota not
  below every principal's `home_quota` (`steward.sizes.agent.quota`, `Why::Quota`, the refusal's
  text extended); the core's `Sizes` carries it to the steward's line. Test
  `the_agent_size_s_quota_is_below_every_home_s` (refused at 8 MiB against an 8 MiB home; the
  line carries `agent_quota=1048576`; absent, the line has none).
- Server: `own.rs` `How::Carved` gains `below_quota`, `binding(.., lease_quota)` sets it for a
  lease's slot 1 and 0 for a session's (`tests/own.rs`: the lease's table carries 1 MiB);
  `bin/steward.rs` keeps `agent_quota` from the sizes and `fresh_carved` mints the `below`
  connection with it (the ledger gives it back at the lease's disconnect, 3R's cleanup).
- Image: `agent.quota` 1 MiB of alice's 8 MiB home; `agent.pages` re-measured on the echo path
  (below); README arithmetic; the boot-profile mirror.
- Pages: steward.md (the sizes line; "Leases": the carve, the refused mint → failed step →
  `start_agent` answers `failed`; "A home's quota": the lease's root below it), init.md's row,
  agents.md "Leases" (the quota among the lease's sizes).

### The agent's size re-measured on the echo path

`beamlet-agent-footprint` measured a VM that had failed its `/project` read (no binding there)
and was scanned a few ms after start: 716/691 pages. Now the case stages `in` = `keep hello` on
its disk (`tests/data/beamlet-footprint/stage/in`), binds the data volume's root at `/project`
(`bind=/project=walfsd:data`; the manifest hands beamlet `walfsd:data`), and waits for the
agent's `agent: result written` before the scan. `Redoubt.Agents.Echo` gains `keep TEXT` (the
echo path, then idle) and `finish` says the line on the console the VM has, if any (a lease's
has none). Measured: heap peak 1,993 pages rv64, 2,082 rv32 → `sizes.agent` = 2 × 2,082 + 44 +
1,024 + 1 = 5,233 → **5,248** (was 2,560, on which the echo path OOMed in 0.8 s, exit 101).
Alice's `{}` share: 34,179 + 5,249 = 39,428 of 40,959.

### `steward-end-lease`: the notice precedes the reply

The lease batch sends `lease_ended` before `reply_ok`, and the shell's listener prints the notice
as it arrives, above the prompt the result is printed under: the case expected the reply first
and timed out past the notice. The two expectations are swapped (the measured gap is still
line-typed to notice-matched).

## Gates (commands and results)

Every command through `scripts/q run`; the tree at head 476fff8d3 (rebased onto main 43588e0d6,
train 37) unless said.

- `cargo test --locked --no-fail-fast -p redoubt-steward -p redoubt-steward-gen -p redoubt-steward-trace -p redoubt-steward-server -p redoubt-init -p redoubt-wire-gen`: all green (core 37 + 6 + 18, gen 10, trace 6, server 2 + 4 + 28, init 65 + 18 + 1), exit 0.
- `cargo check --locked -p redoubt-steward-server -p redoubt-init --target riscv64gc-unknown-none-elf` and `--target riscv32imac-unknown-none-elf`: clean, exit 0.
- `cargo test --locked --release -p redoubt-model --test properties -- steward_policy steward_noninterference` (MODEL_THREADS=4): both pass, 14.6 s together (before the rebase, at the same core and model sources).
- `REDOUBT_MODEL_MUTATIONS=PolicySponsored cargo test --locked --release -p redoubt-model --test mutations -- --nocapture mutations_are_caught`: `PolicySponsoredEndedByAnyone` caught by `steward_policy` seed 274 (P13), `PolicySponsoredVaultLogin` seed 6 (P1); exit 0.
- `cargo test --locked --release -p redoubt-model --test steward_reach -- the_reach_table_is_reproduced`: green after re-recording (146 / 150 items; last seeds 199 / 439 unchanged).
- `libs/steward/elixir/run-traces` (the Elixir oracle on beamlet): every hand-written trace `ok`, `sponsored.trace` among them (39 events), no untaken row; exit 0 (before the rebase; the oracle's sources are unchanged by it).
- `cargo run -p redoubt-steward-gen` and `cargo run -p redoubt-wire-gen`: the checked-in generated files are current (their tests pass).
- `cargo +nightly fmt -p <the six crates>`: applied; `mix format --check-formatted` in `userland/shell` and `libs/steward/elixir`: clean.
- `userland/shell`: `mix compile --warnings-as-errors` clean; `mix test`: 308 passed, 70 skipped (host).
- After Q3 and the exit line (the amended head): `cargo test --locked -p beamlet-redoubt
  --features fake` in `userland/otp`: 83 passed (console 16, of them the 2 new; files 23;
  limits 5; lookup 2; pack 2; schedulers 6; system 27; userland 2), exit 0; `cargo test --locked
  -p redoubt-steward-server`: 34 passed, exit 0; `cargo check --locked -p
  redoubt-steward-server` for both RISC-V targets: clean; `cargo +nightly fmt -p beamlet-redoubt
  -p redoubt-steward-server`: applied (the `formatting` case had failed on `bin/steward.rs`).
- `cargo testbench docs`: PASS, 1.5 s (at the amended head's docs).
- Bench cases, at the amended head, `make -f scripts/jobs.mk prebuilt` (rv64 327 cases, rv32
  315, 0 failed) then `make -k -j4 -f scripts/jobs.mk set` in two jobs (logs
  `.tmp/STEWARD3A/cases6-rv64.log`, `cases6-rv32.log`; run-dir transcripts copied to
  `.tmp/STEWARD3A/runs/`): stopped after `steward-lease-ends` failed (rv64, 1500 s: alice's
  `{:result, ...}` never came) to find out why; see Q4.
- **Q4 (open, QA thread `STEWARD3A-lease-files`, `.wash/local/STEWARD3A-questions.md`)**: with
  Q3 built the agent's VM boots and runs in 0.23 s, and its `/project` file operations fail for
  two reasons in the pages' rules: walfsd's `in_flight: 2` sessions per account bucket (alice's
  console and SSH sessions hold both; the lease's `collect` is `REFUSED` → `Eio`; with 4 the
  read works, an experiment reverted), and a mint below the home carve with quota 0 is a 0-byte
  live root in walfsd's ledger (R48) → `NoSpace` on the write. Evidence:
  `.tmp/STEWARD3A/exp11/probes.txt`, `exp12/` (stamped probes, temporary, reverted). The three
  lease cases fail until ruled; the WIP commit 3f598848c holds the Q3 work on top of 98a166314
  (to be folded).

### Gates at the amended head (2254d114d, then the ceiling fix amended on top)

`make -f scripts/jobs.mk prebuilt` (rv64 327 / rv32 315 cases built, 0 failed), then
`make -k -j4 -f scripts/jobs.mk set CASES="agent-login shell-compile shell-compile-module
launch-system steward-ssh-two-principals steward-home-quota image-disk beamlet-footprint
beamlet-agent-footprint size-budget formatting unsafe-budget docs steward-host-tests
steward-model-host-tests model-host-tests init-host-tests wire-host-tests elixir-oracles"`
(both widths; log `.tmp/STEWARD3A/gates-rv64-rv32.log`):
- PASS (rv64 and rv32): beamlet-agent-footprint (4.4 / 4.5 s), beamlet-footprint, image-disk,
  launch-system (smp=4), shell-compile, shell-compile-module, steward-home-quota (190 / 174 s),
  steward-ssh-two-principals.
- PASS (host): docs 1.5 s, elixir-oracles 3.1 s, formatting 19.5 s, init-host-tests,
  model-host-tests 137.9 s (the model's families), steward-host-tests, steward-model-host-tests
  15.6 s, unsafe-budget, wire-host-tests.
- FAIL size-budget: `libs/steward` 4084 lines over the 4076 I had set (the `agent_quota`
  parsing): the ceiling is 4084 in the final amend, with the commit's `Size budget:` line; rerun
  below.
- FAIL agent-login (both widths, 1500 s): the agent's shell starts and says `** no home to start
  in (:eio); this session starts in /`: the sponsored session's budget is carved under alice's
  sub-budget with no account of its own (`carve_session`'s `CreateBudget`), so the kernel's child
  inherits hers, and its home session is the third in alice's walfsd bucket: **wall 1, STEWARD3Q's**
  (its derivation already counts the agent's session among alice's contexts). Verified under the
  stand-in cap below. Not rerun since the predecessor's accounts fix: it would have failed so
  then too. Transcripts `.tmp/STEWARD3A/agent-login/`.
- Not run here (blocked on 3Q, verified under the stand-in): steward-lease-ends, steward-end-lease,
  steward-sub-agent-dies-with-agent; and rv32 of those.
- Under the stand-in cap (`in_flight: 4`, not committed), rv64: **agent-login PASS 9.7 s**
  (`.tmp/STEWARD3A/agent-login-standin/`): the sponsored login's prompt, `pwd`, `whoami`, and
  alice's 30 processes.
- At the final tree: **size-budget PASS** (host, after the ceilings: `libs/steward` 4084,
  `model` 11417, `servers/steward` 1738, `servers/init` 2592, `libs/wire` 3936, each named in
  the commit's `Size budget:` lines; three of the raises were the predecessor's work the case
  had reported one crate at a time), **formatting PASS** 18.5 s (rv64 prebuilt at 59ecda384).
- Not rerun at the final head: the rest of the set (code unchanged since 2254d114d).

## Residuals and what the reviewer should know

- **Four cases are blocked on STEWARD3Q** (`in_flight` per account bucket): steward-lease-ends,
  steward-end-lease, steward-sub-agent-dies-with-agent, agent-login. Each passes on rv64 with the
  one-hunk stand-in (`servers/walfsd/src/server.rs`, `in_flight: 4`), as recorded above; rv32 of
  them not run. The train that merges 3A must run them on 3Q's tree (3Q's derivation, 6, needs
  3Q's budget raise: 6 fails `Limits::fits` at 3 MiB, walfsd exit 4).
- **The lease-end latency** is bounded, not measured to the call: the bench stamps matches, not
  the line it types; the notice matched 36 ms before the reply's print.
- **The exit line after `end_lease`**: the driver keeps the ended process's object and name
  (`Steward::ended`) because the core lets the lease go before the kernel's exit notice arrives;
  the bin prints the line and raises no event for it. Host test extended
  (`end_lease_from_the_sponsor_destroys_the_lease_and_tells_her_sessions`). steward.md's
  console-lines sentence says "how each process it launched ended, once" and holds.
- **Files read whole** by this implementer: `userland/otp/redoubt/src/{lib,fixture}.rs`,
  `tests/console.rs`, `servers/steward/src/bin/steward.rs`; the rest of the touched files
  (`libs/steward/src/manifest.rs`, `servers/init/src/{manifest,check,refusal}.rs`,
  `servers/steward/src/{drive,own,lib}.rs`, the tests, the pages, `trace.ex`, `echo.ex`) by
  the hunks I changed and their surroundings; the predecessor's commit covered them whole.
- `beamlet-agent-footprint`'s manifest (`tests/data/beamlet-footprint/manifest.json`) is shared
  with `beamlet-footprint`: that case's shell VM is now also handed `walfsd:data` (unused: no
  `bind=`); it passes on both widths.

## Rebased onto STEWARD3Q (794449d21, over 3R and 3S)

`git rebase --onto 794449d21 43588e0d6 wp-STEWARD3A`: nine conflicts, resolved as both truths:
- `servers/steward/src/{lib,drive}.rs`, `bin/steward.rs`, `tests/steward.rs`: 3R's R41 threads a
  revocation `scope` through `Kernel::connect`, `fresh`, `fresh_carved` and the console mint, and
  holds every handle as a `u64` key (`hold`/`handle_of`, the steward's copies closed after the
  launch); mine takes the owner `Object` (a session's or a lease's table), `agent_quota` into
  `binding`, and `below`/`below_quota` into the carved mint. Merged: `connect(owner, slot,
  scope)`, `fresh_carved(.., below, below_quota, scope)` →
  `new_connection(lend, below, below_quota, Some(&scope))`; `launch` resolves the keys on the
  stack as 3R does and returns the notices send right as a held key; `notify` takes the key
  (`handle_of`). The Recorder and both `Connect` expectations carry `kind` and `scope`.
- `libs/wire/{src/proto/steward.rs,elixir/proto/steward.ex}`: taken from main and regenerated
  from the merged table (3R's `optional` layout field with opcode 17).
- `tests/size-budget.toml`: the sum rule, each ceiling main's plus this package's raise, stated
  in the commit: libs/wire 3915+28 = 3943, servers/steward 1576+202 = 1778, servers/init
  2591+33 = 2624 (libs/steward 4084 and model 11417 unchanged by main).
- `docs/userland/agents.md`: 3R's "When a lease ends" (the scope's destruction, R41) merged
  with the notice sentence and my three bullets; the section is built, so main's stray
  `**Open:** none.` went (the docs checker named it). `image/README.md`: my arithmetic
  (alice 81,920, the agent sponsored, the lease 5,248) with 3Q's derivation sentence: alice's
  sum is 6 (the console, three contexts, a lease, a spare) = the servers' constant, and the
  `walfsd` entries' 1,280 pages.
- Two tests of main's needed the merged truth: `servers/init/tests/manifest.rs`
  `servers_that_do_not_fit_in_system_are_refused` still summed walfsd:data at 1,024 (main's
  manifest has 3Q's 1,280: stale at main); `tools/testbench/src/case.rs`
  `the_servers_admit_what_a_manifest_lets_one_account_hold` read every principal's `budget`
  and panicked on the sponsored one, which has none: it now skips a sponsored principal, whose
  sessions are its sponsor's leases and count in the sponsor's sum.
- `image/manifest.json` auto-merged: alice 81,920/64 and the sponsored agent (mine), the
  steward's heap 32 (3R), both `walfsd` entries 1,280 and `buckets=6` (3Q), `agent` 5,248 with
  `quota`.

Head after the rebase: **04b3be8cc**. Host at that tree: steward server 2+4+29, core 37+6+18,
trace 6, init 66, wire 35+2+2+4, testbench 213 (3Q's derivation test included), beamlet-redoubt
83, docs PASS, size-budget PASS, formatting PASS, model release build clean, rv64/rv32 checks
of the steward server and init clean. (The machine run and the shell/oracle runs: below.)

## Summaries checked

- `README.md`: M3's row says the milestone's goal (an agent contained under a lease; approvals;
  the attack suite); the milestone is still planned as a whole: no change.
- `GETTING-STARTED.md`: the agent login paragraph now says agent's session is a lease of alice's
  and what her shell prints when it ends (changed).
- `docs/README.md`: the book's index and M3 row name the goal; no claim of what is built: no
  change.
- `docs/userland/README.md` "What an agent sees": status planned → built · partly tested with
  the lease cases (changed).
- `docs/servers/README.md`: the overview's lease sentences (a launcher disconnects at a lease's
  end; agents under leases in the figure) hold as written: no change.
- `docs/plan/m3-agents.md`: the AGENT0 paragraph now says the agent is sponsored and
  `start_agent`/`end_lease` run on the machine (changed; merged with AGT3's compile sentence).
- `docs/userland/sessions.md`: the image's contexts sentence and the startup handles (changed).
- `docs/userland/shell.md`: where a `[lease NAME ended]` line lands, in the machine mode and out
  of it (changed).
- `docs/servers/init.md`: the manifest table's `principals` row (`sponsor`, budget optional)
  (changed).
- `docs/userland/beamlet.md`: the named handles example gains a session's `notices`; "beamlet
  on Redoubt": `/dev/cons` optional in the platform table and the intro, the status line names
  the two console-less host tests (changed).
- `docs/userland/agents.md` "Leases": a lease has no console; its output goes nowhere but
  `/project/out`; the steward's exit line (changed). "The agent harness" bullet "Hand the agent
  the sponsor's things" already said the agent never gets the sponsor's `/dev/cons`: no change.
- `docs/servers/steward.md` "Two embedders": the console-lines sentence gains the exit line
  (changed); "Leases" already says a lease's namespace has no console: no change.
- `docs/kernel/model.md`: the mutation count (177) and R39's mutations; the reach counts
  (changed).
- `image/README.md`: the principals and the arithmetic (changed).
- `docs/SECURITY.md`: R39's row to built with its tests (changed).
- `libs/wire/tables/steward.md`: `lease_ended` is a send (changed); `libs/steward/tables/README.md`
  unchanged (the notation did not change).
- `servers/steward/README.md`, `libs/steward/README.md`: none exist.

## Departures and residuals

- Bob's refusal in `steward-end-lease`: the core answers a caller of another account `unknown`
  (the brief's case said `NotSponsor`); the core's rows win; the machine case has bob name an id
  he cannot know (R36/R37 leave him no way to learn it), the real id's `unknown` is the host test's
  and the trace's.
- The lease-end latency: measured from the bench log's step timestamps in `steward-end-lease`
  (the call's echo to the notice line), reported below, not asserted as a bound.

## Review folds (steward-red-3, `Merge verdict: OK with notes`), all in 2d831801b

- P1, notice forgery: `Redoubt.Shell.Notices` prints a `lease_ended` only from badge 1 (the
  steward's, `@notices`); `line/4` takes the badge; `test/redoubt/shell/notices_test.exs` (the
  steward's badge prints; badges 0 and 2 print nothing; another message prints nothing). Shell
  `mix test`: 311 passed, 70 skipped.
- P1, the rebase: `connect(owner, slot, scope)` merges 3R's revocation scope with the owner; the
  lease's connections and its `/project` carve are minted in the lease's scope
  (`fresh_carved(.., below, below_quota, scope)`); the admission derivation test follows init's
  `holds`; steward.md's "until R41's scope lands" clause is gone.
- P2, a nested lease: guard `caller_session` (guards.rs, the oracle's guards.ex, the generator's
  registry; the row `| - | StartAgent | !caller_session | - | refuse |` first in
  `tables/lease.md`, generated files regenerated with `cargo run -p redoubt-steward-gen`); core
  test `a_lease_cannot_start_an_agent`; `lease_bounded_and_carve_lease` no longer nests;
  steward.md "Leases" says a lease's badge is answered `unknown`; `lease.trace` rewritten; the
  reach table re-recorded (`LAST = [(274, 148), (296, 151)]`, model.md's two sentences follow).

Host gates at 2d831801b (implementer-2): core 38+6+18, gen 10, trace 6, steward server 2+4+29,
init 66, wire 35+2+2+4, testbench 213, beamlet-redoubt 83 (`--features fake`), shell 311,
oracle run-traces rc 0 (22 ok, no untaken), model families steward_policy + steward_noninterference
pass (12.5 s), mutations `PolicySponsored*` caught, reach table reproduced, docs PASS, size-budget
PASS, formatting PASS, rv64/rv32 checks of the steward server and init clean, model release build
clean.

## Final gates at 2d831801b (implementer-3, 2026-10-16; logs `.tmp/STEWARD3A/final/` in the worktree)

Every command through `scripts/q` (`scripts/jobs.mk`), the tree untouched throughout.

- `make -f scripts/jobs.mk prebuilt`: rv64 329 cases, rv32 317 cases, 0 failed, rc 0
  (`final/prebuilt.log`).
- The four lease cases for real, on 3Q's tree (walfsd's `in_flight` 6, no stand-in), one at a
  time (`make -f scripts/jobs.mk <width>/<case>`; `final/run-four.sh`, `final/run-four.out`;
  each run dir's `*.log` and the job log copied to `final/<case>/<width>/`):

  | case | rv64 | rv32 |
  | --- | --- | --- |
  | steward-lease-ends | PASS 9.7 s | PASS 9.1 s |
  | steward-end-lease | PASS 13.6 s | PASS 12.8 s |
  | steward-sub-agent-dies-with-agent | PASS 25.8 s | PASS 25.6 s |
  | agent-login | PASS 9.3 s | PASS 8.8 s |

  The steward's lines: `steward: lease alice/agent-1 of 1001/{} exited 0` (lease-ends, both
  widths), `was killed 0` (end-lease and sub-agent, both widths: the destruction, by `end_lease`
  and by the deadline). `steward-end-lease`, alice's transcript: the notice `[lease alice/agent-1
  ended]` matched 8.447 s after ssh started on rv64 and the reply `{:ended, {:ok, %{}}}` 79 ms
  later (8.526 s); rv32 7.770 s and 64 ms later (7.834 s). As before, the typed line is not
  stamped, so call-to-notice is bounded by the reply's round trip (under 80 ms).
- `make -k -j4 -f scripts/jobs.mk set CASES="beamlet-agent-footprint steward-three-contexts
  steward-ssh-two-principals shell-compile init-boot image-disk docs size-budget formatting"`
  (both widths; `final/set.log`, rc 0; the rv32 transcripts in `final/set/`, the rv64 run dirs
  were pruned before the copy, their verdicts are in `set.log`): all 15 PASS:
  beamlet-agent-footprint 4.3 / 4.3 s (rv32's scan: `heap beamlet 1993 of 11220 pages`; the
  size 5,248 holds), steward-three-contexts 10.3 / 9.4 s, steward-ssh-two-principals 15.0 /
  14.6 s, shell-compile 10.3 / 9.4 s, init-boot 6.6 / 6.6 s, image-disk 0.8 / 0.8 s; host: docs
  1.5 s, size-budget, formatting 18.8 s.
- Not rerun here (unchanged since implementer-2's host run at this head, listed above): the host
  test crates, the oracle, the model families and mutations, the full bench; the train runs the
  whole bench.
- No code changed: the head is 2d831801b as handed over; no failure needed a diagnosis.

## Train 42 fix (steward3a-fixer, 2026-10-16; worktree `.worktrees/STEWARD3A-fix`, logs `.tmp/STEWARD3A-fix/`)

Head **e825c2defa** on 794449d21 (one commit, 2d831801b amended; the message's last paragraph before the `Size budget:` lines states the fix). Diagnosis in full: `.wash/local/STEWARD3A-train42-diagnosis.md`. `model/src/gen.rs` is unchanged by the package: no draw moved; both mutations were not caught at all (`mut-*.log`), not late.

### The causes and the fixes

1. **userland-boot (both widths).** A sponsored principal has no boot carve (`Manifest::carves()` skips it) and the carve is the steward's only per-principal boot line, so `steward: carved users/agent account 1003: {}` never prints. `tests/userland-boot.toml`: the expect line is gone, `steward: carved users/agent` is forbidden (as agent-login has it), the description says agent is sponsored and carved nothing.
2. **PolicyNoContextCap: masked, not unreached.** The reach table at 2d831801b reaches `under_cap refuses` at seed 3 (`reach-head.log`), but each label set's share of a top's 12 processes is 4 = two sessions = the cap of two contexts: with the guard passed the kernel refused the third login for processes and P19 saw nothing. Only carol's one-set domain (12 processes, 6 sessions) could exceed the cap, and her sponsorship moved her sessions under alice's {} share. Fix (`model/src/policy.rs`): alice's top gets 18 processes (a share holds three sessions; she holds carol's too); bob's keeps 12 so his domains still fill (a first try with 18 for both pushed `audit_start_failed` to seed 16,353 in `steward_policy` and out of `steward_noninterference`: `reach-fix.log`). model.md ("The steward model") records why some top must hold one more session than the cap. Caught at seed 3 (`mut3-PolicyNoContextCap.log`).
3. **PolicySubAgentOutlivesAgent: uncatchable since the review's P2 fold** (orchestrator's ruling: option A). `machines.rs:399` was the only place a lease got a parent, a lease starting an agent, which `caller_session` now refuses; `carve_lease`'s sub-agent arm was dead and the mutated `carve_lease` computed what the core does. Dropped: the mutation (`Mutation::ALL` 176), the arm (`effects.rs` `carve_lease`, `carve_lease_in` folded in; the oracle's `effects.ex` likewise), the model's sub-agent checks (`policy.rs`: the StartAgent P9 block now says a lease that started an agent inside itself is a P9 failure; the P1 `in_agent`/"P1 a sub-agent"/P9 "outlives its agent" checks). Kept with one sentence each: `Lease.parent` (store.rs, always None) and `Sizes::sub_agent` (manifest.rs, the format's) as the later package's.
4. **Reach table** re-recorded (`reach-fix2.log`): `steward_policy` 148 items, last at seed **439** (`copying refuses`; was 274), `steward_noninterference` 151 at 296 (unchanged); `LAST` and model.md follow. 4 × 439 still rounds to 2,000.
5. **Size budget** tightened to the tree (ceilings only fall): libs/steward 4086 (was 4097 for 4086 lines), model 11389 (was 11417), libs/wire 3937 (was 3943 for 3937 lines); the message's three lines corrected (+144, +2, +22).

### Pages

steward.md: the guards table row `lease_bounded, carve_lease` (no sub-agent clause, one mutation), the "model checks leases" list, R39's status line (mutation dropped) and text (a sub-agent is a child budget the kernel ends with its parent, R10; the steward carves none, nested leases later). SECURITY.md's R39 row: the mutation dropped. model.md: 176 variants, the R39 row, the sessions and leases property rows, the reach numbers, the 18-processes note. agents.md:113 "a sub-agent's budget inside an agent's" (was "lease"). Checked unchanged: agents.md 148-150 (a sub-agent's budget carved from the agent's, R10: true of the kernel), image/README.md and sessions.md on the sponsored agent (true), steward.md "Leases" (already says a lease's badge is told unknown).

### Gates (all PASS; job logs copied to `.tmp/STEWARD3A-fix/cases/`)

- model-mutations through q: **176/176**, 422.6 s (`make -f scripts/jobs.mk rv64/model-mutations`); `cargo run --example mutations` lists 176.
- model-host-tests 151.3 s (the reach table reproduced).
- userland-boot rv64 12.0 s, rv32 12.1 s; agent-login rv64 9.7 s, rv32 9.0 s; steward-lease-ends rv64 9.7 s, rv32 9.4 s; init-boot rv64 6.7 s. Prebuilt both widths rc 0.
- steward core `cargo test --release -p redoubt-steward`: 38 + 6 pass (one pre-existing warning from main's `drop_requests` test, untouched). `cargo run -p redoubt-steward-gen`: generated files unchanged.
- oracle `run-traces` rc 0: 22 ok, no UNTAKEN; 61 informational "never reached by the model's traces" (59 before: session.md rows 28 and 53, a session's failed start and a detached session's end, fall out of the first seeds' recorded traces now that alice's shares hold more).
- docs PASS, size-budget PASS on the committed tree, `cargo +nightly fmt -p redoubt-model -p redoubt-steward --check` clean.

Not run: the whole bench (the train's); rv32 of init-boot (asked once); steward-end-lease and steward-sub-agent-dies-with-agent (not in the gate list; the core change touches no path they take beyond carve_lease's unchanged agent arm).
