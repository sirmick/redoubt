# STEWARD3Z report (implementer)

Worktree `.worktrees/STEWARD3Z`, branch `wp-STEWARD3Z` on main 68c4b2904; one commit, head
**262e1cc87** (`rt, steward, consoled: …`); `git status --short | wc -l` = 0. Reviewer:
steward-red-4. Not pushed.

## Design taken

**(1), the primary; (2) not needed.** A minted connection's `State` is charged to its minter at
the mint and is *pending*; the first request on it, a T-message, a `ninep_common` one or a typed
one, moves that `State` to the caller's bucket and share (`Minted::key`/`share`), where the
caller's caps apply; a caller whose bucket is full is refused that request (`TOO_MANY` / typed
`refused`) and the connection stays pending for a later one; the same bucket (a system caller's
chain, keyed by its root; a client's own chain) settles in place; settled once it never moves;
disconnect/forget release from wherever it was charged by then. **The pending pool's bound, as
implemented:** the minter's own `State` cap in its own bucket (the steward's root badge at
`erofsd:system`: the default 8 at its account-0 bucket, which has no shares to divide): the ninth
mint is refused before anyone calls. No copy, no new bucket at any server, no init change, no
steward code change (steward.md's Authority bullet says what the steward's share holds).

Code: `libs/rt/src/server/minted.rs` (`Entry::settled`, `Minted::pending`, `Minted::settle`),
`libs/rt/src/server/ninep.rs` (`pub fn arrived`, called at the top of `answer`, in
`answer_common` after decoding, and before `own` for typed requests), module docs in both and in
`admit.rs`.

## A machine-only failure, found and fixed (one line outside the owned paths: flagged)

With (1) the host tests were green but every session VM on the machine died at once (exit 1, no
output): `steward-three-contexts` and the new case alike. Bisection on the machine (settle in
place at one server class per run) left ipd: `servers/ipd/src/server.rs` `open_sockets` runs
before `serve_parking` for every 9P request and reserves the caller's bucket up to its `State`
cap for the sockets the request may make, so the first request on a session's `/net` grant found
its account's bucket full, the move was refused, the shell's namespace open failed and the VM
halted. Fix: `arrived` is public and documented for a server that charges ahead of the
skeleton, and ipd calls it first in `open_sockets` (one line + a comment; ipd.md one sentence).
ipd is not in the brief's owned paths: the orchestrator should confirm or hand that line to its
owner; without it the package does not boot a session.

## The derivation's new term

`tools/testbench/src/case.rs` `the_servers_admit_what_a_manifest_lets_one_account_hold`: every
shared server's `state` is at least the steward's mints for one account there: erofsd
2 × sessions + leases (sessions = console + cap + spare: 2 × 5 + 1 = 11), bootfsd/walfsd/littlefsd
sessions + leases (6 ≤ 8), consoled the console session's + a spare (2); `>=`, with the reason
(a session's own mints count in the same bucket). erofsd's `state` 8 → 11 (a bucket 226,816 B,
six fit 1.5 MiB, not a seventh). Dev-deps on erofsd, bootfsd, consoled added to the testbench.

## consoled: (A), per the ruling's four conditions

Default bucket: 2 connections, 4 fids (derived: the UART session's console the steward mints for
the console principal — a context over ssh gets its relay's, a lease none, by the steward's slot
table — plus the spare); init's root badge 1 (init refuses any handed consoled, so init's is the
first) gets an `Override` of MAX_THREADS connections and 2 × MAX_THREADS fids; the fit is
`Admission::with_overrides` + `Admission::fits`: 161,280 B per default bucket, 355,584 (rv64) /
183,552 (rv32) for init's; 2 MiB holds init's + 10 defaults on rv64, + 11 on rv32; BAD_LIMITS and
the entry's 1,024 pages unchanged; the count rule (domains + init's one bucket, the steward's
console counting there) on consoled.md. No manifest changes.

## The six-principal wall (ruled to STEWARD3V)

STEWARD4's six-principal manifest asks keyd 10, consoled 9, bootfsd 10, ipd 11, walfsd:data 10,
walfsd:alice-secrets 9, erofsd:system 10 buckets; erofsd (1.5 MiB, 6 × 226,816), bootfsd (1 MiB,
6 × 165,888) and walfsd (4 MiB, 6 × 626,688) hold six, so consoled was only the first server
init started. Per the Q2 ruling: not this package's; STEWARD3V (every shared server's usable
budget from its entry's pages) takes it with steward-latency's N = 4 and 16 and the table.
scheduling.md untouched.

## Cases

- `tests/steward-many-sessions.toml` + `tests/data/steward/many-sessions.manifest.json`
  (three-contexts' manifest with alice 81,920 pages, bob 40,960 pages / 30 processes, the
  image's agent size 5248/2): console + alice.a + bob + bob.b, alice.a and bob each start two
  spinning ten-minute leases, every session evaluates while all four leases are live (twelve
  steward-minted system-volume connections), the sponsors end their leases. PASS both widths.
  Verdicts from the system: the steward's audit lines (four `AgentStarted`, four `LeaseEnded`),
  sshd's login lines, each session's own output and exit 0; `steward: a step failed`, refused
  logins and `:refused` forbidden.
- rt host tests: `a_connection_minted_for_another_account_is_admitted_there_from_its_first_request`
  (eight minted, ninth refused, six accounts' first calls move theirs, a second for one account
  counts beside its first, settled once never moves, the minter's disconnect frees a pending one,
  a settled one's disconnect frees the account's) and
  `a_first_request_on_a_connection_its_account_cannot_hold_is_refused_and_it_stays_pending`.
  Two existing tests' counts updated with the reason (the minted connection now counts in its
  holder's share).

## Gates (every command through `scripts/q run` / `scripts/jobs.mk`; logs in `.tmp/steward3z-*.log`)

| gate | command | result |
| --- | --- | --- |
| rt host tests | `q run --cores 4 -- cargo test -p redoubt-rt --lib` | 90 passed, 0 failed (exit 0) |
| rt fuzz crate | `q run --cores 4 -- cargo check` in libs/rt/fuzz | builds (exit 0) |
| init / steward / ipd / consoled / consrelay / sshd host tests | `q run --cores 4 -- cargo test -p <crate>` | all ok (exit 0) |
| bench host test | `q run --cores 4 -- cargo test -p testbench the_servers_admit` | 1 passed (exit 0) |
| every rt-linking guest package, rv64 and rv32 | `q run --cores 8 -- cargo build --release --target <triple> -p …` (26 packages) | exit 0 both; one pre-existing init warning (check.rs:1154, untouched) |
| docs | `make -f scripts/jobs.mk docs` | PASS (rc 0) |
| fmt | `cargo +nightly fmt --all --check` | clean |
| unsafe-budget | `make … rv64/unsafe-budget` | PASS |
| size-budget | `make … rv64/size-budget` against the commit | PASS (libs/rt 4013→4048, consoled 408→423, ipd 2925→2926, each with its `Size budget:` line) |
| steward set + others, both widths | `make … set CASES="steward-ssh-two-principals steward-context-cap steward-lease-ends pipe-sixty-four aio-many-reads-two walfsd-quota init-boot steward-three-contexts"` | 16/16 PASS (exit 0) |
| steward-many-sessions, both widths | `make … set CASES=steward-many-sessions` | PASS rv64 26.3 s, rv32 26.0 s (exit 0) |

Not run: the whole bench (the train's); steward-latency (3V's).

## Pages and summaries checked

Changed: serving.md (`admit` rule + status 14, Minted connections, R26 text + status 16, two
residuals: the minter's share before a first call; a cleanup disconnect dropped at a busy server
leaving its State in the account's bucket until the server's restart, 3W's), SECURITY.md R26 row
(two tests), steward.md Authority, erofsd.md, consoled.md (the two bucket kinds, the fit, the
count rule), ipd.md (the move before the reservation), littlefsd.md, walfsd.md, image/README.md
(the system-volume connections per account, 11). Checked, no change needed: README.md,
GETTING-STARTED.md, docs/README.md, docs/TOUR.md, docs/SUMMARY.md, docs/servers/README.md,
docs/plan/m1–m3 (no capacity or consoled-sizing claim), init.md (the count rule lives on
consoled.md; init's check is unchanged), bootfsd.md (its 8 stands above the need 6; its text
makes no per-minter claim), scheduling.md (3V's).

## Open risks

- The ipd line is outside the owned paths (above).
- A server that charges a caller's bucket ahead of the skeleton elsewhere would hit the same
  ordering; ipd is the only one today (grep: no other `charge_of` before `serve_parking`).
- `state` caps are held `>=` the derived need, not `==`: a server may keep room above the sum.

## Fold 1 (steward3z-fixer, 2026-10-16): review notes P3a, P3b; P2 confirmed

Head **badb98675** (262e1cc87 re-amended; tree and message identical to it after the revert
below), on main 68c4b2904; `git status --short | wc -l` = 0; fmt check clean.

- **P3a, the `unused variable s` warning.** Its only source in a net-tests build is
  `servers/init/src/check.rs:1154` (`blocks`; redoubt-init is tests/net's dev-dependency), not
  tests/net itself (`cargo check -p redoubt-net-tests --all-targets`, `.tmp/STEWARD3Z/
  net-tests-check.log`). The line is identical on main (from 15ffdd4fe, 2026-10-05): found
  pre-existing and **left as main has it** (the orchestrator's ruling: not this package's; noted
  for a future init package). A one-line fix was made and then reverted: the package touches
  init nowhere.
- **P3b, the rt host-test log.** `q run --cores 4 -- cargo test -p redoubt-rt` (no `--lib`: every
  target), exit 0: lib 90 passed, 0 failed; the integration targets all ok (budgets 1, connection 2,
  echo 7, given_up 5, heap 5, held_clock 1, ipc 4, mapping_views 1, mux 12, ninep_status 1,
  outcomes 1, panic 1, parked 3, parked_write 1, refusals 1, registers 2, regressions 5, serve 4,
  sleep 1, sync 9, thread 5, transport 1, typed_parking 1, vectors 1; doc-tests 6). Log:
  `/home/mcloonan/redoubt/.tmp/STEWARD3Z/rt-host-tests.log`.
- **P2** (the ipd line): confirmed by the orchestrator, no change.

Next: the rebase onto main's tip when the orchestrator sends its hash.
