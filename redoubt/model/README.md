# redoubt-model: the executable security model

WP-M0 (planning/redoubt/BUILD-PLAN.md). A host Rust crate that implements every object, system
call, error, rule (R1-R12, R4a, R4b) and invariant (I1-I14) of `planning/redoubt/KERNEL-SPEC.md`,
with the same names and arguments, plus the steward's milestone 1 policy as a layer above it;
property tests over random operation sequences; deliberate rule breaks that show the tests are not
vacuous; and a trace format that WP-C1 replays on the real kernel. KERNEL-SPEC.md owns the
semantics; where the model and the spec differ, the model is wrong.

The library is `no_std` + `alloc`, has no dependencies and no `unsafe` (`#![forbid(unsafe_code)]`).

## Running it

    cargo test -p redoubt-model --release                  # everything (20,000 sequences per family; ~1 min)
    cargo test -p redoubt-model --release --test properties -- --ignored million   # the 10^6 acceptance run
    REDOUBT_MODEL_SEQUENCES=1000000 cargo test -p redoubt-model --release --test properties   # the same, via the variable
    cargo test -p redoubt-model --release --test mutations -- --nocapture   # which property catches each rule break
    REDOUBT_MODEL_MUTATIONS=R10,Policy cargo test -p redoubt-model --release --test mutations -- --nocapture   # only those

A failing kernel sequence is reported with its seed, shrunk to the ops that matter, and printed as
a trace (below) that replays the failure. Use `--release`: debug builds are about 20 times slower.
The workspace keeps overflow checks on for this crate in release builds (root `Cargo.toml`), so an
arithmetic overflow in the model is still an I14 failure there.

## Files

| File | What |
| --- | --- |
| `src/spec.rs` | KERNEL-SPEC.md's constants, errors, class, cause, counters; the ABI encodings (`redoubt-sys`'s values; `NO_HANDLE`, the one place the "no handle" sentinel is named) |
| `src/syscall.rs` | the 24 calls as data (`Syscall`, `CALL_NAMES` in the spec's order), their results, and the other events (`Op`) |
| `src/kernel.rs` | the kernel: objects and their costs, the calls (one method per call, the spec's names), R1-R12, time, IRQs |
| `src/sched.rs` | R12: two classes, stride over budgets, round-robin threads |
| `src/ghost.rs` | records the invariants need, taken from primary objects at each event (and I11's bookkeeping) |
| `src/invariants.rs` | `Checker`: I1-I9, I12, I13 and the per-rule checks, recomputed from the objects and the ghost |
| `src/check.rs` | the kernel's property families (`kernel_sequence`, `budget_lifecycle`, `flood`, `scheduler_fairness`), shrinking, the conformance epilogue |
| `src/gen.rs` | the seeded generator (splitmix64) of random sequences |
| `src/mutation.rs` | the deliberate rule breaks, one switch each |
| `src/steward.rs` | the steward's M1 policy, running as processes on the kernel model |
| `src/policy.rs` | the policy's properties P1-P10 and their families |
| `src/trace.rs` | the trace format: write, parse, replay-and-compare |
| `tests/` | `properties` (all families), `mutations` (every break caught), `coverage` (every call succeeds and every error occurs), `traces` (round trip, replay against a broken kernel, hostile traces, the example) |
| `traces/` | an example trace (R3: a lender dies mid-call) |

## What is modelled, and how abstractly

- **Memory** is page frames with one abstract word of content each, and per-process maps from
  virtual page to (frame or device page, flags, state). User loads, stores and instruction fetches
  are events (`Op::Read`, `Op::Write`, `Op::Exec`) that fault unless the page allows them; a fault
  ends the process with an exit notice `faulted`.
- **Charging** (R6) follows KERNEL-SPEC.md's cost table (`kernel::Costs`, written into every
  trace). Page tables are counted as Sv39 lays them out: a root per process, one middle table per
  GiB and one leaf table per 2 MiB that holds a mapping (choice 20). The property tests boot with 8
  handles per handle-table page instead of 128 (`Boot::testing`), so that table growth happens in
  short sequences.
- **Time** is logical: it advances only in `Op::Tick` (at most `MAX_TICK`, an hour), during which
  the scheduler (R12) runs threads and timeouts and budget deadlines fire. System calls are
  instantaneous and made by whichever runnable thread the sequence names; the scheduler decides
  only how CPU time is shared.
- **Interrupts**: `Op::Irq { n }` raises line `n` (level-style: a raised line stays pending until
  it fires).
- **The ABI's records** (message bodies, budget specs, handle lists, the receive record, the
  `random` buffer) are not modelled as memory: their contents are the call's fields, and their own
  checks (alignment, lying in the caller's memory) are the kernel's and not modelled.
- **Arguments** have the spec's names (`h`, `budget`, `exit_endpoint`, ...); `redoubt-sys` uses
  more descriptive names for the same registers, which is fine: order and meaning are the same.
- **Invariant checking** recomputes each budget's usage from the objects charged to it, each
  frame's payer from where it is mapped, each handle's stamp from how it was made, each delivered
  message from how it was sent, and so on, after every step; see `invariants.rs`.

## Order of checks

KERNEL-SPEC.md, "Errors and the order of checks", owns which error a call returns and in which
order it checks; the model follows it, including the two stated exceptions (weight over the
parent's free weight is `InvalidArgument`; `mint` from a message whose endpoint or stamp is gone is
`Dead`). Each call's method in `kernel.rs` checks in the spec's row order. Details the model makes
concrete:
- decoding treats a lend or transfer as its two registers: (0, 0) is none, and exactly one of them
  0 is `InvalidArgument`; the registers are decoded before the record (the message body);
- an optional-handle slot holding `NO_HANDLE` (0) is none (`Some(0)` in the model's `Syscall` means
  the same as `None`).

## Trace format

A trace is ASCII text, one record per line, tokens separated by single spaces. Lines starting
with `#` are comments. It is written by `trace::record` and read by `trace::parse`; `trace::check`
replays a trace on the model and requires the same text back. `trace::tokens` splits a line into
`Token`s and needs only `core` and `alloc`; nesting is fixed and shallow (a field's value may be a
list, a list holds simple tokens), so hostile text cannot make it recurse, and a hostile trace gets
an error, never a panic or a hang (`tests/traces.rs`).

### Tokens

| Token | Meaning |
| --- | --- |
| `123`, `0x7b` | a literal number (`u64`) |
| `-` | none |
| `forever` | `FOREVER` |
| `[x,y,...]` | a list of simple tokens (no spaces) |
| `key=value` | a named field; the value is a simple token or a list |
| `BASE@PAGES` | a page range (a lend or transfer): `BASE` is a number or an address name |
| `h:N` | a handle index, in the process the record is about |
| `a:0xBASE+0xOFF` | an address `OFF` bytes into a region whose base the kernel returned to that process (`ok a:0xBASE` names the region) |
| `p:N`, `t:N`, `m:N`, `pa:0xN` | a pid, a tid, a message id, a physical address |
| `tm:N` | a time the kernel read (`time_now`) |
| anything else | a word: a call name, `ok`, `err`, an error name, `user`, `system`, `call`, `send`, ... |

**Names** (`h:`, `a:`, `p:`, `t:`, `m:`, `pa:`) are values the kernel chooses. A replayer keeps a
map per kind (per process for `h:` and `a:`): **a name in a result binds it** to the real value
returned (rebinding if it was bound before; handle indices and addresses are reused), and **a
name in an argument is looked up**. A name used in an argument that was never bound stands for
"a value that is not valid here", and the replayer substitutes one it knows to be invalid (for
handles, an index it has not allocated, such as `u32::MAX`; for message ids, `u64::MAX - 1`).
Handle values that fail decoding (0, or over `u32::MAX`) are written as literals and passed as they
are. **Times** (`tm:`) are compared only for order: each time a replayer's kernel returns must be
at least the previous one.

Literal addresses are ones the kernel did not choose. The model places its own mappings above
everything a process has mapped, from `0x10_0000_0000` (`kernel::KERNEL_CHOSEN_BASE`), and keeps
user space below `1 << 38`. The generator's literal addresses are: `0x1234` (unaligned),
`0x800_0000` (unmapped), `0x1000_0000`-`0x1000_3000` (`process_map` destinations),
`0xffff_ffff_ffff_f000` (outside user space), and, in 4% of calls (`Gen::hostile_syscall`), any
value at all (0, small numbers, page multiples, `(1 << 38) - 0x1000`, powers of two, `u64::MAX`,
random). A kernel whose layout maps something at a literal address must move it, or leave those
traces out.

Bindings the replayer gets without a result: `init` is `p:1`, its thread `t:1`, and its handles
are `h:1` root, `h:2` system, `h:3` users, then one per `device` line in order (`h:4`, ...). After
`process_start`, the child's `h:1`..`h:n` are the handles listed, in order (KERNEL-SPEC.md fixes
the slots).

### Records

    redoubt-model-trace 1                                     the header, first line
    boot root=[P,N,W] system=[P,N,W] users=[P,N,W]            the three budgets' pages, processes, weight
    costs budget=N process=N thread=N endpoint=N handles_per_page=N page_table=N open_call=N exit_slot=N
    device mmio base=0xN pages=N dma=0|1                      one per device object, in init's order
    device irq n=N
    device reset
    start p:1 t:1                                             init
    do p:P t:T CALL ARGS... -> RESULT                         a system call by thread T of process P
    write p:P t:T ADDR VALUE -> ok|fault                      a user store of one word
    read p:P t:T ADDR -> ok word N|fault                      a user load
    exec p:P t:T ADDR -> ok|fault                             an instruction fetch
    fault p:P t:T                                             the thread faults (the process ends, `faulted`)
    irq N                                                     interrupt line N is raised
    tick DT                                                   at least DT µs pass
    note process p:CREATOR h:H p:NEW                          process_create's handle H names process NEW
    note thread p:P t:T                                       process_start started P with thread T
    wake p:P t:T -> RESULT                                    a blocked call returned, caused by the record above

`note` and `wake` records follow the event that caused them, in order. A replayer drives each
event from the named thread and compares:
- the result of a `do`. `blocked` means the call has not returned by the time the replayer goes on
  to the next record: the replayer gives it a grace period, and a return before its `wake` record is
  a mismatch. `gone` means the thread no longer exists (`thread_exit`, `process_exit`, destroying
  its own budget, a fault);
- then each `wake`: the named thread's pending call must have returned that result;
- `tick DT` means at least DT µs pass (a kernel cannot stop time; the model's timeouts fire in
  order, so a replayer that waits longer still sees the same results, as long as nothing else is
  scheduled by time).

A thread named by a record is always runnable at that point.

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
    process_map H SRC DST LEN FLAGS
    process_start H ENTRY SP [H,...]
    endpoint_create
    mint SOURCE BADGE BUDGET                  SOURCE: m:N (a message) or h:N; BUDGET: h:N or - (none)
    call H [W,W,W,W] [H,...] LEND TIMEOUT     LEND: BASE@PAGES or -
    send H [W,W,W,W] [H,...] TRANSFER TIMEOUT
    receive H|- TIMEOUT MAX_TRANSFER          max_transfer in pages
    reply m:N [W,W,W,W] [H,...]
    handle_close H
    budget_create H PAGES PROCESSES WEIGHT CLASS [LABEL,...] ACCOUNT DEADLINE    class: user, system, or a raw tag; deadline absolute µs or forever
    budget_destroy H
    budget_usage H
    time_now
    random LEN
    system_reset H KIND                       kind: 1 power off, 2 reboot

Results:

    ok                                        no value
    ok a:0xBASE                               map_anon, map_device (binds a region)
    ok a:0xBASE pa:0xPHYS                     dma_alloc
    ok t:N | ok h:N                           thread_create | process_create, endpoint_create, mint, budget_create
    ok reply words=[...] handles=[h:...]      call
    ok message call|send m:N badge=N account=N labels=[...] words=[...] handles=[h:...] buffer=-|[lend,a:0xBASE,PAGES]|[transfer,a:0xBASE,PAGES]
    ok interrupt h:N                          receive on an IRQ handle
    ok exit p:N cause=exited|faulted|killed code=N blamed=N
    ok usage [PAGES_LIMIT,PAGES_USAGE,PROCESSES_LIMIT,PROCESSES_USAGE,WEIGHT_LIMIT,WEIGHT_USAGE]
    ok time tm:N | ok random LEN | ok word N
    err NAME                                  one of KERNEL-SPEC.md's errors
    blocked | gone

**What a replay can compare exactly.** Page counts (`usage`, and where `OutOfMemory` strikes)
depend on the costs line and on where the kernel places mappings, since page tables are charged as
allocated. A kernel that uses the model's costs and placement (choice 20) matches exactly; one that
does not can still compare everything but `PAGES_USAGE` and page-limit edges.

**Conformance traces end with an epilogue** (`check::epilogue`): results show a divergence only
once something depends on it. The epilogue lets every blocked call with a timeout return; every
process that can run reads its pages and its budgets' usage; `init` destroys the budgets it made
one at a time, reading the usage of those it still holds after each; and every process still alive
closes handle indices 1 to 64. `tests/traces.rs` replays 2,000 random traces with it against a
kernel breaking one rule at a time: every break of R1-R11 and of the other kernel statements fails
some replay, except three no random trace shows: R12 (scheduling shows only in timing; WP-K5's
tests), an IRQ source left unmasked (every result is the same; WP-K3 must test the mask), and
`MAX_OPEN_CALLS` (random traces never reach 64 open calls; the flood family's do).

Example: `traces/lender-dies-mid-call.trace`.

## Property tests

| Family | Checks |
| --- | --- |
| `kernel_sequence` | after every step of a random sequence: I1-I9, I12, I13, and the rule checks (R2's cap and groups, R3's lend staying mapped, R4, R4a, R4b, R5, R10's liveness, R12's class order at every pick); every delivered message is the one sent (kind, badge, account, labels from the sender's handle and budget) and went through a receive right; every mint's source is one the minter holds or serves; every owed exit notice is waiting |
| `budget_lifecycle` | I10: create a budget, let only its own processes act, destroy it (and receive its processes' exit notices, whose slots the creator paid): every other budget's usage is unchanged |
| `flood` | PLAN.md's "10,000 blocked senders on `fsd`, and Alice is still served in her turn" on a big boot: most senders get `Busy`, Alice's call is taken within as many receives as there are groups (I11); hoarding servers reach `MAX_OPEN_CALLS` (R4a). A thousandth as many seeds as the others |
| `scheduler_fairness` | R12: class order; every always-runnable user budget gets its weight's share of user CPU time over every interval, within `SLICE x (2 + 2 w/w_min + n)`, against sleepers that sleep long and then burst |
| `steward_policy` | P1-P9 (policy.rs) and the kernel invariants underneath |
| `steward_noninterference` | P10: the vault sessions' work changes nothing an unlabelled session observes (10,000 sequences by default, a half of the others: each runs three steward boots) |

Random sequences are generated from a small world the generator builds first (principals' budgets
with accounts and labels, a system server, shared endpoints, handles minted into budgets), a relay
that hands an endpoint owned by a user budget to processes in other label sets, then random calls
biased toward what each actor holds; 4% of calls take arguments drawn without regard to the state
(I14). Now and then a flood, a thread bomb, or a budget destroyed while messages are in flight.

## Deliberate rule breaks

`src/mutation.rs` has 58 mutations: 33 break R1-R12 (every rule at least once), 2 break R4a (the
open-call limit, and a `receive` forgetting open calls), 10 break other statements of the spec
(what messages carry, exit notices, the served account, deadlines, the receive right, `mint`'s
source, the answers to QUESTIONS 9 and 12), and 13 break the steward's policy. `tests/mutations.rs`
requires each to fail some property within 20,000 sequences (flood: 20) and prints which one.

## The owner's answers the model implements

QUESTIONS.md 1-27 were answered (planning/redoubt/ANSWERS.md) and are in KERNEL-SPEC.md. The model
follows them: 1 (`receive` returns the message kind; `reply` to a `send` is `InvalidArgument`),
2 (open calls, R4a), 4 (R1's receiver is the endpoint's owner), 5 (R4), 6 (R4b), 7 (the exit
slot), 8 (system-class exemptions), 9 (system-class children need a system caller), 10 (handle 0 is
none; `MAX_START_HANDLES`), 11 (the counters), 12 (weight 0 holds no process), 13 (the cost
table), 14 (the spec owns the order of checks), 15 (`MAX_RANDOM`; W+X and badge 0 refused at
decoding), 17 (groups and caps by (account, label set)), 18 (unpredictable steward ids).

## Interpretation choices

Where the spec still leaves a detail open, the model chose as follows; the code says
`(README choice N)` where it applies one. Numbers are stable: a choice the spec has since settled
says so and stays in the list.

1. Which error a check returns: now the spec's (QUESTIONS 14).
2. Weight over the parent's free weight: `InvalidArgument` (now the spec's stated exception).
3. Labels not ⊇ the parent's in `budget_create`: `LabelDenied` (now in the spec's table).
4. **Account (R8)**: when the parent's account is non-zero, the argument is ignored (as `redoubt-sys`
   documents).
5. **Handle indices**: 0 is never allocated (now the spec's); a new handle takes the lowest free
   index from 1.
6. **Serving** (QUESTIONS 37, open): a thread may hold several open calls (R4a); `reply` and `mint`
   from a message act for the thread that took it; a `send`'s message is served (for `mint`) until
   that thread takes another message.
7. **The served account** (QUESTIONS 31 and 37, open): the account of the newest message the thread
   still serves; `reply` recomputes it; a `send` sets it too.
8. **Exit notices before messages** when both are pending on an endpoint.
9. R1's receiver: now the spec's (the endpoint's owner, QUESTIONS 4).
10. **A send the receiver cannot pay for** except in frames (its handles, the page tables to map
    its transfer): the receiver's `receive` gets `OutOfMemory` and the message stays queued, as R4a
    says for calls. (R4 settles the frames: `Refused`.)
11. **The reply's handles do not fit the caller**: the caller's `call` gets `OutOfMemory` (the
    spec's `call` row); the server's `reply` succeeds.
12. **Lent and transferred pages** are mapped read-write in the receiver.
13. A server dies: now the spec's (R4b).
14. **Calls in flight to a destroyed endpoint** fail with `Dead` (R10), and their lends stay with
    the server as in R3 (unmapping them could fault a server that did nothing wrong).
15. **Endpoints** live until their owner budget is destroyed; closing the last handle does not free
    one.
16. **The last `thread_exit`** ends the process as `process_exit(0)`.
17. **A deadline already past** destroys the budget at the end of the call that created it (the
    returned handle is then closed).
18. **IRQ sources start masked**; the first `receive` unmasks. A line raised while masked stays
    pending and fires at the next unmask. The oldest waiter gets a fire; with several threads
    waiting on one IRQ, the source is masked after one is given an interrupt, so a second event
    waits for the next `receive` to begin (nothing is lost; a driver should wait with one thread).
19. **`init`** lives in `root`, has no exit endpoint, and gets handles 1-3 = root, system, users,
    then the devices. Its own objects are charged to `root`; `root` keeps what `system` and `users`
    do not take.
20. **Page tables** (QUESTIONS 13 says "as allocated"): Sv39's layout; the root is allocated with
    the process (`process_create` charges it with the process object); a middle or leaf table is
    allocated when a mapping first needs it and freed when it maps nothing; a lent-out page keeps
    its reservation, so its tables stay. The kernel chooses addresses above everything the process
    has mapped, from `KERNEL_CHOSEN_BASE`.
21. **An exit slot whose payer is destroyed** goes with it (the spec: "freed with the creator's
    budget"): a notice it would have held is not sent.
22. **Steward ids** are random (keyed), and session names count per principal (QUESTIONS 18).
23. **Policy numbers** the design leaves open: `PENDING_CAP` 4, `DECLASSIFY_MAX` 256 bytes of
    printable ASCII, `FIELD_CAP` 64 characters, `MAX_LEASE` 24 h (QUESTIONS 33, pending), session
    and lease sizes; a sub-agent gets half its agent's free pages; the approver of a request is the
    requester's own principal; the blame window counts crashes less than ten minutes old, and the
    count restarts after a logout.
24. **A receiver waiting while its process reaches `MAX_OPEN_CALLS`** (other threads took calls
    meanwhile): the call is not delivered to it; its `receive` gets `Busy` and the call stays
    queued.

## Spec problems and open questions

Found while building the model; none is worked around silently.

1. **I10 and exit slots.** "Creating and then destroying a budget leaves its parent's usage and free
   limits unchanged" does not hold as stated when the creator is in the parent and starts processes
   in the child: their exit slots are charged to the creator (QUESTIONS 7) and stay until the
   `killed` notices are received. The model's I10 test receives them first. I10 should say "once
   the exit notices of its processes are received or dropped".
2. **R4/R4a for sends** (choice 10): R4a says what happens when the receiver cannot pay for a
   call's handles or its lend's page tables; the same for a send's handles and its transfer's page
   tables is unstated, and R4's "free pages to hold them" does not say whether it counts page
   tables.
3. **R4a at delivery** (choice 24): R4a checks `MAX_OPEN_CALLS` when `receive` begins; a thread
   already waiting when its process reaches the limit is not covered.
4. **I7 and R1's owner (QUESTIONS 4).** R1 checks the endpoint's owner; a receive right held by a
   process in another budget (the owner can hand it on) receives messages whose labels were never
   compared with its own. The model's I7 checks the owner, as R1 says; whether I7 should also cover
   the receiving budget, or receive rights should not leave their owner's label set, is open.
5. **Page tables "as allocated"** (choice 20): when page tables are freed, and where the kernel
   places mappings, decide the page counts; neither is stated, so a replay compares usage exactly
   only if the kernel does what the model does.
6. **Crash blame per account is a channel out of a vault.** Blame counts crashes by account, and a
   vault session shares its owner's account: a vault session that crashes a shared server three
   times logs out its owner's unlabelled sessions too. QUESTIONS 17 keyed the caps by label set for
   this reason; blame is still by account. The model's P10 leaves crashes out.
7. **Where the lends of in-flight calls go when their endpoint is destroyed** (choice 14): R10 says
   the calls fail with `Dead`; the lends are not mentioned.
8. **Open questions the model keeps its behaviour for**: 30 (revocation does not reach messages
   already in flight: a queued message sent through a handle stamped with a destroyed budget is
   still delivered, and the red team's mutation for it is not in the list), 31 (a `send` sets the
   served account), 32 (only queued messages count for `WAIT_CAP`), 33 (`MAX_LEASE` 24 h), 34
   (rendering: done as recommended, a printable-ASCII whitelist with the session's kind and name),
   35 (a labelled session's free text still reaches the approval screen, capped), 37 (open calls
   and the served account, choice 7), 38 (not the model's).
