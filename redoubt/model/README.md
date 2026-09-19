# redoubt-model: the executable security model

WP-M0 (planning/redoubt/BUILD-PLAN.md). A host Rust crate that implements every object, system
call, error, rule (R1-R12) and invariant (I1-I14) of `planning/redoubt/KERNEL-SPEC.md`, with the
same names and arguments, plus the steward's milestone 1 policy as a layer above it; property
tests over random operation sequences; deliberate rule breaks that show the tests are not
vacuous; and a trace format that WP-C1 replays on the real kernel.

The library is `no_std` + `alloc`, has no dependencies and no `unsafe` (`#![forbid(unsafe_code)]`).

## Running it

    cargo test -p redoubt-model --release                  # everything, 20,000 sequences per family (~30 s)
    cargo test -p redoubt-model --release --test properties -- --ignored million   # the 10^6 acceptance run
    REDOUBT_MODEL_SEQUENCES=1000000 cargo test -p redoubt-model --release --test properties   # same, via the variable
    cargo test -p redoubt-model --release --test mutations -- --nocapture   # which property catches each rule break

A failing kernel sequence is reported with its seed, shrunk to the ops that matter, and printed as
a trace (below) that replays the failure. Use `--release`: debug builds are about 20 times slower.

## Files

| File | What |
| --- | --- |
| `src/spec.rs` | KERNEL-SPEC.md's constants, errors, class, cause; the ABI encodings (`redoubt-sys`'s values) |
| `src/syscall.rs` | the 24 system calls as data (`Syscall`), their results, and the other events (`Op`) |
| `src/kernel.rs` | the kernel: objects, the calls (one method per call, spec names), R1-R11, time, IRQs |
| `src/sched.rs` | R12: two classes, stride over budgets, round-robin threads |
| `src/ghost.rs` | history the invariants need that the kernel does not keep |
| `src/invariants.rs` | I1-I9, I11-I13 and the per-rule checks, recomputed from the objects |
| `src/check.rs` | property families: `kernel_sequence`, `budget_lifecycle` (I10), `scheduler_fairness` (R12); shrinking; the conformance epilogue |
| `src/gen.rs` | the seeded generator (splitmix64) of random sequences |
| `src/mutation.rs` | the deliberate rule breaks, one switch each |
| `src/steward.rs` | the steward's M1 policy over the kernel model |
| `src/policy.rs` | the policy's properties P1-P10 and their families |
| `src/trace.rs` | the trace format: write, parse, replay-and-compare |
| `tests/` | `properties` (all families), `mutations` (every break caught), `coverage` (every call succeeds and every error occurs), `traces` (round trip, conformance direction, the example) |
| `traces/` | an example trace (R3: a lender dies mid-call) |

## What is modelled, and how abstractly

- **Memory** is page frames with one abstract word of content each, and per-process maps from
  virtual page to (frame or device page, flags, state). User loads, stores and instruction fetches
  are events (`Op::Read`, `Op::Write`, `Op::Exec`) that fault unless the page allows them; a fault
  ends the process with an exit notice `faulted`.
- **Charging** (R6): each object costs a fixed number of pages (`kernel::Costs`: budget, process,
  thread, endpoint, and one handle-table page per 8 handles). Page-table pages are part of the
  process cost; per-mapping page tables are not modelled (see "Spec problems", 1).
- **Time** is logical: it advances only in `Op::Tick`, during which the scheduler (R12) runs
  threads and timeouts and budget deadlines fire. System calls are instantaneous and are made by
  whichever runnable thread the sequence names; the scheduler decides only how CPU time is shared.
- **Interrupts**: `Op::Irq { n }` raises line `n` (level-style: a raised line stays pending until
  it fires).
- **The ABI's buffers** (message bodies, budget specs, handle lists, the receive record) are not
  modelled as memory: their contents are the call's fields. A bad buffer pointer is the kernel's
  business and has no case here.
- **Invariant checking** recomputes each budget's usage from the objects charged to it, each
  frame's payer from where it is mapped, each handle's stamp from how it was made (ghost `Origin`),
  and so on, after every step; see `invariants.rs`.

## Order of checks

Where a call has several faults, which error it returns is fixed, so a replay compares like with
like. Every call checks in this order:

1. **Decoding**, argument by argument in register order, as `redoubt-sys` decodes: a handle value
   that cannot be an index (`>= u32::MAX`) is `BadHandle`; a list over its array (handles over
   `MAX_MSG_HANDLES`, labels over `MAX_LABELS`, counted before deduplication) is `TooLarge`; any
   other malformed encoding (unknown flag bits, an unknown class or reset tag, a 32-bit field
   (exit code, process count, weight) over 32 bits) is `InvalidArgument`.
2. **Kernel argument checks**, left to right: the handle exists (`BadHandle`), names the right
   object (`WrongObject`), ranges are page-aligned, non-empty, inside user space and mapped as
   required (`InvalidArgument`), sizes within fixed caps (`TooLarge`).
3. **Permission**: `NotPermitted`, `ClassDenied`, `LabelDenied`.
4. **Resources**: `OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`.

Per call (errors in the order they are checked; decoding errors are left out):

| Call | Errors |
| --- | --- |
| `map_anon(len, flags)` | `InvalidArgument` (len 0 or unaligned; flags 0 or W without R), `NotPermitted` (W+X), `OutOfMemory` |
| `unmap(addr, len)` | `InvalidArgument` (range bad, or a page not an own accessible mapping, including a page lent out) |
| `set_flags(addr, len, flags)` | `InvalidArgument` (range; flags), `NotPermitted` (W+X), `InvalidArgument` (not own mapping) |
| `map_device(h)` | `BadHandle`, `WrongObject` (not MMIO) |
| `dma_alloc(h, npages)` | `BadHandle`, `WrongObject`, `InvalidArgument` (npages 0), `NotPermitted` (no DMA flag), `OutOfMemory` |
| `thread_create(entry, sp, arg)` | `TooManyThreads`, `OutOfMemory` |
| `process_exit(code)` | (decode only) |
| `process_create(h budget, h exit)` | `BadHandle`, `WrongObject` (each, in order), `OutOfProcesses`, `OutOfMemory` (the budget's, then the caller's table) |
| `process_map(h, src, dst, len, flags)` | `BadHandle`, `WrongObject`, `InvalidArgument` (src range, dst range, flags), `NotPermitted` (W+X, then started), `InvalidArgument` (src not own RAM; dst occupied), `OutOfMemory` (child's budget) |
| `process_start(h, entry, sp, handles)` | `BadHandle`, `WrongObject`, `BadHandle` (each handle), `NotPermitted` (started), `OutOfMemory` (child's budget: thread, then table) |
| `endpoint_create()` | `OutOfMemory` |
| `mint(source, badge, budget?)` | message source: `InvalidArgument` (not serving it), `Dead` (its endpoint or stamp is gone); handle source: `BadHandle`, `WrongObject`, `NotPermitted` (badge not 0); then `InvalidArgument` (badge 0), `BadHandle`, `WrongObject`, `NotPermitted` (budget not the default stamp or below), `OutOfMemory` |
| `call(h, words, handles, lend, timeout)` | `BadHandle`, `WrongObject`, `BadHandle` (each handle), `TooLarge` (lend over `MAX_LEND_PAGES`), `InvalidArgument` (lend range, not own writable RAM), `Busy` (R2 cap); later: `LabelDenied` (R1), `Timeout`, `Dead`, `OutOfMemory` (the reply's handles do not fit the caller) |
| `send(h, words, handles, transfer, timeout)` | as `call` without the lend cap; later: `LabelDenied`, `Refused` (R4, or the receiver's budget cannot hold the pages), `Timeout`, `Dead` |
| `receive(h?, timeout, max_transfer)` | `BadHandle`, `NotPermitted` (badge not 0), `Busy` (the thread serves an unreplied call), `WrongObject` (not an endpoint or IRQ); later: `Timeout`, `Dead` (endpoint destroyed), `OutOfMemory` (the message's handles do not fit; it stays queued) |
| `reply(msg_id, words, handles)` | `InvalidArgument` (the thread does not serve `msg_id`), `BadHandle` (each handle) |
| `handle_close(h)` | `BadHandle` |
| `budget_create(h, pages, processes, weight, class, labels, account, deadline)` | `BadHandle`, `WrongObject`, `ClassDenied` (system under user), `LabelDenied` (not ⊇ parent's), `ClassDenied` (labels added by a user-class caller), `TooLarge` (depth), `OutOfMemory` (pages over the parent's free pages, or no room for a scope's own object), `OutOfProcesses`, `InvalidArgument` (weight over the parent's free weight), `OutOfMemory` (pages fewer than the budget's own object; then the caller's table) |
| `budget_destroy(h)` | `BadHandle`, `WrongObject` |
| `budget_usage(h)` | `BadHandle`, `WrongObject`, `LabelDenied` (caller's labels ⊉ target's) |
| `time_now()` | none |
| `system_reset(h, kind)` | `BadHandle`, `WrongObject` (not the Reset device) |
| `random(len)` | `TooLarge` (over 64) |

## Trace format

A trace is ASCII text, one record per line, tokens separated by single spaces. Lines starting
with `#` are comments. It is written by `trace::record` and read by `trace::parse`; `trace::check`
replays a trace on the model and requires the same text back. `trace::tokens` splits a line into
`Token`s and needs only `core` and `alloc`.

### Tokens

| Token | Meaning |
| --- | --- |
| `123`, `0x7b` | a literal number (`u64`) |
| `-` | none |
| `forever` | `FOREVER` |
| `[x,y,...]` | a list (no spaces) |
| `key=value` | a named field |
| `BASE@PAGES` | a page range (a lend or transfer): `BASE` is an address token |
| `h:N` | a handle index, in the process the record is about |
| `a:0xBASE+0xOFF` | an address `OFF` bytes into a region whose base the kernel returned to that process (`ok a:0xBASE` names the region) |
| `p:N`, `t:N`, `m:N`, `pa:0xN` | a pid, a tid, a message id, a physical address |
| anything else | a word: a call name, `ok`, `err`, an error name, `user`, `system`, ... |

**Names** (`h:`, `a:`, `p:`, `t:`, `m:`, `pa:`) are values the kernel chooses. A replayer keeps a
map per kind (per process for `h:` and `a:`): **a name in a result binds it** to the real value
returned (rebinding if it was bound before; handle indices and addresses are reused), and **a
name in an argument is looked up**. A name used in an argument that was never bound stands for
"a value that is not valid here", and the replayer substitutes one it knows to be invalid (for
handles, `u32::MAX - 1`; for message ids, `u64::MAX - 1`). A literal is passed as it is: hostile
handle values `>= u32::MAX` are written as literals, because they fail at decoding. Literal
addresses are ones the kernel did not choose (a `process_map` destination, or hostile values);
the model places its own mappings from `0x10_0000_0000` up (`kernel::KERNEL_CHOSEN_BASE`), keeps
user space below `1 << 38`, and the generator's literal addresses are `0x1234`, `0x800_0000`,
`0x1000_0000`-`0x1000_3000` (process_map) and `0xffff_ffff_ffff_f000`; a kernel whose layout
maps something there must remap these ranges.

Bindings the replayer gets without a result: `init` is `p:1`, its thread `t:1`, and its handles
are `h:1` root, `h:2` system, `h:3` users, then one per `device` line in order (`h:4`, ...). After
`process_start`, the child's `h:1`..`h:n` are the handles listed, in order (KERNEL-SPEC.md fixes
the slots).

### Records

    redoubt-model-trace 1                                              the header, first line
    boot ram_pages=N root_processes=N root_weight=N system_pages=N system_processes=N system_weight=N users_pages=N users_processes=N users_weight=N
    costs budget=N process=N thread=N endpoint=N handles_per_page=N    pages per object (R6)
    device mmio base=0xN pages=N dma=0|1                               one per device object, in init's order
    device irq n=N
    device reset
    start p:1 t:1                                                      init
    do p:P t:T CALL ARGS... -> RESULT                                  a system call by thread T of process P
    write p:P t:T ADDR VALUE -> ok|fault                               a user store of one word
    read p:P t:T ADDR -> ok word N|fault                               a user load
    exec p:P t:T ADDR -> ok|fault                                      an instruction fetch
    fault p:P t:T                                                      the thread faults (the process ends, `faulted`)
    irq N                                                              interrupt line N is raised
    tick DT                                                            DT µs pass
    note process p:CREATOR h:H p:NEW                                   process_create's handle H names process NEW
    note thread p:P t:T                                                process_start started P with thread T
    wake p:P t:T -> RESULT                                             a blocked thread's call returned, caused by the record above

`note` and `wake` records follow the event that caused them, in order. A replayer drives each
event from the named thread and compares: the result of a `do` (or `blocked` if it did not return,
or `gone` if the thread no longer exists: `thread_exit`, `process_exit`, destroying its own budget),
then each `wake`. A thread named by a record is always runnable at that point.

Call arguments are in KERNEL-SPEC.md's order, spelled:

    map_anon LEN FLAGS                        flags: R=1 W=2 X=4 (redoubt-sys MemFlags)
    unmap ADDR LEN
    set_flags ADDR LEN FLAGS
    map_device H
    dma_alloc H NPAGES
    thread_create ENTRY SP ARG
    thread_exit
    process_exit CODE
    process_create H_BUDGET H_EXIT
    process_start H ENTRY SP [H,...]
    process_map H SRC DST LEN FLAGS
    endpoint_create
    mint SOURCE BADGE BUDGET                  SOURCE: m:N (a message) or h:N; BUDGET: h:N or -
    call H [W,W,W,W] [H,...] LEND TIMEOUT     LEND: BASE@PAGES or -
    send H [W,W,W,W] [H,...] TRANSFER TIMEOUT
    receive H|- TIMEOUT MAX_TRANSFER          max_transfer in pages
    reply m:N [W,W,W,W] [H,...]
    handle_close H
    budget_create H PAGES PROCESSES WEIGHT CLASS [LABEL,...] ACCOUNT DEADLINE    class: user, system, or a raw tag; deadline absolute µs or forever
    budget_destroy H
    budget_usage H
    time_now
    system_reset H KIND                       kind: 1 power off, 2 reboot
    random LEN

Results:

    ok                                        no value
    ok a:0xBASE                               map_anon, map_device (binds a region)
    ok a:0xBASE pa:0xPHYS                     dma_alloc
    ok t:N | ok h:N                           thread_create | process_create, endpoint_create, mint, budget_create
    ok reply words=[...] handles=[h:...]      call
    ok message m:N badge=N account=N labels=[...] words=[...] handles=[h:...] buffer=-|[lend,a:0xBASE,PAGES]|[transfer,a:0xBASE,PAGES]
    ok interrupt h:N                          receive on an IRQ handle
    ok exit p:N cause=exited|faulted|killed code=N blamed=N
    ok usage [PAGES_LIMIT,PAGES_USED,PROCESSES_LIMIT,PROCESSES_USED]
    ok time N | ok random LEN | ok word N
    err NAME                                  one of KERNEL-SPEC.md's errors
    blocked | gone

Page counts in `usage` depend on `costs`: a replay against the kernel compares them only if the
kernel charges the same (see "Spec problems", 1).

**Conformance traces end with an epilogue** (`check::epilogue`): results show a divergence only
once something depends on it, so every process that can run reads its pages and its budgets'
usage, `init` destroys the budgets it created, and every process closes handle indices 1 to 64.
`tests/traces.rs` shows that with it, a kernel breaking any of R1-R11 fails to replay some trace,
except for two breaks no result shows (R12 scheduling; an IRQ source left unmasked), which the
kernel's own tests must cover.

Example: `traces/lender-dies-mid-call.trace`.

## Property tests

| Family | Checks |
| --- | --- |
| `kernel_sequence` | after every step of a random sequence: I1-I9, I11-I13, R2's cap, R3's lend staying mapped, R4, R5's "no second fire without a receive" and "no lost interrupt", R12's class order at every pick, the object graph's consistency |
| `budget_lifecycle` | I10: create a budget, let only its own processes act, destroy it: every other budget's usage is unchanged |
| `scheduler_fairness` | R12: class order; every always-runnable user budget gets its weight's share of user CPU time over every interval, within `SLICE x (2 + 2 w/w_min + n)`, against sleepers that sleep long and then burst |
| `steward_policy` | P1-P9 (policy.rs) and the kernel invariants underneath |
| `steward_noninterference` | P10: the vault sessions' work changes nothing another principal observes |

I14 (no panic) is checked by the runner, which counts a panic as a failure; 4% of the
generated system calls have arguments drawn without regard to the state.

## Deliberate rule breaks

`src/mutation.rs` has 25 kernel mutations (at least one per rule R1-R12) and 8 policy ones.
`tests/mutations.rs` requires each to fail some property within 20,000 sequences, and prints
which one.

## Interpretation choices

Where the spec leaves a detail open and the choice does not change what the spec says, the model
chose as follows. Each should be confirmed or overruled in KERNEL-SPEC.md.

1. **Which error.** The spec names checks but not always their error: the tables above are the
   model's mapping (W+X is `NotPermitted`; a range that is not the caller's own mapping, including
   a lent page, is `InvalidArgument`; a lend over `MAX_LEND_PAGES`, labels over `MAX_LABELS`, a
   budget deeper than `MAX_DEPTH` and `random` over 64 are `TooLarge`; `mint` with badge 0 is
   `InvalidArgument`; a thread that does not serve `msg_id` gets `InvalidArgument`).
2. **Carving weight.** A child's weight over the parent's free weight is `InvalidArgument` (the
   error list has no weight error).
3. **Labels not ⊇ the parent's** in `budget_create` is `LabelDenied`.
4. **Account (R8).** When the parent's account is non-zero, the argument is ignored (as
   `redoubt-sys` documents).
5. **Handle indices** start at 1; index 0 is never valid. New handles take the lowest free index.
6. **A thread serves one call at a time.** `receive` on an endpoint by a thread that serves a
   `call` it has not replied to is `Busy`; receiving on an IRQ or sleeping is allowed. This is what
   I5's bound ("at most `MAX_LEND_PAGES` per thread") implies. A `send` message is served until the
   thread takes another message or replies to it (a reply to a `send` only ends serving it).
   `reply` and `mint` from a message act for the thread that received it.
7. **Serving account.** A thread's account is set when `receive` delivers a message and cleared by
   its `reply`, as the spec says; delivering an interrupt or exit notice leaves it.
8. **Exit notices before messages** when both are pending on an endpoint.
9. **R1 with several waiting receivers**: the check is against the first waiting receiver; if it
   fails, the sender gets `LabelDenied` (it is not offered to later receivers).
10. **A transfer the receiver's budget cannot hold** is `Refused` to the sender (like R4) and the
    kernel moves on; the pages never overcommit the receiver.
11. **Handles the receiver cannot pay for**: its `receive` returns `OutOfMemory` and the message
    stays queued. For a reply: the caller's `call` returns `OutOfMemory`, the server's `reply`
    succeeds.
12. **Transferred and lent pages** are mapped read-write in the receiver.
13. **The server of a call dies** (thread or process): the caller gets `Dead` and its lend back.
    Blocked senders stay queued (the endpoint survives; INIT.md's restart relies on that).
14. **Calls in flight to a destroyed endpoint** fail with `Dead`, and their lends stay with the
    server as in R3 (unmapping them could fault a server that did nothing wrong).
15. **Endpoints** live until their owner budget is destroyed; closing the last handle does not free
    one.
16. **The last `thread_exit`** ends the process as `process_exit(0)`.
17. **Deadlines** are absolute µs (`FOREVER` = none); a deadline already past destroys the budget at
    the end of the call that created it (the returned handle is then closed).
18. **Weight 0** (other than a revocation scope): the budget's threads never run.
19. **IRQ sources start masked**; the first `receive` unmasks. A line raised while masked stays
    pending and fires at the next unmask.
20. **`init`** lives in `root`, has no exit endpoint, and gets handles 1-3 = root, system, users,
    then the devices. Its own objects are charged to `root`; `root` keeps what `system` and `users`
    do not take.
21. **Budget usage** returns (page limit, pages used, process limit, processes used), as
    `redoubt-sys` does.
22. **Policy numbers** the design leaves open: `PENDING_CAP` 4, `DECLASSIFY_MAX` 256 bytes of
    printable ASCII, `FIELD_CAP` 64 characters, session and lease sizes; the approver of a request
    is the requester's own principal; the blame window is three crashes within ten minutes of each
    other, and the count restarts after a logout.

## Spec problems

Found while building the model; none is worked around silently. Each needs a decision in
KERNEL-SPEC.md or the policy notes.

1. **Object costs are not specified.** R6 charges every object in pages, but no size is given for a
   budget, process, thread, endpoint, handle table or page table, so `budget_usage` results and
   `OutOfMemory` points cannot be compared between the model and the kernel. The model takes them
   as parameters (`Costs`) written into each trace; the kernel must report its own, and the spec
   should say at least how page tables are charged (per mapping they depend on the address
   layout; the model counts none beyond the process's fixed cost, which undercharges an attacker
   who maps a device or tiny regions many times).
2. **Exit notices outlive what pays for them.** "One pending exit slot" per process suggests a
   notice lives in its process until received, but R10's `killed` notices must outlive the budget
   (and so the process) being destroyed. Nothing then pays for pending notices: a budget that
   repeatedly creates and destroys children whose exit endpoint nobody receives on grows kernel
   memory without bound. The model queues notices on the endpoint, uncharged. Options: charge
   the notice to the endpoint's owner, or cap pending notices per endpoint and drop the oldest.
3. **The per-account cap on pending approval requests is a channel out of a vault.** CONTAINMENT.md
   applies no write-down to the steward's records, but a vault session (`alice+X`) and Alice's
   unlabelled session share one account, so the vault's pending requests change whether the
   unlabelled session's next request is refused at the cap: one bit per request, out of the
   label. P10 therefore observes only other principals. Fix: count the cap per (account, label
   set).
4. **Exit notices and `budget_usage` have no system-class exemption** (R1 exempts only messages).
   As written, `init` and the steward (unlabelled, system) never receive exit notices of labelled
   budgets and cannot read their usage, so crash blame and restarts cannot see labelled processes.
   The model follows the spec; if system servers are meant to see them, R1 and `budget_usage`
   should say so.
5. **The error for most checks is not named** (choices 1-3), and neither is the order of checks
   within a call; the order above must become the spec's, or conformance must accept any error a
   faulty call could return.
6. **`process_start`'s handle list has no cap.** The model bounds it only by the child's page limit
   (the table is charged to the child); the ABI copies the list from user memory, so the kernel
   also needs a bound on how much it copies in one call.
7. **Receive while serving.** The spec's "the account of the message it is serving" (singular) and
   I5's per-thread lend bound imply one served call per thread, but nothing says what `receive`
   does when a thread already serves one (choice 6).
8. **Which receiver R1 checks** when several threads of different budgets wait on one endpoint is
   unspecified (choice 9).
9. **A transfer larger than the receiver's free pages** has no stated outcome (choice 10);
   `Refused` tells the sender one bit about the receiver's budget, which R7 tries to avoid.
10. **Blocked senders when a server dies.** INIT.md says that when a server exits, "calls in
    flight and blocked senders get `Dead`", but KERNEL-SPEC.md says the endpoint survives the
    death of processes receiving on it and fails blocked calls only when the endpoint is
    destroyed. The model follows KERNEL-SPEC.md (choice 13): calls the dead server had taken get
    `Dead`; senders still queued wait for the restarted server.
11. **Who may create a system-class budget.** `budget_create` allows class `system` whenever the
    parent is `system`, whatever the caller's class; a user-class process that somehow holds a
    handle to a system budget can create system-class children (they run before every user
    budget, R12). Probably intended (handles are authority), but worth stating, since adding
    labels, by contrast, checks the caller's class.
