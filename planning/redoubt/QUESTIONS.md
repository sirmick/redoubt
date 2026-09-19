# Open questions for the owner (milestone 1 build)

Raised by the wave 1 packages and their reviews (2026-09-19). Each touches the frozen design (v4),
so each needs your decision; the answer goes into the named note with a HISTORY.md entry.
**Rec** is the orchestrator's recommendation. Reply with numbers, e.g. "all Rec except 7: ...".

**1-101 answered 2026-09-19** (ANSWERS.md, four tranches; each "Answered" line says where the
answer now lives). **Open: 102-114** (at the end: K1's handle limit, and the design editor's choices in applying 56-101). The round-4 answers revised 56 (handle
kinds are checked by use) and replaced 57 and 58 (by 82).

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
    **Answered:** revised with round 4 (the simplifier's check by use): a wrong kind gets
    `WrongObject` on first use, and the table's kind is documentation; KERNEL-SPEC.md, Handle;
    WIRE.md, Tables.

57. **Which call a fault blames.** The editor read answers 37 and 31 as "the most recently taken
    call that is still open". The other reading is "the most recently taken call, even if
    replied to". The model uses the first. *Rec:* still open.
    **Answered:** decided: still open; then replaced by 82 (the thread's current call, set by
    `receive` and `serve`); KERNEL-SPEC.md, Process.

58. **A panic in a thread with no open calls, while another thread of the process holds them.**
    Blame is per thread, so the notice says `faulted` but blames no one.
    *Rec:* blame falls back to the process's most recently taken open call.
    **Answered:** changed: blame nobody, no fallback; kept by 82, which replaces it; KERNEL-SPEC.md,
    Messages (exit notices); CONTAINMENT.md, Crash blame.

59. **When a revoked call fails (answer 30).** The caller gets `Dead` at once, not when the
    server replies, to match answer 49. *Rec:* at once, as written.
    **Answered:** as written; KERNEL-SPEC.md, R10.

60. **Answer 44: handles against pages.** The editor's reading: pages and their page tables are
    refused with `Refused` (R4), and handles get `OutOfMemory` with the message staying queued.
    *Rec:* accept.
    **Answered:** as written, then reversed by 72 (every delivery failure is `Refused`);
    KERNEL-SPEC.md, R4.

61. **Answer 48 adds a field.** Keying blame by label set needs `blamed_labels` in the exit
    notice, which the answers didn't state. *Rec:* accept.
    **Answered:** as written; KERNEL-SPEC.md, Messages (exit notices).

62. **The badge notice's details (answer 53) are the editor's.** They cover:
    - a badge slot costs 1 page per 128 slots, to be confirmed by the kernel implementer;
    - a re-mint withdraws a pending notice;
    - handles in messages not yet received count as held;
    - the label rule uses the last holder's budget;
    - notices come before messages.

    *Rec:* accept, with the cost figure confirmed in K2.
    **Answered:** as written, then moot: 69 removed badge notices from the kernel.

63. **Does the kernel enforce `MAX_LEASE`?** The spec says only the steward does; the model's
    comment implies the kernel does. *Rec:* the steward only. The kernel knows deadlines, not
    leases.
    **Answered:** KERNEL-SPEC.md, Budget (`deadline`); the constant moved to CAPABILITIES.md (78).

64. **Startup-block `Hndl` names** follow `startup.rs` (non-empty, no NUL), not INIT.md's
    manifest name rule. *Rec:* apply the same name rule to both, for one rule in one place.
    **Answered:** INIT.md, Startup block (`Hndl`); BUILD-PLAN.md, WP-R1b.

65. **Where the loader stub finds the ELF image** is unspecified (PACKAGES.md). *Rec:* the image
    is a named entry in the startup block (`Hndl` or a new tag) pointing at pages the parent
    mapped. R2 defines it and PACKAGES.md states it.
    **Answered:** PACKAGES.md, Launching; INIT.md, Startup block (fields of the `startup` message,
    75); BUILD-PLAN.md, WP-R2.

66. **R10's reach into in-flight messages, and badge slots, are placed in WP-K2**, not WP-K1,
    because K1 has no endpoints or messages. *Rec:* accept.
    **Answered:** BUILD-PLAN.md, WP-K2 (R10's reach into messages; badge slots then removed by 69).

67. **INIT.md's worked example** doesn't show the vault session's read access to its owner's
    unlabelled volume (which answer 51 relies on). *Rec:* add it.
    **Answered:** INIT.md, Worked example (Scenarios, Vault).

68. **KERNEL-SPEC.md says "five kinds" of objects but lists four.** *Rec:* fix the count
    (editorial).
    **Answered:** KERNEL-SPEC.md, Objects; CAPABILITIES.md, Handles.

## Design review round 4 (Fable red team, simplifier and editor over answers 1-55)

The reviewers read the whole design as it stands after answers 1-55. The items below were answered
by the owner (ANSWERS.md, answers to 69-101: all Rec, with 81 option (a) and a note on 84). Editorial fixes and build-plan corrections are being applied separately (HISTORY.md).
**Timing:** A2 (the ABI records), W2 and K2 (endpoints) haven't started, so this is the cheapest
point to change the IPC design. Several items interact; the cross-references say where.

### Simplifications (the simplifier): each keeps the hole its answer closed

69. **Badge notices out of the kernel** (partly reverses answer 53). *Proposal:* delete the badge
    slot, the badge notice and I15. A server frees a client's state when told to: its typed
    connect operation (item 83) returns a random connection id to the launcher, and only the
    holder of that id can `disconnect(id)`, which frees the connection and everything minted
    under it. Launchers disconnect a child when they receive its exit notice; the steward does so
    at logout and at lease expiry.
    *Stated residual:* a launcher that dies without disconnecting leaks its children's
    connections, but only until its own connection is freed, and the leak counts against its own
    (account, label set).
    *Saves:* an estimated 150-250 lines of the most error-prone new kernel code (a count updated
    at every handle copy and drop). *Rec:* accept. It keeps the kernel minimal, and every
    milestone 1 client has a launcher that outlives it.
    **Answered:** KERNEL-SPEC.md (no badge slot, badge notice or old I15); CAPABILITIES.md, Handles
    (disconnect); NAMESPACES.md, `ninep_common`; CONTAINMENT.md, `admit`.

70. **A lend is charged to both sides while its call is open.** *Proposal:* taking a `call`
    charges its lent pages, and their page tables, to the receiver as well as the caller. A
    receiver that can't pay doesn't take the call (item 72). When the call is abandoned, the
    pages stay with the receiver and are charged only there. This removes the "over its page
    limit" budget state and I5's exception, so usage ≤ limit becomes unconditional. *Cost:* a
    server's budget must cover its open lends up front (about 4 MiB for 64 open 9P calls).
    *Rec:* accept.
    **Answered:** KERNEL-SPEC.md, R3, R6, the cost table, I5 (unconditional); RESOURCES.md, Budgets.

71. **Define "abandoned call" once** (editorial). R3, R4b and R10 each restate it; define it in R3.
    Being applied, with item 70's wording if 70 is accepted.
    **Answered:** KERNEL-SPEC.md, R3 (defined once; R4b and R10 point to it).

72. **One way a delivery fails: `Refused` to the sender** (reverses answer 44's "stays queued").
    *Proposal:* a message is delivered only if the receiver's budget can pay for everything it
    brings (handles, page tables, lent or transferred pages) and a transfer fits `max_transfer`.
    Otherwise the sender gets `Refused`, the kernel moves on, and a `receive` never fails for want
    of pages. The one bit `Refused` reveals is one a sender with a clock already has.
    *Rec:* accept. It also removes red-team item 85(c)'s stuck-cursor case.
    **Answered:** KERNEL-SPEC.md, R4 (`Refused`; `receive` never fails for want of pages), the error
    table.

73. **Class is inherited, and `budget_create` takes no class** (alters answer 9).
    `process_create` and `budget_destroy` already let a holder of a system budget handle act
    there, so a class check on `budget_create` alone guards one door of three. What actually
    protects system budgets is never handing them to users (item 79). *Rec:* accept.
    **Answered:** KERNEL-SPEC.md, Budget, `budget_create`, I8.

74. **The process object is the exit slot** (changes answer 7's mechanism, not its guarantee). A
    process is charged to its creator, outlives its death until its exit notice is received or
    dropped, and is freed with the creator's budget (which kills it if it still runs). This
    removes the exit slot as a separate object. *Rec:* accept.
    **Answered:** KERNEL-SPEC.md, Process, the cost table, R10; RESOURCES.md.

75. **The startup block as one typed wire message** (alters answer 39's format). The block becomes
    one WIRE.md message (`namespace`, `handles` and `argv` as `bytes` fields with a stated inner
    layout), decoded by `redoubt-wire`, instead of a second framing format with CRCs that protect
    nothing, since the parent writes both. *Saves:* 200-300 lines in `redoubt-rt`. *Rec:* accept;
    R1b rewrites `startup.rs` anyway.
    **Answered:** INIT.md, Startup block (the `startup` message); WIRE.md, Layout in a message;
    BUILD-PLAN.md, WP-R1b.

76. **A budget's own page is always charged to its parent** (a v4 clause of R6). This removes the
    revocation-scope special case in accounting. *Rec:* accept.
    **Answered:** KERNEL-SPEC.md, the cost table, R6, `budget_create`'s errors.

77. **`random` returns one `u64`** (alters answer 15). This drops `MAX_RANDOM`, a buffer and a
    range check; a 32-byte seed takes four calls. *Rec:* accept.
    **Answered:** KERNEL-SPEC.md, `random`, Constants, ABI.

78. **`MAX_LEASE` out of the kernel spec** (alters answer 33's placement). The kernel never reads
    it (question 63). The constant moves to CAPABILITIES.md and out of `redoubt-sys`, and the
    steward still refuses leases over 24 h. *Rec:* accept.
    **Answered:** CAPABILITIES.md, Minting and revocation (Leases); KERNEL-SPEC.md, Constants
    (removed).

### Holes (the red team): each is a concrete attack against today's wording

79. **Only `init` and the steward ever hold a handle to a system-class budget.** Today every
    server's startup block includes its budget. A compromised `ipd` could then create
    system-class children with any labels and any account: forged admission and blame in Alice's
    name, and a labelled reader. *Rec:* server startup blocks omit `budget`, a manifest that
    grants one is refused, and an attack test checks it.
    **Answered:** INIT.md, Boot and The boot manifest; BUILD-PLAN.md, WP-R3; PLAN.md, attack suite.

80. **A narrowing handle is always a revocation scope.** To mint a connection narrowed to a
    child's budget, a server must hold that budget handle, and a budget handle is a destroy
    right, so a compromised `fsd` could end every session. *Rec:* the steward passes servers only
    scopes created for that purpose, never a budget that holds processes. Attack test: a server
    can't destroy a session.
    **Answered:** CAPABILITIES.md, Minting and revocation; INIT.md, steward; BUILD-PLAN.md, WP-S2.

81. **Open calls can be pinned, and a server is never told a call was abandoned** (High). Bob
    parks 64 lent calls at `ipd`, each with a 1 µs timeout. `ipd` hits `MAX_OPEN_CALLS`, and
    `receive` then returns `Busy` for everything on its endpoint, including `netd`'s frames. Every
    SSH session dies. *Options:*
    - (a) An abandoned-call notice. The flag lives in the open call's own page, the server
      replies to free it, and at the limit `receive` refuses only calls, still delivering sends
      and notices.
    - (b) A call's timeout applies only until the server takes it. After that the caller waits
      for the reply or the server's death, so abandoning needs the caller's death, which is
      bounded by its process limit. This changes I13's wording.
    - Either way, `admit`'s caps must sum to less than `MAX_OPEN_CALLS` with headroom, and parked
      calls get a server-side deadline.

    *Rec:* (a), which also answers "how does a server learn a caller is gone". If you take item
    69, this is the one kernel notice that stays.
    **Answered:** option (a): KERNEL-SPEC.md, Process, Messages, R3, R4a, I15; CONTAINMENT.md, the
    shared server library; PLAN.md, attack suite.

82. **Blame can be steered in event-driven servers** (High; makes 57 and 58 moot). `ipd` and
    `sshd` park calls and later process an old one when an interrupt or a `send` arrives. The
    "most recently taken" call is then a bystander's. Bob crashes `ipd` on his own connection
    while Alice's call is the newest, and Alice is logged out. *Rec:* a new call, `serve(msg_id)`,
    names which open call the thread is now working on; the server library calls it before
    resuming a parked call. A thread doing event work with no current call blames nobody, with no
    fallback (question 58). Attack test: a crash triggered by a `send` while a bystander's call is
    parked blames nobody.
    **Answered:** KERNEL-SPEC.md, Process (the current call), `serve`, Messages (exit notices);
    CONTAINMENT.md, Crash blame.

83. **The fresh-connection operation (answer 50) has no protocol.** *Rec* (the editor's text): a
    9P endpoint also serves typed operations, where word 0 = 0 is 9P and anything else is an
    opcode. Every 9P server serves `new_connection` (opcode 2, reply `conn: handle[0] endpoint`),
    minting a connection rooted at or below the caller's. The table lives in NAMESPACES.md as
    `ninep_common`, R1b implements it, and R4 serves it. It also carries item 69's connection id.
    **Answered:** NAMESPACES.md, `ninep_common`; WIRE.md, Messages; CAPABILITIES.md, Handles.

84. **User work runs at system priority inside servers** (CPU amplification). Bob makes
    `fsd`/`keyd`/`ipd` do expensive work, and no user budget runs meanwhile. *Rec:* strict
    system-first ordering only for `init`, the steward and drivers. Servers working for users run
    in the stride queue with a weight from the manifest, and bound the work of one request. The
    stated residual: that cost is paid by the server's weight, not the requester's.
    **Answered:** accepted with the note on the steward: KERNEL-SPEC.md, Budget (`first`), R12;
    RESOURCES.md, Scheduling; CONTAINMENT.md, covert channels (Server CPU).

85. **Shared pools that aren't carved.**
    - (a) Bob fills the `data` volume and Alice's saves fail.
    - (b) Bob floods `fsd` with handles, growing its handle table until `fsd` can't pay.
    - (c) A vault session loops on fresh connections, using up `fsd`'s budget, which is a channel.

    *Rec:* a byte quota per attach root in `fsd`; the server library closes every handle it didn't
    ask for; the per-client caps are sized so every bucket at its cap fits the server's budget.
    **Answered:** NAMESPACES.md, Filesystem servers (quota); CONTAINMENT.md, the shared server
    library.

86. **Revocation must reach handles inside queued messages** (R10). Today a revoked handle
    arrives in a message sent before the revocation, and if a server reuses badges, that's a
    zombie connection. *Rec:* R10 also sweeps handles in messages not yet received (they arrive as
    0), and servers never reuse a badge number.
    **Answered:** KERNEL-SPEC.md, R10, I2; CONTAINMENT.md, the shared server library (badges never
    reused).

87. **System callers share one fairness group.** R2 groups every account-0 sender as `(0, {})`, so
    a busy `fsd:data` fills `WAIT_CAP` at `blkd`, and `fsd:alice-secrets` gets `Busy`: a DoS and a
    channel out of the vault. *Rec:* for account 0, the group key includes the sender's budget id.
    **Answered:** KERNEL-SPEC.md, R2; CONTAINMENT.md, covert channels.

88. **Global counters are a channel.** If message ids come from one global counter, one process
    can see the gaps in them grow with another process's traffic, including a vault's. PIDs do the
    same at process-creation rate. *Rec:* message ids are unique within the receiving process
    only; PIDs are drawn at random from free ASIDs; `ps` and `budget` show only the caller's
    (account, label set).
    **Answered:** KERNEL-SPEC.md, Messages (message ids), Process (random PIDs), I12; INIT.md, The
    shell; CONTAINMENT.md.

89. **Carving under one top budget is a channel.** A vault session's leases change Alice's top
    budget's free limits, which her unlabelled agent can probe. *Rec:* at boot the steward splits
    each principal's top budget into fixed sub-budgets, one per (principal, label set) named in
    the manifest.
    **Answered:** CONTAINMENT.md, covert channels; INIT.md, The boot manifest, steward, worked
    example.

90. **An agent can lock out its sponsor.** It shares its sponsor's buckets. It fills
    `(alice, {})`'s caps at the steward and `fsd`, and Alice can't even end the lease, for up to
    24 h. *Rec:* a fair share per badge within a bucket, with the bucket as the ceiling; ending a
    lease is always accepted from the sponsor, ahead of admission. Attack test: an agent floods
    the steward and `fsd`, and Alice still opens a file and ends the lease.
    **Answered:** CAPABILITIES.md, Agents; CONTAINMENT.md, `admit`, steward; PLAN.md, attack suite.

91. **A logout isn't a lockout, and agents survive it.** Bob crashes `fsd:data` three times, is
    logged out, logs straight back in (or his agent carries on), and three more crashes reboot
    the box. INIT.md's tree also puts agents beside sessions, not under them. *Rec:* the third
    blamed crash destroys every budget of that (account, label set), sessions and leases alike,
    and new sessions are refused until the window passes. INIT.md's tree is reconciled.
    **Answered:** CONTAINMENT.md, Crash blame; INIT.md, worked example (tree reconciled).

92. **The audit file and "an approval is waiting" are unlabelled sinks.** A labelled request's
    target lands in the audit file, and the notification timing reaches the unlabelled session.
    *Rec:* audit records carry the request's labels and are read under `check`. A labelled
    request's notification reaches only channels whose labels ⊇ the request's, plus
    `approve@box`.
    **Answered:** CONTAINMENT.md, steward and covert channels; CAPABILITIES.md, Limits and labels.

93. **`process_create` accepts a badged exit endpoint**, so anyone can spray exit notices at a
    server. *Rec:* the exit endpoint must carry badge 0 (`NotPermitted`).
    **Answered:** KERNEL-SPEC.md, `process_create` (`NotPermitted`).

94. **The approval screen shares `sshd` with the most hostile input.**
    - A `sunset` bug reached from Bob's channel controls the screen.
    - A network flood delays approvals.
    - CONTAINMENT.md's "no owner exemption at any sink" contradicts `sshd` carrying a vault channel
      to its owner.

    *Rec:* state `sshd` as the one sink cleared for a label (only the channel its owner
    authenticated: a pty session with no forwarding, subsystems or `exec`), and state the
    milestone 1 residual. Milestone 2 gives `approve@` its own `sshd` instance or the console.
    **Answered:** CONTAINMENT.md, Sessions and vaults; CAPABILITIES.md, Milestone 1; INIT.md, sshd.

95. **`keys` in a lease is a signature oracle.** A hijacked agent signs SSH user-auth blobs relayed
    from its peer, so the peer can log in as Alice elsewhere. *Rec:* a lease carries `keys` only
    if the approval named the key, and a `keyd` badge names one key and one purpose (for SSH, the
    session identifier `keyd` computed itself), never arbitrary bytes.
    **Answered:** CAPABILITIES.md, Agents (7); INIT.md, keyd; BUILD-PLAN.md, WP-S1.

### Smaller points needing a decision (the editor)

96. **Badge slots are charged to `init`,** because `init` creates every server's endpoint. This is
    moot if 69 is accepted. Otherwise, *Rec:* charge the minting process.
    **Answered:** moot with 69.

97. **`init`'s blame report to the steward has no message table.** *Rec:* one typed message, whose
    table S2 writes into INIT.md's steward section. Until then, the R3 case expects the logout
    signal naming (account, label set), and the logout itself is S2's.
    **Answered:** INIT.md, Restarts and reboots; BUILD-PLAN.md, WP-R3 and WP-S2.

98. **A table can't mark a typed message as a `send`.** *Rec:* every milestone 1 typed message is a
    `call`, and a `kind` column is added when a protocol first needs a transfer.
    **Answered:** WIRE.md, Tables.

99. **A deliberate exit with open calls is a blamed fault** (from 55). *Rec:* a server that means to
    exit replies to every open call first, and the spec says so.
    **Answered:** KERNEL-SPEC.md, Messages (exit notices), R4b.

100. **Decoding order.** ANSWERS.md's "`BadHandle`, then `TooLarge`, then `InvalidArgument`" reads as
     a sequence, but the spec orders errors by register position. *Rec:* confirm that the answer
     was a classification, and that the spec's positional order stands.
     **Answered:** the answer-14 order was a classification; the spec's positional order stands
     (KERNEL-SPEC.md, Errors and the order of checks).

101. **Which way the steward and a reader budget talk** (54). *Rec:* the steward `call`s the reader,
     which fills the steward's lend with the snapshot. This matches "labelled callers can only
     submit requests".
     **Answered:** CONTAINMENT.md, Declassification.

### The earlier open questions, revisited

- **56 (handle kinds):** the simplifier recommends "check by use" (`WrongObject` on first use).
  Accepted in the answers to 69-101, revising answer 56.
- **57 and 58:** replaced by item 82 (`serve`, with no fallback).
- **62 (badge-notice details):** moot if 69 is accepted. Otherwise, the "last holder" is undefined
  when the last copy was in a discarded message: use the sender's budget.
- **64 (one name rule):** as recommended.

## From the kernel (WP-K1)

102. **A per-process handle limit.** K1 caps a process's handle table at 32 pages (4096 handles),
     so the kernel can't be made to allocate an unbounded table. Past that, calls that add a
     handle get `OutOfMemory`. The spec names no such limit; a budget's page limit bounds the
     table only indirectly.
     *Rec:* a constant `MAX_HANDLES` = 4096 in KERNEL-SPEC.md. A call that would exceed it gets
     `TooLarge`, which is distinguishable from a budget running out of pages, and C1 then compares
     like with like.

## From applying answers 56-101 (the design editor's choices; each needs confirming)

103. **How answer 84 works: a `first` flag on budgets.** Your answer gave the policy but no
     mechanism. The editor added a budget flag `first`, set at `budget_create` in place of the old
     class argument. Only a caller that is itself `first` can set it, and only under a
     system-class parent; `root` and `system` have it. R12 runs `first` budgets before the stride
     queue. This is the largest thing the editor invented. *Rec:* confirm.

104. **Where the abandoned-call notice (81) is delivered.** It is delivered once, on the holding
     thread's next `receive` on the endpoint the call came in on. A thread that never receives
     there again never gets it. *Rec:* confirm, and the server library makes each serving thread
     keep receiving.

105. **"At `MAX_OPEN_CALLS`, refuse only calls" (81).** The editor reads this as: calls stay
     queued and R2 skips them, sends and notices still arrive, and `receive` no longer returns
     `Busy`. *Rec:* confirm.

106. **PIDs under answer 74.** A finished process keeps its PID until its notice is received, but
     stops counting against its budget's process limit when it dies. *Rec:* confirm.

107. **The reply side of answer 72.** A `reply` whose handles don't fit the caller still gives
     the caller `OutOfMemory`, since 72 speaks only of `receive`. *Rec:* confirm.

108. **The new message layouts (75, 83) are the editor's.** They are the fields of the `startup`
     message and the `ninep_common` table: `disconnect` as opcode 3, and a `root: string` in
     `new_connection`. Both are fenced until WP-R1b generates them. *Rec:* confirm; R1b may
     adjust the layouts, recorded in HISTORY.md.

109. **The new I15.** Every abandoned call is reported exactly once and stays open until replied
     to. It reuses the number the badge-notice invariant had. *Rec:* confirm.

110. **58 under 82.** A thread with no current call blames nobody, with no fallback. *Rec:* as
     written.

111. **What a handle table with holes costs.** The kernel charges one page for each table page in
     use. The model charges `ceil(handles / 128)`. With handles 1-129 held and handle 5 closed,
     the kernel charges 2 pages and the model 1, and WP-C1's replay will see the difference.
     *Rec:* pages in use. It's what the memory actually costs, and handles are never moved to
     compact the table. The cost table says "1 per table page holding a handle", and the model
     follows.

## From the generator update (WP-W2)

112. **The `startup` message can't be decoded from its page as specified.** INIT.md says the rest of
     the page after the message isn't read, but a typed message has no overall length, and the
     decoder refuses trailing bytes.
     *Rec:* the page starts with a `u32` byte length, then the `startup` message; the decoder reads
     exactly that many bytes. INIT.md states it.

113. **Opcodes on a shared 9P endpoint.** Every 9P server also serves `ninep_common` (opcodes 2 and
     3). WIRE.md doesn't say whether a server may also serve its own typed protocol on the same
     endpoint, or how their opcodes are kept apart.
     *Rec:* `ninep_common` reserves opcodes 1-15 on every 9P endpoint, and a server's own protocol
     on that endpoint uses 16 and up. The generator refuses a table marked as a 9P server's protocol
     (a new `<!-- wire: NAME ninep -->` marker) that uses opcodes below 16.

114. **`disconnect` and a stranger's connection id.** *Rec:* the `ninep_common` error table gains
     code 2, `not_yours`, for a `disconnect` naming an id the caller didn't receive. That makes it
     indistinguishable from an id that doesn't exist, so nothing is revealed.
