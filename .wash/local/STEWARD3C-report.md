# STEWARD3C report: crash blame on the steward

**Head: 15cea878a**, ONE commit on STEWARD3A's accepted head 2d831801b, branch wp-STEWARD3C,
worktree /home/mcloonan/redoubt/.worktrees/STEWARD3C, tree clean. Gates below, logs under the
worktree's `.tmp/STEWARD3C/`. Written by steward3c-implementer. Reviewer: steward-red-3 (Tier A).

Ruling built from: `.wash/local/STEWARD3C-Q1-ruling.md` (QA `STEWARD3C-shared-restart`): the
restart gap is a design gap with its own follow-up node (STEWARD3X, the Architect's draft); case 7
proves one blame attributed and counted on the machine; the lockout at three is the host test;
the blame timeout is `RELEASE_TIMEOUT` (1 s). Questions: `.wash/local/STEWARD3C-questions.md`.

## What was delivered

- **The core needed no change**: the `blame` machine, `lock_out`, the `LockedOut` rows of the
  session and lease machines, `blame.trace`, the oracle's clauses and the model's two R40
  mutations (`PolicyBlameNoWindow`, `PolicyNoLockout`) were already built. No new row, guard or
  RNG draw, so no oracle, trace or reach-table change.
- **Server** (`servers/steward/src/protocol.rs`): `Message::Blame` on `init`'s class →
  `EventKind::Blame { account, labels }` (labels decoded from the wire bytes), answered
  `BlameReply` as it arrives whatever it counted toward; on any other badge malformed as before.
  The core's `LockedOut` end batches run through the existing drive (DestroyBudget, releases,
  notices); the audit lines `Blamed` and `LockedOut { until }` print as every record does.
- **init** (`servers/init`): `check.rs` plans `init`'s own badge at the steward's entry as
  `STEWARD_INIT_BADGE` 3 (the steward's `protocol::INIT`), refuses a `handed` item naming 3 at
  the steward's endpoint (`Why::Badge`), and counts `init` among the steward's callers;
  `bin/init.rs` mints it at each start of the steward (`own`), and `restart()` keeps the fault
  line's `; blamed on nobody: no steward` only when no steward runs or the steward itself
  faulted (`; blamed on nobody` for a notice naming nobody), then after `start(i)` calls
  `blame(account, labels, server)` with `typed::call_within` and `blame::TIMEOUT` and says
  `init: NAME's fault blamed on account A, labels L: the steward took it | lost: the steward did
  not answer within 1 s | lost: the steward ended | lost: the steward refused it`. New pure module
  `src/blame.rs` (`goes_to_the_steward`, `nowhere`, `taken`, `said`, `TIMEOUT`) with three host
  tests, the timeout's among them.
- **Case** `tests/steward-blame-lockout.toml` (both widths): the image's programs with `walfsd`
  built `cut-after-write`; bob writes a block, arms the cut (`File.stat("/home/bob/walfsd-cut-1")`
  → `:enoent`), writes again (`{:cut, {:error, :eio}}`); console: `init: walfsd:data (PID 24)
  faulted, code 10, serving account 1002, labels none`, `init: restarted walfsd:data`, `steward:
  audit 1002/{} at N: Blamed`, `init: walfsd:data's fault blamed on account 1002, labels none: the
  steward took it`; bob's shell answers `{:bob_alive, 2}`, alice's `{:after, 2}` and her dead home
  `{:home, {:error, :eio}}` (stated, not hidden). Forbidden: `LockedOut`, `lost: the steward`,
  `blamed on nobody`, any other server ending, a volume-check problem. Verdicts: the kernel's blame
  in init's lines, the steward's audit, the two shells' own answers (rule F).
- **Host tests**: `redoubt-steward-server::three_blames_in_the_window_lock_the_domain_out_until_the_window_passes`
  (two blames change nothing but the count; the third destroys bob's session and lease budgets
  and nothing else, audits `Blamed`, `LockedOut { until }`, `LeaseEnded`; bob's login refused
  `locked_out` while alice's vault login is admitted; blame on nobody and on an unknown label set
  taken and counted nowhere; after the window bob is admitted; three blames on alice/{} destroy her
  unlabelled session alone, her vault session stands, her vault login is admitted while her
  unlabelled one is `locked_out`); `every_operation_on_another_badge_class_is_malformed` gains
  `blame` on every other class; `redoubt-init`: `a_handed_badge_is_a_root_badge_given_once_at_its_endpoint`
  (badge 3 at the steward refused, 2 admitted), `the_image_manifest_passes...` (init's badge
  (11, 3)), and the blame module's three.
- **Pages**: steward.md "Crash blame" and R40 built · partly tested (the lockout at three is the
  host test); "The steward's protocol" (`blame` bullet; `init`'s badge minted, not handed); "The
  lease and approval operations" status; "Failure and restart" gains the ruling's bullet "A
  shared server restarts" (the gap, the planned way out, the counts kept). init.md "Restarts and
  reboots" (the blame bullets with the lines, the timeout, the badge; status with the case and
  the three host tests), "Fresh connections per child" (the gap beside the consoled residual),
  the manifest's "smallest badge" bullet. SECURITY.md R40 row built, partly tested.
  m3-agents.md's crash-blame row and progress paragraph. agents.md's blame bullet already said
  what the code does: no change.

## Gates (every command through `scripts/q`)

- `cargo test --locked -p redoubt-steward-server`: 2 + 4 + 30 passed (`.tmp/STEWARD3C/server-tests-4.log`).
- `cargo test --locked -p redoubt-init`: 21 + 1 + 66 passed (`init-tests-2.log`; 21 includes the blame module's 3).
- `cargo check --locked -p redoubt-steward-server -p redoubt-init --target riscv64gc-unknown-none-elf` and `riscv32imac-unknown-none-elf`: clean.
- `cargo +nightly fmt -p redoubt-init -p redoubt-steward-server -- --check`: clean.
- `make -f scripts/jobs.mk prebuilt`: rv64 329 cases / rv32 318 cases, 0 failed (`prebuilt-2.log`).
- `make -f scripts/jobs.mk rv64/steward-blame-lockout`: **PASS 11.1 s**; `rv32/`: **PASS 10.0 s** (`case-rv64.log`, `case-rv32.log`; console and session logs in `blame-rv64/`, `blame-rv32/`).
- `make -k -j4 -f scripts/jobs.mk set CASES="init-restart walfsd-power-loss steward-restart steward-session-ends steward-lease-ends steward-host-tests init-host-tests elixir-oracles steward-model-host-tests model-host-tests size-budget formatting unsafe-budget docs"` at c0a248244 (the same code; the pages and the case were added after): all PASS, rc 0 (`set-1.log`): init-restart 2.0/1.7 s, walfsd-power-loss 0.8/0.8 s, steward-restart 186.4/186.3 s, steward-session-ends 56.7/55.7 s, steward-lease-ends 10.4/9.3 s; host: steward-host-tests, init-host-tests, elixir-oracles 24.4 s (run-traces: no untaken row), steward-model-host-tests 28.7 s, model-host-tests 148.4 s (the families), size-budget, formatting 19.8 s, unsafe-budget, docs.
- At the final head: `cargo testbench --exact docs` PASS 2.1 s; `--exact size-budget` PASS (the commit's two `Size budget:` lines: servers/steward 1785, servers/init 2702).
- Not rerun at the final head: steward-lease-ends, steward-session-ends, steward-restart, init-restart (code unchanged since c0a248244; only pages, the case file and the server test changed, and the server test crate was rerun). The unsafe ratchet: no `unsafe` added.

## Summaries checked

- `README.md`, `docs/README.md`: name M3's goal, claim nothing built of blame: no change.
- `GETTING-STARTED.md`: no blame claim: no change.
- `docs/plan/m3-agents.md`: the crash-blame row and the progress paragraph (changed).
- `docs/servers/README.md`: the overview's steward sentence ("decides what a crash costs") holds: no change.
- `docs/userland/agents.md`: the blame bullet says what the code does: no change.
- `docs/kernel/processes.md` R21: "the steward counts it by (account, label set)" holds: no change.
- `docs/kernel/model.md`: no mutation or reach change: no change.
- `libs/wire/tables/steward.md`: the `blame` row and intro hold: no change.
- `servers/init/README.md`, `servers/steward/README.md`: none exist.

## Departures, residuals and traps for the reviewer

- **The brief's case 7** ("three times within the window ... his next login refused ... a second
  run from the vault") is not what the machine can show: a restarted `walfsd:data` strands the
  steward's kept connection (and every session's), so no second login of bob's gets a home. The
  ruling makes it one blame on the machine and the lockout, the window and the vault arm the host
  test; the gap has its own node. `init.md`'s "A worked configuration" (planned) still describes
  the three-crash lockout; it is planned text and stands.
- **Blame's `server` string** reaches the core nowhere: `Record::Blamed` carries no server (the
  core's records are not this package's to change); `init`'s own line names the server. The audit
  log (M5) is where the record would carry it.
- **mdbook's search index crossed its 10 MB warning threshold** (10,000,112 bytes) with this
  package's prose, and `the_book_builds` fails on any mdbook `WARN`; I trimmed ~600 bytes of my
  additions to pass. Any package adding prose next will hit it: a book-wide decision (the search
  index's scope or the test's tolerance) is the orchestrator's/docs owner's, not taken here.
- **Pre-existing**: `servers/init/src/check.rs:1146` unused variable `s` warning (at base).
- **`walfsd:alice-secrets`** also runs with `cut-after-write` in the case (one binary); nobody
  arms it, and its volume check line is matched once.
- Files read whole by this implementer: everything committed (protocol.rs, the two test files,
  blame.rs, lib.rs, check.rs, init.rs, steward.md, init.md, SECURITY.md, m3-agents.md,
  size-budget.toml, the case file).

## Review folds and the rebase onto STEWARD3A's amended head (steward-red-3: OK with notes)

- Folds (docs only, amended into the one commit, 09c209d7f): steward.md "Crash blame" gains the
  residual line that `Record::Blamed` carries no server name (`init`'s line does, until the audit
  log); SECURITY.md R40's residual column names the restart gap (STEWARD3X, open). Docs PASS.
- `git rebase --onto e825c2defa 2d831801b wp-STEWARD3C` → **head e562a2879**, ONE commit on
  e825c2def (STEWARD3A amended for train 42), tree clean. One conflict, `docs/SECURITY.md`: 3A's
  new R39 row beside my R40 row, both kept. `tests/size-budget.toml` auto-merged: 3A's ceilings
  (libs/wire 3937, libs/steward 4086, model 11389) and mine (servers/steward 1785, servers/init
  2702, the commit's `Size budget:` lines).
- Gates at e562a2879 (via q): fmt clean; `cargo test -p redoubt-init -p redoubt-steward-server`
  21+1+66 and 2+4+30 passed (`host-tests-rebased.log`); docs PASS 2.3 s; size-budget PASS;
  prebuilt rv64 329 / rv32 318, 0 failed (`prebuilt-3.log`); **steward-blame-lockout PASS rv64
  11.0 s, rv32 10.0 s** (`case3-rv64.log`, `case3-rv32.log`; transcripts `blame3-*/`).
- Range-diff for the renewal: base e825c2def, previous head 09c209d7f on 2d831801b.
