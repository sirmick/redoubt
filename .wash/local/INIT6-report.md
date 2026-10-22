# INIT6 report

Branch wp-INIT6, base 4c2c063e8, head 8c891d1d2 (two commits):

- c78e3c2fd `rt: a Tversion by one call ends the connection's multiplexed session`
- 8c891d1d2 `tests: a restarted walfsd client opens its own session, as walfsd counts it`

## What changed

- `libs/rt/src/server/ninep.rs`, `answer`'s `Tversion` arm: after it drops the key's fids it ends
  the key's mux session (`mux.end_where(.., |k| *k == key)`). That is `end`'s path: the parked call
  is answered `ENDED` (status 4), and the pending requests go with their `Requests`/`Pages`, and
  `InFlight` too. The module doc's "Protocol corners" now says so.
- `ninep_mux.rs`: `end_badge` is generalized to `end_where(admission, which)`, used by
  `connection_gone` (badge) and the `Tversion` arm (key). It is never reached mid-fill: a mux
  `Tversion` ends its session in `take` and is never pending. "The end" in the module doc lists
  the `Tversion` on either path.
- Nothing in `init`; nothing in the hub.

## Tests

- Host, `libs/rt/tests/mux_held.rs` (held clock, so the session bound cannot end the session
  first): `a_tversion_by_one_call_ends_the_keys_session`. It runs two phases. (1) A parked call
  plus three waiting reads (held = 4): a successor on the same badge versions; the parked call is
  answered ENDED and held drops to 0 with no session left. (2) A session with a read answered at
  once and one that waits, both untried and no call parked (held = 3): the next successor
  versions, held drops to 0, its first hold-0 call is `[0,0,OPENED,0]` with no bytes, and a read
  on the same tag gets its own answer. A mux `Tversion` still ends a session. It fails on the code
  without the fix ("A's parked call is answered: not reached") and passes with it. It is placed in
  mux_held.rs rather than mux.rs because of the held clock. A live client's `Tversion` with no
  session: unchanged and covered by the existing ninep_tests.rs unit tests.
- Boot, new `tests/init-restart-mux.toml`, both widths. The case uses walfsd with the test-only
  feature `session-count`, `tests/init-programs` bin `mux-restartee`, and manifest
  `tests/data/init/restart-mux.json`. The verdict is walfsd's lines, in walfsd's `[con]`:
  `1 session open`, then init's exit and restart lines, then `0 sessions open` (the successor's
  Tversion), then `1 session open` (its OPENED). The program's line follows:
  `read 5 bytes, wrote 5`, TEST PASSED. The case forbids `2+ sessions open`, TEST FAILED and
  reboots.
  - With the fix: PASS on rv64 and rv32. Logs are in .tmp/INIT6/logs/init-restart-mux-*.log.
  - Without it (a scratch worktree with the one `end_where` line removed): FAIL on both widths.
    The successor's first completion call answered `[0, 16, 0, 0]` with the predecessor's pending
    Rread "hello". walfsd said only `1 session open`. Logs are in
    .tmp/INIT6/logs/neg-init-restart-mux-*.log.
- The case has no `reporter`: the bench takes a reporter's verdict only under the first `started`
  console, and this entry is restarted. walfsd's lines match by text, and no other program prints
  them.

## The test-only feature (orchestrator's conditions)

- `session-count` is off by default. The image's recipe cannot name features
  (tools/testbench/src/case.rs `RecipeEntry`, `deny_unknown_fields`, `features: Vec::new()`), so
  the image never builds it.
- I first gated it with a `compile_error!` without debug assertions, as the kernel does. That
  failed the case: the bench builds programs release always; `debug_assertions` checks only the
  kernel and loader. So the gate went, and the feature follows the other servers' test-only
  features (cut-after-write, restart-probe).
- A plain walfsd release build is byte-identical with and without the feature code, on the runtime
  commit c78e3c2fd: sha256 7648b62460777c80… rv64, 8aca180d68c411c9… rv32. (Against main the
  binaries differ by the runtime fix itself.)
- size-budget counts source lines, so it cannot show the build is unchanged. The servers/walfsd
  ceiling goes 901 → 930: the feature's 29 code lines, with a `Size budget:` line in the commit and
  a comment in tests/size-budget.toml (as netd's restart-probe). libs/rt stays at its ceiling of
  4130: the `end_badge` → `end_where` merge keeps the fix to net zero code lines.
- The line prints only when the count changes (`turn`). walfsd.md names it as this case's
  witness only.

## Gates (exit codes)

Run through `make -f scripts/jobs.mk set CASES=...`.

- Runtime-consumer sweep at the first runtime commit (c037f1446, before the `end_where` merge):
  rc 0, 29 cases, 43 jobs. It covered rt-host-tests, rt-build, walfsd, littlefsd, erofsd,
  consrelay, ipd, init, blkd, netd, sshd, steward, piped, fileserver, client, net and memory host
  tests, and bootfsd, consoled, keyd, init, sshd, piped, ipd, netd, blkd, littlefsd, consrelay and
  client builds. After the merge: rt-host-tests rc 0, and the mux tests (14 + 3) pass.
- docs, formatting, no-cruft, size-budget, unsafe-budget, walfsd-host-tests, init-host-tests,
  host-tests: rc 0. The last run of docs, formatting, no-cruft and size-budget was at head
  8c891d1d2.
- The load run (orchestrator's go) at 7369f2d26 passed both widths for: init-restart,
  init-handed-revoked, steward-restart (185 s), steward-server-restart, aio-many-reads,
  aio-many-reads-two, beamlet-files, userland-boot, init-boot, bench-net-peer, ipc-outcomes,
  sum-clear, and lend-untouched-page (smp 1 and 4). That is 26 of 28 jobs. The other two were
  init-restart-mux, which failed on the compile_error above. After the amend to 8c891d1d2, it
  passes on both widths. The amend touched only walfsd's feature gate (cfg'd out in a plain
  build), comments, docs, the case and size-budget, so the 26 results stand for the head.
- Not run: the whole bench (the train's).

## Deleted

- serving.md: B95's residual paragraph (a restarted entry meets its predecessor's session).
- init.md: the Restart bullet's false clause ("disconnects the dead instance's connections at every
  server"). It now says: init disconnects the console connection and the steward's fresh
  connections; a re-minted root badge's fids and mux session go at the successor's `Tversion`.

## Pages

- serving.md: "Multiplexed connections" (status: +host test, +bench case; "The end"), the 9P
  skeleton's "Protocol corners", R77 (status +host test; text), and "A client dies".
- init.md: "Restarts and reboots" (status +bench case; Restart bullet).
- walfsd.md: "Failure and restart" (a client restarts; the test-only line).
- SECURITY.md: R77's row, with the host test added.

Summaries checked: README.md, GETTING-STARTED.md, docs/README.md, docs/servers/README.md
("Cleaning up after a child", "Restarts and crash blame": still true, no change),
docs/servers/wire.md ("A launcher releases its child's grants": about grants and fresh connections,
still true), and docs/userland/native.md (the hub's passages: no claim about restarts or Tversion).
None needed a change.

## Round 1 fold

Branch wp-INIT6, base 4c2c063e8 (unchanged), new head 09826b05c (two commits, amended; no fix-ups):

- b2744a615 `rt: a Tversion by one call ends the connection's multiplexed session`
- 09826b05c `walfsd: a test-only session-count feature witnesses that a restarted client opens its own session`

### Editor P2: the Tversion rule stated once

The rule now lives in serving.md "Multiplexed connections", **The end**: the list says "at a
`Tversion` by one call", and one paragraph says what it does (clunks every fid, ends the session;
one sent multiplexed is a protocol error, above) and why a restarted entry meets nothing of its
predecessor's. Everything else links to it (`#multiplexed-connections`, "the end"):
- serving.md Protocol corners: "`Tversion` is accepted at any time and starts the connection
  afresh ([the end] says what goes)".
- serving.md R77 rule text: "at a `Tversion` by one call ([the end])".
- serving.md "A client dies": the re-minted badge, "whose `Tversion` clears it ([the end])".
- init.md Restart bullet: "what the dead instance held at that server under it goes at its
  successor's `Tversion` ([the end])"; the fids/session/admission list and "which a client sends
  before it attaches" went.
- walfsd.md "A client restarts": "its `Tversion` ends what the dead one held here ([the end])".
- serving.md:354 (Tags) kept as on main: it states that a multiplexed `Tversion` is a protocol
  error, the Tags rule, not the end's; The end refers to it ("above").
- The rt module docs (ninep.rs "Protocol corners", ninep_mux.rs "The end") are code comments,
  not pages; unchanged.

### Editor P3

- walfsd.md's line: `walfsd: N session(s) open`, as the case matches it (1 session / 0 sessions).
- Commit 2's subject names walfsd; the body names the new host test and "never 2 or more".

### Red P3-1

`tools/testbench/src/case.rs` `only_init_restart_mux_builds_the_session_count`, beside
`only_steward_latency_builds_the_latency_probe`: the tests/*.toml naming `"session-count"`
(quoted, as a recipe does; size-budget.toml's comment names it unquoted) are exactly
`init-restart-mux.toml`, and no manifest under image/ or tests/data names it. walfsd.md names the
test. Run by name: pass (`cargo test -p testbench --bins only_`: 14 passed). Negative: a copy of
the case as tests/zz-neg-scratch.toml fails it (left ["zz-neg-scratch.toml",
"init-restart-mux.toml"]); copy removed.

### Red P3-2: both builds, full sha256

Plain release builds (`cargo build --profile release --target <t> -p redoubt-walfsd --bin walfsd`,
as the bench's build.rs runs it, no features), at the head 09826b05c (feature code present) and at
b2744a615 (no feature code; a detached scratch worktree, removed):

- rv64 head  `.tmp/INIT6/tm/riscv64imac-unknown-none-elf/release/walfsd`
  7648b62460777c80d70d2060c62d45d9faee420c86997a65f4375b6a2eaa4428
- rv64 base  `.tmp/INIT6/tb/riscv64imac-unknown-none-elf/release/walfsd`
  7648b62460777c80d70d2060c62d45d9faee420c86997a65f4375b6a2eaa4428
- rv32 head  `.tmp/INIT6/tm/riscv32imac-unknown-none-elf/release/walfsd`
  8aca180d68c411c9087d1772360572bdcf6b55a2da136a4c34fb48b7a65e6602
- rv32 base  `.tmp/INIT6/tb/riscv32imac-unknown-none-elf/release/walfsd`
  8aca180d68c411c9087d1772360572bdcf6b55a2da136a4c34fb48b7a65e6602

(`.tmp` = /home/mcloonan/redoubt/.tmp.) Byte-identical per width.

### Red P3-3

- mux_held.rs: "as it always did" dropped.
- The case's forbid is now `walfsd: ([2-9]|[1-9][0-9]+) sessions open` (any count >= 2; the old
  one missed 10-19, 100-199, ...).

### Gates at 09826b05c (pinned, taskset -c 8-23 nice -n 10)

`make -f scripts/jobs.mk set CASES="docs formatting no-cruft size-budget host-tests rt-host-tests
walfsd-host-tests"`: rc 0, all seven PASS (host-tests holds the testbench's tests). Log:
.tmp/INIT6/gates-r1b.log.

Not run: init-restart-mux on either width (waiting for the orchestrator's go; the fold changed
its forbid pattern only), the whole bench.

### Summaries checked

README.md, GETTING-STARTED.md, docs/README.md, docs/servers/README.md, docs/servers/wire.md,
docs/userland/native.md, docs/SECURITY.md (R77's row: unchanged by the fold; its tests are the
same): no claim about Tversion or restarted sessions moved; no change needed.
