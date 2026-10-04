# AIO1 report (aio1-implementer)

Branch wp-aio1 from main f9135ce8a, worktree /home/mcloonan/redoubt/.worktrees/aio1.

## Step 0: reading done; design questions (blocking parts named)

### Q1. Which resource a request takes (blocks: server limits, the machine case)

The brief: each request takes one `InFlight` of the connection's bucket and share. Today:
- `fsd` and `bootfsd` have `in_flight: 0`; `consoled` has 2 per bucket (a lone badge's share: 1);
  `ipd` 5 (fake) / `DEFAULT_IN_FLIGHT`.
- Every `InFlight` cap sits under the open-call headroom (`buckets * in_flight <= 192`,
  `Admission::new`), because `InFlight` counts open calls. A multiplexed request is a `send`, not an
  open call; the one open call per connection is its completion call.

So under today's caps `fsd`/`bootfsd` answer every multiplexed request `busy`, and no server's
share holds 64 outstanding reads (a non-zero account's lone badge holds < cap/2; 64 needs a cap of
130+, i.e. at most one bucket).

- (a) Keep `InFlight`: raise each server's `in_flight` cap and its `Cost` (fsd, bootfsd, consoled,
  ipd), still under the headroom; aio-many-reads then needs a server run with 1 bucket, or an
  account-0 test program on a server with in_flight >= 65 and <= 2 buckets.
- (b) **Recommended.** A new `Resource::Requests` (multiplexed requests, held until delivered),
  with its own per-server cap and cost (a request may hold a transfer of up to 16 pages), outside
  the open-call headroom; the completion call alone is an `InFlight` (an open call). Each 9P
  server's limits gain `requests` (fsd, bootfsd not in my owned paths: I'd report them).

### Q2. The receive loop (blocks: fsd and bootfsd getting multiplexing "without their own code")

`fsd` and `bootfsd` serve through `server::serve`, which drops sends and ignores abandoned notices;
and every server receives with `max_transfer` 0, so the kernel refuses a request's transfer (R4).
Proposal: a skeleton loop `NineServer::run(endpoint, own)` (receive with `max_transfer` =
`MAX_LEND_PAGES`, sends to the multiplexer, abandoned notices, deadlines); `fsd.rs` and
`bootfsd.rs` switch to it (about 3 lines each). consoled and ipd call the multiplexer from their
own loops. May I touch the two bins?

### Q3. The wire (blocks: nothing yet; one sentence on a page not in the brief)

wire.md "9P in a lend": "A 9P call with a non-zero word or no lend is malformed". Proposal:
- request: a `send`, word 0 = 0, the T-message packed in words 1-3 (24 bytes on rv64, 12 on rv32:
  Tclunk, Tflush, Topen, Tremove, Tstat fit both; Tread only rv64) or at the start of a transfer;
- completion call: a 9P `call` with words `[0, COLLECT=1, 0, 0]` and a lend; reply `[0, bytes, 0,
  0]`, R-messages end to end at the start of the lend; refusals in word 0 as typed errors.
That needs wire.md's sentence amended ("... except the completion call, serving.md"). Or a
`ninep_common` opcode instead (a wire-table change). Recommend the word-1 marker + wire.md line.

### Q4. Servers whose waits have no deadline (consoled: FOREVER)

R7x: "A death between two completion calls is found at the requests' deadlines." consoled's
parked reads have none, so a client that dies between completion calls (or never makes one) holds
its share for good (bounded by the share, never freed). Options: (a) a residual on consoled.md;
(b) **recommended**: a connection with no completion call parked ends after a fixed
`COLLECT_WAIT` (say 10 s) whatever the server's longest wait; the rule's bullet then reads
"... found at the requests' deadlines, or `COLLECT_WAIT` after its last completion call".

### Q5. The hub's in-process queue (blocks: libs/client's hub)

The queue and the per-submitter slots hold owned `Buffer`s shared between threads. `libs/client`
is `forbid(unsafe_code)` and no_std; `redoubt_rt` has no lock (beamlet's VM has none on the
target either). Safe Rust cannot share a mutable owned value between threads without one.
- (a) **Recommended.** A small `Lock<T>` in `redoubt_rt` (an atomic flag, `sleep(0)` to yield; one
  `unsafe impl Sync` and one view of the `UnsafeCell`, documented): an unsafe-budget raise of 2 for
  `libs/rt/src`, with its `Unsafe budget:` line.
- (b) No lock: the kernel is the queue. Submit = a `send` to the hub's endpoint carrying the
  request's words and its buffer as an in-process transfer; completion = the hub's `send` to the
  submitter's own endpoint with a short timeout, retried at the next wake. Every buffer move is an
  unmap/remap, and the process's submitters share one `WAIT_CAP` (32) at the hub.

### Decided (inside the brief; for the record)

- A connection's multiplexed session opens with its first completion call, answered at once and
  empty (or `busy` if not admitted), so the hub sends no request before the server holds the
  session. A request on a connection with no session is dropped and what it brought freed.
- Requests are served when a completion call is parked, straight into its lend: no completion is
  copied or held in the server; a request not yet delivered is the "undelivered completion" and
  holds its admission. A read is served into the remaining room only if its whole count fits
  (no short read caused by packing); else it waits for the next call.
- `Tflush` acts at receipt: oldtag's pending request (or pending `busy`) is dropped, and the
  `Rflush` is queued after anything already delivered for it.
- Tags 0..255; `Tversion` (NOTAG) by multiplexing is out of range: it ends the session.
- Crash blame: requests are served with the completion call as the thread's current call.

## Step 1: the serving library's multiplexed connections (server side) — done, pending Q1/Q2/Q3/Q4

Branch tip 83367b908 (4 commits on f9135ce8a):
- 6cdc91f69 rt: the 9P skeleton's fuzz target takes minted's connection id (it did not build on
  main: `FileServer::minted` gained `id`, the target was never updated).
- 7cbe042a0 rt: a 9P connection multiplexes requests through one completion call
  (`libs/rt/src/server/ninep_mux.rs` new, child module of `ninep.rs`; `answer_into`; two
  `FileServer` hooks `serving`/`served`, default no-op; `NineServer::run`; 7 host tests in
  `libs/rt/tests/mux.rs`; fuzz target gains multiplexed inputs; size budget libs/rt 2960 -> 3346).
- 006f220f1 consoled: a multiplexed connection's reads wait for input (consoled 339 -> 351).
- 83367b908 ipd: a multiplexed connection's requests wait on the network (ipd 2887 -> 2919).

What is built (as in "Decided" above), with Q1-Q4 provisional:
- requests take `InFlight` (Q1 (a) shape for now; one `const REQUEST: Resource` to change for (b));
- `NineServer::run` exists; fsd/bootfsd NOT switched (Q2);
- wire: completion call words `[0, 1, 0, 0]`; open reply `[0, 0, OPENED=1, 0]`; refused 3; ended 4;
  wire.md not touched (Q3);
- Q4 as (b): a parked completion call is answered empty, and a session with none ends, after
  `min(requests_wait, COLLECT_WAIT=10 s)`; consoled's requests have no deadline (FOREVER), its
  sessions still end at 10 s without a completion call.
- ipd: every multiplexed request waits at most DATA_WAIT_US (30 s), ctl reads included (its one-call
  ctl reads wait 60 s). ipd's socket reservation moved into `NetFs::reserve`/`unreserve`, used by its
  own `open_sockets`/`close_sockets` and by the hooks. A served multiplexed request is followed by a
  receive before the stack is polled (R21), as a call is.

The seven attack cases (libs/rt/tests/mux.rs). Verdicts come from the server: its admission table
and session count, published by the server thread before each receive, and the answers it wrote.
- a_never_polling_client_holds_only_its_share: A floods 10 waiting reads, never polls; the server's
  peak InFlight for A is its share (4: slot + 3); B is served by call and by its own session
  meanwhile; A's next collection is Rerror "busy" for tags 3..9.
- death_with_requests_parked_frees_the_connection: A's completion call times out (abandoned):
  server sessions 0, A's InFlight 0, open calls 0; a new session works.
- a_death_between_completion_calls_is_found_at_the_deadline: A stops calling; the session ends at
  the server's wait (300 ms in the test), admission 0.
- a_flush_racing_a_completion_answers_once: flushed before collection -> Rflush alone; answered
  first -> answer, then Rflush alone; a waiting read flushed while the call is parked -> Rflush
  only, and its tag is reusable.
- a_flood_of_sends_at_wait_cap_never_blocks_the_server: 4 threads send 256 requests (NineServer::run),
  all taken; B's 64 one-call reads all answered meanwhile; A then gets 253 busy.
- a_reused_or_out_of_range_tag_ends_the_connection: reused tag ends it (sessions 0, admission 0); tag
  256 ends it and the parked call gets ENDED; a request with no session is dropped, holds nothing.
- a_second_completion_call_is_refused: MALFORMED at once; the first still gets the next answer
  (sent in a transfer).

Commands (all from the worktree via .wash/local/in-dev), exit codes:
- cargo testbench host-tests: 0 (16 PASS incl. rt, client, fsd, ipd, net, r4, model)
- cargo testbench build: 0 (12 build cases x rv64+rv32 PASS)
- cargo testbench size-budget / formatting / unsafe-budget: 0 / 0 / 0 (unsafe unchanged)
- cargo test -p redoubt-rt --test mux, 5 runs: 7 passed each
- fuzz ninep_server (no cargo-fuzz in the image: built with cargo-fuzz's sancov flags, run
  directly): 11,053,332 runs in 241 s, no failure.
Not yet: pages (serving.md section, R7x, R26/R28 sentences, SECURITY.md row, files.md), the client
hub (Q5), fsd/bootfsd (Q2), caps (Q1), the machine case, doccheck/no-cruft.

## Step 2: owner's amendment applied; the client hub; fsd/bootfsd; the pages — done, pending Q1 and the Architect's text

Branch tip 2f85cf9c0 (6 commits on f9135ce8a):
- 6cdc91f69 rt: the 9P skeleton's fuzz target takes minted's connection id
- b510f8d50 rt: a 9P connection multiplexes requests through one completion call (+ serving.md
  section "Multiplexed connections", R77, R26/R28 sentences, SECURITY.md row; rt 2960 -> 3347)
- 83222e086 consoled: ... (339 -> 351)
- e1ee4a44e ipd: ... (2887 -> 2919)
- e826356c8 fsd, bootfsd: served by the 9P skeleton's own loop (Q2, granted)
- 2f85cf9c0 client: a hub keeps many 9P requests outstanding on few threads (+ native.md "Many
  requests at once", module table row; client 632 -> 932)

Hub (libs/client/src/aio.rs), per the owner's amendment: `Hub` is a value, `&mut self` methods,
no lock in the library (the caller's own lock if shared; point A to the Architect). `submit`/`read`/
`write` send inline with SUBMIT_TIMEOUT_US = 1 ms; not taken -> queued in order, retried at every
entry (submit, wait, deliver, poll). One connection: `Hub::wait(conn, hold_us)` makes the completion
call itself with the hold in word 2 and no client timeout (point B). More: `spawn_waiter(conn,
receive, badge)`: the waiter hands its filled buffer over as the transfer of its wake-up send;
`Hub::deliver` takes only its badge's. Untrusted answers end the connection; every outstanding
buffer comes back once (`Done`), Flushed/Ended/Busy/Rerror/Read/Wrote/Reply.
Server side gained word 2 = hold (µs) on the completion call: held min(hold, bound); 0 answers at once.

Client host tests (libs/client/tests/aio.rs), 10 runs clean: an_inline_submit_to_a_busy_server_
returns_and_goes_at_the_next_poll (submit returned in >= 1 ms and < 500 ms, queued 1, sent at a later
poll, answered, same pages back); one_connection_needs_no_waiter_thread (64 reads on the caller's own
completion calls; a 50 ms hold returns empty, session lives); buffers_come_back_to_their_submitter_
by_value_in_any_order (pointer identity, in and out of tag order); a_flushed_requests_buffer_is_
returned_exactly_once (sent: once, with the Rflush; queued: at once, nothing sent);
two_connections_have_a_waiter_each_and_the_caller_idles_in_receive (+ a stranger's wake-up refused).

Fixed on the way: two rt mux tests read the server's published state before it was published
(flaky ~1/3 after the hold change); they now wait for it; 12 runs clean.

Gates at 2f85cf9c0 (in-dev cargo testbench): rt-host-tests 0, client-host-tests 0, docs 0,
size-budget 0, formatting 0, no-cruft 0, unsafe-budget 0 (unchanged), client-build 0 (rv64, rv32);
earlier at the step-1 tip host-tests 0 (16) and build 0 (12 x 2), fsd/bootfsd builds and fsd-host-
tests, r4-host-tests 0 after their switch.

### Page lines as written (for the Architect)

- serving.md, new section "Multiplexed connections" after "Parked calls" (status: 7 host + fuzz):
  requests, the completion call ([0, 1, hold, 0]; first opens the session, word 2 = 1), served into
  the lend, tags, flush, admission (links R77), the end, crash blame, "Who runs it". Full text in the
  commit b510f8d50.
- serving.md "On the wire": "... is malformed, except a multiplexed connection's completion call,
  words `[0, 1, hold, 0]`"; "Protocol corners": "`Tflush` on a call is answered at once, since a
  call's request is handled whole; on a multiplexed connection it drops the request it names".
- R26 (draft): "A multiplexed request is admitted as a call is, in the same bucket and share, and
  counts until its answer is delivered (R77)."
- R28 (draft): "A multiplexed request the file server asks to hold is held the same way, its
  `InFlight` kept until its answer is delivered or its session ends (R77)."
- R77 (multiplexed requests): the brief's text verbatim. It says "found at the requests'
  deadlines"; the code finds it at the session's bound, min(server's wait, COLLECT_WAIT 10 s),
  which is also what makes consoled's (FOREVER) case finite (Q4).
- SECURITY.md row after R28's: "A client holds no more of a server through a multiplexed
  connection than one share, however many requests it sends or completions it leaves uncollected",
  R77, ninep_mux.rs, the 7 tests, built, partly tested.
- native.md: "Many requests at once" (hub, owner not thread, inline submit, data and pages, one
  connection no thread, waiters, untrusted server) and the `aio` module row. beamlet.md untouched.
- wire.md: untouched (Q3).
To fix at the final rebuild: serving.md in the rt commit links native.md's section and names the
server loops, which arrive in later commits (docs passes at the tip).

### Next: the machine case (needs Q1 and the host)
fsd and bootfsd have in_flight 0 (every session REFUSED); consoled 2 (a lone share of 1, the
session's own). The case's server needs a cap with room for 64 + 1 in one share. Plan: a new bin
`aio-reader` in tests/fsd-programs: burst (1 thread, 0 waiters, 64 reads of an fsd file held
uncollected while a second program reads, then collected) and two-connection (1 + 1 waiter).

## Step 3: rulings 1-8 applied — tip 30007fa00 (6 commits on f9135ce8a)

- Resource::Requests (admit.rs): Limits.requests, Cost.request, outside the open-call headroom, share
  rule and SMALLEST_CAP apply, overrides keep the default; every Limits/Cost literal in the tree names
  it (rt tests/bins, servers, tests/programs, tests/init-programs, userland/otp fixture) in the rt
  commit. Unit test requests_are_outside_the_open_call_headroom. The four 9P servers: requests 0
  pending the cost question (a/b/c) sent to the Architect.
- Batching: a send's transfer carries one or more T-messages end to end, word 1 their length (server:
  shared Arc'd pages, freed with the last request; client: `Hub::batch`, queue packed into one
  transfer). Tests: 64 reads in one send (client); two reads in one transfer (rt).
- Q4: session ends COLLECT_WAIT after its last completion call returned with none parked, or at a
  request's deadline with none parked; a parked call held min(hold, COLLECT_WAIT).
- Ruling 8: client timeout = hold + COLLECT_MARGIN_US (1 s); waiter likewise. Ruling 7: no lock,
  Hub Send not Sync, documented; idle rule (COLLECT_WAIT/2) documented in aio.rs and native.md.
- Pages: wire.md:61 exact line; serving.md admit row `Requests`, R77 bullets as ruled, section
  updated (hold, batching, served only into a parked completion call's lend, end conditions).
- Size: rt 2960->3363, ipd 2887->2892 (rt commit, sweep) ->2924 (ipd commit), keyd 539->541 (rt
  commit, sweep), consoled 339->351, client 632->956; each with its line.
Gates at 30007fa00: host-tests 0 (16 PASS), build 0 (24 PASS), size-budget 0, docs 0, formatting 0,
no-cruft 0, unsafe-budget 0 (unchanged); fuzz ninep_server 8,389,599 runs / 181 s clean.

## Step 4: hold tests; aio-reader and the two cases — tip see git log (8 commits)

- Ruling 8's host tests: rt `a_completion_call_is_held_at_most_its_hold_and_the_servers_bound`
  (hold 0 answers at once and empty; a 200 ms hold answers empty at >= 200 ms; a 1 h hold answers at
  COLLECT_WAIT, 10 s; the session lives on); client
  `a_server_that_breaks_its_hold_loses_the_session_at_the_margin` (the server takes the call and stops
  answering: the client gives up at hold + 1 s, the read comes back Ended with its buffer, later submits
  are Disconnected, and the server, resumed, ends the session on the abandonment: a new connect opens
  a fresh one). Both listed in the pages' status lines. 6 client runs clean; rt 8/8.
- `tests/fsd-programs/src/bin/aio-reader.rs` (burst / second), `tests/data/aio/boot.json`, cases
  `aio-many-reads` (one connection, prints "1 thread, 0 waiters") and `aio-many-reads-two` (two
  connections, a waiter each: prints "1 thread, 2 waiters" — see the open question). Builds for rv64 and
  rv32; `cargo testbench --list` parses both. NOT RUN: needs the host, and fsd's caps (in_flight 0 today
  refuses the session; requests 0 until the cost is ruled).
- `Hub::waiters()` added for the thread count.
Gates at the tip: size-budget, docs, formatting, no-cruft 0 (client ceiling 957).

## Step 5 (aio1-implementer-2): rebase, gates at the tip, pages per commit

- `git rebase --onto 128bd45d9 f9135ce8a wp-aio1`: clean, no conflicts. `in-dev cargo test -p
  testbench`: 0 (79 passed).
- Gates at 64ea94455 (in-dev cargo testbench <case>, exit codes): host-tests 0 (16 PASS), build 0
  (24 PASS, rv64+rv32), size-budget 0, unsafe-budget 0 (unchanged), docs 0, formatting 0,
  no-cruft 0.
- Fuzz ninep_server (sancov+ASan build, libs/rt/fuzz, target/aio1-fuzz): 1,114,639 runs / 181 s,
  clean (fewer runs than before: HW1's whole bench was loading the host).
- Final page rebuild (owed item 5): the rt commit no longer names later work; each piece moved
  into the commit that builds it. rt: status without the boot clause, no hub link, "Who runs it"
  without server names. consoled: "(none for `consoled` ...)" and "`consoled` when input comes".
  ipd: "`ipd` after each poll" and "(`ipd`'s sockets, [ipd](ipd.md))". fsd, bootfsd: "(`fsd`,
  `bootfsd`)". client: the hub link in serving.md; native.md status "only on the host". tests:
  both status lines gain `aio-many-reads`. Tree at the new tip 73274d178 == 64ea94455 (empty
  diff); docs 0 at each of the 8 commits alone.
- Tip **73274d178** on main 128bd45d9, 8 commits, clean. Machine cases still NOT RUN (host).

## Step 6 (aio1-implementer-2): simplifier and editor folds

Each fold in its owning commit; history rebuilt on 128bd45d9 (tip below).

1. **Status lines (editor P1).** serving.md "Multiplexed connections": "attacked in host tests with
   the runtime's fake kernel; no boot has run it yet". native.md "Many requests at once": "only on
   the host; no boot has run it yet". The tests commit no longer touches them; the commit that
   records the machine run flips both and adds `bench:aio-many-reads(-two)`.
2. **Loops (simplifier).** `NineServer::run_around(endpoint, impl Around<S>)` in rt:
   `Around { call; turn (before each receive, after expire); abandoned (notices that are no
   completion call's) }`; `run(endpoint, own)` is `run_around` with a closure adapter. consoled
   runs it (`Readers(Parked<()>)`); the mux tests' own copy of the loop is gone too (`Publish`, an
   `Around`). **ipd keeps its own loop**, said on serving.md "Who runs it": it polls its stack only
   after a receive that returned no call (R21), receives with no wait after each call, takes
   netd's frames on sends, and bounds each wait by its stack timers and link retry; hooks for all
   of that would be its loop again. Two loop shapes remain: run(_around) and ipd's.
   **Bug found and fixed in the move** (consoled commit): `wake_readers` returned early when input
   came with no one-call read parked, so a multiplexed read waiting for input was answered only
   when its completion call's hold ran out (10 s). New host test
   `redoubt-consoled::a_multiplexed_read_waits_for_input` (servers commit: consoled's caps are 0
   before it, so the read is `busy` there): fails at the hold's end with the old `return`, passes
   with `break` (5 runs). consoled.md: the read bullet states it; /dev/cons status tested (12).
3. **admit.rs tests:** a `caps(buckets, in_flight, files, state)` helper; 13 literals gone.
4. **rt size.** main 2960. Report figures were per stage: 3346 (step 1, first mux build), 3363
   (ruling 8's hold), 3417 (ruling 9: the `Pages` resource in admit.rs, a send's pages admitted
   all-or-none before its requests, held by `Arc` until the last of them, the refused-send path).
   Now **3432**: +15 for `Around`/`run_around`. The rt commit's `Size budget: libs/rt` line says so;
   tests/size-budget.toml 3432. native.md names no line count. consoled: 339 -> 336 (consoled
   commit, the loop goes) -> 338 (servers commit, caps; its Size budget line stays).
5. **Rewraps** to 100 columns: serving.md R4a send line, Tflush corner, R26 root's-bucket
   sentence; native.md client intro and the asynchronous link; consoled.md Admission; ipd.md
   Admission.
6. **Deliberate, as built:** R77 is prose, and the death bullet names the session bound rather
   than "found at the requests' deadlines": ruling 4 as the Architect refined it (session bound =
   min(server's longest wait, COLLECT_WAIT)); a request's deadline answers it `timeout`, it never
   ends the session. The R26 sentence is the Architect's one sentence verbatim (ruling 9's text);
   `Pages`' own share is in the admit table's line. The renamed host test is
   `redoubt-rt::a_death_between_completion_calls_is_found_at_the_session_bound`.

## Step 7 (aio1-implementer-2): Red's P1s and P2s

Each fold in its owning commit; tip **856b19a7b** on 128bd45d9 (simplifier/editor folds of step 6
included).

- **P1-a (client commit).** `free_tag` now skips every tag an outstanding flush names (one pass
  over the slots into a 256-entry map; a queued request holds its slot, so the queue scan went).
  Test `redoubt-client::a_tag_a_flush_names_is_not_reused_before_its_rflush`: a waiter's wake-up
  carries the read's answer, the Tflush goes, the answer is delivered (slot freed), a new read is
  submitted (must not get the read's tag), the Rflush comes alone, and the new read is answered
  when its gate opens. With the old `free_tag` it fails "tag 0 reused".
- **P1-b (rt commit).** `run_around` (so `run`: fsd, bootfsd; and consoled, now on it) reads the
  time again after `receive` returns and serves what came at that time. Test
  `redoubt-rt::a_request_late_in_a_hold_leaves_the_session_its_whole_bound` (real clock, bound
  1 s: a read at 0.7 s into a hold, the next call 0.6 s after its answer): the session lives; with
  the stale `now` the next call opens a new session. Listed in serving.md (section 11, R77 9) and
  SECURITY.md's R77 row. ipd already took `now()` after receive.
- **P2-a (client commit): the hub's batch is at most one page**, not the share raised. A batch
  goes end to end in transfers of `PAGE_SIZE`; a single request longer than a page goes alone in
  the pages it needs. Arithmetic: `consoled` and `bootfsd` have `pages` 2 a bucket, so a lone
  badge of a non-zero account has a share of 2 / (1 + 1) = 1 page; a `Tread` is 23 B, so a page
  holds 178 of them (MAX_TAGS 256): 64 reads stay one page on either width. A `Twrite` of more
  than 4 073 B of data needs two pages and is still `busy` at a 1-page share: at `bootfsd` writes
  are refused anyway; at `consoled` a writer that big writes in pieces or by call (residual, said
  here, not on a page). Test `redoubt-client::a_batch_goes_a_page_at_a_time` (200 reads: 2 sends;
  1 send before). native.md and the hub's doc say it; client 957 -> 964.
- **P2-b (rt commit): the fuzz target** installs a small kernel of its own (`Kern`, a host
  `Transport`: receive hands out queued messages, reply records answers and frees the lend,
  map/unmap on the heap, the target's clock). New paths: requests end to end in 1-2 page
  transfers (copies retagged; length right or off by a few bytes), so `Pages` are admitted and
  partly refused; completion calls parked (hold 0 / 1 ms / 3 s / 20 s, lend 1-2 pages) and
  answered by the server, emptied at their hold, ended, or abandoned by the target (reply
  undelivered); `wake`. Checks: every answer frames and decodes; across all collections a tag is
  answered no more times than it was sent; every resource within its cap every step (the Pages
  assertion is live); at the end, every session past its bound, no `Requests` or `Pages` held,
  every call answered, every page freed. Mutations: never releasing a send's pages -> "a page
  outlived its requests" within seconds; never clearing a refused tag -> "a tag answered more
  times than it was sent" (needs 7 requests in a session and two collections: found at once from
  a hand-built seed, not in 120 s from an empty corpus; the gate run seeds it). Unsafe in the
  target: the transport impl and four record/page accesses, each with its SAFETY line.
- rt 3432 -> 3434 (the second `time_now`).

## Step 8 (aio1-implementer-2): the Architect's fold

- **`Hub::write` is one page at most** (client commit): `MAX_WRITE = PAGE_SIZE - IOHDRSZ`
  (4 072 B); a longer write is `Wire(TooLarge)`, nothing queued or sent, for its caller to split.
  So every send the hub makes needs one page at most, and a 1-page share (consoled, bootfsd)
  takes every write. **The step-7 residual "Twrite > 4073 B busy at consoled" is gone.** Test
  `redoubt-client::a_write_is_at_most_one_page` (MAX_WRITE: one send, `Wrote(4072)`; one byte
  more: TooLarge, no send; fails without the cap). native.md: writes go a page at a time; status
  tested (9). client 964 -> 966.
- Seen once, not reproduced in 30 runs: `cargo test -p redoubt-client --test console` failed once
  (which test lost); the file is untouched by this branch.
- Tip **028434b64** on 128bd45d9.

## Step 9: rebase onto 0d207732f, gates, a timing test made deterministic

- `git rebase --onto 0d207732f 128bd45d9 wp-aio1`: clean. Gates at its tip 65f8e43df: build 0 (24),
  size-budget, unsafe-budget, docs, formatting, no-cruft 0; docs 0 at each of the 8 commits;
  **host-tests 1**: `a_batch_goes_a_page_at_a_time` saw 1 send, not 2 (load: a send not taken in
  `SUBMIT_TIMEOUT_US` goes at the next entry, so counting the client's sends is timing).
- Fix (client commit, tests only): the test server records the transfers it took (count, most
  pages). The batch test asserts 2 transfers of 1 page after every answer; the 64-read test 1 of
  1; the write test 1 of 1 (the refused write still: no send at all). Mutation (old 16-page room):
  fails "200 reads in two transfers" 1 vs 2. 5 runs serial, 12 runs four at a time: all pass.
- Tip **9e196ff25** on 0d207732f: client-host-tests 0, host-tests 0 (16), formatting 0, no-cruft 0,
  docs 0 (other gates unchanged: only libs/client/tests/aio.rs changed since 65f8e43df).

## Step 10: the machine cases, and what they found

Host given at 9e196ff25 (on 0d207732f). Commands from the worktree, `in-dev cargo testbench ...`:
- `--arch rv64 aio-many-reads` (the filter takes both cases): both FAILED, "wrote 4072 of 4096
  bytes": aio-reader wrote `/aio` in one call, and a one-call write is bounded by its one-page
  lend. Fixed: it writes in a loop (tests commit).
- Again: both exited 101 with nothing printed: the hub's `encode` kept a `vec![0; MSIZE]` per
  request (`truncate` keeps the capacity), so 64 queued reads held 4 MiB against the burst's 512
  pages: out of memory, and the panic report could not map its lend either. Fixed: a queued
  request keeps only its bytes (`scratch[..n].to_vec()`; client commit). The machine case is the
  test that caught it.
- Again: aio-many-reads PASS; -two FAILED "receive: Timeout": with waiters every answer arrived
  while the burst waited for the second program, and the collection loop blocked in `receive`
  before draining the hub. Fixed: drain first (tests commit).
- Then: `--arch rv64 aio-many-reads` 0 (2 PASS); `--arch rv32 aio-many-reads` 0 (2 PASS).
- The servers' cases, one filter at a time, both widths, each exit 0, 77 PASS in all: `fsd-`,
  `init-`, `image-disk`, `uart-irq`, `net-tcp`, `net-pinned`, `net-attacks`, `netd-restart`.

Then folded (autosquash into the client and tests commits); the tests commit flips the status lines:
serving.md "Multiplexed connections" "a boot runs it only in `aio-many-reads` and
`aio-many-reads-two`" with both `bench:` entries (tested 13); native.md "Many requests at once"
"on the machine only in ..." (tested 11). Tip **fa354b4e4** on 0d207732f, 8 commits, clean: docs 0
at each commit; client-host-tests, size-budget, unsafe-budget, formatting, no-cruft 0. The tree is
the one the cases ran on, plus the two status lines. Build (both widths) was exercised by the boots
of the same code; the full host-tests and build gates ran at 9e196ff25 (0).

## Step 11 (aio1-implementer-3): rebase over FSD4 and BEAM2

`git rebase --onto ac84b87b3 0d207732f wp-aio1`; resolved in the owning commits:
- rt commit (f9d66aa3f): size-budget fsd 1401 (FSD4) + 2 = 1403.
- servers sizing commit (354c24195): size-budget fsd 1405; servers/init/tests/manifest.rs's fit
  sum becomes BEAM2's with bootfsd 640: 256 + 1024 + 640 + 512 * 2 + 1024 + 4096 + 1024 * 2 +
  24_576 + 10. Its 22 manifests and image/manifest.json give bootfsd 640 (all 23 that run it).
- tests commit: tests/data/aio/boot.json's blkd gains "args": ["endpoint=blkd"] (amended). No
  other manifest the branch adds or changes had a blkd without it.
- fsd: FSD4's listing window (server.rs) and the NineServer::run switch (bin/fsd.rs) merged with
  no conflict; both one-call and multiplexed reads reach FileServer::read, so the window serves
  both. The fsd-* machine cases are what checks it.
Each commit's Size budget line already names fsd; the counts are the new base's plus each
commit's own delta (measured at the tip: fsd 1405 of 1405).

Gates at tip 4d31c0bf7 (in-dev cargo testbench <case>), exit codes: size-budget 0, unsafe-budget 0,
docs 0, formatting 0, no-cruft 0, host-tests 0 (16 PASS), build 0 (24 PASS, rv64+rv32).
Fuzz ninep_server (sancov+ASan, target/aio1-fuzz, corpus): 747,517 runs / 181 s, clean, exit 0.
Machine cases: not run, waiting for the host.

Machine cases at 4d31c0bf7, host alone, one at a time (`in-dev cargo testbench --arch A F`):
rv64 aio-many-reads 0 (2 PASS: aio-many-reads, -two); rv64 fsd- 0 (12 PASS); rv32 aio-many-reads
0 (2 PASS); rv32 fsd- 0 (12 PASS); rv64 image-disk 0 (1 PASS, 6.1 s); rv64 userland-boot 0
(1 PASS, 148.5 s). Tip **4d31c0bf7** on ac84b87b3, 8 commits, clean.
