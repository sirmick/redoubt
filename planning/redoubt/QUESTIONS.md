# Open questions for the owner (milestone 1 build)

Raised by the wave 1 packages and their reviews (2026-09-19). Each touches the frozen design (v4),
so each needs your decision; the answer goes into the named note with a HISTORY.md entry.
**Rec** is the orchestrator's recommendation. Reply with numbers, e.g. "all Rec except 7: ...".

Blocking: **K1 cannot start until 1-16 are settled** (they fix the ABI, `redoubt-sys`).
17-26 block later packages only.

## Kernel: messages and IPC (KERNEL-SPEC.md)

1. **Is a reply owed?** A received message doesn't say whether it came by `call` (the sender is
   blocked, so a reply is owed) or by `send`. It also doesn't say whether its buffer is a lend or a
   transfer.
   *Rec:* `receive` returns the kind: `call` (reply owed, optional lend) or `send` (no reply,
   optional transfer). `reply` to a send's message id gets `InvalidArgument`.
   *Alt:* word 0's opcode implies it (WIRE.md). A server that gets it wrong strands a caller.

2. **Receiving while serving.** Can a thread `receive` again while it still owes a reply? I5's
   bound of `MAX_LEND_PAGES` per thread implies one call per thread; the model returns `Busy`.
   *Rec:* yes, one call per thread and `Busy` otherwise. The consequence: a server that holds many
   calls open (a blocking read on `/dev/cons`) needs a thread per open call, up to `MAX_THREADS`.

3. **Message ids.** *Rec:* message ids are non-zero and never reused, like budget ids, so a stale
   `reply` or `mint` can't hit a later message.

4. **Which receiver R1 checks.** Threads from different budgets can wait on one endpoint, and the
   label check then depends on which thread takes the message.
   *Rec:* R1 compares against the budget that owns the endpoint (its creator's), not the receiving
   thread's. That makes the check deterministic.

5. **Transfers larger than the receiver can hold.** R4 covers the opt-in (`max_transfer`), but
   not a transfer that fits `max_transfer` and exceeds the receiver's free pages.
   *Rec:* the sender gets `Refused` and the kernel moves on, as in R4 (the model does this). It
   tells the sender one bit about the receiver's budget, and only a sender the receiver chose to
   accept transfers from.

6. **A server dies with calls queued.** INIT.md says blocked senders get `Dead`; KERNEL-SPEC.md
   says the endpoint survives its receivers.
   *Rec:* follow KERNEL-SPEC.md. Queued senders wait for the restarted server; calls the dead
   server had already taken fail with `Dead` (R3 still applies to their lends). Fix INIT.md's
   wording.

7. **Exit notices whose payer is gone.** When a budget is destroyed, the notices for the
   processes it killed are owed to creators elsewhere, and nothing pays for them. Pending notices
   can grow without bound.
   *Rec:* the exit slot is charged to the creator when it calls `process_create`: one slot per
   process, already paid, so the notice never allocates.

8. **System class and exit notices, `budget_usage`.** R1 exempts system-class receivers from the
   message label check, but exit notices and `budget_usage` need receiver ⊇ target labels. As
   written, `init` and the steward never see a labelled agent exit.
   *Rec:* the same exemption: a system-class receiver or caller skips the ⊇ check.

9. **System-class children.** A user-class process holding a handle to a system budget can
   create system-class children; adding labels, by contrast, checks the caller's class.
   *Rec:* a system-class child also needs the caller's own budget to be system (`ClassDenied`).

## Kernel: objects, arguments, errors

10. **Handle slot 0 and the start list.** The spec copies handles into "slots 1..n", never says
    what slot 0 is, and doesn't cap the list.
    *Rec:* index 0 is never allocated and means "no handle" (the model already treats 0 as
    invalid). New constant `MAX_START_HANDLES` = 64; a longer list gets `TooLarge`.

11. **`budget_usage` counters.** "-> counters" is undefined.
    *Rec:* page limit and usage, process limit and usage, and weight limit and carved weight. R7
    carves weight, so the steward needs the free weight.

12. **Weight 0.** R12 divides by weight, and revocation scopes have weight 0.
    *Rec:* a budget with weight 0 can't hold a process (`process_create` gets `InvalidArgument`).

13. **What objects cost in pages.** This is unspecified, page tables included, so `budget_usage`
    and `OutOfMemory` can't be compared between the model and the kernel.
    *Rec:* a cost table in KERNEL-SPEC.md, one page per object, with page tables charged per
    page-table page as allocated:
    - budget, process, thread and endpoint: 1 page each;
    - handle table: 1 page per 256 handles (the orchestrator's guess; the kernel implementer
      confirms the figure).

    The model already takes the costs as parameters.

14. **Errors and the order of checks.** Most checks don't name their error or their order, and
    WP-C1's replay compares exact errors.
    *Rec:* adopt the model's table (`redoubt/model/README.md`, branch `wp-m0`) as normative, by
    reference from KERNEL-SPEC.md. Decoding errors come first: `BadHandle` for a malformed handle,
    `TooLarge` for an over-long list, `InvalidArgument` for anything else.

15. **Small ABI clarifications.** *Rec, all:*
    - records passed by address must be 8-byte aligned, otherwise `InvalidArgument`;
    - a budget deadline is absolute µs since boot, and `FOREVER` as a deadline means none;
    - relative timeouts are added with saturation;
    - the ABI decoder already refuses badge 0 and W+X (the kernel checks again);
    - `random` is capped by a named constant, `MAX_RANDOM` = 64.

16. **ABI layout (FYI; the orchestrator decided it in review).** A `u64` argument always takes two
    32-bit register halves, on both widths. This removes every width cfg from `redoubt-sys`, at a
    small register cost on rv64. Say so if you object.

## Containment (CONTAINMENT.md, CAPABILITIES.md)

17. **Per-account caps are a covert channel out of a vault.** A vault session and its owner's
    unlabelled session share one account, so they share the steward's pending-request cap and the
    kernel's `WAIT_CAP` (R2) on system endpoints. Filling one shows up as `Busy` on the other. The
    10^6 model run showed the effect reaching the owner's budget usage too.
    *Rec:* key both caps, and R2's round-robin, by (account, label set), not by account alone.

18. **Steward ids.** The model found that sequential session ids leak how many sessions other
    principals start.
    *Rec:* CONTAINMENT.md says every id the steward hands out is unpredictable (keyed random), not
    only request ids.

## Wire formats (WIRE.md)

19. **No typed-message tables exist.** None for blkd↔fsd, the steward, keyd, sshd↔steward, or
    ipd's connect and listen. WP-W1 built the generator and a table format, marked with
    `<!-- wire: NAME -->`, and the generated code fails its test when it drifts from the notes.
    *Rec:* WIRE.md adopts that format, and each server's package writes its own table into its
    note as part of the package, with a HISTORY.md line. So D1, D2, D3, S1, S2 and S3 each carry
    a small design addition.

20. **Replies, and which requests need a buffer.** WIRE.md doesn't define a reply's layout. It
    also decides inline versus buffer from the request's fields alone. But `reply` carries only
    words and handles, so reply data can travel only in the caller's lend. Take a blkd read: the
    request is 12 bytes and goes inline, and the inline codec refuses the lend the data must come
    back in.
    *Rec:*
    - A table has a `Reply` column.
    - A message is buffer-shaped if its request or its reply needs a buffer.
    - Word 0 of a reply is a status: 0 = ok, otherwise a code from the protocol's error table.
      That table is marked `<!-- wire-errors: NAME -->`.
    - An inline reply's fields go in words 1-3; a buffer reply is written into the lend, with
      its length in word 1.

    The W1 editor's full WIRE.md draft is ready to apply.

21. **Packing (W1's choices).** *Rec, accept:*
    - inline fields pack 4 bytes per word into words 1-3 (12 bytes, the same on both widths);
    - a buffer holds the fields only, with the opcode staying in word 0;
    - inline or buffer is fixed per message type.

22. **Compound fields** (label sets, IP prefixes). *Rec:* no new types for milestone 1; use a
    `bytes` field with its inner layout stated in the table.

23. **JSON integers.** A small integer may be written as a number or as a string; INIT.md's own
    example writes `"4096"`. *Rec:* accept both. Only values above 2^53 must be strings.

## Tooling and tenets

24. **C in host-only test tooling.** libFuzzer (C++, built with `cc`) is used for fuzzing, and the
    littlefs C reference for differential tests. Both are in separate crates, excluded from the
    workspace, and never linked into anything that runs on the machine. Tenet 3 says "no C
    toolchain in the build".
    *Rec:* amend tenet 3 to allow host-only test oracles and fuzz drivers outside the workspace
    build.

25. **littlefs scope.** No wear levelling (virtio disks do their own), no superblock expansion,
    and an attribute and file data are two commits, not one.
    *Rec:* accept for milestone 1. WP-D2 asks for the atomic attribute commit if it needs one.

26. **Attack tests must not trust the attacker's output (FYI, a plan change).** The console
    doesn't say which process printed a line, so a hostile program can print its own PASSED line.
    *Rec:* BUILD-PLAN.md WP-E1 states that attack success is asserted by the system (kernel,
    victim, or a clean power-off), never by the attacker's own output.

## Added later

27. **Typed operations written into a 9P file.** NAMESPACES.md carries `ipd`'s connect, listen
    and close as writes to `/net/tcp/N/ctl`. File contents have no words, so there is nowhere to
    put an opcode.
    *Rec:* the file's contents are the opcode as a `u32` followed by the buffer-shape encoding,
    one operation per `Twrite`. WIRE.md gets one sentence saying so.
