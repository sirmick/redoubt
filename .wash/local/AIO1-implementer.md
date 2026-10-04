# AIO1: one I/O hub per process, many outstanding 9P requests, no kernel change

Tier A (the serving library and the 9P skeleton every file server runs on; the client library).
Size M+. Needs nothing (ABI2 has merged); start from main. BEAM3 builds on it. Run every cargo and
bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

## The owner's shape (2026-10-03)

"One thread on the non-kernel side holds the buffers, so it never breaks the receiver-owns-buffer
rule that keeps the kernel small. AIO is via that thread instead." The design and its trade-offs
are in `.wash/local/async-ipc-assessment.md`, with its addendum; this brief is what was ruled.

## Ruled (architect-14)

**The completion side is (i): a hub (an owner and a queue, run by whichever thread submits), plus a thin waiter thread per server connection once there is more than one (with one, the caller may wait in it itself).**

**(ii) is refused**: a request carrying a client-minted, badge-scoped handle to the client's
completion endpoint, with the server's completion sent there.
- The authority would be acceptable. R1 checks a send as it checks a call (ipc.md: between `user`
  budgets the labels are equal, and a system server is unchecked), so a handle the client minted
  gives a server exactly the pairs a reply already may reach. It is revocable, and within R34's
  one-label-set servers.
- The blocking is not. A completion `send` holds the server's thread until the client's one
  thread takes it. That thread may itself be blocked sending a request to that same server: a
  **deadlock**.
- Any timeout breaks the shape another way. A dropped completion leaves a client unable to tell
  whether a write happened, and a server thread held by a client breaks R26 ("one client cannot
  use up a shared server").

**Exactly one thread needs one small kernel object, not proposed here: a notification.** A word
of badge bits a holder ORs in without blocking, and its owner waits on in `receive`. That is
seL4's notification, and the same kind of pending state as an IRQ handle's. The server would
signal "completions ready", and the hub would call that server to fetch them, a short call
answered at once. The waiters then collapse into the hub. It is the owner's choice later
(AIO2, S-M, after IPC3).

The endpoint-set wait does **not** collapse the waiters: they block in `call`s, and a set is
receive-side.

## Context rules (read these first)

- **Don't read whole files.** In `libs/rt/src/server/`: `parked.rs` (`Parked`), `admit.rs`
  (`Admission`, `InFlight`), and in `ninep.rs` the trait, `answer_in_place` and the `read`
  dispatch. In `libs/client/src/`: `file.rs` (`File`, the connection type) and `lib.rs`'s
  exports.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/AIO1-report.md`.

## Reading list

- `docs/kernel/ipc.md`: "The calls", R1, R2 (`WAIT_CAP`), R4 and R4a (a reply is never refused;
  sends are taken at `MAX_OPEN_CALLS`).
- `docs/servers/serving.md`: "Parked calls", "The 9P server skeleton", R26, R28.
- `docs/userland/beamlet.md`: "Asynchronous underneath" (what BEAM3 will change; read only).

## The settled design

1. **The hub is an owner and a queue, not a thread** (`libs/client`, new module `aio`; amended
   by the owner, 2026-10-03: "does it even need to be a first-class thread? just that the buffers
   are owned non-kernel side?").
   - **Ownership.** Per process, one `Hub` value owns every I/O buffer and every connection's
     tag table and queue. A buffer is handed in by value with a request and back by value with
     its completion, so Rust's ownership is the page's rule: one owner at a time, whichever
     thread runs the hub.
   - **Who runs it is the caller's choice.** Any thread may submit inline (`Hub::submit`, under
     the hub's lock): it sends the request itself. A process wanting a dedicated hub thread may
     have one, but nothing needs it.
   - **The send rule (inline submission's one trap).** A request is sent with timeout
     `SUBMIT_TIMEOUT_US` (a named constant, small: 1 ms to start, measured). A `send` waits until
     its server takes it, so a busy server could otherwise stall the submitting thread: a VM
     scheduler, under BEAM3. A `send` that times out was not taken: the request stays in the
     connection's queue, and the caller goes on.
   - **Retries.** A queued request is retried at the hub's next entry: any submit, any completion
     handed in, or `Hub::poll`. A caller must enter the hub at least whenever it is about to idle,
     so nothing queued waits past its next idle.
   - **Lends and transfers** follow the kernel's rules unchanged:
     - a request with data is a transfer: the pages are the server's until it answers;
     - a completion's data comes back copied into the completion call's lend, and the hub hands
       it on by value.
2. **Waiters: the one thread per server connection the design needs.** A connection's
   **completion call** is one long-poll `call`, lending a completion buffer the hub hands out by
   value. The server parks it and answers it with one or more tagged completions when any are
   ready. A blocked `call` occupies its thread, so each connection's completion call needs a
   thread to wait in it.
   - **One connection:** the caller may wait in the completion call itself when it would otherwise
     idle, with the call's timeout its own next deadline. Then no extra thread exists at all. This
     holds only while nothing else must wake it: a thread blocked in a `call` to one server
     cannot be woken by anything else.
   - **More connections:** each has a waiter thread blocked in its completion call. The caller
     idles in `receive` on one endpoint of its own process, with its next deadline as the timeout.
     A waiter hands its filled buffer to the hub by value, then `send`s one word to that endpoint
     to wake the caller, takes a fresh buffer and calls again. A waiter owns no other buffer and
     talks to no other server.
   - **BEAM3** runs the hub on its scheduler thread, submitting inline and idling in `receive` on
     its own endpoint, with waiters for its connections: `fsd` per volume, `ipd`, `consoled`,
     `bootfsd`. A small native program with one connection runs no extra thread.
   - **AIO2's notification** (not in this package) would remove the waiters: the caller would
     idle in `receive` with the notification among what wakes it, and fetch completions with a
     short call.
3. **The wire: 9P2000 messages, multiplexed by tag.**
   - **A connection's requests** are `send`s on its badge, each one 9P T-message in the four
     words (small) or a transferred page (with data, or too long for the words).
   - **Completions** are R-messages packed end to end into the completion call's lend, each with
     its tag. The call's reply words say how many bytes.
   - **Tags:** at most `MAX_TAGS` (256) outstanding per connection. A request with a tag already
     in use or out of range is a protocol error that ends the connection, as a malformed 9P
     message does today.
   - **Flush:** `Tflush(oldtag)` is answered by `Rflush` after any completion for `oldtag`, and
     none comes after it (intro(5)'s rule). A client accepts either order: a completion then
     `Rflush`, or `Rflush` alone.
   - The completion call and the requests use the connection's one badge, and are admitted
     together.
4. **The serving library: parked requests** (`libs/rt/src/server/`). The 9P skeleton serves both
   ways on one endpoint: today's one-call-per-request, and the multiplexed connection. A server
   built on the skeleton gets the second without its own code, except where it parks today
   (`consoled`'s read and `resize`, `ipd`'s reads), whose `Parked` use carries over to requests.
   - **Admission (R26/R28).** Each request takes one `InFlight` from the connection's bucket and
     share, in the same `Admission`, held until its completion is delivered in a completion
     call's reply.
   - **Over the share,** a request is not served. Its tag goes into the connection's 256-bit
     refused set, answered as `Rerror` "busy" at the next completion call: bounded, 32 bytes per
     connection.
   - **Completions waiting** for a completion call count against the same share. So a client that
     never polls fills only its own share, and is then refused, while other clients are served as
     before.
   - **Deadlines.** Every parked request keeps the server-side deadline parked calls have today.
     At the deadline it completes with an error.
   - **The completion call** is an ordinary parked call: one per connection. A second is
     refused. Its abandonment (I15's notice: the client died or timed out) ends the connection:
     every parked request and waiting completion of it is dropped and its admission released.
   - **One thread.** As today, a server's parked requests are served by the thread that took
     them.
5. **What it does not change.** The kernel, the ABI, R1-R4, the typed protocols (`blkd`'s), and
   today's one-call 9P for every client that does not multiplex.

## The rule (serving.md, a new property: R76 is VOL1's, so take the next free ID and name it)

> **R7x (multiplexed requests).** A connection's outstanding requests are bounded by its
> admission share, and its undelivered completions count against the same share. So a client
> that never collects its completions, or floods requests, holds no more of a server than one
> client's share, and its excess is answered `busy`, never queued without bound.
> - A client's death, or its completion call's timeout, ends the connection and frees every
>   request and completion it held.
> - A death between two completion calls is found at the requests' deadlines.
> - A flushed request is answered once at most, before its `Rflush`, never after.

Add its row to SECURITY.md.

## The cases

1. **Host, `libs/rt`** (the fake kernel):
   - `a_never_polling_client_holds_only_its_share`: another client on the same server is served
     throughout, and the never-polling client's excess is `busy`.
   - `death_with_requests_parked_frees_the_connection`: abandoning the completion call frees every
     request, completion and admission.
   - `a_death_between_completion_calls_is_found_at_the_deadline`.
   - `a_flush_racing_a_completion_answers_once`: both orders, never a completion after `Rflush`.
   - `a_flood_of_sends_at_wait_cap_never_blocks_the_server`.
   - `a_reused_or_out_of_range_tag_ends_the_connection`.
   - `a_second_completion_call_is_refused`.
2. **Host, `libs/client`:**
   - an inline submit to a server that does not take within `SUBMIT_TIMEOUT_US` returns, and
     the request is sent at the next `poll`, losing nothing;
   - one connection with no waiter thread: the caller's own completion call delivers;
   - buffers come back to their submitter by value, in tag order or out of it;
   - a flushed request's buffer is returned exactly once.
3. **Machine, both widths: `aio-many-reads`.** One test program's hub keeps 64 reads outstanding
   on `consoled` (or a `fsd` file) on one thread with no waiter (one connection), and a second program
   reads meanwhile. Verdicts:
   - every read is answered;
   - the second program's read is answered during the burst;
   - the thread count is printed: 1 thread, 0 waiters. A second variant runs two connections:
     the caller idling in `receive`, plus 2 waiters.
4. Every existing host test and the whole bench, both widths: today's one-call 9P is unchanged.

## Page lines (exact text in the report)

- **serving.md:**
  - a section "Multiplexed connections" after "Parked calls": the hub's wire as seen by a
    server, tags, flush, the completion call, admission;
  - the R7x rule above;
  - R26 and R28 each gain one sentence: a multiplexed request is admitted like a call, and its
    undelivered completion counts.
- **files.md** (or wherever the client library's connection is described): the hub and its
  waiters, buffers by value.
- **SECURITY.md:** R7x's row.
- **beamlet.md:** nothing. BEAM3 rewrites "Asynchronous underneath" on top of this.

## Owned paths

- `libs/client/src/aio.rs` (new) and its exports.
- `libs/rt/src/server/{parked.rs,admit.rs,ninep.rs}` and their tests.
- The servers' parking code only where it moves to requests (`consoled`, `ipd`): report each.
- The test programs and the case, and the pages above.

**Not yours:** the kernel, the ABI, `libs/sys`. **Hotspots:** IPC3 (kernel only, no overlap);
BEAM3 starts on top of this.

## Gates

- The whole bench on both widths, alone.
- `rt-host-tests`, the client's host tests, every server's host tests, the fuzz corpus of
  `ninep_server` (it must still pass; add multiplexed inputs to it).
- `cargo fmt --check`, the size and unsafe budgets, doccheck, no-cruft.

Report each command with its exit code, the case's verdicts and thread counts, the rule's ID, and
the page lines as written.

## Rulings during the build (architect-14, 2026-10-03)

1. **Q1, the admission resource: (b).** A new `Resource::Requests`: multiplexed requests, each
   held from its take until its completion is delivered. Its cost is its state plus the pages it
   brought, and it sits outside the open-call headroom, since a request is a `send`, not an open
   call. R26's share rule (`limit / (n + 1)` per badge, the account-0 root rule) applies to it
   unchanged. The completion call alone is an `InFlight`.
   - Each 9P server's limits gain a `requests` cap (`fsd`, `bootfsd`, `consoled`, `ipd`) that its
     budget can pay at full transfers. Report the values and the budget arithmetic.
   - A transfer is charged to the server at the take (R4), before admission can refuse it, so a
     refused request's pages are freed at once: the excess costs one message at a time.
2. **Q2, the loop: yes.** `NineServer::run` in the skeleton receives with `max_transfer` =
   `MAX_LEND_PAGES`, and handles sends, abandoned notices and deadlines. `fsd.rs` and `bootfsd.rs`
   switch to it, and their limits gain `requests`: those paths are now yours. `server::serve`
   stays for servers that take only calls.
3. **Q3, the wire: the word-1 marker, and wire.md amended.** The completion call is
   `[0, COLLECT, 0, 0]` with a lend; its reply is `[0, bytes, 0, 0]`. A request `send` (word 0 = 0)
   carries one T-message in words 1-3 when it fits, or **one or more T-messages end to end** at
   the start of a transfer, word 1 their byte count. Batching keeps 64 reads to one page on either
   width, rather than a page per `Tread` on rv32.
4. **Q4, waits with no deadline: (b).** A session with no completion call parked ends
   `COLLECT_WAIT` (a named constant, 10 s) after its last completion call returned, whatever the
   server's longest wait.
   - The hub keeps a completion call parked whenever it has requests outstanding, re-calling at
     once.
   - The one-connection inline mode is allowed only to a caller that idles at least every
     `COLLECT_WAIT / 2`. Otherwise it uses a waiter. Say so in `Hub`'s doc.
5. **Q5, the queue: (a), a `Lock<T>` in `redoubt_rt`.**
   - **The unsafe:** one `unsafe impl Sync` and one `UnsafeCell` view, each with its `// SAFETY:`
     reason. The unsafe budget rises by 2 for `libs/rt/src`, with its `Unsafe budget:` line.
   - **Tests:** a host test of the lock under contention (threads hammering a counter), run under
     Miri if `libs/rt`'s unsafe is Miri-run today.
   - **The rule, in its doc:** nothing blocks under the lock (no IPC, nothing that waits); a
     critical section is a queue push or pop. Contention spins with `sleep(0)`, which on this
     kernel runs no other thread first (timer.md), so a contended lock costs the spinner at most
     its slice, on one hart.
   - (b) was refused: every buffer move would be an unmap and remap, and submitters would share
     one `WAIT_CAP`.
6. **The "Decided" list in the report: accepted as written.** Serving requests only into a parked
   completion call's lend means nothing is copied or held in the server, and a request waiting
   for a call holds its admission. State that in serving.md's section.

**Page lines added:**
- **serving.md, `admit`:** the resources gain "`Requests`: multiplexed requests, each held from its
  take until its completion is delivered and costing its state and the pages it brought. Outside
  the open-call headroom, since a request is a send, not an open call."
- **serving.md, R7x:** the death bullet reads "A death between two completion calls is found at
  the requests' deadlines, or `COLLECT_WAIT` after the last completion call returned, whichever
  comes first."
- **wire.md:61:** "A 9P call with a non-zero word or no lend is malformed (status 1), except a
  multiplexed connection's completion call, whose word 1 is `COLLECT`
  ([serving](serving.md#multiplexed-connections))."
- **native.md**, where the runtime's pieces are listed: the `Lock` and its rule, one sentence.
7. **Q5 replaced (2026-10-03): no lock in the library.**
   - `Hub` is a plain value with `&mut self` methods, and `Send`. A caller that shares it across
     threads wraps it in its own lock (beamlet has `sync::Lock`), and "under the hub's lock" in
     point 1 means the caller's lock. No `Lock<T>` goes in `redoubt_rt`, and no unsafe budget
     changes.
   - A waiter touches no hub state. It hands its filled completion buffer to the caller as the
     transfer of its one-word wake `send`: in-process, so no charge moves, and the caller's
     `receive` names a `max_transfer` that large. It then maps a fresh buffer for its next call.
   - native.md's `Lock` sentence goes. `Hub`'s doc says it is `Send`, never `Sync`.
8. **The completion call's hold time (2026-10-03).** The completion call is
   `[0, COLLECT, hold_us, 0]`. The server holds it at most `min(hold_us, its own bound)`, answering
   empty when that passes, and `hold_us` = 0 answers at once.
   - The client's kernel timeout on the call is `hold_us + COLLECT_MARGIN_US` (a named constant,
     1 s to start). A kernel timeout therefore means the server broke its promise: the session is
     lost, and the server ending it on the abandonment is right. I13 holds, and a hung server never
     holds a caller past its own deadline plus the margin.
   - "Wait until my next deadline" is `hold_us` = that deadline, which abandons nothing.
   - serving.md's section states the hold. R7x's "its completion call's timeout" bullet reads "the
     client's death, or its completion call's abandonment (a client that gave up)".
9. **Q1 corrected (2026-10-03): two resources, charged for what was brought.** This replaces
   ruling 1's "pages it brought, at full transfers" and the unit count (cap 80) sent before it.
   - **`Requests`** counts requests at their state cost (a named `REQUEST_STATE`, about 256 B),
     held until the answer is delivered.
   - **`Pages`** counts the transferred pages a connection's requests hold: one per page, however
     many T-messages it carries. A page is held until the last request it brought is answered or
     dropped, and is admitted several at a time, as `ipd`'s sockets are. It is charged at 4096 B.
   - Each resource has its own cap and cost per server, sized within that server's budget.
   - **R26 holds per resource.** Each has its own per-badge share (`limit / (n + 1)`, and the
     account-0 root rule), so a badge's pages and its requests are each bounded, and neither can
     use another badge's share. A request that would take its badge past either share is `busy`.
     A batched page whose requests are partly refused is held only by those admitted, and freed
     with the last of them.
   - **Sizes:** `fsd` 128 requests and 32 pages per bucket (32 KiB + 128 KiB), within its 512 KiB.
     `consoled` must also hold 65 requests and 1 page in one account-0 share. `bootfsd` takes no
     write data: give it requests within its budget and the fewest pages a batched transfer needs
     (1-2). `ipd` sized the same way. `netd` and `keyd` get 0. Raise a budget only if a server
     cannot meet those floors, and report every server's arithmetic.
   - **Page line**, serving.md `admit`, the resources list: "`Requests`: a multiplexed
     connection's requests, each held until its answer is delivered. `Pages`: the pages their
     transfers brought, one per page however many requests it carries, held until the last of them
     is answered. Both are outside the open-call headroom, since a request is a send, not an open
     call, and each has its own share (R26)."
   - **R77's first sentence** becomes: "A multiplexed connection's requests, and the pages they
     brought, are bounded by its admission shares of `Requests` and `Pages`, and a request whose
     answer is not yet delivered still counts." The rest is as sent.
