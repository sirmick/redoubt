# ABI2: lends and transfers defined by ownership, not mapping

Tier A (the IPC pages, the fake kernel), size S. Needs ABI1 merged (they share the fake and
`libs/sys`'s docs); start from main after it. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The owner (2026-10-03): "Define page lend and transfer by ownership, not mapping. Have the ABI doc
state the semantics as ownership rules (lend means the callee may read or write until reply,
transfer means ownership moves) so a no-MMU backend can implement them as a buffer handoff
enforced by Rust ownership."

## Context rules (read these first)

- **Don't read whole files.** In `kernel/src/message.rs` read only `check_buffer`, `take_buffer`,
  `move_buffer` and `give_buffer_back` (a queued message's buffer returned to its sender);
  in `libs/rt/fake/src/lib.rs` only `message`, the `Call::Call`/`Call::Send` arms and `abandoned`.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/ABI2-report.md`.

## Reading list (only these)

- `docs/kernel/ipc.md`: "Messages", "How a call completes" (both tables), R3 and its figure.
- `docs/kernel/memory.md`: "Lending at the page-table level" (this kernel's implementation).
- `docs/kernel/invariants.md`: I9.

## What exists

ipc.md states lends and transfers as mappings: a lend "leaves the caller's address space while the
call is open", "the server sees them at an address the kernel picks"; R3's abandoned lend is "the
server's alone, still mapped there"; the figure says "the lend is mapped in exactly one address
space in every state". The kernel already holds the ownership rule below: `take_buffer` removes a
lend or transfer from the sender at `call`/`send` (a touch is a fault), a failure before a take
gives it back, the server's mapping is read-write until its reply, and a transfer changes owner
and payer at the take. The fake kernel does not: "no lend unmapping from the caller".

## The settled design: the rule is ownership, the page tables enforce it here

1. **The rule, on ipc.md** ("Messages"; exact text below): a lend's pages are given up by the caller
   for the whole `call`; a server that takes the call may read and write them until it replies;
   the call's return says who owns them (`returned`: the caller; `consumed`: the server, until its
   reply frees them). A transfer's pages are given up at the `send` and become the receiver's,
   owner and payer, when a receiver takes the message; a `send` that fails before that gives them
   back. Every state has exactly one owner, and only the owner touches the pages.
2. **This kernel's enforcement** is stated once, as the implementation: giving pages up removes
   them from the giver's address space (a touch is a fault, I9), taking them maps them in the
   taker's. A backend without an MMU may hand the same buffer over instead, provided every rule
   in point 1 holds; in safe Rust the runtime's types already make it so (`Endpoint::call` and
   `send` take the `Buffer` by value).
3. **No kernel change.** Verify each rule of point 1 against the code above; if one is false (for
   example, a failed `send` that does not give its transfer back), stop and report it, with the
   line, before writing the page.
4. **The fake kernel enforces what it can.** It records each range a process has given up and
   refuses, as the kernel does, every call that names one of those pages: a second lend or
   transfer, `unmap`, `process_map`, and a record inside one (`InvalidArgument`, abi.md's record
   check); a reply or abandonment releases the record. Direct loads and stores in one host
   process cannot be caught: say so in its module doc ("no lend unmapping from the caller"
   becomes "a given-up range is refused to every call; a direct touch is prevented only by the
   runtime's types").
5. **The types say ownership.** `redoubt_sys::LendDisposition` gains a doc: "Who owns a call's
   lent pages once it returns: `None`, there was no lend; `Returned`, the caller again;
   `Consumed`, the server, until its reply frees them: never touch them again." `redoubt_rt`'s
   `Buffer` doc: "Pages this process owns, readable and writable: what it lends in a `call`,
   transfers in a `send`, or was transferred. Lending or transferring takes it by value, so no safe
   code touches pages it has given up. Unmapped on drop." Change no type or signature.
6. **Yield is `sleep(0)`** (the owner, 2026-10-03: no `yield_now` alias). It is the no-MMU seam's
   yield point, and stated, not built: a receive from nothing with a timeout of 0, which returns
   `Ok` at once.
   - On this kernel, state from the code (the kernel's exit path and `sched::pick`) whether the
     entry lets another thread run first, and write what you find, citing the line.
   - A cooperative backend switches threads there.
   - **Host test** in `libs/rt` on the fake: `sleep(0)` returns `Ok` without blocking.
7. **Unchanged:** the ABI's encoding, every call's behaviour, the model, I9 (this kernel's
   invariant keeps its name: it is the enforcement), memory.md's "Lending at the page-table level".

## The cases

1. **Host tests on the fake:** a lend of a range the caller already lent, an `unmap` of it, and a
   call whose record lies in it are each refused while the call is open and accepted after its
   return; a transfer's range is refused to its sender after the take. Name them under ipc.md's
   "Messages" status.
2. **Every host test and every machine case unchanged:** `rt-host-tests`, the client's and the
   servers' host tests, the model's, and the whole bench on both widths.

## Page lines (exact text in the report)

- **ipc.md, "Messages"**: the paragraph "A **lend** is up to `MAX_LEND_PAGES` … at least that
  large." becomes:
  > A message's pages move by **ownership**: every page has one owner at a time, and only its
  > owner touches it.
  > - A **lend** is up to `MAX_LEND_PAGES` (16 pages, 64 KiB) of pages the caller owns and may
  >   write. From the `call` until it returns, the caller has given them up: it may not read,
  >   write, lend, transfer, map or unmap them. A server that takes the call may read and write
  >   them, at an address the kernel picks, until it replies. The call's return says who owns
  >   them: `returned`, the caller again; `consumed`, the server, whose reply frees them
  >   ([R3](#r3-lends-and-abandoned-calls)).
  > - A **transfer** is pages the sender owns, given away for good: the sender gives them up at
  >   the `send`, and when a receiver takes the message they become the receiver's, owner and
  >   payer both. A `send` that fails before a receiver takes it gives them back. A receiver takes
  >   a transfer only if its `receive` named a `max_transfer` at least that large.
  >
  > On this kernel giving pages up removes them from the giver's address space, so a touch is a
  > fault ([I9](invariants.md#i9-pages-wx-zeroed-lends-unmapped)), and taking them maps them in
  > the taker's ([memory](memory.md#lending-at-the-page-table-level)). A backend without an MMU
  > may hand the same buffer over instead, as long as every rule above holds.
- **ipc.md, "How a call completes"**: the lend bullet becomes "the **lend disposition**, who owns
  the lent pages now: `none` (no lend), `returned` (the caller) or `consumed` (the server, until its
  reply frees them; never touch them again);".
- **ipc.md, R3**: "the caller's charge ends and the lend becomes the server's alone, still mapped
  there;" becomes "the caller's charge ends and the lend becomes the server's alone: it may still
  read and write it until its reply;". The figure's caption becomes "*Figure: the life of a call.
  The lend has exactly one owner in every state.*"
- **timer.md**, after "The Rust runtime's `sleep` (`libs/rt/src/handle.rs`) is exactly that.":
  "`sleep(0)` is the runtime's yield: it enters the kernel and returns at once. [What point 6
  finds this kernel does at that entry.] A backend that switches threads cooperatively switches
  there." The same sentence goes on `sleep`'s doc comment. List the host test under timer.md's
  status.
- **abi.md**, `call`'s and `send`'s argument rows (if they say "mapped" or "unmapped"): state the
  giving-up in ownership words, pointing to ipc.md's "Messages". Quote any line you change.

Use the anchors as doccheck accepts them.

## Owned paths

- `docs/kernel/ipc.md`, `docs/kernel/abi.md` (those rows), `libs/sys/src/ret.rs` (the doc),
  `libs/rt/src/ipc.rs` (`Buffer`'s doc), `libs/rt/src/handle.rs` (`sleep`'s doc and its host test),
  `docs/kernel/timer.md` (that paragraph), `libs/rt/fake/**` (the given-up ranges, its module doc,
  its tests).

**Not yours:** the kernel (point 3: report, do not change), the model, memory.md, invariants.md.

## Gates

- The whole bench on both widths, alone.
- `rt-host-tests`, the client library's and every server's host tests, the model's tests.
- `cargo fmt --check`, the size and unsafe budgets, doccheck.

Report each command with its exit code, each rule of point 1 with the code line that holds it,
the fake's new refusals, and each page line as written.
