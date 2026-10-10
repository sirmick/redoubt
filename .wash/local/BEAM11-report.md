# BEAM11 report: a file request answered busy goes again after the retry interval

Branch wp-BEAM11, worktree /home/mcloonan/redoubt/.worktrees/BEAM11, head 45f1880bc on main
3ea64210e (rebased after BEAM10's merge; range-diff against the reviewed 49639067d: context only).
One commit: `beamlet-redoubt: a file request answered busy goes again after the retry interval`
(userland/otp/redoubt/src/files.rs, src/lib.rs, src/fixture.rs, tests/files.rs,
docs/userland/beamlet.md, docs/userland/files.md). Tier B (userland/otp only).

## Change

- files.rs: an `Op` answered `busy` gets `retry_at = now + RETRY_US` and keeps its ask unsent;
  `go_on` leaves such an op in the table until the retry is due; `Table::pump` (called from the
  platform's `dispatch`) sends the due ones, and ends an abandoned one without asking again;
  `Table::retry_in` bounds the platform's idle wait like the console's; `busy()` counts an op
  waiting to retry, so the VM does not give up with one pending; the answers are counted
  (`busy_answers`).
- lib.rs: `dispatch` pumps the files; `idle`'s wait is bounded by the earliest of the console's and
  the files' retries; the I/O report says `beamlet: io: the files answered busy N times` when N > 0
  (a line of its own, so BEAM9's console line and the beamlet-files case's match are unchanged).
- fixture.rs: `SinkServer`/`sink(busy)`: one writable file `out` under a root, `consoled`'s
  one-page share (`pages: 2`), the first `busy` writes answered `busy`, every write stamped, the
  bytes taken kept. A file operation on `/dev/cons` is refused `Emfile` by the table, so the
  console could not serve as the busy file server; this is the smallest server that can.
- tests/files.rs `a_write_answered_busy_goes_again_after_the_retry_interval`: `put` to `/sink/out`
  on a sink answering two writes `busy`; the verdict is the sink's write stamps (three, each at
  least `RETRY_US` after the one before) and its bytes. Without the change: FAILED, "a retry went
  early: [2146, 2247, 2359]" (µs). With it: passes. `with_home_and` binds the sink at `/sink`
  through `session_built`'s extra handles.

## The red's P2 fold: a close's clunk answered busy

`close_handle`'s clunk (`Owner::Close`) answered `busy` freed the fid as if served; the server
still held it, so the next walk reusing that number was refused (an unrelated operation's Eio)
and the server-side fid was never clunked. Now `Owner::Close` carries the hub connection; a busy
clunk becomes a `PendingClose` with `retry_at`, covered by `pump` (sent again), `retry_in` and
`busy()`; the fid is freed only when the clunk is answered (any outcome but busy; `Ended`/`Flushed`
leave it in use as before). The sink fixture gets a one-request share (`requests: 2`), a
`hold_reads` knob (a read of `out` waits while set, holding that one request) and clunk stamps.
Test `a_close_answered_busy_is_retried_and_its_fid_is_kept_until_the_clunk_is_served`: two
handles on `/sink/out`; a held read on one, the other closed (its clunk `busy`); the read then
completes and a third open walks a fresh fid; the sink serves exactly one clunk. Without the
retry: the reopen is `Err(Eio)` (the server refused the walk of the fid it still held). With it:
passes (files 18/18).

## Documentation check

- docs/userland/beamlet.md: the sentence "a console write RETRY_US later, a file operation's
  request at once" now says both wait the retry interval, counted and reported.
- docs/userland/files.md: status `tested (22)` lists both new tests; its prose has no claim about
  busy answers.
- docs/servers/serving.md Residual risks ("A share of two pages is one page"): still true; its
  last clause said the second request "is its caller's to send again"; beamlet now does so after
  the retry interval. Unchanged (the rule is the server's; the client's behaviour is beamlet.md's).
- README.md, GETTING-STARTED.md, plan M1: nothing names the files' busy handling.

## Gates (head 49639067d before the rebase; jobs.mk / q)

- prebuilt rv64 216 cases / rv32 202, 0 failed; docs PASS; formatting PASS; size-budget PASS;
  unsafe-budget PASS; no-cruft PASS.
- beamlet-redoubt host tests (`cargo test --manifest-path userland/otp/redoubt/Cargo.toml
  --features fake`, q, 4 cores): console 11, files 17, limits 7, lookup 2, pack 2, userland 2 all
  green; system 9 of 10: `an_endpoint_served_stays_open_when_its_term_is_dropped` FAILED at line
  318 (the caller got 0). That is BEAM10's flake (an idle sleeping on an event already taken),
  which this tree does not have the fix for (main 226245507), under the load of my concurrent
  BEAM10/K27 series on the host. Not this change's; the rebase onto main after BEAM10 merges
  removes it.

## Gates on the final head 45f1880bc (main 3ea64210e)

- docs PASS; console 12/12 (BEAM10's EOF test among them), files 18/18, system 11/11 — on
  general cores (`q run --cores 4`): the quiet set was held for the whole period by another
  tenant's job. The earlier EOF-test failure on e68263c87 was one run under K27's prebuilt and
  flood case; it passed in every run since.
- formatting, size-budget, unsafe-budget, no-cruft after a fresh prebuilt: below.
