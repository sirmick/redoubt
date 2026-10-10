# CONS1 design checkpoint: parking a typed call, then `consol` (size, resize)

Base: origin/main 44d970ab4, worktree .worktrees/CONS1 (no code yet).

## 1. What is built already (status lines, code, tests)

| Piece | State | Evidence |
| --- | --- | --- |
| Parked calls (`Parked<T>`: park/resume/resume_first/expired/abandoned, InFlight admission, FOREVER) | built | serving.md "Parked calls", 4 host tests (`parked_calls_are_served_abandoned_and_expired`, ...) |
| Typed dispatch (`TypedServer::handle`, `answer`, `serve_call`, `finish`) | built | serving.md "Typed dispatch", 5 host tests |
| 9P parking (`NineServer::serve_parking` hands back a `Read::Wait` request with its lend intact) | built | consoled.rs, sshd.rs, ipd use it |
| **Parking a typed call** | **planned** | serving.md: Status planned, three Open questions |
| `consol` wire table: `size` 16, `resize` 17 (both `cols: u16, rows: u16`), `ended` 18 (send) | built | libs/wire/tables/consol.md, generated libs/wire/src/proto/consol.rs |
| consoled: `/dev/cons`, parked reads, `ended` | built | consoled.md (12 + 14 tests) |
| consoled: `size`/`resize` | **refused as malformed** | consoled.rs:123 `serve_parking(request, \|_, r\| refuse_malformed(r))`; consoled.md "a `consol` opcode is only sent to see it refused" |
| sshd: each channel's window (pty-req, window-change, zero refused, cut at 1,024) | built | sshd lib.rs `Window`, console.rs `Chan.window` |
| sshd: `consol` per channel | **refused as malformed** | sshd.rs:257 same closure; sshd.md status: "a channel's window size reaches the session only once `consol`'s `size` and `resize` are served" |
| Client library `Console::size` (None on malformed) and `Console::resize` | built | libs/client/src/console.rs; tested against an in-test `consol` server (`size_and_resize_come_from_the_server`) |
| beamlet on Redoubt `console_size` → `Console::size`, asked afresh | built (answers None today: servers refuse) | userland/otp/redoubt/src/lib.rs:508; beamlet.md table |
| Shell driver: size asked at each prompt via `beamlet:console_size/0`, 80 x 24 when `unknown` | built | driver.ex |
| Resize delivered to the VM or the shell | **nothing** | shell.md "…the console's size": planned `Console.await_resize(pid)`, Open |

So CONS1 is: the library's typed parking; `consol` served by consoled **and by sshd**, since sshd is
the only console whose size changes; a resize path from the platform to the shell's driver.

## 2. Parking a typed call (serving.md Open questions answered)

- **How a server says "wait".** It is a provided method on `TypedServer`, with no new trait and no
  change to any existing server:
  `fn waits(&mut self, caller: &Caller, request: &P::Request<'_>) -> bool { false }`.
  - `typed::serve_parking::<P, S>(server, request) -> Result<Option<Request>, Error>` decodes the
    request. If `waits` is true, it hands the request back unanswered, its words and lend intact.
    Otherwise it is `serve_call`.
  - `serve_call` treats `waits` true as a server bug and answers `malformed` rather than leave
    the call hanging, as `serve_with` already does for 9P.
- **Decoded fields are not kept.** A waiting request is decoded again when it is served, as a
  parked 9P read is read again from its lend. `Parked<T>` keeps the server's own state `T`. For
  `consol` that is the size generation the call parked at.
- **Answering a resumed call.** `typed::reply::<P>(request, &reply)` encodes the reply into the
  request's lend and `finish`es it. The server answers from its own state (the size now) without
  going through `handle` again. Admission, `serve` before resuming, the deadline and abandonment
  are `Parked`'s, unchanged.
- **Which operations may park.** Only those the server's `waits` names. A request that carries
  handles never waits: the dispatcher answers it `malformed` and closes them, so a parked call
  holds no handle of the caller's. `consol` parks `resize` only. `size` is answered at once.
- **Joined to 9P.** `NineServer::serve_parking`'s `own` closure returns
  `Result<Option<Request>, Error>` (was `Result<(), Error>`), so a typed request can come back to
  be parked next to the 9P reads.
  - This is a mechanical change at 15 call sites (consoled, sshd, ipd, beamlet's fixture,
    init-programs, the rt/client tests, the fuzz target).
  - `serve_with` keeps answering anything handed back.
  - Runtime contract change: every bin linking redoubt_rt is built before any whole bench.

## 3. `consol` on consoled and sshd

- **`size`** answers the console's columns and rows at once.
  - consoled: from a new argument `size=COLS,ROWS` (each 1 to 1,024; the default and the image's
    is 80 x 24). A bad value means no start (`BAD_LIMITS`), as `buckets=` does.
  - sshd: the channel's `Window` (pty size, else 80 x 24).
- **`resize`** parks with state = the console's size generation at parking. It is answered with
  the new size when the generation moves.
  - consoled's generation never moves: a UART has no window, so its `resize` waits until its
    caller gives up. The page says so.
  - sshd bumps the channel's generation on an accepted `window-change`, and its turn resumes
    that channel's parked `resize` calls.
- **Who may park.** Any holder of a connection to that console: the same callers who may read
  it. `size`/`resize` are reads, so labelled callers may ask. A labelled reader learning the size
  of an unlabelled console is a read up, not a write down (R69 unchanged).
- **Bounds.**
  - Each parked `resize` takes one `InFlight` from its caller's bucket and share, in the
    server's `Admission`, shared with the 9P reads: consoled `in_flight: 2` per bucket, sshd 4.
    A full bucket answers `TOO_MANY` now.
  - No deadline (`FOREVER`), since it waits on a person. Freed at once when abandoned.
  - Lost on a server restart (callers get `Dead`). No handles held.
- **Isolation.** sshd runs one `NineServer` and one `Parked` per connection, so a channel's
  `resize` waiters can only be answered by that channel's window changes. Another channel's
  waiter stays parked and learns nothing (consoled.md's rule).

## 4. The shell's driver gets the size and resize events

- **Size.** It is built already: `beamlet:console_size/0` → `Console::size`, fresh each call. Once
  the servers answer, the driver lays out at the real size at each prompt. No change.
- **Resize** goes through the platform, not a per-process call. The VM keeps no call open that
  nobody needs.
  - A new `Platform::console_resized(&mut self) -> Option<(u16, u16)>`, default `None`, is
    polled where console input is (vm.rs, the `beamlet_console` delivery). The console
    subscriber gets `{beamlet_console_resize, {Cols, Rows}}`.
  - On Redoubt the platform starts one **resize thread** when the console first gets a
    subscriber. The thread loops on `Console::resize` (one parked call at the server at a time),
    stores the size and wakes the VM through the hub's `io.wake()`.
  - A dedicated thread, not the typed-call pool, which has 2 threads and would lose one for good.
  - It lives as long as the VM. Its stack is a few pages (to measure; footprint case).
- **The driver** handles `{beamlet_console_resize, {C, R}}`: it resizes `Redoubt.Term`, lays
  the line out again (the page's rule today), and sends `{:resize, C, R}` to the screen in front
  (shell.md's screens already take that message).
- shell.md's planned `Console.await_resize(pid)` is replaced by this. One subscriber already
  owns the console, so a per-pid call would be a second reader of the same event. Its Open on the
  serving library is answered by section 2. The page changes with the code.
- **Host:** no SIGWINCH here; a residual in shell.md. The host's `console_size` is asked afresh
  already, and the host driver tests send the message directly.

## 5. Cases and tests (both widths for bench cases)

Host:
- rt typed parking:
  - `waits` hands the request back, the lend is intact, and it decodes again;
  - serve_call answers a waiting request `malformed`;
  - a waiting request with handles is malformed and its handles closed;
  - `typed::reply` answers a resumed call;
  - admission is released on resume and on abandonment.
- consoled (fake UART):
  - `size` is the argument, 80 x 24 by default;
  - a bad `size=` does not start;
  - `resize` parks, is freed on abandonment, and a third parked call in one bucket is `TOO_MANY`;
  - a read still parks beside it.
- sshd (its host platform):
  - `size` is the pty's;
  - window-change answers the channel's parked `resize`;
  - a second connection's parked `resize` stays parked (isolation);
  - a zero window-change answers nothing.
- client: `console.rs` against the real consoled replaces the in-test server.
- beamlet: the platform's resize thread delivers to the subscriber (fake); the driver test for
  `{beamlet_console_resize, …}` re-lays out and forwards to a screen.

Bench, rv64 and rv32:
- `consol-size`: a test program on consoled started with `size=132,43`. `size` gives 132 x 43.
  `resize` calls with a short client timeout, more times than the bucket's 2, each abandoned and
  freed (the program's 3rd and later calls are not `TOO_MANY`), then a read still answered. The
  verdict is the program's PASS, from values the server returned.
- `steward-ssh-resize`: an SSH session with a pty.
  - `:beamlet.console_size()` prints the pty's size.
  - A bench `resize` step to 100 x 40, then the same call prints `{100, 40}`.
  - The driver got the event: the next prompt is laid out at 100 columns, checked by a line wider
    than 80 drawn unwrapped.
  - A second principal's session, open together, still reads its own size (isolation).
- The shell-cases set (scripts/shell-cases) for the beamlet and driver changes. Whole-bench
  consumers of redoubt_rt are built before any whole bench (runtime contract change).

## Decisions I'd like confirmed

1. sshd's `consol` is in CONS1 (recommended: it is the only resize source, and its page waits on
   it), not a later package.
2. Typed parking as a provided `waits` method plus `typed::serve_parking`/`typed::reply`, and
   `own` returning `Option<Request>`. The alternative, a third `handle` outcome, touches every
   typed server.
3. Resize reaches the shell as a platform-polled message to the console subscriber, through one
   resize thread, replacing shell.md's per-pid `Console.await_resize`.
4. consoled's argument `size=COLS,ROWS` (1 to 1,024).
5. Host SIGWINCH stays out (a residual).
