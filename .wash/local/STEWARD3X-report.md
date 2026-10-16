# STEWARD3X report: a shared server's restart no longer strands the sessions it served

**Head: 86aa77767**, ONE commit rebased onto main 76d60b116 (train 44, STEWARD3B), folds P2 and P3 in, branch wp-STEWARD3X, worktree
/home/mcloonan/redoubt/.worktrees/STEWARD3X, tree clean. Written by steward3x-implementer.
Reviewer: steward-red-3 (Tier A). Logs under the worktree's `.tmp/STEWARD3X/`.

Rulings built from: `.wash/local/STEWARD3X-ruling.md` (QA `STEWARD3X-rebind`: all five defaults,
Q4 amended: the bound measured, the end-batch stall a residual with its number, STEWARD3Y drafted
for the fix). Questions: `.wash/local/STEWARD3X-questions.md`. **Wire opcodes renumbered per the
orchestrator: `reconnected` 19, `server_restarted` 20, `rebind` 21 (18 is STEWARD3B's
`approval_waiting`); the table has the gap at 18 until the rebase onto main carries 3B.**

## Gates (every command through `scripts/q`; the final chain `.tmp/STEWARD3X/host-tests-7.log`, `checks-5.log`, `docs-9.log`, `prebuilt-11.log`, `*-final4.log`, `set-4.log`)

- `cargo test --locked -p redoubt-init -p redoubt-steward-server`: init 23 (the reconnect module's 2 among them) + 1 + 66; steward server 2 + 4 + 31 (the new `a_servers_restart_tells_its_sessions_and_a_session_rebinds_its_slot`, the badge-class sweep extended, the lockout test's no-detach assertion). `cargo test -p redoubt-steward` 38+6; `-p redoubt-wire` 35+2+2+4 (generated files current).
- `cargo check --locked -p redoubt-init -p redoubt-steward-server --target riscv64gc-unknown-none-elf --target riscv32imac-unknown-none-elf`: clean.
- `cargo +nightly fmt -p redoubt-init -p redoubt-steward-server -p redoubt-steward -- --check`: clean; `cargo testbench --exact formatting` PASS 19.8 s.
- `userland/shell`: `mix format --check-formatted` clean; `mix test` 313 passed, 70 skipped (notices_test 5).
- `make -f scripts/jobs.mk prebuilt`: rv64 331 / rv32 319 cases, 0 failed.
- **`steward-server-restart`: rv64 PASS 16.4 s, rv32 PASS 15.2 s** (`final4-steward-server-restart-*/`). Measured there: bob's write fails ~5.0 s after his ssh started, his re-bind line ~0.85 s later, alice's the same and her read-back `{:ok, "before"}` ~0.4 s after; the later login read the file 2.0 s after its ssh started; the first restart's recovery (init's `after N ms`) 652/656 ms rv64, 580/592 ms rv32, later restarts 8-9 ms; the third blame's lockout: `LockedOut`, bob's channel ends, `Permission denied` on his next login.
- `steward-blame-lockout`: rv64 PASS 11.3 s, rv32 PASS 10.6 s (alice's home now re-bound: `{:home, :ok}`; recovery 818/779 ms).
- `make -k -j2 ... set CASES="init-restart steward-restart steward-session-ends steward-lease-ends steward-context-login steward-restart-context"`: all PASS both widths (init-restart 2.0/1.7, steward-restart 186.4/186.4, session-ends 56.3/56.1, lease-ends 9.9/11.2, context-login 7.2/8.2, restart-context 29.2/29.2); earlier chains also steward-context-idle 89.0/88.1 and steward-end-lease 13.8/13.6 PASS.
- `cargo testbench --exact docs` PASS; `size-budget` PASS at the head (three raises named in the commit: libs/wire 3997, servers/steward 1931, servers/init 2779); `unsafe-budget` PASS (no unsafe added).
- Not run: the whole bench (the train's); elixir-oracles and model (no core row, oracle or model change: `effects.rs` only makes two slot constants `pub`).

## What was delivered

- **Wire** (`libs/wire/tables/steward.md`, regenerated Rust and Elixir): 19 `reconnected(server, connection: handle[0])` on init's class; 20 `server_restarted(server, slot: u16)` the steward's send on `notices` (Class::None as a call); 21 `rebind(slot: u16)` → `at, connection: handle[0]` on a session's class.
- **init** (`servers/init/src/reconnect.rs` new, pure; `bin/init.rs`): after `start(i)` and the blame, for each of the steward's handed items at the restarted server's endpoints: mint the badge (stamped `users`), `new_connection` waiting `reconnect::SERVER_TIMEOUT` (3 s = twice the measured maximum, rounded up to the next whole second; the module doc and init.md quote the measurement), close the badge, `reconnected` within `blame::TIMEOUT`, the fresh-list entry replaced only when the steward took it, else the connection disconnected again; the line names the wait and the outcome (`init: walfsd:data reconnected for the steward after 652 ms: the steward took it`), a server not serving in time `init: X made the steward no new connection within 3 s of its restart`.
- **Steward** (`servers/steward`): `Kernel` gains `reconnected`, `slots_at`, `restarted`, `rebind`; the machine swaps the handle (`replaced`), drops its attached connection, kept carves (now keyed with the server name) and held records at that server, derives the slots to announce from the binding table (path-bound, at that server; the console and the named handle never), sends the notice as `notify` does, and re-makes a slot's connection through `connect` (the same scope, root, quota); the driver's `reconnected`/`rebind` (the `made` token replaced, the dead record released) with `effects::{SCOPE, CONNECTIONS}` made `pub`; the protocol routes the three messages by class and answers `rebind` with the handle (`close_after_reply`); `Step::Detach` skips the relay call when the destroy already took the relay's process (a lockout's batch no longer waits out a dead relay's second).
- **Shell** (`Redoubt.Shell.Notices`): `notice/4` decodes both notices; on `server_restarted` the listener calls `Steward.rebind` on the session's own steward connection, binds the reply's connection at the answered path (`Redoubt.Namespace.bind` replaces the prefix) and prints `[walfsd:data restarted: /home/alice re-bound]` or `[... slot N not re-bound (reason)]`.
- **Cases**: `tests/steward-server-restart.toml` (three cuts, re-binds, a login after, the lockout on the machine, the refused login; the console session's line is not in the ordered expectations: it lands anywhere relative to init's blame line); `steward-blame-lockout` updated to the re-bound home and the `reconnected` line.
- **Pages**: steward.md "Failure and restart" (the rule, the operations, the order, the residuals), "The steward's protocol", "The lease and approval operations", "Leases", "Crash blame" (the lockout at three on the machine), R40 (built · tested); init.md "Fresh connections per child" (the rule, the bound and its measurement, the lines, the window before `reconnected`), "Restarts and reboots" (Reconnected after the blame, why; status), residual risks (a lost hand-over; a lockout's blame reported lost); sessions.md, shell.md, agents.md (a lease is not re-bound: `end_lease` and a new `start_agent`); SECURITY.md R40 built (tested); m3-agents.md's crash-blame row and progress.

## Departures, residuals and traps for the reviewer

- **Order: blame first, then reconnect** (Q4 amended on the thread, accepted): the blame's second must not be spent behind the server's recovery; init.md states the order and why.
- **The third blame's `lost` line.** On both widths `steward-server-restart`'s third blame prints `init: walfsd:data's fault blamed ...: lost: the steward did not answer within 1 s` while the steward audits `Blamed` and `LockedOut` and ends bob's session. Diagnosed with temporary timing lines (not committed; `.tmp/STEWARD3X/diag3/`): the lockout's end batch releases bob's channel console with an `ended` send to `sshd` that waits its full `RELEASE_TIMEOUT` second (sshd closed the channel first and is calling the steward, or is busy), and the blame is answered after the batch. The same wait hit `steward-blame-lockout` under load before (a logout coinciding with the cut). Stated as the residual on steward.md and init.md with the numbers; STEWARD3Y (the Architect's draft) owns the deferral of end batches' cleanup. The case therefore forbids `lost` for `reconnected` and for `ended`/`refused` blames only; the audit lines are the lockout's verdict (rule F: the steward's own lines).
- **A lease is not re-bound** (Q3 (a)): agents.md and steward.md say so; (c), a `notices` endpoint for leases, is a later node.
- **Named handles and the console**: slot 5 (`erofsd:system`) is not announced (a code source no bind replaces; its restart leaves sessions dead); slot 4 never, `rebind(4)` unknown.
- **A `reconnected` the steward does not take** strands it at that server until its own restart; a login between the restart and `reconnected` is refused and retried by the person: both stated.
- **`steward-server-restart`'s bob step at the third cut** matches the shell's redraw of the accepted line before closing his input (a line typed and closed on at once was a logout: run 4 in `.tmp/STEWARD3X/restart-rv64-4/`), and expects no answer: his VM dies with the budget before it prints.
- **Measurements for the bound** (`init: ... after N ms`): first restart 589-953 ms over nine runs on both widths, one earlier run past the old 1 s bound before the instance served, unmeasured (`lockout-alone/`); later restarts 8-71 ms. The rule, stated on init.md and in the module: twice the measured maximum plus a second's margin for the run the old bound cut short, rounded up: 2 × 0.953 + 1 = 2.9 s, so 3 s.
- The `expect` list of a case is waited for before the sessions start: a line produced during the sessions cannot go there (one failed attempt, run 6).
- Files read whole by this implementer: every file committed (bin/steward.rs, bin/init.rs, tests/steward.rs, effects.rs included), the pages by their edited sections and status lines.

## Summaries checked

- `README.md`, `docs/README.md`, `GETTING-STARTED.md`: no claim about restarts or re-binding: no change.
- `docs/plan/m3-agents.md`: the crash-blame row and the progress paragraph (changed).
- `docs/servers/README.md`: the steward's and init's sentences hold: no change.
- `docs/servers/serving.md` R27 ("the server restarts" with empty tables): holds; the recovery is the steward's and init's, named on their pages: no change.
- `docs/kernel/processes.md` R21: no change.
- `docs/userland/README.md`, `docs/userland/beamlet.md`: the notices handle is described on sessions.md: no change.
- `libs/wire/tables/steward.md` intro: updated for the three rows.
- `servers/init/README.md`, `servers/steward/README.md`: none exist.

## Rebased onto main 76d60b116 (train 44): head 86aa77767

- `git rebase --onto 76d60b116 15cea878a wp-STEWARD3X`: 11 conflicts (SECURITY.md, steward.md, the wire table and its three generated codecs, steward lib/protocol/bin/tests, size-budget.toml, notices.ex), resolved by hand: the wire table reads 17 lease_ended / 18 approval_waiting / 19-21 mine and the codecs were regenerated from it; the steward's `Kernel`, protocol classes, binary and test Recorder carry 3B's approval methods beside mine; `notices.ex` decodes `approval_waiting` too (`line/4` prints `[an approval is waiting]`, 3B's test passes); the test harness pushes both consoles for `approval_opened` as 3B's does.
- **Review folds** (steward-red-4, OK with notes): P2, `rebind` from a lease's badge is answered `unknown` (drive.rs; steward.md and agents.md name it; host test asserts the refusal and the holding unchanged); P3, docs rerun: PASS.
- Ceilings by the sum rule over main's: libs/wire 3943 → 4003, servers/steward 2278 → 2427 (3B's 2278 + my 146 + 3 for the lease check), servers/init 2702 → 2779; named in the commit; `size-budget` PASS.
- Gates at 86aa77767 (`.tmp/STEWARD3X/*-rebase.log`, `set-rebase.log`): host tests init 23+1+66, steward server 3+4+4+34, core 38+6, wire; fmt clean; shell `mix format` clean, `mix test` 314 passed; rv64+rv32 checks clean; docs PASS; prebuilt rv64 334 / rv32 322, 0 failed; **steward-server-restart PASS 16.2 s / 14.9 s; steward-blame-lockout PASS 11.2 / 10.1 s; init-boot PASS 6.6 / 6.7 s** (3B's manifest, 6 keys); set PASS both widths: init-restart, steward-restart, steward-session-ends, steward-lease-ends, steward-context-login, steward-restart-context, steward-end-lease; unsafe-budget PASS.
