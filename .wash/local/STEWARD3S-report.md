# STEWARD3S report (steward3s-implementer)

HEAD: wp-STEWARD3S 90977a48d (one commit on dcdb9bce1, STEWARD3R's accepted head; main 43588e0d6). GATES: all green on both widths, see "Gates" below; evidence under `.worktrees/STEWARD3S/.tmp/STEWARD3S/`.

## What was built

- **Wire** (`libs/wire/tables/ipd.md`): op 16 `grant` is `scope: bytes`, `revocation: handle[0]? budget`
  (reply unchanged: `conn: handle[0] endpoint`, `id: u64`). The optional slot follows the rule
  3R added to `tables/example.md` (`handle[N]? KIND`, after every required slot). The plan body
  wrote the field as `scope: handle[0] budget`; the grant's one existing field is already
  `scope: bytes` (the network scope, the word `scope` means prefixes and ports throughout
  servers/ipd.md), so the handle is named `revocation`. Codecs regenerated with
  `cargo run -p redoubt-wire-gen`: `libs/wire/src/proto/ipd.rs` (layout `handles: 1, optional:
  1`, `handle_names` `["revocation"]`), `libs/wire/elixir/proto/ipd.ex`,
  `libs/wire/elixir/client/ipd.ex` (`grant(conn, scope, revocation \\ nil, timeout)`).
- **Serving library** (`libs/rt/src/server/ninep.rs`): `mint_rooted(caller, root, scope, kernel)`
  passes the scope to `mint_at`, which already stamped `new_connection`'s mint with it. Callers
  that mint in their own stamp pass `None`: `servers/sshd/src/bin/sshd.rs` (2), `servers/sshd/tests/console.rs`
  (2), `servers/consrelay/src/bin/consrelay.rs`, `libs/rt/fuzz/fuzz_targets/ninep_server.rs`,
  `libs/rt/src/server/ninep_common_tests.rs` (4).
- **ipd** (`servers/ipd/src/server.rs`, `answer_grant`): the request's one optional handle is
  the revocation scope, passed to `mint_rooted`; on success the outcome closes the handles the
  request brought plus ipd's copy of the minted handle (before: only the minted copy); on a
  failed encode the same list. A refused grant closes what it brought as before. The kernel's
  refusal of a scope not at or below ipd's stamp is `NO_ID` from `mint_at`, mapped to
  `not_permitted` as every non-`too_many` mint failure already was; nothing is minted.
- **Steward** (`servers/steward/src/bin/steward.rs`, `Kernel::connect`, `How::Grant`): the grant
  is sent with `&[scope]`, the object's scope `drive.rs` already hands `connect` for every slot
  (there is no `Server::Net` in `drive.rs`: `/net` is a `Server::Shared(slot)` bound to
  `How::Grant` by `own::binding`, so the drive is unchanged). The comment saying the grant carries
  no scope is gone.

## Tests added

- `host:redoubt-ipd::a_granted_connection_in_a_scope_ends_when_the_scope_is_destroyed`
  (`servers/ipd/tests/program.rs`, the whole program over the fake kernel; the twin of rt's
  `a_connection_minted_in_a_scope_ends_when_the_scope_is_destroyed`): ipd stamped with a
  `users`-like budget (`Fake::runs_in`); the steward grants with a scope elsewhere (refused
  `not_permitted`, nothing minted), then in S and in T; ipd's held handles are back to the count
  before (no copy of a minted handle, no scope kept); each granted connection connects to the
  peer through its principal; S destroyed: the principal's handle is `BadHandle` and its table
  empty, T still connects; ipd's record of S freed by the steward's `disconnect` once (`Remote`
  the second time). The verdict is the fake kernel's stamp rule (3R's `mint` from a call defaults
  to the handle's stamp and only narrows; destroying a budget takes every handle stamped with
  it), not the test's: if ipd ignored the slot, the S connection would be stamped `users` and
  survive, and the `BadHandle` assertion would fail.
- `host:redoubt-ipd::a_grant_is_minted_in_the_revocation_scope_it_brings` (`servers/ipd/tests/fs.rs`,
  `answer_grant` over the fs fake Minter, which now records each mint's scope): the handle
  brought reaches `mint` as the scope and the outcome closes `[revocation, conn]`; without one
  the mint is stamped `None` and only ipd's copy closes; a refused grant mints nothing and closes
  what it brought.

## Docs changed

- `docs/servers/steward.md`: R41's residual bullet ("ipd's grant carries no scope") removed from
  Residual risks; the R41 paragraph says ipd's typed grant carries the scope in its optional slot,
  so every connection kind a session holds ends with its scope; the status line lists the two
  ipd tests. Status stays "built · partly tested" (STEWARD3A's attack case and the lease deadline
  remain theirs).
- `docs/SECURITY.md` R41 row: where (+ `servers/ipd/src/server.rs`, "every connection kind"),
  tests (+ the two ipd tests), residual `none`.
- `docs/servers/ipd.md`: the `grant(scope)` bullet describes the `revocation: handle[0]? budget`
  slot, the stamp, the close, the kernel's refusal answered `not_permitted`, and that the steward
  sends a session's or lease's scope; "Scopes and grants" test list 11 → 13. The wire row is
  included from the table.
- `docs/servers/serving.md`: `mint_rooted` mints "in the revocation scope the grant brought, if any".

## Summaries checked (no change needed)

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`: no claim about ipd's grant or R41.
- `docs/plan/m3-agents.md` (R41 rows: "reach the network" via R25/R41; "use its sponsor's 9P
  connection" not yet): the rows name the rule, not the residual; unchanged.
- `docs/kernel/objects.md:305`, `docs/userland/agents.md:84`, `docs/TENETS.md:88`,
  `docs/plan/m6-persist.md:43`, `docs/kernel/model.md:403`, `docs/servers/init.md:620`: cite R41
  as a rule; none says ipd's grant is the exception.
- `docs/servers/wire.md` `ninep_common`: describes `new_connection`'s scope; the grant's slot is
  on ipd's page and in its table row. "Granting and releasing" describes the pattern, not the slot.
- `servers/ipd/src/lib.rs` module doc, `docs/GLOSSARY.md` "scope": the network scope; unchanged.
- No crate README for ipd or the steward mentions the grant's wire.

## Gates

Evidence: `/home/mcloonan/redoubt/.worktrees/STEWARD3S/.tmp/STEWARD3S/` (logs named below; the
per-case `target/jobs/<w>-<case>.log` copied to `jobs/`). Every build and run through `scripts/q`.

- `cargo check --workspace --tests` (host): ok (`check-host-1.log`). Fuzz workspaces
  `libs/rt/fuzz`, `servers/ipd/fuzz`: `cargo check` ok (`check-rt-fuzz.log`, `check-ipd-fuzz.log`).
- Focused host tests, `cargo test -p redoubt-rt -p redoubt-wire -p redoubt-wire-gen -p redoubt-ipd
  -p redoubt-steward-server -p redoubt-sshd -p redoubt-consrelay -p redoubt-client`: 75 test
  binaries, **416 passed, 0 failed** (`host-tests-1.log`; run before the last two amendments,
  which changed only the twin test's two added assertions, `tests/size-budget.toml` and a line
  break in the rt fuzz target). `cargo test -p redoubt-ipd --test program` after the twin's
  tightening: 7 passed (`ipd-program-3.log`).
- `make -f scripts/jobs.mk prebuilt`: rv64 323 / rv32 311, 0 failed (`prebuilt.log`,
  `prebuilt-4.log` on the final head). Every bin linking `redoubt-rt`, both widths, is built by it:
  the consumer sweep for `mint_rooted` and the grant's decoder (ipd) and encoders (the steward,
  tests/net/client, ipd's tests).
- Cases, `make -k -j4 -f scripts/jobs.mk set ... WIDTHS=rv64,rv32` (`cases-2.log`, on 63a147e48,
  which differs from the head only by the fuzz target's line break; `cases-3.log` on the head for
  the host-side gates):
  - PASS both widths: bench-net-peer, net-tcp, net-attacks, net-pinned, bench-net-peer-count,
    bench-net-peer-twice, bench-net-self-unrefused, bench-net-peer-pcap-empty,
    steward-session-ends (56 s), steward-restart (187 s).
  - PASS: ipd-host-tests, net-host-tests, docs, size-budget, unsafe-budget, formatting (`cases-3.log`).
  - First run (`cases-1.log`): size-budget FAIL, libs/rt 3962 of 3961 lines, servers/ipd 2925 of
    2923: ceilings raised to 3962 and 2925 with `Size budget:` lines in the commit; the steward
    did not grow (1576 of 1576). Second run (`cases-2.log`): formatting FAIL on
    `libs/rt/fuzz/fuzz_targets/ninep_server.rs` (my sed lengthened a line past 110): formatted
    with `cargo +nightly fmt` in that workspace. Failing logs kept: `rv64-size-budget.log`,
    `rv64-formatting-fail.log`.
- `cargo +nightly fmt --all -- --check` (workspace and both fuzz workspaces): clean (`fmt.log`).
- Not run: the whole bench (the train's), the fuzzers themselves, rv32 `cargo check` by hand
  (prebuilt builds rv32).

## Branch

`wp-STEWARD3S` at **90977a48d**, one commit on dcdb9bce1 (amended, no fix-ups), tree clean.
Nothing pushed.

## Risks and notes for the reviewer

- Naming: `revocation` rather than `scope` for the handle (see Wire above). If the Architect
  wants `scope`, the bytes field must be renamed (`rules`?), which touches every encoder and the
  page's vocabulary; I did not improvise that.
- Behaviour change for a grant that brings a handle and is refused or malformed: unchanged
  (closed as before). For one that succeeds: the brought handle is now closed by the outcome,
  where before a grant never brought one.
- `ipd`'s fuzz target `session` compiles against the new `mint_rooted` signature indirectly (it
  drives `answer_grant`); not run under the fuzzer here. rt's `ninep_server` target passes `None`.

## Review fold (steward-red-3, OK with notes)

- P2: `revocation` kept; ipd.md's grant bullet says once that it is the revocation budget
  `new_connection`'s `scope` slot names, called `revocation` here because this page's scope is
  prefixes and ports.
- P3: steward.md R41 and SECURITY.md's row say "every shared server's connection a session
  holds" (9P through `new_connection`, the network through ipd's grant), not "every connection
  kind".
- Docs only; amended; `docs` case PASS (`cases-4.log`, prebuilt-5.log). Holding for the rebase
  onto the new main after train 39 (libs/rt ceiling there 4012 → 4013 with the reason line).

## Rebased onto main c1eb9aa3c (train 39 published)

`git rebase --onto c1eb9aa3c dcdb9bce1 wp-STEWARD3S` → **eb5cf690a**, one commit, tree clean. One
conflict, `tests/size-budget.toml` libs/rt (main 4012 vs mine 3962): resolved to 4013 (+1, the
`Size budget: libs/rt` line in the commit); servers/ipd 2923 → 2925 as before; steward unchanged.

Gates on eb5cf690a (all logs under `.tmp/STEWARD3S/`, per-case logs in `jobs-rebased/`):
- prebuilt rv64 + rv32: rc=0 (`prebuilt-6.log`).
- Case set, both widths (`cases-5.log`): PASS bench-net-peer, net-tcp, net-attacks, net-pinned,
  bench-net-peer-count, bench-net-peer-twice, bench-net-self-unrefused, bench-net-peer-pcap-empty,
  steward-session-ends (56 s each), steward-restart (187 s / 186 s); PASS ipd-host-tests,
  net-host-tests, docs, size-budget, unsafe-budget, formatting. exit=0.
- Focused host tests (rt, wire, wire-gen, ipd, steward-server, sshd, consrelay, client):
  418 passed, 0 failed (`host-tests-2.log`).
- `cargo +nightly fmt --all -- --check`: clean (`fmt-2.log`).
