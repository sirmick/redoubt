# STEWARD3B report: the powerbox at `approve@box`

Base **377d1770a** (main: 3A's amend, 3C, B78; rebased from 2d831801b). Head: **5d289d0c2**, the
one commit on `wp-STEWARD3B` (3d51c0982 plus tests/init-boot.toml: `keyd holds none of the 6
keys`, the two approval keys counted; init-boot PASS both widths, steward-approve-box rv64 PASS,
`.tmp/STEWARD3B/run-initboot/`), reviewed at 9ddfa0a45 (steward-red-4, OK with notes), the folds in
and renewed at fdb689802; 3d51c0982 differs by one doc comment (the readers' bound said as it
is: at most `CHANNELS` readers plus those between a channel's end and the main loop's taking of
its report). Tree clean. Worktree `/home/mcloonan/redoubt/.worktrees/STEWARD3B`. Questions
and ruling: `.wash/local/STEWARD3B-questions.md`, `.wash/local/STEWARD3B-ruling.md` (thread
`STEWARD3B-channel`, all six points ruled, built as ruled).

## The operations and their rows

No table row, guard, effect or constant of the core changed: the request and approval_channel
machines, `owns_labels`, `pending_cap`, `fair_share`, `approval_key`, `rendered_here`,
`hash_matches`, `notify`, `render`, `drop_requests`, `grant_lease` and the `Granted` row of the
lease machine were there and traced (`request.trace`, `channel.trace`). The oracle, the model
and the reach table are untouched (run-traces: 22 ok, no untaken row; the two steward families
pass). What this package adds is the embedding:

- **`submit`** (protocol.rs, a minted badge): kind 0 a `Note { what }`, kind 1 an `Agent {
  labels, lease }` (the label set as the wire's bytes); kinds 2 and 3 (declassify, push) are
  `unknown` until STEWARD4. Answered `Submit { request }` from `Answer::Request`.
- **`approval_opened(key, console, reader)`** (sshd's badge): the key's id; the principal
  resolved from it (approval keys, then login keys, else `""`), so the core's `approval_key`
  guard keeps the rule and says `bad_key` for a login key, `unknown` for nobody's; the channel
  id drawn as every id the server hands the core (`MINTED | random`); the two consoles stashed
  (`Kernel::approval_consoles`). On the core's Ok: `Kernel::channel_opened` (the consoles are
  the channel's, its reader starts; `Err` → the core is told `ApprovalClosed`, the call fails),
  then a `Pending { channel }` event, so the first screen is written at once; answered
  `ApprovalOpened { channel }`.
- **`approval_closed(channel)`**: `Kernel::channel_closed` (consoles closed, the reader meets the
  end of its input), the kept screen dropped, the core's `ApprovalClosed`.
- **`pending`, `approve`, `deny`** (badge 2, `APPROVAL`): the core's events. `approve`'s `hash`
  is the full 32 bytes, or 4: the code, expanded by `Steward::hash_of_code` from the last screen
  written on that channel (that request's hash starting with the code); not there →
  `hash_mismatch` to the caller, which is the channel's reader; a channel with no screen passes
  a zero hash so the core's answer (`unknown` for a channel nobody holds) is the one given.
- **Outputs** (drive.rs `Steward::event`): `Notice { Session, ApprovalWaiting }` →
  `Kernel::notify_approval` on the session's notices endpoint (wire 18 `approval_waiting`, no
  fields, a send); `Notice { Channel(c), ApprovalWaiting }` → a `Pending { c }` queued in the
  same event, so an open channel of the principal is redrawn; `Screen`s collected per channel and
  written as one block at the event's end (`compose`), the (id, hash) pairs kept as the channel's
  last screen.

## The channel's design (as ruled, Q1/Q2)

- `sshd` (`bin/sshd.rs` `Slot::approve`): the user `approve` with no label and no context mints
  two connections to the channel's console exactly as a login does (`Consoles { steward, relay
  }`: the steward's for screens and `ended`, the second for its reader), sends
  `approval_opened(key, console, reader)` bounded by `LOGIN_WAIT_US`, takes the channel id as the
  session's id, labels `[]`; a refusal is `sshd: login approve: refused (BadKey)` and the login is
  denied (ssh exits 255). `Slot::closed` sends `approval_closed` for an approval channel
  (`approval: bool` on the slot); the console line is `sshd: approval channel ID ended`.
- The steward (`bin/steward.rs`): `CHANNELS = 4` open channels at most (the bound on its reader
  threads, declared as the watchers are), `READER_STACK_PAGES = 8`, `SCREEN_US = 1 s`. On
  `channel_opened`: a send right of badge `APPROVAL` minted on the steward's own endpoint, a
  one-shot `hand` endpoint, `thread_create(reader_thread, stack, hand)`, the hand-over sent on
  it (words: the channel; handles: the reader console, the badge-2 send), then the main loop
  keeps only the screen console under the channel id. `reader_thread`: receives the hand-over,
  closes the hand endpoint, attaches the reader console (ORDWR), loops in `read_at` (blocking:
  sshd parks a read with no input), gathers lines (`approval::Lines`: `\n` ends, `\r` dropped,
  a line over `MAX_LINE` 256 dropped whole and answered `line too long`), parses each
  (`approval::parse`) and calls the steward on badge 2 with `Pending`/`Approve`/`Deny`; the
  answer is written back on the same console: `approved ID`, `denied ID`, `refused:
  hash_mismatch` (the wire's snake name), `unknown command`, or `the steward did not answer`. A
  read of 0 or an error (the channel ended) ends the thread (`thread_exit`). `screen`:
  `Connection::attach_within(SCREEN_US)`, open OWRITE, `write_at`, close: a timed-out write is
  dropped (the person retypes `pending`); the main loop never waits on a reader, a reader's call
  is served by the main loop, so the pair cannot deadlock. `notify_approval`: one bounded send
  (`NOTICE_US`) as `lease_ended`'s. `approval_consoles(None, None)` after every call drops the
  consoles of a refused open, as `console(None, None)` does a refused login's.
- No echo on the channel (it has no line discipline): a residual, named below.

## The rendered screen (Q3)

Per channel, one block of `\r\n` lines, printable ASCII only (the core's `Rendered.text` is
`render.rs`'s whitelist output; the rest is the server's own literals and hex):

```
pending approvals for alice:
request 8000000000000001 hash 3b2c...(64 hex) code 3b2c0e1f
  request from alice (agent-1, labels []): start an agent labelled [7] for 10 min; reason (untrusted): "audit the vault"
type: approve <id> <hash or code> | deny <id> | pending
```

or `no approval is waiting for alice`. The header names the channel's principal (the core's
index). `steward-screen-inert` greps the approver's bytes: no ESC, nothing outside 0x20..0x7E
(the bench drops `\r` before its line checks, `\n` ends the line).

## The namespace (Q4) and the Echo agent

`own::Child::Lease { labelled }`; a labelled lease's slot 1 is `own.vault(principal, labels)`,
fresh at the vault's root, bound at `/project` (no `agent_quota` carve: the vault's own quota);
an unlabelled lease's is the home's `project` carve as before (`tests/own.rs` has both rows).
`Redoubt.Agents.Echo` gains `note TEXT` (one submit, the reason TEXT), `notes N TEXT` (N
submits, the answers one per line) and `agent LABEL MS TEXT` (an agent request for label id
LABEL, MS milliseconds), each on the lease's own `steward` connection through the generated
client, the answers written to `/project/out/result`.

## The bench (Q5) and the case of the share (Q6)

`tools/testbench/src/ssh.rs`: `Step::Expect` keeps the capture groups of the last pattern that
had any; `send` expands `$1`..`$9` (`$$` a `$`; any other `$` stays) (`expand`, host test
`send_types_back_the_last_expects_captures`; testbench.md's session block). `steward-pending-cap`
follows `fair_share = PENDING_CAP / live`: alice's console session, her SSH session and the
agent make the agent's share 1, so its second note is `cap` (the five answers read from the
project), a second agent still gets one, alice's own note is dropped with her session, and the
screen opened after her exit lists the two agents' notes and nothing of hers.

## Cases

- `steward-approve-box` (5 sessions): alice stages `agent 7 600000 audit the vault` and starts
  her agent, whose request is in 1001/{7} (its audit domain); her shell prints `[an approval is
  waiting]`; bob's prints nothing; her vault session staged `/vault/in`; `approve@box` with the
  login key is refused (BadKey, ssh 255) first, then with the approval key shows the screen; a
  zero hash and a wrong code are `refused: hash_mismatch`; the code approves; the labelled lease
  is carved from 1001/{7} (2 processes), runs `echo from the vault` in the vault (`/vault/out/
  result` read by the vault session), ends (0 processes); `pending` shows none waiting.
- `steward-screen-inert`: the reason is 64 scalar values (8 × ESC[2J, 8 × U+202E U+2066 U+200B
  BEL); the screen shows `"?{64}"`; the approver session's forbid is `\x1b` and `[^\x20-\x7e]`.
- `steward-pending-cap`: above.

## Docs

steward.md: the badge classes (badge 2 is the steward's own), the protocol's bound operations,
"The lease and approval operations" and "The powerbox and approvals" statuses, the share
sentence, the figure built, "The channel, as built", the attack test's case, R38 built;
sshd.md: `approve@box` built with how `sshd` serves it and why the code is accepted, R68 built;
sessions.md: `approve@` built, the code and the notice line; agents.md: a labelled agent's
`/project`; SECURITY.md: R38 and R68 rows; m3-agents.md: the two approval rows; testbench.md:
the capture line; the wire table's intro; book.toml unchanged (B78 on main fixed the search
index's cause; the level-2 split of the first round was dropped in the rebase).

## Gates (logs under `.tmp/STEWARD3B/` in the worktree)

Bench cases, prebuilt (`make -f scripts/jobs.mk prebuilt`, rc 0 both widths), one job per case
through q (`run8/`, `run10/`, transcripts included): rv64 steward-approve-box 16.8 s,
steward-screen-inert 7.2 s, steward-pending-cap 12.5 s, steward-ssh-two-principals 14.8 s,
agent-login 9.7 s, steward-lease-ends 9.8 s; rv32 15.8 s, 6.6 s, 11.8 s, 14.9 s, 8.8 s, 9.4 s:
all PASS. The ten of run8 ran at the head's code; after it only `tests/steward-pending-cap.toml`
(its expectations), `tests/formatting.toml`, `tests/size-budget.toml` and `image/README.md`
changed, and pending-cap reran green on both widths (run10). Earlier rounds (run1–run7, 9):
the opening screen written inside sshd's `approval_opened` call (deferred past the reply), the
parser lacking the code form, rv32's 32-bit words for the hand-over (split), and expectation
mistakes (the `{7}` usage counts hold the vault session's 10 processes; `[2J` stays printable;
a poll reading an empty file; the shell wrapping long results; the session's end vs. the
bench's close of the pty; the screen's order by id).

Host (`final-host-tests.log`): `cargo test -p redoubt-steward-server -p redoubt-sshd
-p redoubt-init -p redoubt-steward -p redoubt-wire-gen -p testbench`: 438 passed, 0 failed
(the server 2+4+32 incl. the three approval tests and the fuzz corpus of 6 seeds; the bench
214 incl. `send_types_back_the_last_expects_captures`). `cargo testbench --exact`: docs PASS,
size-budget PASS (sum rule: servers/sshd 1356 → 1419, servers/steward 1778 → 2251, both named
in the commit), formatting PASS (servers/steward/fuzz added to its roots), unsafe-budget PASS.
Shell (`shell-tests.log`): `mix format --check-formatted` clean, `mix test` 312 passed, 70
skipped. Cross checks (`final-cross-check.log`): `cargo check --locked -p redoubt-steward-server
-p redoubt-sshd -p redoubt-init` on riscv64gc and riscv32imac, rc 0. Oracle
(`oracle-run-traces.log`): run-traces rc 0, 22 traces ok, no untaken row (the core is
unchanged). Model (`model-families.log`): steward_policy and steward_noninterference pass
(55.7 s); no new rule, no new mutation, the reach table untouched. `cargo +nightly fmt --check`
clean on the four changed crates. Not run: the whole bench (the train's), the model mutations
(no rule changed), a fuzz campaign beyond the seed corpus (`cargo fuzz run approval
seeds/approval` from `servers/steward/fuzz` is the command).

## Summaries checked

- `README.md`: M3 still "planned" as a milestone; the powerbox is one package of it: no change.
- `docs/README.md`, `GETTING-STARTED.md`: no approval claims: no change.
- `docs/servers/README.md` (trusted servers cross principals for approvals), `docs/userland/
  README.md` (`approve@box` the one place Alice answers): true as written: no change.
- `docs/plan/m3-agents.md`: the two approval rows of the attack table now name the cases.
- `image/README.md`: names alice's and bob's approval keys among the test keys.
- `userland/README.md`, `servers/steward/README.md`, `servers/sshd/README.md`: none speak of
  approvals or do not exist: no change.
- `docs/kernel/model.md`: the reach table's numbers are unchanged (no row added): no change.

## Rebase and review folds (fdb689802)

- Rebase onto 377d1770a: conflicts in steward.md (badge classes, the bound operations, the
  operations' status line: 3C's blame and this package's powerbox merged), SECURITY.md (R38 mine,
  R39/R40 main's), protocol.rs and tests/steward.rs (3C's blame arm and tests beside the
  powerbox's), tests/size-budget.toml (sum rule over main's: servers/steward 1785 → 2278
  (+493), sshd 1356 → 1419 (+63), libs/wire 3937 → 3943 (+6), each a `Size budget:` line).
- P1 readers pooled: one work endpoint for the readers (`reader_work`, hand-over badge `HAND`,
  bounded `HAND_US` 1 s: P3), a reader takes the next channel after reporting its end on
  `READER_DONE` (badge 5) to the main loop, which counts it idle (`Watchers`, the exit watchers'
  pool); a new thread only when every reader is busy; `CHANNELS` bounds both. Host test
  `watchers::channels_coming_and_going_reuse_one_reader` (a hundred opens and closes: one
  stack); the bin's pool is not host-testable, its arithmetic is.
- P2 `every_operation_on_another_badge_class_is_malformed`: submit off a minted badge,
  approval_opened/closed off SSHD, pending/approve/deny off APPROVAL, no console taken.
- P3 after the deny: approving the denied request by its (earlier screen's) code is
  `hash_mismatch`.

Gates at fdb689802 (`.tmp/STEWARD3B/`): cases both widths PASS (run-fold/): approve-box
18.2/17.0 s, screen-inert 8.1/7.8, pending-cap 13.9/12.5, ssh-two-principals 15.8/15.7,
agent-login 10.1/9.3, lease-ends 10.3/9.9, beamlet-footprint 5.3/5.1 (rv64/rv32). Host tests
444 passed, 0 failed (fold-host-tests.log); at 3d51c0982, under q, output kept: docs PASS
(`run-fold/host-docs.log`), size-budget PASS (`run-fold/host-size-budget.log`), formatting PASS
(`run-fold/host-formatting.log`), unsafe-budget PASS (`run-fold/host-unsafe-budget.log`),
`cargo run -p redoubt-wire-gen -- --check` rc 0 (`run-fold/host-wire-gen-check.log`); fmt --check
clean; rv64/rv32 checks rc 0; shell 312 passed, format clean (rebase-shell-tests.log).

## Residuals

- No echo on the approval channel (no line discipline on it): a person types blind; the bench
  does not need it. A later package can echo from the reader.
- `pending`/`approve`/`deny` as wire calls are the steward's own readers' alone; the wire table
  keeps them so the badge class is one place.
- Declassification and push: STEWARD4 (`submit` kinds 2 and 3 answer `unknown`).
- The approval channel's reader count is a server constant (`CHANNELS` 4), not the core's.
