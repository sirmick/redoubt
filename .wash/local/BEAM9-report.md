# BEAM9 report: the steward's console session on rv32 printed nothing after its prompt

Branch wp-BEAM9, worktree /home/mcloonan/redoubt/.worktrees/BEAM9, on wp-STEWARD2's tip 28cfb0c0a
(main b60c7cc5c + 15 STEWARD2 commits). Two commits of mine, neither touching steward code:

- `rt: a multiplexed request that fits the words leaves the page it came in` (libs/rt/src/server/ninep_mux.rs,
  libs/rt/tests/mux.rs, docs/servers/serving.md, docs/userland/native.md, tests/size-budget.toml)
- `beamlet-redoubt: a console write answered busy goes again after the retry interval`
  (userland/otp/redoubt/src/lib.rs, src/fixture.rs, tests/console.rs, docs/userland/beamlet.md)

## Cause

Not a lost completion: an admission share. The session VM calls `consoled` with a non-zero account
(the steward's top budget carries the manifest's account, `servers/steward/src/lib.rs`; main's VM
under `init` is account 0), so at `consoled` its bucket is keyed (account, labels) and divided into
shares: with `pages: 2` a share may hold `floor(2 / 2) = 1` page (serving.md R26, "less than
`limit / (n + 1)`, at least one"). On rv32 the request words carry 12 bytes (`IN_WORDS = 3 * WORD`),
so the hub sends the 23-byte `Tread` in a page; on rv64 it fits the words. `consoled` parks that
read until typing, and the skeleton held the read's page for as long as the read lived: the share's
one page. Every later `Twrite` arrives in a page of its own, and `take_all` refuses a send whose
pages the share cannot pay for: `busy`. The hub returns `Outcome::Busy`; `ConsoleIo` re-sent the
write at once; `busy` again, for ever. The probe excluded `Busy` from its print, so it saw silence.
Reads kept working because the parked one already held its page; module reads go below `erofsd`
by blocking 9P and were never involved (the probe's `verityd` line showed them running).

This explains the four cases, including boot-profile's missing `beamlet: first console read` line:
it is queued behind the prompt's write, and the first read is sent (and parked, with its page)
before that write completes, so the boot-stats write is the first refused.

## Fix (libs/rt, Tier A)

`stored()` copies a request that fits the record's 24-byte inline store (`Stored::Words`) out of
the page it came in, on either width; a page is charged and held only while a request too long for
that lives in it (`take_all` now takes the batch's pages from any such request, not the first
message). A send whose requests all fit the words holds no page past its taking. A parked small
read then pins nothing, and rv32 behaves as rv64.

Rejected: raising `consoled`'s `pages` (two shares -> one page each again; `erofsd`/`bootfsd` are at 2
too); packing 23 bytes into rv32's 12-byte words.

## Second defect: the busy re-send (beamlet-redoubt)

`ConsoleIo` re-sent a `busy` write at once. It now sets a retry time `RETRY_US` (10 ms) later, as
the hub's rule for a queued request has it; every wait of the platform's meanwhile (`idle`, a full
output queue in `console_write`, the flush in `Drop`) is bounded by it, and `dispatch` sends the
write when it is due. The answers are counted and reported with the I/O line (`beamlet: io: the
console answered busy N times`, only when N > 0, so the beamlet-files case's match on the existing
line is unchanged).

`files.rs` still re-sends a file operation's `busy` request at once (`take -> answer -> go_on ->
send`); beamlet.md now says so. Not changed here: not the console, and no case reaches it.

## Tests

- libs/rt/tests/mux.rs `a_parked_read_sent_in_a_page_leaves_the_page_to_a_write` (new; host, fake
  kernel): a client of a non-zero account sends a waiting read in a page, then a write in a page of
  its own; the verdict is read from the server's admission table (the read holds `1 + 1`: session
  and request, no page) and from the answer (`Rwrite`, not `busy`). Before the fix: "the read
  waits, holding no page: not reached". After: passes.
- `a_sends_pages_count_once_and_go_back_with_its_last_request` amended: two small reads in one
  page hold no page past the take; the "page held until the last request" property now uses two
  writes too long for the words, one answered and one waiting (`Files::write_or_wait` on `wait`),
  and the "two pages exceed the share" step a long write in two pages.
- userland/otp/redoubt/tests/console.rs `a_write_answered_busy_goes_again_after_the_retry_interval`
  (new; host, fake kernel): the fixture's console answers the first two writes `busy` and stamps
  each write; the test asserts three writes, each at least `RETRY_US` after the one before. Without
  the ConsoleIo change: FAILED, "a retry went early: [590, 696, 764]" (µs). With it: passes.

## Gates

Every command through `make -f scripts/jobs.mk -C .worktrees/BEAM9 <target>` (q-leased cores) or
`scripts/q run --quiet -- cargo test ...`; exit codes as printed by jobs.mk.

- prebuilt: rv64 223 cases, rv32 209 cases, 0 failed (rc 0).
- rv32/userland-boot PASS 13.3 s (timed out at 450 s before); rv32/userland-read-only PASS 13.3 s;
  rv32/boot-profile PASS 6.7 s; rv32/boot-profile-unverified PASS 6.9 s (make rc 0).
- rv64/userland-boot PASS 13.0 s; rv64/userland-read-only PASS 15.3 s; rv64/boot-profile PASS
  6.1 s; rv64/boot-profile-unverified PASS 5.8 s; rv64/beamlet-console PASS 0.9 s;
  rv32/beamlet-console PASS 1.2 s (make rc 0).
- docs PASS; formatting PASS; unsafe-budget PASS; no-cruft PASS.
- size-budget: FAIL at first (libs/rt 3536 lines, ceiling 3528): the fix's 8 lines; ceiling raised
  to 3536 in the rt commit (Size budget: libs/rt +8 -> 3536); the rerun on the final head is below.
  STEWARD2's branch moves libs/client's and adds libs/sha256's ceilings, not libs/rt's, so the rt
  commit cherry-picks onto main without conflict there.
- host (q --quiet): redoubt-rt tests/mux.rs 11/11; beamlet-redoubt tests/console.rs 11/11
  (`cargo test --manifest-path userland/otp/redoubt/Cargo.toml --features fake --test console`).
- rv64/rt-host-tests PASS 10.6 s; rv64/client-host-tests PASS 16.2 s; rv64/r4-host-tests (consoled,
  keyd, bootfsd) PASS 2.2 s; rv64/rt-miri PASS 92.8 s (make rc 0).
- Final head (after the size-budget fold; the gates above ran on the same code with the old
  ceiling): prebuilt, rv64/size-budget, docs and the four rv32 cases: see "Final head" below.

Heads: rt commit 7b568468e, beamlet-redoubt commit fdf31891f (branch head), on 28cfb0c0a.

### Final head (fdf31891f)

- prebuilt: rv64 223 cases, rv32 209 cases, 0 failed (rc 0).
- docs PASS.
- rv32/userland-boot PASS 9.8 s; rv32/userland-read-only PASS 9.7 s; rv32/boot-profile PASS 4.9 s;
  rv32/boot-profile-unverified PASS 4.9 s (make rc 0).
- rv64/size-budget FAIL: `libs/wire: 3623 lines, ceiling is 3622`. Not this branch's: my two
  commits change nothing under libs/wire (`git diff --stat 28cfb0c0a HEAD -- libs/wire` is empty);
  wp-STEWARD2 adds 771 lines there against main and moves that ceiling itself. libs/rt is within
  its raised ceiling (the first run's only failure was libs/rt, reported before libs/wire). The
  rt commit alone on main: libs/rt +8 -> 3536 is the only size change.
- Not rerun on the final head (the fold changed only tests/size-budget.toml): the rv64 cases,
  beamlet-console both widths, the host-test cases, formatting, unsafe-budget, no-cruft, all green
  on the same code above.

## Main-based form (wp-BEAM9-main, .worktrees/BEAM9-main)

Head f118ef834 on main f11e8c2e9: 30229f27e (rt) and f118ef834 (beamlet-redoubt), cherry-picked.
The one conflict was tests/size-budget.toml's libs/rt ceiling (main 3522, STEWARD2's base 3528):
resolved to 3530, the message line in the checker's form `Size budget: libs/rt: <reason>`. The
red's note (no node ID in mux.rs's test comment) is folded into the rt commit on both branches;
the proof branch wp-BEAM9 is fb1c9c8d7 (rt ab6cf7b73).

range-diff against wp-BEAM9: commit 2 `=`; commit 1 differs only in that ceiling value and line.

Gates: prebuilt rv64 216 / rv32 202, 0 failed; build-rv64 rc 0; build-rv32 rc 0; rt-host-tests
PASS 12.4 s; client-host-tests PASS 18.0 s; rt-miri PASS 697 s; userland-boot PASS rv64 9.1 s /
rv32 9.2 s; beamlet-console PASS rv64 0.9 s / rv32 1.0 s; docs, formatting, size-budget,
unsafe-budget, no-cruft PASS. The boot cases and size-budget ran on f118ef834 after its prebuilt;
the rest on 879f96c39, which differs only by the test comment.

## Documentation check

- docs/servers/serving.md: `Pages` row, "Multiplexed connections" request and admission bullets,
  status `tested (14)` with the new test; Residual risks gains "A share of two pages is one page".
- docs/userland/native.md "Many requests at once": the clause that the server keeps a small
  request out of its page, so a waiting read does not pin the share's page.
- docs/userland/beamlet.md: console section status `tested (10)` with the new test; "asked again"
  now says when (console write after `RETRY_US`, file request at once).
- docs/servers/consoled.md "Admission" (80 requests and 2 pages per bucket): unchanged, still true.
- README.md, GETTING-STARTED.md, docs/plan M1 progress: searched for "multiplexed", "pages", "busy",
  "console"; no claim affected (no feature moves between planned/built/host-only).
- libs/rt/src/server/ninep_mux.rs module doc: amended with the fix.

## Residuals

- A non-account-0 badge gets one page per share at a server with `pages: 2` (`consoled`, `erofsd`,
  `bootfsd`), so two requests too long for the words outstanding at once see the second answered
  `busy` (serving.md Residual risks). Reachable by the steward's session today only if Elixir code
  opens `/dev/cons` as a file beside the console (`File.write("/dev/cons", ...)`: `files::Table` on
  the same connection), not by the shell's own output path, which keeps one write out at a time;
  `files.rs` would re-send that `busy` at once (see above).
- `files.rs` re-sends a `busy` request at once; no case reaches it. Candidate follow-up node.
