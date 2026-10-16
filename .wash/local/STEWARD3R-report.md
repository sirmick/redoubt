# STEWARD3R report (steward3r-implementer)

HEAD: wp-STEWARD3R dcdb9bce1 (one commit on main 43588e0d6, rebased after review). GATES: all green on both widths ("Gates (rebased)" below); one commit, amended, no fix-ups.

## The ruling and its thread

QA thread `STEWARD3R-scope` (blocking, assignee architect, on behalf of this implementer):
question `.wash/local/STEWARD3R-scope-question.md`, ruling and amendment
`.wash/local/STEWARD3R-ruling.md`. Ruled: shape (a) — a scope carried at `new_connection`,
the serving library stamping the mint with it — with (b)'s disconnect kept as the cleanup
(destroy the scope, then disconnect each recorded connection); amended to (a1) after my
correction that the steward's badges at other servers were stamped with its own budget under
`system`, not `root`: `init` stamps the steward's entry's badges with `users`. No ABI or 9P
change, so no owner decision; `ninep_common` is Redoubt's own sideband table.

Settled on the thread after the build (my 9bbbb830, the Architect's 1586991d; recorded in the
ruling file under "Cases, settled with the implementer"):
- no new `steward-restart` clause: a probe between `release_fresh` and the reap would be the
  attacker's clock, and a disconnected connection answers NO_CONNECTION, not `Dead`; the property
  is the rt host test over the fake kernel plus `steward-restart`'s existing expectations.
- **For the orchestrator, so STEWARD3A hears it:** the Architect withdraws the clause it had asked
  STEWARD3A to add to `steward-lease-ends` (an agent dead with its deadline holds nothing to probe
  with).
- `ipd`'s typed `grant` carries no scope: a residual under R41, named on the page as a follow-up
  without its id; the Architect drafted STEWARD3S (a scope field on `ipd`'s grant) needing this
  package.
- the Machine's re-keying is fine; steward.md R41 says the steward holds no copy of a child's
  scoped connections, so none can outlive its scope in the steward's own table.

Also: `redoubt_client::file::Connection::new_connection` (the launcher wrapper over rt's) gains
the same `scope` argument — the steward's `consoled` connection is that type, and the RISC-V-only
bins (init, the steward) only showed it in the target build, not the host `cargo check`.

## What was built

- **Wire** (`libs/wire`): `ninep_common` op 2 is `new_connection(root, quota, scope: handle[0]?
  budget)`. The tables learn `handle[N]? KIND`, a slot the sender may leave empty, only after the
  required slots; `Layout` gains `optional` with `required()`/`check()`; the file framing admits a
  message whose only handles are optional; the Elixir codec's layouts are 4-tuples with the same
  range check; the generated Elixir client takes an optional slot as an argument defaulting to
  `nil` (`NinepCommon.new_connection(conn, root, quota, scope \\ nil, timeout)`), so the shell's
  three-argument calls stand. All generated codecs regenerated (`cargo run -p redoubt-wire-gen`).
- **Serving library** (`libs/rt`): `Minter::mint(badge, scope)`; `Minted::commit` takes the
  scope; `NineServer::new_connection`/`make_connection`/`mint_at` carry it to the kernel's `mint`
  from the call; `answer_common` reads the request's one handle as the scope and closes it with
  the minted handle's copy; `mint_rooted` passes none. A kernel refusal (`NotPermitted`) is a
  failed mint: `refused`, admission released. Client: `Connection::new_connection(lend, root,
  quota, scope: Option<&Budget>)`, one function; `Lend::call` already carried handles.
- **Fake kernel** (`libs/rt/fake`): a pending message and an open call carry the stamp of the
  handle they came through; `mint` from a call defaults to that stamp (falling back to the
  process's budget where a handle has none, as before) and refuses a budget not at or below it;
  `Fake::runs_in(pid, budget)` for tests.
- **Steward** (`servers/steward`): `Kernel::connect(domain, slot, scope)`; every `Connect` is
  minted in the object's scope (`fresh`, `fresh_carved`, the UART console at `consoled`; the
  steward's own badge as before); `forget` destroys budgets first, then releases connections;
  the `Machine` keys what it holds for children by a counter (`Held { handle, via, console }`),
  closes its copies once `launch` has copied them into the child (keeping `sshd`'s console for
  `ended` and a relay's control connection), and disconnects the console-session connection by id
  (`Via::Console`). `Steward::holding()` for the tests.
- **init** (`servers/init`): `check::handed_stamp(m, s)` — `Users` for the steward's entry,
  `Instance` for every other; `start` mints the handed badges with that stamp.
- **Consumer sweep**: every `Minter` impl (keyd, sshd, consrelay, sshd-host, test fakes in
  walfsd, littlefsd, erofsd, ipd, keyd) and every `new_connection` caller of rt's client and of
  `redoubt_client::file::Connection` (init, the steward, libs/client, tests, test programs) gains
  `None`; `Grants::connection` passes none. Every bin linking `redoubt-rt` type-checks on
  riscv64imac and riscv32imac (`cargo check --target ... --bins` over the 23 bin crates) and the
  host workspace with tests.

## Tests added

- `host:redoubt-rt::a_connection_minted_in_a_scope_ends_when_the_scope_is_destroyed`
  (`libs/rt/tests/echo.rs`): over the fake kernel, the echo server's connections stamped with a
  `users`-like budget; a scope elsewhere refused; connections in S and T; S destroyed: the child's
  handle gone (`BadHandle`), the launcher's copy too, T serves; the server kept no handle; the
  launcher's disconnect frees S's record once.
- `host:redoubt-rt::the_fake_kernel_s_mint_only_narrows` (the fake's own test).
- `host:redoubt-rt::a_connection_is_minted_in_the_scope_its_requester_sends`
  (`ninep_common_tests.rs`): the scope reaches the mint and is closed; a refused scope is
  `refused`, nothing minted, admission back; two handles are malformed.
- `host:redoubt-wire::an_optional_slot_may_be_empty`, `file_framing` extended;
  `host:redoubt-wire-gen::an_optional_handle_is_a_slot_that_may_be_empty`; `tables_parse` checks
  the new field.
- `host:redoubt-steward-server::a_session_s_end_destroys_its_budget_before_it_releases_its_connections`.
- `host:redoubt-init::the_steward_s_entry_alone_is_given_the_manifest_lines` gains the stamp
  assertions (`handed_stamp`).
- The fuzz target `redoubt-rt/ninep_server` sends the scope slot empty, with a scope, with one
  the kernel refuses, or with two handles, and checks every handle brought is closed
  (`cargo check` of the fuzz workspace; not run under the fuzzer here).

## Docs changed

- `docs/servers/steward.md` R41: built · partly tested (the attack case and a lease's deadline are
  STEWARD3A's), the mechanism paragraph, `Open` gone; residual "`ipd`'s grant carries no scope".
- `docs/SECURITY.md` R41 row: where, tests, `built, partly tested`, the residual.
- `docs/servers/serving.md` `ninep_common`: the scope's paragraph. `docs/servers/wire.md`: the
  op's third field, the status list (7). `docs/kernel/objects.md` `mint`: the R41 user.
  `docs/userland/agents.md` "When a lease ends". `docs/userland/native.md`: the signature.
  `docs/servers/init.md` "Restarts and reboots": the steward's `users` stamp and why it is safe.
  `libs/wire/tables/example.md`: the `handle[N]? KIND` syntax.

## Summaries checked

- `README.md`, `GETTING-STARTED.md`: no claim about `new_connection`'s signature or R41; no change.
- `docs/servers/README.md` 166/181/186: `new_connection` named as the launcher's way to a fresh
  connection, without its fields; still true; no change.
- `docs/userland/files.md` 232: "`new_connection(root, quota)`: it mints a connection rooted at a
  subdirectory, with its own byte quota" — the shell's client, whose generated function keeps
  those two arguments (the scope defaults to `nil`); the sentence stays true of what the shell
  passes. No change.
- `docs/servers/littlefsd.md`, `walfsd.md`, `erofsd.md`, `piped.md`, `bootfsd.md`, `ipd.md`:
  `new_connection` as the root-and-quota mint; no field list; no change.
- `docs/GLOSSARY.md` 232: the pair `new_connection`/`disconnect`; no change.
- `docs/servers/steward.md` "Authentication and sessions" / the sequence diagram line 538
  ("new_connection for the session's namespace"): still true; the scope is R41's paragraph.
- Milestone progress: R41 was planned under M3; the mechanism is built here, the lease's use is
  STEWARD3A's; the M3 node's text is the plan's, not a page.

## Gates

Every build and case through `scripts/q`; env as instructed. Commands and exit codes:
- `make -f scripts/jobs.mk prebuilt` → `prebuilt rv64: 317 cases, 0 failed`, `rv32: 306 cases, 0
  failed`, rc=0 (final tree; the kernel, loader, every server and test program on both widths).
- `cargo check --target riscv64imac-unknown-none-elf --bins -p <23 crates linking redoubt-rt>` and
  the same for riscv32imac; `cargo check --workspace --tests`: rc=0.
- `make -k -j4 -f scripts/jobs.mk set CASES="rt-host-tests wire-host-tests steward-host-tests
  init-host-tests client-host-tests ipd-host-tests elixir-oracles size-budget unsafe-budget
  formatting docs ninep-newconn-discard init-rollback init-restart steward-restart
  steward-session-ends steward-context-labels redoubt-revoke beamlet-launch"`: 27 of 28 rc=0 at
  3c92c7981; `rv64/steward-session-ends` rc=1 with `steward: heap needs 32 pages for twice its
  16-page peak, capped at 30` (deterministic: the same on a lone rerun). The steward's heap peak
  rose from 15 to 16 pages with what it holds for each child; the launch's handle resolution was
  moved to the stack (no change to the peak), and the image's `heap_pages` for the steward is 32,
  twice the measured peak as every server's cap is (`image: cap each server's heap at twice its
  measured peak`). Then at 5c86ea113: `rv64/size-budget` rc=0, `rv64/steward-session-ends` rc=0,
  `rv32/steward-session-ends` rc=0; `steward-restart` rv64+rv32 rc=0 at 731bbd09b (unchanged
  since but for the manifest's heap cap and a ceiling).
- Size budget: six ceilings raised to the counts, each with a `Size budget: <crate>: <reason>`
  line in the commit (libs/rt 3961, libs/wire 3915, libs/client 1124, servers/steward 1576,
  servers/init 2591, servers/consrelay 780); `rv64/size-budget` rc=0.
- `rv64/unsafe-budget` rc=0 (no `unsafe` added). `rv64/formatting` rc=0 (`cargo +nightly fmt --all`,
  the fuzz target by `rustfmt +nightly --config skip_children=true`); `git diff --check` clean.
- `make -f scripts/jobs.mk docs` → `PASS docs`, rc=0; `cargo run -q -p redoubt-doccheck` rc=0.
- `cargo check --manifest-path libs/rt/fuzz/Cargo.toml` rc=0 (the fuzzer itself not run).
- Not run: the whole bench (the train's). Failing logs kept under `.tmp/STEWARD3R/`.

Earlier failures found and fixed on the way (all three steward machine cases at first): the
steward's `/dev/cons` connection is minted by `init` through its own `consoled` connection, stamped
with `consoled`'s budget, so the session's `Connect` for slot 4 (the console) was `refused` by the
kernel's narrowing; `init` now mints the steward's console connection through its badge at
`consoled` stamped `users` for the one call (init.md says so beside the rule).

## Review round (steward-red-3: OK with notes) and the rebase

- P1, the modified host suites not yet run (under q, on the pre-rebase head d609c97d1):
  `cargo test -p redoubt-bootfsd -p redoubt-consoled -p redoubt-erofsd -p redoubt-keyd
  -p redoubt-littlefsd -p redoubt-piped -p redoubt-sshd -p redoubt-walfsd`: 243 passed, 0 failed,
  2 ignored (the ignored two are pre-existing). `cargo test -p beamlet-redoubt --features fake`
  in `userland/otp`: 81 passed, 0 failed.
- P2: the four test-data manifest copies (`boot-profile/manifest-unverified`,
  `steward/vault-launch`, `steward/idle`, `pipe`) carry the steward's `heap_pages` 32 with the
  image; folded into the one commit.
- P3: the relay control-connection stale-index note is STEWARD3A/4's (the orchestrator's).
- Rebased onto main 43588e0d6 (AGT1-4, B74): clean, no textual conflicts; main's hunks in the
  fake kernel (the held clock's tick) and the steward were outside the regions this package
  rewrote; every manifest copy agrees with the image at 32. On the rebased head: host workspace
  with tests, the rt/steward/init/wire/client suites, nightly fmt and doccheck all clean. The
  bench gates on the rebased head are listed under "Gates (rebased)".

## Gates (rebased), head dcdb9bce1

`make -f scripts/jobs.mk prebuilt` rc=0 (both widths); `make -k -j4 -f scripts/jobs.mk set CASES=
"size-budget formatting docs unsafe-budget rt-host-tests wire-host-tests steward-host-tests
init-host-tests client-host-tests init-boot init-restart init-rollback ninep-newconn-discard
redoubt-revoke beamlet-launch steward-restart steward-session-ends steward-context-labels
steward-ssh-two-principals agent-login"`: 32 of 32 rc=0 (rv64 all; rv32 for every case with a
target). Host: `cargo check --workspace --tests`, the rt/steward/init/wire/wire-gen/client
suites, `cargo +nightly fmt --all -- --check`, `cargo run -p redoubt-doccheck`: all rc=0.

## Open risks and notes

- `ipd`'s grant carries no scope (residual, follow-up).
- A relay's control connection is made by the relay in the context's budget and kept by the
  steward for `detach`: it dies with the context's budget, before the steward's `detach`/`release`
  of it — a pre-existing stale-index hazard the re-keying does not worsen (the handle is only
  closed, never sent on, after a failed lookup) but does not remove; noted for STEWARD3A/4.
- The fake's `mint` default falls back to the process's budget where the handle has no stamp, a
  fudge kept so existing tests (servers played in a child's budget) keep their stamps.
