# ABI2 report (abi2-implementer)

Branch `wp-abi2`, worktree `.worktrees/abi2`, from main 04b23071b. No kernel change.

## Point 3: each ownership rule against the kernel (all hold; nothing to stop on)

- **A lend or transfer is given up at the `call`/`send`, after every check**: `kernel/src/message.rs:696`
  `check_buffer` (aligned, backed, owned, writable for a lend), then labels (R1) and `Busy` (R2), then
  `:726` `take_buffer` -> `lend_out` per page (`:789`). A failure anywhere before `:726` has moved nothing.
- **A touch while given up is a fault**: `lend_out` clears `VALID` and sets `S` (memory.md, "Lending at
  the page-table level"; I9).
- **The server reads and writes a lend until its reply**: `move_buffer` `:1272` maps each frame with
  writable = `m.kind == MsgKind::Call` (`:1284`); the reply returns it (`return_lend`, `:1392`).
- **A transfer becomes the receiver's, owner and payer, at the take**: `move_buffer` `:1292-1294`,
  `drop_lent` from the sender, then `mm.move_frame(phys, spid, rpid)`.
- **A send or call that fails before the take gives the pages back**: `unwind` `:573` (`Wait::Send`, so
  still queued) -> `give_buffer_back` `:1306` -> `lend_back` per page.
- **Abandoned: the lend is the server's alone until its reply frees it**: `unwind` `Wait::Reply` -> `abandon`
  (R3; memory.md: "A lend whose call is abandoned goes the same way to the server").

## The fake's new refusals (`libs/rt/fake/src/lib.rs`)

`State::given_up`: message id -> (giving process, address, length), recorded when a lend or transfer
is queued. It is released when the call settles (on every way out of `message`), at `reply` (the lend is
the caller's again), and at a transfer's take in `receive`. While it is recorded, every call of the giver's
that names a page in it gets `InvalidArgument` (`not_given_up`):
- `call`/`send` naming the range as its lend or transfer (`message`);
- `unmap` (after the mapping check);
- `process_map`'s source;
- a record inside it: a `call`/`send` body, `receive`'s record, and `process_start`'s handle record.
  A `reply` record is not checked (the module doc says so); I couldn't test it cheaply.
After a transfer is taken, the sender's mapping has gone, so the existing `owns`/mapping checks refuse
it from then on. The module doc no longer says "no lend unmapping from the caller". It now says: a
given-up range is refused to every call, and a direct touch is prevented only by the runtime's types.

## Tests (new, `libs/rt/tests/`; `rt-host-tests` runs `redoubt-rt`, not the fake's crate)

- `given_up.rs::a_lent_page_is_refused_to_every_call_until_the_call_returns`: while the call is open,
  seven calls are refused (a second lend, a send of it, `unmap`, `process_map`, a call body in it, a
  receive record in it, a `process_start` record in it). After the call returns, a lend whose body lies
  in it, the receive record, the `process_start` record and `unmap` all pass. Without the change, the
  in-page call queues and hangs, and `unmap` succeeds.
- `given_up.rs::a_transferred_page_is_refused_to_its_sender_once_taken`: after the take, a lend of the
  page, a re-send of it and `unmap` are refused.
- `sleep.rs::sleep_zero_is_one_receive_from_nothing_that_returns_at_once`: see point 6 below.

## Page lines as written

The text is in the commit. `docs/kernel/ipc.md`:
- "Messages": the paragraph is replaced by the brief's exact text (the ownership lead-in, the lend and
  transfer bullets, and the enforcement paragraph). The status now names the two host tests (tested (6)).
- "Messages" figure (not in the brief, but it restated mapping): the note "the lent pages move: unmapped
  from the caller until the reply, mapped in the server at an address the kernel chose" is now "the caller
  gives the lent pages up until the reply; the server reads and writes them at an address the kernel
  chose". "the pages come back to the caller" is now "the pages are the caller's again". The caption is now
  "*Figure: what a call carries. The lent pages have one owner at a time.*"
- "How a call completes": the lend bullet is the brief's text. In the sequence figure (also not in the
  brief), "lend unmapped from client" is now "client gives the lend up", and "lend unmapped from server,
  remapped in client, reply record written" is now "lend the client's again, reply record written".
- R3: the abandoned bullet and the caption are the brief's text.
- `docs/kernel/abi.md`: **unchanged**. The `call` row's "`InvalidArgument` (lend not page-aligned, wraps,
  unmapped, lent, ...)" uses "unmapped" for a page that is not mapped at all, a precondition, not for
  the giving-up, and "lent" already says the page is given up. The `send` row is "as `call`". The record
  check's "the lend is mapped back before the reply is written into it" describes this kernel's
  mechanism.

Docs only, no type change: `redoubt_sys::LendDisposition` and `redoubt_rt::ipc::Buffer` take the brief's
text. `redoubt_rt::handle::sleep` gains one paragraph: `sleep(0)` is the yield, one receive from no
handle with timeout 0, the point where a cooperative backend may switch, and there is no yield call of
its own.

## Point 6: the yield (`sleep(0)`)

On this kernel no other thread runs first. `receive` from no handle -> `mark` + `settle`
(`kernel/src/message.rs:889-892`). `settle` sees the deadline already past and fails it with `Timeout`
without blocking (`:518-522`), returning `Ok(None)`. `redoubt::handle` maps that to `Outcome::Resume`
(`kernel/src/redoubt.rs:52`), and `system_call` calls `resume_current()` (`kernel/src/arch/riscv/irq.rs:83`,
`:53-57`), which resumes the current thread, the caller. `sched::pick` (`kernel/src/sched.rs:363`) is
called only from `kmain` (`kernel/src/main.rs:131`), never on this path. A timer preemption can still
come at any instruction, as always, but not because of this entry.

timer.md, "Timeouts and `FOREVER`", after "... is exactly that.":
> `sleep(0)` is the runtime's yield: it enters the kernel and returns at once. This kernel runs no
> other thread first: a deadline already past times out without blocking, and the caller resumes
> without the scheduler picking again. A backend that switches threads cooperatively switches there.

The same text is on `sleep`'s doc, with a pointer to timer.md. The status now names
`host:redoubt-rt::sleep_zero_is_one_receive_from_nothing_that_returns_at_once` (tested (5)). That test
(`libs/rt/tests/sleep.rs`) uses a recording transport that forwards to the fake. It asserts `Ok(())`, a
return under 5 s (a blocked sleep would hit the fake's 60 s STUCK panic), and exactly one call,
`Receive { from: None, timeout: 0, max_transfer: 0, .. }`. No alias and no new call.

## Gates

(filled below)

All gates were run as `/home/mcloonan/redoubt/.wash/local/in-dev cargo testbench <filter>` from the worktree:
- `host-tests` 0 (16 cases PASS: blkd, client, fsd, host-tests, init, ipd, littlefs, model, net, netd, r4,
  rt, sshd, steward, stride, wire)
- `formatting` 0
- `docs` first 1: C5, ipc.md:51 cited I9 without its name. The link text is now "I9 (pages W^X, zeroed,
  lends unmapped)", and the rerun is 0.
- `unsafe-budget` 0 (redoubt-rt 11 unsafe, 0 undocumented, unchanged; the new unsafe is a test-only
  `unsafe impl Transport` in libs/rt/tests/sleep.rs, outside the on-target budget)
- `size-budget` 0
- `rt-miri` 0 (runs the new tests under Miri)
- `rt-build` 0, rv64 and rv32

No libs/rt or libs/sys contract change (docs only; the fake is dev-only), so the consumer sweep is not
needed. The formatting and docs passes ran after the point 6 edits. The whole bench is not run: it is
the orchestrator's. No QEMU was run.

## Branch

wp-abi2: 4a31e342c fake, 87596fe11 ipc (pages + type docs), 6239e973f rt sleep(0); tip 6239e973f.

## Open risks / notes

- The new tests live in `libs/rt/tests/` (the `redoubt-rt` crate), not `libs/rt/fake/**`, because
  `rt-host-tests` runs redoubt-rt, redoubt-sys and stub, not the fake's crate.
- The fake checks no `reply` record against given-up pages.
- The fake's pre-existing imprecision is unchanged: a call taken and then hit by endpoint destruction
  reports `Returned` there, where the kernel says `consumed`.

## Fold (editor, simplifier, red, Architect): tip after the fold

The branch was rebuilt as three commits on 04b23071b: dd2628956 fake, 6085d8b42 ipc/abi docs + type docs,
10e8a5f38 sleep(0). Earlier sections above are superseded where they differ.

- Editor: abi.md:194 now reads "the lend is the caller's again before the reply is written into it".
  timer.md's `sleep(0)` paragraph and `sleep`'s doc are now word for word the same ("Redoubt's kernel",
  no cross-reference in either). The fake's and the tests' citations are now `docs/kernel/...`.
- Red (1): `returned` now reads "as before the call" in ipc.md "Messages", the "How a call completes"
  bullet, and `LendDisposition`'s doc. The fake and the kernel (redoubt.rs:56-66) report Returned for a
  call refused at its arguments, and "the caller again" was false there.
- Red (2): an endpoint destroyed with the call taken now goes to the new `abandon()` helper (shared with
  the timeout path). The caller gets Dead with the lend Consumed, the lend moves to the server, and no
  notice follows (R3). An abandoned call returns past the settle-release, so its given-up record stays
  until the server's reply, which releases it. That is the simplifier's point too; red's ruling decided it.
- Red (3): the `reply` record is checked (`not_given_up`), and the module-doc exemption is gone. The
  module doc's release points now read "until the reply, or the take; a message that fails while still
  queued gives it back".
- Simplifier: `not_given_up` uses `checked_add` on both ranges, and an overflowing end counts as an
  overlap. The abandonment-by-timeout test is added. One line: after abandonment the caller's calls were
  already refused because the mapping moved to the server (lib.rs `abandon`, formerly :867-868). The
  record is now kept anyway, so the refusal no longer rests on that.
- Red (4), new tests (named in ipc.md Messages, now tested (9)):
  `queued_pages_are_refused_until_the_message_times_out_and_gives_them_back`. A lending call, then a
  transferring send, are each queued where nobody receives. While queued, `unmap`, a re-lend and a
  `reply` whose record lies in the page are refused; the page is still the sender's mapping, so the
  refusal comes from the given-up record. Each times out (call: Timeout + Returned; send: Timeout). Then
  the reply and the `unmap` pass. The test waits on the new `Fake::queued`.
  `a_destroyed_endpoint_abandons_a_taken_lend_to_the_server_until_it_replies` gets Dead + consumed, the
  caller's `unmap` and re-lend refused, and the server's reply discarded and freeing the lend. It fails
  before the fix, because the fake reported Returned.
  `an_abandoned_lend_is_refused_to_its_caller_until_the_server_replies` (the simplifier's timeout case).
- Red (5): a thread already blocked in `receive`, whose record then falls inside pages its process
  gives up, is not caught by the fake (the check is at entry). That is unreachable from safe rt: rt's
  records are stack `Record`s, never inside a `Buffer`.

### Gates on 10e8a5f38 (in-dev cargo testbench <filter>)
host-tests 0 (16 PASS), formatting 0, docs 0, unsafe-budget 0 (redoubt-rt 11, unchanged), size-budget 0,
rt-miri 0, rt-build 0 (rv64, rv32). No QEMU; the whole bench is the orchestrator's.
