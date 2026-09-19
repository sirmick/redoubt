# Open questions for the owner (milestone 1 build)

Raised by the wave 1 packages and their reviews (2026-09-19). Each touches the frozen design (v4),
so each needs your decision; the answer goes into the named note with a HISTORY.md entry.
**Rec** is the orchestrator's recommendation. Reply with numbers, e.g. "all Rec except 7: ...".

**1-55 answered 2026-09-19** (ANSWERS.md, in two tranches; each "Answered" line says
where the answer now lives). No question is open.

Blocking: **K1 cannot start until 1-16 are settled** (they fix the ABI, `redoubt-sys`).
17-26 block later packages only.

## Kernel: messages and IPC (KERNEL-SPEC.md)

1. **Is a reply owed?** A received message doesn't say whether it came by `call` (the sender is
   blocked, so a reply is owed) or by `send`. It also doesn't say whether its buffer is a lend or a
   transfer.
   *Rec:* `receive` returns the kind: `call` (reply owed, optional lend) or `send` (no reply,
   optional transfer). `reply` to a send's message id gets `InvalidArgument`.
   *Alt:* word 0's opcode implies it (WIRE.md). A server that gets it wrong strands a caller.
   **Answered:** KERNEL-SPEC.md, Messages (`receive` reports `call` or `send`) and R4a (`reply` to a
   send is `InvalidArgument`).

2. **Receiving while serving.** Can a thread `receive` again while it still owes a reply? I5's
   bound of `MAX_LEND_PAGES` per thread implies one call per thread; the model returns `Busy`.
   *Rec:* yes, one call per thread and `Busy` otherwise. The consequence: a server that holds many
   calls open (a blocking read on `/dev/cons`) needs a thread per open call, up to `MAX_THREADS`.
   **Answered:** changed: several open calls per thread, `MAX_OPEN_CALLS` = 64 per process, a page
   each, `Busy` beyond, I5 per open call; KERNEL-SPEC.md (Process, R4a, I5).

3. **Message ids.** *Rec:* message ids are non-zero and never reused, like budget ids, so a stale
   `reply` or `mint` can't hit a later message.
   **Answered:** KERNEL-SPEC.md, Messages; I12.

4. **Which receiver R1 checks.** Threads from different budgets can wait on one endpoint, and the
   label check then depends on which thread takes the message.
   *Rec:* R1 compares against the budget that owns the endpoint (its creator's), not the receiving
   thread's. That makes the check deterministic.
   **Answered:** KERNEL-SPEC.md, R1 (the endpoint's owner budget); Endpoint.

5. **Transfers larger than the receiver can hold.** R4 covers the opt-in (`max_transfer`), but
   not a transfer that fits `max_transfer` and exceeds the receiver's free pages.
   *Rec:* the sender gets `Refused` and the kernel moves on, as in R4 (the model does this). It
   tells the sender one bit about the receiver's budget, and only a sender the receiver chose to
   accept transfers from.
   **Answered:** KERNEL-SPEC.md, R4.

6. **A server dies with calls queued.** INIT.md says blocked senders get `Dead`; KERNEL-SPEC.md
   says the endpoint survives its receivers.
   *Rec:* follow KERNEL-SPEC.md. Queued senders wait for the restarted server; calls the dead
   server had already taken fail with `Dead` (R3 still applies to their lends). Fix INIT.md's
   wording.
   **Answered:** KERNEL-SPEC.md, R4b; INIT.md, Restarts and reboots.

7. **Exit notices whose payer is gone.** When a budget is destroyed, the notices for the
   processes it killed are owed to creators elsewhere, and nothing pays for them. Pending notices
   can grow without bound.
   *Rec:* the exit slot is charged to the creator when it calls `process_create`: one slot per
   process, already paid, so the notice never allocates.
   **Answered:** KERNEL-SPEC.md, Process and the cost table (one page, charged to the creator).

8. **System class and exit notices, `budget_usage`.** R1 exempts system-class receivers from the
   message label check, but exit notices and `budget_usage` need receiver ⊇ target labels. As
   written, `init` and the steward never see a labelled agent exit.
   *Rec:* the same exemption: a system-class receiver or caller skips the ⊇ check.
   **Answered:** KERNEL-SPEC.md, R1, `budget_usage`, I7; CONTAINMENT.md, Labels.

9. **System-class children.** A user-class process holding a handle to a system budget can
   create system-class children; adding labels, by contrast, checks the caller's class.
   *Rec:* a system-class child also needs the caller's own budget to be system (`ClassDenied`).
   **Answered:** KERNEL-SPEC.md, `budget_create` class and labels; I8.

## Kernel: objects, arguments, errors

10. **Handle slot 0 and the start list.** The spec copies handles into "slots 1..n", never says
    what slot 0 is, and doesn't cap the list.
    *Rec:* index 0 is never allocated and means "no handle" (the model already treats 0 as
    invalid). New constant `MAX_START_HANDLES` = 64; a longer list gets `TooLarge`.
    **Answered:** KERNEL-SPEC.md, Handle, Constants, the error table; INIT.md, Startup block.

11. **`budget_usage` counters.** "-> counters" is undefined.
    *Rec:* page limit and usage, process limit and usage, and weight limit and carved weight. R7
    carves weight, so the steward needs the free weight.
    **Answered:** KERNEL-SPEC.md, `budget_usage` counters.

12. **Weight 0.** R12 divides by weight, and revocation scopes have weight 0.
    *Rec:* a budget with weight 0 can't hold a process (`process_create` gets `InvalidArgument`).
    **Answered:** KERNEL-SPEC.md, Budget, R12, the error table (`process_create`).

13. **What objects cost in pages.** This is unspecified, page tables included, so `budget_usage`
    and `OutOfMemory` can't be compared between the model and the kernel.
    *Rec:* a cost table in KERNEL-SPEC.md, one page per object, with page tables charged per
    page-table page as allocated:
    - budget, process, thread and endpoint: 1 page each;
    - handle table: 1 page per 256 handles (the orchestrator's guess; the kernel implementer
      confirms the figure).

    The model already takes the costs as parameters.
    **Answered:** changed: 128 handles per page, WP-K1 confirms; the open-call and exit-slot pages
    added; KERNEL-SPEC.md, What objects cost.

14. **Errors and the order of checks.** Most checks don't name their error or their order, and
    WP-C1's replay compares exact errors.
    *Rec:* adopt the model's table (`redoubt/model/README.md`, branch `wp-m0`) as normative, by
    reference from KERNEL-SPEC.md. Decoding errors come first: `BadHandle` for a malformed handle,
    `TooLarge` for an over-long list, `InvalidArgument` for anything else.
    **Answered:** changed: the table and order copied into KERNEL-SPEC.md, Errors and the order of
    checks; the spec is normative and the model conforms.

15. **Small ABI clarifications.** *Rec, all:*
    - records passed by address must be 8-byte aligned, otherwise `InvalidArgument`;
    - a budget deadline is absolute µs since boot, and `FOREVER` as a deadline means none;
    - relative timeouts are added with saturation;
    - the ABI decoder already refuses badge 0 and W+X (the kernel checks again);
    - `random` is capped by a named constant, `MAX_RANDOM` = 64.
    **Answered:** KERNEL-SPEC.md, Constants (`MAX_RANDOM`, time and deadlines), ABI, the error
    table.

16. **ABI layout (FYI; the orchestrator decided it in review).** A `u64` argument always takes two
    32-bit register halves, on both widths. This removes every width cfg from `redoubt-sys`, at a
    small register cost on rv64. Say so if you object.
    **Answered:** KERNEL-SPEC.md, ABI; MEMORY-LAYOUT.md.

## Containment (CONTAINMENT.md, CAPABILITIES.md)

17. **Per-account caps are a covert channel out of a vault.** A vault session and its owner's
    unlabelled session share one account, so they share the steward's pending-request cap and the
    kernel's `WAIT_CAP` (R2) on system endpoints. Filling one shows up as `Busy` on the other. The
    10^6 model run showed the effect reaching the owner's budget usage too.
    *Rec:* key both caps, and R2's round-robin, by (account, label set), not by account alone.
    **Answered:** KERNEL-SPEC.md, R2, `WAIT_CAP`, I11; CONTAINMENT.md; CAPABILITIES.md (all by
    (account, label set)).

18. **Steward ids.** The model found that sequential session ids leak how many sessions other
    principals start.
    *Rec:* CONTAINMENT.md says every id the steward hands out is unpredictable (keyed random), not
    only request ids.
    **Answered:** CONTAINMENT.md, the shared server library (steward).

## Wire formats (WIRE.md)

19. **No typed-message tables exist.** None for blkd↔fsd, the steward, keyd, sshd↔steward, or
    ipd's connect and listen. WP-W1 built the generator and a table format, marked with
    `<!-- wire: NAME -->`, and the generated code fails its test when it drifts from the notes.
    *Rec:* WIRE.md adopts that format, and each server's package writes its own table into its
    note as part of the package, with a HISTORY.md line. So D1, D2, D3, S1, S2 and S3 each carry
    a small design addition.
    **Answered:** WIRE.md, Tables; BUILD-PLAN.md, Settled before the build.

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
    **Answered:** WIRE.md, Tables and Layout in a message.

21. **Packing (W1's choices).** *Rec, accept:*
    - inline fields pack 4 bytes per word into words 1-3 (12 bytes, the same on both widths);
    - a buffer holds the fields only, with the opcode staying in word 0;
    - inline or buffer is fixed per message type.
    **Answered:** WIRE.md, Layout in a message.

22. **Compound fields** (label sets, IP prefixes). *Rec:* no new types for milestone 1; use a
    `bytes` field with its inner layout stated in the table.
    **Answered:** WIRE.md, Tables.

23. **JSON integers.** A small integer may be written as a number or as a string; INIT.md's own
    example writes `"4096"`. *Rec:* accept both. Only values above 2^53 must be strings.
    **Answered:** changed: no "either"; the schema fixes each field's JSON type (64-bit quantities
    strings, small counts numbers); WIRE.md, strict JSON; INIT.md, the boot manifest (example
    fixed).

## Tooling and tenets

24. **C in host-only test tooling.** libFuzzer (C++, built with `cc`) is used for fuzzing, and the
    littlefs C reference for differential tests. Both are in separate crates, excluded from the
    workspace, and never linked into anything that runs on the machine. Tenet 3 says "no C
    toolchain in the build".
    *Rec:* amend tenet 3 to allow host-only test oracles and fuzz drivers outside the workspace
    build.
    **Answered:** TENETS.md, tenet 3.

25. **littlefs scope.** No wear levelling (virtio disks do their own), no superblock expansion,
    and an attribute and file data are two commits, not one.
    *Rec:* accept for milestone 1. WP-D2 asks for the atomic attribute commit if it needs one.
    **Answered:** NAMESPACES.md, littlefs.

26. **Attack tests must not trust the attacker's output (FYI, a plan change).** The console
    doesn't say which process printed a line, so a hostile program can print its own PASSED line.
    *Rec:* BUILD-PLAN.md WP-E1 states that attack success is asserted by the system (kernel,
    victim, or a clean power-off), never by the attacker's own output.
    **Answered:** BUILD-PLAN.md, How to read a work package (every attack case); PLAN.md, attack
    suite.

## Added later

27. **Typed operations written into a 9P file.** NAMESPACES.md carries `ipd`'s connect, listen
    and close as writes to `/net/tcp/N/ctl`. File contents have no words, so there is nowhere to
    put an opcode.
    *Rec:* the file's contents are the opcode as a `u32` followed by the buffer-shape encoding,
    one operation per `Twrite`. WIRE.md gets one sentence saying so.
    **Answered:** WIRE.md, Layout in a message.

28. **Handle kinds in typed-message tables.** A table's `handle[N]` says that a handle is in slot
    N, but not what kind of object it must be. Every receiver has to check the kind itself, and
    nothing tells it which kind to expect.
    *Rec:* the table names the kind, e.g. `range: handle[0] endpoint`. The generator puts the kind
    in the docs, and a generated helper checks it.
    **Answered:** WIRE.md, Tables (`handle[N] KIND`).

29. **Names in the boot manifest.** The parser correctly accepts any JSON string, including an
    empty one, or one with NUL, U+FEFF or C1 controls. Manifest names become endpoint names,
    volume names and 9P paths.
    *Rec:* INIT.md adds a rule for names: 1-64 bytes of `[a-z0-9_:+-]`, starting with a letter.
    That covers `fsd:data` and `alice+secrets`, and the manifest decoder enforces it.
    Relatedly, WIRE.md should say that member names are compared byte for byte, with no Unicode
    normalisation, and that opcode 0 is reserved (it is the reply status "ok", question 20).
    *Partly applied (2026-09-19):* the two WIRE.md sentences went in with answer 20; the INIT.md
    name rule is still open.
    **Answered:** INIT.md, The boot manifest (Names); the WIRE.md sentences as above.

## From the model's red team

30. **Revocation doesn't reach messages already in flight.** R10 closes handles in every table, but
    a message queued or taken through a handle stamped with the destroyed budget is still
    delivered, with its badge. The server's reply, including new handles, still reaches the
    sender.
    *Rec:* R10 also fails queued messages whose stamp is destroyed, with `Dead`, and discards
    replies to taken calls whose stamp is destroyed (the handles are dropped).
    **Answered:** KERNEL-SPEC.md, R10 (the taken call's caller gets `Dead` at once; its lend as in
    R3).

31. **Blame outlives a `send`.** The served account is cleared only by `reply`. A server thread
    that took Bob's `send` and faults an hour later, idle, blames Bob.
    *Rec:* the served account is set only while a `call` is open. A `send` never sets it (it
    can't be replied to, question 1).
    *Also, after answer 2:* a thread may now hold several open calls, so "the account the thread
    is serving" must say which one. KERNEL-SPEC.md still says: set by each delivered message,
    cleared by `reply`. The same word, "serving", decides which message ids `mint` accepts.
    **Answered:** KERNEL-SPEC.md, Process (open calls and the serving account; a `send` is never
    open) and `mint`.

32. **What counts as a blocked sender for `WAIT_CAP` (R2)?** Only queued messages, or also callers
    whose message was taken and who now wait for the reply?
    *Rec:* only queued messages. A taken call is bounded by the server's threads (question 2).
    **Answered:** KERNEL-SPEC.md, R2 and Constants (`WAIT_CAP`).

33. **Leases are chosen by the requester and unbounded.** A powerbox request can ask for
    `lease = u64::MAX`, which saturates to `FOREVER`, meaning no deadline. An agent can also
    start sub-agents as siblings with longer leases, which outlive it and use up its sponsor's
    processes (so the sponsor can't log in).
    *Rec:* a constant `MAX_LEASE` (24 h?). An agent's sub-agents are carved from the agent's own
    budget, and their leases end no later than the agent's. CAPABILITIES.md already implies this
    ("lease expiry destroys everything").
    **Answered:** clarified: `MAX_LEASE` = 24 h in KERNEL-SPEC.md, Constants; CAPABILITIES.md,
    Minting and revocation (leases; longer requests refused) and Agents 3 (sub-agents inside the
    agent's budget, no siblings).

34. **The approval screen.**
    - It must show who is asking: the session kind and a steward-assigned name (`agent-7`), not
      only the principal.
    - "Control characters stripped" misses bidi and format characters (U+202E, U+2066, U+200B).

    *Rec:* rendered fields are a whitelist of printable ASCII, and the requester's kind and name
    are shown.
    **Answered:** CAPABILITIES.md, The powerbox and approvals (Rendering).

35. **Free text from labelled sessions on the unlabelled approval screen.** A vault session's
    request reason and note (64 characters each) reach `approve@box` with no snapshot, hash or
    printable check. That is a channel out of the vault.
    *Rec:* a labelled session's request shows only fixed, steward-generated text (kind, target,
    size). Any free text goes through the declassification procedure.
    **Answered:** CAPABILITIES.md, Rendering; CONTAINMENT.md, Sessions and vaults.

## From the littlefs red team

36. **The contract between blkd and fsd.** littlefs's power-loss safety depends on how the block
    device behaves when a write is torn. It's safe if a torn write persists a prefix and later
    writes overwrite, as on a disk. It isn't safe if a torn write persists an arbitrary subset
    of the write's units, as on raw flash, or if `sync` is acknowledged before the data is
    durable. IO-ARCHITECTURE.md states neither.
    *Rec:* IO-ARCHITECTURE.md states blkd's contract:
    - writes overwrite whole sectors;
    - requests complete in order;
    - a torn write persists a prefix;
    - `sync` returns only after virtio-blk's flush completes.

    blkd (WP-D1) implements this with a flush on every `sync`, and fsd relies on nothing more.
    The known residue (littlefs has no data checksums) goes in NAMESPACES.md's accepted limits.
    **Answered:** IO-ARCHITECTURE.md, Storage (`blkd`'s contract); NAMESPACES.md, littlefs
    (accepted limits).

## From applying the answers

37. **Several open calls per thread (answer 2) against blame and `mint`.** A thread can now hold
    several open calls, so "the account the thread is serving" (crash blame) and "a message the
    caller is serving" (`mint`) no longer say which call.
    *Rec:*
    - Blame goes to every account with an open call on the faulting thread, sorted and
      deduplicated, at most `MAX_OPEN_CALLS`. The exit notice carries the first; init gets the
      list through the steward's blame handling.
    - `mint` accepts any message id among the caller's open calls. A `send`'s message id is never
      open (question 31), so it can't be a mint source.

    **Answered:** changed: blame goes to the most recently taken open call of the failing thread,
    one account; KERNEL-SPEC.md, Process (serving account), Messages (exit notices), `mint`;
    CONTAINMENT.md, Crash blame.

38. **Answered by answer 17** (CONTAINMENT.md: caps are counted per (account, label set); `admit`'s
    limits are caps, and line 65 now says so). **The server library's `admit(account)` has the same shape as question 17.** Answer 17 keyed
    the kernel and steward caps by (account, label set), but the shared server library still
    admits per account. A vault session filling fsd's admission slots would show up in its
    owner's unlabelled session.
    *Rec:* `admit` is keyed by (account, label set) too. CONTAINMENT.md's shared-server-library
    paragraph says so.
    **Answered:** by answer 17; CONTAINMENT.md, The shared server library.

## From the runtime (WP-R1)

39. **The startup block's format.** INIT.md says what the block holds, but not its tags or
    layouts. R1 defined them in `redoubt/rt/src/startup.rs`:
    - an `SBlk` header (version, length, handle count);
    - `NmSp`, `Hndl` and `Argv` entries;
    - the budget handle as a named handle `budget`.

    *Rec:* INIT.md adopts this format and owns it, since it's the contract between every parent
    and child. The crate implements it.
    **Answered:** INIT.md, Startup block (Format).

40. **How a process finds its startup block.** `process_start` takes an entry and a stack only,
    and nothing says where the startup page is mapped or how the child learns its address.
    *Rec:* the parent maps the page with `process_map`, and `process_start`'s `arg` register
    carries its page-aligned address (0 = none). Or, simpler, a fixed address in MEMORY-LAYOUT.md.
    This affects K4 and R2.
    **Answered:** the `arg` register, no fixed address; KERNEL-SPEC.md, `process_start`; INIT.md,
    Startup block.

41. **What a 9P call's words are.** WIRE.md says a 9P message travels in a lend, but not what the
    call's four words hold.
    *Rec:* the words are all zero in the request and in a successful reply. A request with other
    words, or with no lend, is refused with a reply status of 1 ("not a 9P message").
    **Answered:** WIRE.md, Messages (status 1 = `Malformed`, as in 42).

42. **A common error code for a malformed typed request.** WIRE.md has no error status that
    works across protocols, so each server names its own.
    *Rec:* reserve code 1 in every protocol's error table as `Malformed`. The generator adds it.
    **Answered:** WIRE.md, Tables (Errors).

## From the model's second round (WP-M0)

43. **I10's wording, now that exit slots are charged to the creator (answer 7).** Destroying a
    child budget restores the parent's usage only once the `killed` notices are received.
    *Rec:* I10 reads "... unchanged, once its processes' exit notices are received or dropped."
    **Answered:** KERNEL-SPEC.md, I10.

44. **A send's handles and page tables when the receiver can't pay for them.** Nothing says what
    happens, or whether R4's "free pages to hold them" counts page tables.
    *Rec:* R4's check counts the transferred pages plus the page tables to map them. A receiver
    that can't take the message's handles or pages gets `OutOfMemory` from `receive`, and the
    message stays queued (the model does this).
    **Answered:** KERNEL-SPEC.md, R4 (page tables counted; handles `OutOfMemory`, message queued).

45. **R4a at delivery.** A thread already waiting in `receive` when its process reaches
    `MAX_OPEN_CALLS` isn't covered.
    *Rec:* that `receive` returns `Busy` and the call stays queued (the model does this).
    **Answered:** KERNEL-SPEC.md, R4a and the `receive` row.

46. **I7 against R1's owner rule (answer 4).** A receive right handed to a process in another
    budget receives messages whose labels were compared only with the endpoint owner's.
    *Rec:* this is accepted, because handing out a receive right is delegation. I7 says flows are
    checked against the endpoint owner, and CONTAINMENT.md says a receive right must never be
    handed across label sets. A system server that does so is buggy, not the kernel.
    **Answered:** KERNEL-SPEC.md, I7; CONTAINMENT.md, Labels.

47. **Page-table freeing and address placement.** Both are unstated, and exact `usage` replay
    (WP-C1) depends on them.
    *Rec:* page tables are freed when they map nothing. `map_anon` addresses are the kernel's
    choice, and C1 compares usage only in the model's placement profile.
    **Answered:** KERNEL-SPEC.md, R11.

48. **Crash blame keyed by account alone.** A vault session that crashes a shared server three
    times logs out its owner's unlabelled sessions too. This is the same leak as question 17.
    *Rec:* blame, its limit and the logout it triggers are keyed by (account, label set).
    **Answered:** CONTAINMENT.md, Crash blame; KERNEL-SPEC.md, exit notices (`blamed_labels`).

49. **Where lends go when R10 fails calls in flight to a destroyed endpoint.**
    *Rec:* as in R3, the lend stays with the server, charged to it, until its `reply` or its
    death. The model does this.
    **Answered:** KERNEL-SPEC.md, R10.

Note on 41 and 42: they should share one rule. Status 1 means "malformed" in 9P calls and in typed
protocols alike, so `redoubt/wire/tables/example.md`'s code 1 is renumbered when this is adopted.

## From the runtime's red team (WP-R1)

50. **One badge, one client.** Every copy of a handle carries the same badge, and `process_start`
    copies handles into children. So all of Alice's processes (her shell and her agents) share one
    9P connection to a server: one fid table and one `Tversion`. A hostile agent can read, close or
    wipe its owner's open files. The kernel gives a server no per-process identity.
    *Rec:* a launcher never passes its own connection to a child. It asks the server for a fresh
    connection for each child (a `mint` on the server's side, through a typed "connect" operation)
    and passes that one. CAPABILITIES.md and INIT.md state the rule, and the skeleton also keys
    fids by (badge, account, label set) as a second line of defence.
    **Answered:** CAPABILITIES.md, Handles (one badge, one client); INIT.md, Startup block;
    CONTAINMENT.md, The shared server library; NAMESPACES.md.

51. **Writing up destroys.** `check`'s no-write-down lets an unlabelled caller write into a
    labelled volume. It can therefore truncate, overwrite or remove labelled files it can't read,
    and `Tcreate`'s "file exists" error reveals names inside a directory it can't list.
    *Rec:* creating, truncating and removing need the caller's labels to equal the object's. A
    blind write-up is append-only and gets one fixed error text.
    **Answered:** changed: every write needs equal labels, no blind write-up; CONTAINMENT.md, The
    shared server library (`check`).

52. **Labelled metadata flows down.** A walk returns the target's qid, including its version, with
    no read check on the target, and a directory read returns every entry's stat. Each vault write
    then changes what an unlabelled caller sees: a covert channel out of the vault.
    *Rec:* a qid is a read. Walking into a node the caller can't read is refused. A directory read
    lists only entries the caller can read, so `dir_entry` returns the node, and the skeleton
    checks it.
    **Answered:** CONTAINMENT.md, The shared server library (metadata is a read); NAMESPACES.md.

53. **Admission for system callers and dead clients.** Account 0 is "none", so every system-class
    caller shares one admission bucket, and a daemon can lock the steward out. Nothing releases a
    dead client's fids, so a crashed or hostile agent uses up its account's quota until the server
    restarts.
    *Rec:*
    - Account 0 is admitted per badge.
    - The kernel sends the endpoint's owner a notice when the last handle with a given badge is
      closed or destroyed. It is received like an exit notice, and the server frees that badge's
      state.

    The notice is a small addition to KERNEL-SPEC.md.
    **Answered:** KERNEL-SPEC.md, Endpoint (badge slots), Messages (badge notices), the cost table,
    I15; CONTAINMENT.md, The shared server library (`admit`).

54. **A system-class reader and `check`.** `check` compares label sets only, and a message
    doesn't say the sender's class. So the steward (no labels) can't read a labelled item to
    snapshot it for declassification, or stat a labelled volume.
    *Rec:* declassification reads through the label owner's own session, which the steward
    drives. No universal reader, because the steward stays unlabelled. CONTAINMENT.md states this.
    **Answered:** clarified: a short-lived reader budget carrying exactly the item's labels;
    CONTAINMENT.md, Declassification.

55. **Does a panic count toward crash blame?** A Rust panic exits through `process_exit`, which
    gives cause `exited`, not `faulted`. Blame speaks of "the faulting thread", so the most common
    crash from hostile input may never be blamed.
    *Rec:* an exit while the process holds open calls is blamed like a fault. Each open call's
    account is blamed (see question 37).
    **Answered:** changed: a fault, blamed per 37 (one account, the most recent open call);
    KERNEL-SPEC.md, Messages (exit notices); CONTAINMENT.md, Crash blame.


## From applying answers 28-55 (the design editor)

56. **Handle kinds (answer 28) can't be checked as written.** No system call reports a handle's
    kind, and `receive` doesn't return one, so a generated helper has nothing to check against.
    *Options:* `receive`'s record carries each handle's kind; or a small `handle_kind(h)` query;
    or "check by use", where a wrong kind shows up as `WrongObject` on first use and the table's
    kind is documentation.
    *Rec:* the record carries kinds. It's four small tags, and receivers need them anyway.

57. **Which call a fault blames.** The editor read answers 37 and 31 as "the most recently taken
    call that is still open". The other reading is "the most recently taken call, even if
    replied to". The model uses the first. *Rec:* still open.

58. **A panic in a thread with no open calls, while another thread of the process holds them.**
    Blame is per thread, so the notice says `faulted` but blames no one.
    *Rec:* blame falls back to the process's most recently taken open call.

59. **When a revoked call fails (answer 30).** The caller gets `Dead` at once, not when the
    server replies, to match answer 49. *Rec:* at once, as written.

60. **Answer 44: handles against pages.** The editor's reading: pages and their page tables are
    refused with `Refused` (R4), and handles get `OutOfMemory` with the message staying queued.
    *Rec:* accept.

61. **Answer 48 adds a field.** Keying blame by label set needs `blamed_labels` in the exit
    notice, which the answers didn't state. *Rec:* accept.

62. **The badge notice's details (answer 53) are the editor's.** They cover:
    - a badge slot costs 1 page per 128 slots, to be confirmed by the kernel implementer;
    - a re-mint withdraws a pending notice;
    - handles in messages not yet received count as held;
    - the label rule uses the last holder's budget;
    - notices come before messages.

    *Rec:* accept, with the cost figure confirmed in K2.

63. **Does the kernel enforce `MAX_LEASE`?** The spec says only the steward does; the model's
    comment implies the kernel does. *Rec:* the steward only. The kernel knows deadlines, not
    leases.

64. **Startup-block `Hndl` names** follow `startup.rs` (non-empty, no NUL), not INIT.md's
    manifest name rule. *Rec:* apply the same name rule to both, for one rule in one place.

65. **Where the loader stub finds the ELF image** is unspecified (PACKAGES.md). *Rec:* the image
    is a named entry in the startup block (`Hndl` or a new tag) pointing at pages the parent
    mapped. R2 defines it and PACKAGES.md states it.

66. **R10's reach into in-flight messages, and badge slots, are placed in WP-K2**, not WP-K1,
    because K1 has no endpoints or messages. *Rec:* accept.

67. **INIT.md's worked example** doesn't show the vault session's read access to its owner's
    unlabelled volume (which answer 51 relies on). *Rec:* add it.

68. **KERNEL-SPEC.md says "five kinds" of objects but lists four.** *Rec:* fix the count
    (editorial).
