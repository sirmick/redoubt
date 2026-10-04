# AIO1 handoff 1 (aio1-implementer, 2026-10-03)

Worktree /home/mcloonan/redoubt/.worktrees/aio1, branch wp-aio1, tip **8fe85a28e**, 8 commits on
main f9135ce8a. Working tree clean. Every cargo/bench command via
`/home/mcloonan/redoubt/.wash/local/in-dev <cmd>` from the worktree. Brief:
/home/mcloonan/redoubt/.wash/local/AIO1-implementer.md (its "Rulings during the build" 1-9 are the
law; read 9 last, it overrides 1). Progress detail: .wash/local/AIO1-report.md (steps 0-4; step 4's
"Requests units cap 80" is superseded by ruling 9, built as below).

## The commits (oldest first)

1. 6cdc91f69 rt: the 9P skeleton's fuzz target takes minted's connection id — the target did not
   build on main (FileServer::minted gained `id`).
2. 24018fce0 rt: a 9P connection multiplexes requests through one completion call — the whole server
   side: `libs/rt/src/server/ninep_mux.rs` (child module of ninep.rs, `#[path]`), `answer_into`,
   `write_reply`, `tag_of` in ninep.rs, FileServer hooks `serving`/`served`, `NineServer::run`,
   `deliver`/`abandoned`/`expire`/`next_deadline`/`wake`/`requests_wait`/`sessions`, pure core for
   the fuzz (`open_session`, `take_request`, `collect_into`); `admit.rs` gains `Resource::Requests`
   and `Resource::Pages` (Limits.requests/pages, Cost.request/page, outside the open-call headroom,
   overrides keep defaults); every Limits/Cost literal in the tree swept (rt tests/bins, keyd, ipd
   fake/tests, tests/programs, tests/init-programs, userland/otp fixture, client tests common); the
   four 9P servers carry requests/pages 0 here. Tests: libs/rt/tests/mux.rs (10), admit unit test
   `requests_and_pages_are_outside_the_open_call_headroom`. Pages: serving.md section "Multiplexed
   connections", `admit` rows, R26/R28 sentences, R77, residuals; wire.md:61; SECURITY.md R77 row.
   Size budget lines: libs/rt 3417, ipd 2894, keyd 541, bootfsd 229, fsd 1345.
3. 57211af69 consoled: a multiplexed connection's reads wait for input (bin loop: deliver,
   abandoned first, expire/next_deadline, wake after input, requests_wait(FOREVER), max_transfer).
4. 519556ee1 ipd: requests wait on the network (NetFs::reserve/unreserve used by open_sockets and
   the hooks; on_send returns "multiplexed: receive before polling" for R21; DATA_WAIT_US for all).
5. 0eb28a898 fsd, bootfsd: served by the skeleton's own loop (NineServer::run; Q2 granted).
6. 12ce47ce7 client: the hub (`libs/client/src/aio.rs`): Hub value, &mut self, Send not Sync, no lock
   (ruling 7); submit/read/write/batch, SUBMIT_TIMEOUT_US 1 ms, queue retried at every entry, batching
   into one transfer; `wait(conn, hold)` with kernel timeout hold + COLLECT_MARGIN_US (1 s, ruling 8);
   `spawn_waiter`, `deliver` (badge-checked), `waiters()`; untrusted answers end the connection; every
   buffer back once (Done/Outcome). Tests libs/client/tests/aio.rs (6). native.md "Many requests at
   once" + module row; Refusal::NoTag. client 632 -> 957.
7. 1a9c9cee3 servers: each 9P server sizes its multiplexed requests and their pages (ruling 9 numbers,
   budgets, manifests, init system-fit test, bootfsd tests at 4 buckets, server pages).
8. 8fe85a28e tests: aio-many-reads (tests/fsd-programs/src/bin/aio-reader.rs, Cargo [[bin]],
   tests/data/aio/boot.json, tests/aio-many-reads.toml, tests/aio-many-reads-two.toml). NOT RUN.

## Rulings and their state

1 (Requests) — superseded by 9; built as 9. 2 (run loop) built, commit 5. 3 (wire, batching)
built: completion call `[0, COLLECT=1, hold_us, 0]` + lend, reply `[0, bytes, 0, 0]`, open reply
word 2 = OPENED, REFUSED 3, ENDED 4; request send word 0 = 0, one T-message in words 1-3 or one or
more end to end in a transfer with word 1 = length. 4 (Q4) then refined by the Architect's R77 text:
**session bound = min(server's longest wait, COLLECT_WAIT 10 s)**; a session with no completion call
parked for the bound since the last returned ends; a parked call is held min(hold, bound). 5 (Lock)
withdrawn by 7: no lock, unsafe unchanged. 6 Decided list accepted (stated in serving.md). 7 built.
8 built: client timeout hold + COLLECT_MARGIN_US; its three host tests: rt
`a_completion_call_is_held_at_most_its_hold_and_the_servers_bound`, client
`a_server_that_breaks_its_hold_loses_the_session_at_the_margin` (hold 0 / hold / bound covered in the
rt one). 9 built (below).

### Ruling 9 as built (Requests + Pages)
- Requests: 1 per request, cost REQUEST_STATE = 256 (const assert: size_of::<Pending> <= 256; ~216
  on rv64). Pages: 1 per transferred page per send, cost PAGE_SIZE; admitted all-or-none before the
  send's requests (`admit_pages`), refused => every request of the send busy and pages dropped; held
  via Arc until the last admitted request is delivered/flushed/ended (`release`, strong_count == 1),
  or released at the end of `take_all` if no request kept them. Session's completion call = InFlight.
- Numbers (4 buckets, manifests' count):
  - fsd: in_flight 2 (64 KiB each), files 32 (2 KiB), state 8 (512), requests 128, pages 32 =
    364 544 B/bucket, 1 458 176 of BUDGET 2 MiB (5 fit). Budget unchanged.
  - consoled: in_flight 2, MAX_THREADS fids/conns, requests 80, pages 2 = 355 584 B/bucket at 255
    threads, 1 422 336 of 2 MiB (183 552 at 31 threads). Unchanged. Holds 65 req + 1 page/share.
  - bootfsd: in_flight 2, files 32, state 8 (256 each), requests 64, pages 2 = 165 888 B/bucket,
    663 552 of a NEW BUDGET 768 KiB (was 256 KiB; the 2 completion calls alone are 128 KiB/bucket).
    Manifests: bootfsd 512 -> 640 pages (image/manifest.json and every tests/data manifest).
  - ipd: REQUESTS 128, PAGES 32 per default bucket (~712 704 B), fits every manifest's args within
    8 MiB - 1 MiB own (checked with a throwaway test, deleted). Unchanged.
  - netd, keyd: 0.
- init's system-fit test sums bootfsd at 640.

### Page texts (exact, as committed)
- R77: "A multiplexed connection's requests, and the pages they brought, are bounded by its admission
  shares of `Requests` and `Pages`, and a request whose answer is not yet delivered still counts. So a
  client that floods requests, or never collects their answers, holds no more of a server than one
  share, and its excess is answered `busy`, never queued without bound. A session ends, freeing every
  request it held, when its completion call is abandoned (the client died or gave up), or when no
  completion call has been parked for the session bound: the server's longest wait or `COLLECT_WAIT`
  (10 s), whichever is shorter. A flushed request is answered at most once, before its `Rflush`,
  never after." (the Architect's earlier text, with ruling 9's first sentence)
- R26: "A multiplexed request is admitted in the same bucket and share as a call, under its own
  resource, `Requests`, until its answer is delivered (R77)."
- R28: "A multiplexed request is never an open call: only its connection's completion call is, and
  that call is held as any parked call is; the requests count under `Requests` (R77)."
- wire.md:61: "A 9P call with a non-zero word or no lend is malformed (status 1), except a
  multiplexed connection's completion call, `[0, 1, hold, 0]` with a lend (serving)."
- serving.md admit: rows `Requests` / `Pages` + "both outside the open-call headroom ... each has
  its own share (R26)" (ruling 9's page line, split into the table).
- Test renamed: a_death_between_completion_calls_is_found_at_the_session_bound.

## Gates
At the tip 8fe85a28e a full run was started (host-tests, build, docs, formatting, no-cruft,
unsafe-budget, size-budget, then a 3-min fuzz) in a background shell that dies with this session:
**rerun it**. Before the last history rebuild (same code, ruling 9 applied): cargo test of rt,
client, consoled, bootfsd, fsd, ipd, init, keyd all ok; docs PASS; size-budget PASS at the tip. At
e93d87ef0 (ruling-8 state): host-tests 0 (16 PASS), build 0 (24 PASS rv64+rv32), formatting, no-cruft,
unsafe-budget 0. Fuzz: 8.39M runs/181 s clean (before Pages). Fuzz recipe (no cargo-fuzz in the
image): from libs/rt/fuzz, in ONE in-dev invocation, build with cargo-fuzz's sancov RUSTFLAGS,
`--target $(host triple) --target-dir ../../../target/aio1-fuzz`, then run the binary on a corpus dir
there with `-max_total_time=180` (the container's /tmp does not persist).

## What the brief still owes
1. **Machine case on the host** (orchestrator gives the host after HW1's bench and FSD4's boots): run
   `cargo testbench --arch rv64 aio-many-reads` and `aio-many-reads-two`, then rv32. Never run yet:
   expect to debug (init bound/budgets for the 2 aio-readers at 512/256 pages, account-0 shares,
   console lines vs expect regexes, the burst's 64 one-page Buffers). When they pass, add
   `bench:aio-many-reads` (and -two) to serving.md/native.md status lists.
2. **Answered (orchestrator, 2026-10-03):** two connections = the caller idling in receive plus two
   waiters; the case prints "1 thread, 2 waiters", as built. Nothing to change.
   Also asked of us: report each 9P server's `in_flight` arithmetic (one completion call per
   connection per badge); built: fsd 2, bootfsd 2, consoled 2 (unchanged), ipd 5 (unchanged), each
   at 64 KiB a call, in the per-bucket sums above. The Architect rules on the numbers at its check.
3. The whole bench, both widths: the orchestrator's (never run by me).
4. The Architect's check of the page lines; reviewers.
5. Final rebuild before merge: in the rt commit, serving.md links native.md#many-requests-at-once and
   names server loops (fsd/bootfsd/consoled/ipd) that only arrive in later commits; docs passes at the
   tip but not at that commit alone. Move those sentences into their commits.

## Traps
- Never `git stash`; I folded with a reset + cherry-pick dance (/tmp scripts are gone). A whole-file
  checkout of tests/size-budget.toml into an earlier commit carries later ceilings: set per-commit
  ceilings by hand, each with its `Size budget:` line.
- rt mux tests read the server's state as published before each receive: assert with `until`.
- Limits/Cost literals live in ~25 files; a field change needs the sweep and every consumer built.
- ipd's on_send must return true for multiplexed requests (receive before poll, R21).

## What consumed context
Five rounds of design rulings arriving out of order (Q1 cost three times: InFlight -> unit count 80
-> Requests+Pages), each re-plumbed through admit.rs, the mux, ~25 literal sites, four servers,
~40 manifests and per-commit size ceilings; repeated history rebuilds; bench output.
