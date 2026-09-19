# redoubt-model: the executable security model

WP-M0 (planning/redoubt/BUILD-PLAN.md). A host Rust crate that implements every object, system
call, error, rule (R1-R12, R4a, R4b) and invariant (I1-I15) of `planning/redoubt/KERNEL-SPEC.md`,
with the same names and arguments, plus the steward's milestone 1 policy as a layer above it;
property tests over random operation sequences; deliberate rule breaks that show the tests are not
vacuous; and a trace format that WP-C1 replays on the real kernel. KERNEL-SPEC.md owns the
semantics; where the model and the spec differ, the model is wrong.

The library is `no_std` + `alloc`, has no dependencies and no `unsafe` (`#![forbid(unsafe_code)]`).

## Running it

    cargo test -p redoubt-model --release                  # everything (20,000 sequences per family; ~10 min, most of it the mutation list)
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
| `src/syscall.rs` | the 25 calls as data (`Syscall`, `CALL_NAMES` in the spec's order), their results, and the other events (`Op`) |
| `src/kernel.rs` | the kernel: objects and their costs, the calls (one method per call, the spec's names), R1-R12, time, IRQs |
| `src/sched.rs` | R12: `first` budgets before the others, stride over budgets, round-robin threads |
| `src/ghost.rs` | records the invariants need, taken from primary objects at each event (and I11's bookkeeping) |
| `src/invariants.rs` | `Checker`: I1-I9, I11-I13, I15 and the per-rule checks, recomputed from the objects and the ghost |
| `src/check.rs` | the kernel's property families (`kernel_sequence`, `budget_lifecycle`, `flood`, `scheduler_fairness`), shrinking, the conformance epilogue |
| `src/gen.rs` | the seeded generator (splitmix64) of random sequences |
| `src/mutation.rs` | the deliberate rule breaks, one switch each |
| `src/steward.rs` | the steward's M1 policy, running as processes on the kernel model |
| `src/policy.rs` | the policy's properties P1-P13 and their families |
| `src/trace.rs` | the trace format: write, parse, replay-and-compare |
| `tests/` | `properties` (all families), `mutations` (every break caught), `coverage` (every call succeeds and every error occurs), `traces` (round trip, replay against a broken kernel, hostile traces, the example) |
| `traces/` | an example trace (R3: a lender dies mid-call) |

## What is modelled, and how abstractly

- **Memory** is page frames with one abstract word of content each, and per-process maps from
  virtual page to (frame or device page, flags, state). User loads, stores and instruction fetches
  are events (`Op::Read`, `Op::Write`, `Op::Exec`) that fault unless the page allows them; a fault
  ends the process with an exit notice `faulted`.
- **Charging** (R6) follows KERNEL-SPEC.md's cost table (`kernel::Costs`, written into every
  trace): a budget's own page to its parent, a process object to its creator's budget (it outlives
  the process until its exit notice is received or dropped), a lend to both sides while its call is
  open. Page tables are counted as Sv39 lays them out: a root per process, one middle table per GiB
  and one leaf table per 2 MiB that holds a mapping (choice 20). The property tests boot with 8
  handles per handle-table page instead of 128 (`Boot::testing`), so that table growth happens in
  short sequences.
- **Time** is logical: it advances only in `Op::Tick` (at most `MAX_TICK`, an hour), during which
  the scheduler (R12) runs threads and timeouts and budget deadlines fire. System calls are
  instantaneous and made by whichever runnable thread the sequence names; the scheduler decides
  only how CPU time is shared. A tick with one budget runnable and nothing due charges its whole
  slices at once (the same passes as slice by slice), so a long tick costs the model little.
- **Interrupts**: `Op::Irq { n }` raises line `n` (level-style: a raised line stays pending until
  it fires).
- **Randomness**: PIDs are drawn from a deterministic generator standing in for the kernel's CSPRNG
  (`random`'s value is not produced at all). The model cannot judge unpredictability; a replay binds
  `p:` names to whatever the kernel chose.
- **The ABI's records** (message bodies, budget specs, handle lists, the receive record) are not
  modelled as memory: their contents are the call's fields, and their own checks (alignment, lying
  in the caller's memory) are the kernel's and not modelled.
- **Arguments** have the spec's names (`h`, `budget`, `exit_endpoint`, ...); `redoubt-sys` uses
  more descriptive names for the same registers, which is fine: order and meaning are the same.
- **Invariant checking** recomputes each budget's usage from the objects charged to it, each
  frame's payer from where it is mapped, each handle's stamp from how it was made, each delivered
  message from how it was sent, each exit notice's cause and blame from the thread that ended the
  process, and so on, after every step; see `invariants.rs`.

## Order of checks

KERNEL-SPEC.md, "Errors and the order of checks", owns which error a call returns and in which
order it checks; the model follows it, including the two stated exceptions (weight over the
parent's free weight is `InvalidArgument`; `mint` from a message whose endpoint or stamp is gone is
`Dead`). Each call's method in `kernel.rs` checks in the spec's row order. Details the model makes
concrete:
- decoding treats a lend or transfer as its two registers: (0, 0) is none, and exactly one of them
  0 is `InvalidArgument`; the registers are decoded before the record (the message body);
- an optional-handle slot holding `NO_HANDLE` (0) is none (`Some(0)` in the model's `Syscall` means
  the same as `None`);
- `budget_create`'s record is decoded in slot order: processes and weight (32 bits), `first` (0 or
  1), then the label list (`TooLarge` over `MAX_LABELS`).

## Trace format

A trace is ASCII text, one record per line, tokens separated by single spaces. Lines starting
with `#` are comments. It is written by `trace::record` and read by `trace::parse`; `trace::check`
replays a trace on the model and requires the same text back. `trace::tokens` splits a line into
`Token`s and needs only `core` and `alloc`; nesting is fixed and shallow (a field's value may be a
list, a list holds simple tokens), so hostile text cannot make it recurse, and a hostile trace gets
an error, never a panic or a hang (`tests/traces.rs`, which includes `costs` lines whose sums would
overflow: `kernel::check_boot` bounds every cost).

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
| `p:N`, `t:N`, `m:N`, `pa:0xN` | a pid, a tid, a message id (unique within the receiving process), a physical address |
| `tm:N` | a time the kernel read (`time_now`) |
| anything else | a word: a call name, `ok`, `err`, an error name, `call`, `send`, ... |

**Names** (`h:`, `a:`, `p:`, `t:`, `m:`, `pa:`) are values the kernel chooses. A replayer keeps a
map per kind (per process for `h:`, `a:` and `m:`): **a name in a result binds it** to the real
value returned (rebinding if it was bound before; handle indices, addresses and PIDs are reused),
and **a name in an argument is looked up**. A name used in an argument that was never bound stands
for "a value that is not valid here", and the replayer substitutes one it knows to be invalid (for
handles, an index it has not allocated, such as `u32::MAX`; for message ids, `u64::MAX - 1`).
Handle values that fail decoding (0, or over `u32::MAX`) are written as literals and passed as they
are; a handle in a received message that was revoked while it waited arrives as the literal `0`.
**Times** (`tm:`) are compared only for order: each time a replayer's kernel returns must be at
least the previous one.

Literal addresses are ones the kernel did not choose. The model places its own mappings above
everything a process has mapped, from `0x10_0000_0000` (`kernel::KERNEL_CHOSEN_BASE`), and keeps
user space below `1 << 38`. The generator's literal addresses are: `0x1234` (unaligned),
`0x800_0000` (unmapped), `0x1000_0000`-`0x1000_3000` (`process_map` destinations and
`process_start` args), `0xffff_ffff_ffff_f000` (outside user space), and, in 4% of calls
(`Gen::hostile_syscall`), any value at all (0, small numbers, page multiples, `(1 << 38) - 0x1000`,
powers of two, `u64::MAX`, random). A kernel whose layout maps something at a literal address must
move it, or leave those traces out.

Bindings the replayer gets without a result: `init` is `p:1`, its thread `t:1`, and its handles
are `h:1` root, `h:2` system, `h:3` users, then one per `device` line in order (`h:4`, ...). After
`process_start`, the child's `h:1`..`h:n` are the handles listed, in order (KERNEL-SPEC.md fixes
the slots). Every other PID is drawn at random and bound by a `note process` record.

### Records

    redoubt-model-trace 1                                     the header, first line
    boot root=[P,N,W] system=[P,N,W] users=[P,N,W]            the three budgets' pages, processes, weight
    costs budget=N process=N thread=N endpoint=N handles_per_page=N page_table=N open_call=N
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
    process_start H ENTRY SP ARG [H,...]      ARG: the startup page's address, or 0
    endpoint_create
    mint SOURCE BADGE BUDGET                  SOURCE: m:N (an open call) or h:N; BUDGET: h:N or - (none)
    call H [W,W,W,W] [H,...] LEND TIMEOUT     LEND: BASE@PAGES or -
    send H [W,W,W,W] [H,...] TRANSFER TIMEOUT
    receive H|- TIMEOUT MAX_TRANSFER          max_transfer in pages
    reply m:N [W,W,W,W] [H,...]
    serve m:N
    handle_close H
    budget_create H PAGES PROCESSES WEIGHT FIRST [LABEL,...] ACCOUNT DEADLINE    first: 0 or 1; deadline absolute µs or forever
    budget_destroy H
    budget_usage H
    time_now
    random
    system_reset H KIND                       kind: 1 power off, 2 reboot

Results:

    ok                                        no value
    ok a:0xBASE                               map_anon, map_device (binds a region)
    ok a:0xBASE pa:0xPHYS                     dma_alloc
    ok t:N | ok h:N                           thread_create | process_create, endpoint_create, mint, budget_create
    ok reply words=[...] handles=[h:...]      call
    ok message call|send m:N badge=N account=N labels=[...] words=[...] handles=[h:...|0] buffer=-|[lend,a:0xBASE,PAGES]|[transfer,a:0xBASE,PAGES]
    ok interrupt h:N                          receive on an IRQ handle
    ok exit p:N cause=exited|faulted|killed code=N blamed=N blamed_labels=[...]
    ok abandoned m:N                          receive: the thread's open call m:N was abandoned
    ok usage [PAGES_LIMIT,PAGES_USAGE,PROCESSES_LIMIT,PROCESSES_USAGE,WEIGHT_LIMIT,WEIGHT_USAGE]
    ok time tm:N | ok random | ok word N
    err NAME                                  one of KERNEL-SPEC.md's errors
    blocked | gone

**What a replay can compare exactly.** Page counts (`usage`, and where `OutOfMemory` or `Refused`
strikes) depend on the costs line and on where the kernel places mappings, since page tables are
charged as allocated. A kernel that uses the model's costs and placement (choice 20) matches
exactly; one that does not can still compare everything but `PAGES_USAGE` and page-limit edges.

**Conformance traces end with an epilogue** (`check::epilogue`): results show a divergence only
once something depends on it. The epilogue lets every blocked call with a timeout return; every
process that can run reads its pages and its budgets' usage; `init` destroys the budgets it made
one at a time, reading the usage of those it still holds after each; and every process still alive
closes handle indices 1 to 64. `tests/traces.rs` replays 2,000 random traces with it against a
kernel breaking one rule at a time: every break of R1-R11 and of the other kernel statements fails
some replay, except the ones no random trace shows: R12 (scheduling shows only in timing; WP-K5's
tests), an IRQ source left unmasked (every result is the same; WP-K3 must test the mask), and the
three breaks of `MAX_OPEN_CALLS` (random traces never reach 64 open calls; the flood family's do).

Example: `traces/lender-dies-mid-call.trace`.

## Property tests

| Family | Checks |
| --- | --- |
| `kernel_sequence` | after every step of a random sequence: I1-I9, I12, I13, I15 and the rule checks (R2's cap and groups, R3's lend, R4's delivery, R4a, R4b, R5, R10's reach, R12's order at every pick); every delivered message is the one sent (kind, badge, account, labels and handles from the sender's handles and budget; a revoked handle arrives as 0) and went through a receive right; every mint's source is one the minter holds or an open call of its thread; every refusal (`LabelDenied`, `Busy`) is one the rule calls for; every exit notice reports the cause and blame the ghost expects; nothing deliverable waits while a receiver waits for it |
| `budget_lifecycle` | I10: create a budget, let only its own processes act, destroy it (and receive its processes' exit notices, whose objects the creator paid for): every other budget's usage is unchanged |
| `flood` | PLAN.md's "10,000 blocked senders on `fsd`, and Alice is still served in her turn" on a big boot: most senders get `Busy`, two account-0 groups keep their own caps (QUESTIONS 87), Alice's call is taken within as many receives as there are groups (I11); hoarding servers (two threads) reach `MAX_OPEN_CALLS` for the process and still take a send (R4a). A thousandth as many seeds as the others |
| `scheduler_fairness` | R12: `first` budgets first; every always-runnable budget without `first` gets its weight's share over every interval, within `SLICE x (2 + 2 w/w_min + n)`, against sleepers that sleep long and then burst |
| `steward_policy` | P1-P9, P11-P13 (policy.rs) and the kernel invariants underneath |
| `steward_noninterference` | P10: the vault sessions' work changes nothing an unlabelled session observes |

Random sequences are generated from a small world the generator builds first (principals' budgets
with accounts and labels, a system budget that may be labelled and `first`, shared endpoints,
handles minted into budgets), a relay that hands an endpoint owned by a user budget to processes in
other label sets, then random calls biased toward what each actor holds (`serve` and `reply` of its
open calls among them); 4% of calls take arguments drawn without regard to the state (I14). Now and
then a flood, a thread bomb, or a budget destroyed while messages are in flight.

## Deliberate rule breaks

`src/mutation.rs` has 95 mutations: 55 break R1-R12 (every rule at least once, R3's abandoned-call
notice and R4a/R4b included), 16 break other statements of the spec (what messages carry, message
ids, blame and the current call, `serve`, class and `first`, deadlines, the receive right, badged
exit endpoints, `mint`'s source, the open-call limit, weight-0 budgets), and 24 break the steward's
policy. Twelve came from the red team's round-3 switches (all now permanent, all caught).
`tests/mutations.rs` requires each to fail some property within 20,000 sequences (flood: 20) and
prints which one.

## The owner's answers the model implements

All of QUESTIONS.md 1-101 that concern the kernel or the steward's policy, as KERNEL-SPEC.md,
CAPABILITIES.md, CONTAINMENT.md and INIT.md now state them. Among them: open calls and R4a (2),
R1 against the endpoint's owner (4, 46), the cost table (13), groups and caps by (account, label
set) (17) and by budget for account 0 (87), revocation of messages already sent (30, 86), sends
never served (31), `WAIT_CAP` counting queued messages (32), `MAX_LEASE` the steward's (33, 63, 78),
rendering (34, 35), `process_start`'s `arg` (40), blame by (account, label set) with
`blamed_labels` (48, 61), equal labels for every write (51), declassification through a reader
budget (54), an exit holding open calls reported `faulted` (55), check-by-use handle kinds (56),
badge notices removed (69), lends charged to both sides (70), one delivery failure: `Refused` (72),
class inherited and `first` (73, 103), the process object as the exit slot (74), a budget's own page
to its parent (76), `random` returning a u64 (77), system budgets held only by init and the steward
(79), connections narrowed to scopes (80), the abandoned-call notice (81, 104, 105, 109), `serve` and
the current call (82, 110), per-process message ids and random PIDs (88), fixed sub-budgets per
label set (89), fair shares and ending a lease (90), lockout after blame (91), labelled audit
records (92), exit endpoints with badge 0 (93).

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
6. Serving: now the spec's (open calls, the current call; QUESTIONS 82).
7. The served account: replaced by the current call (QUESTIONS 82).
8. **Notices before messages**, in this order: a waiting thread's abandoned-call notices (to that
   thread), then exit notices in the order queued (to the first receiver).
9. R1's receiver: now the spec's (the endpoint's owner, QUESTIONS 4).
10. A delivery the receiver cannot pay for: now the spec's (`Refused`, QUESTIONS 72).
11. **The reply's handles do not fit the caller**: the caller's `call` gets `OutOfMemory` (the
    spec's `call` row; QUESTIONS 107); the server's `reply` succeeds.
12. **Lent and transferred pages** are mapped read-write in the receiver.
13. A server dies: now the spec's (R4b).
14. Calls in flight to a destroyed endpoint: now the spec's (abandoned, R3, R10).
15. **Endpoints** live until their owner budget is destroyed; closing the last handle does not free
    one.
16. **The last `thread_exit`** ends the process as `process_exit(0)` would (so `faulted` if the
    process holds open calls).
17. **A deadline already past** destroys the budget at the end of the call that created it (the
    returned handle is then closed).
18. **IRQ sources start masked**; the first `receive` unmasks. A line raised while masked stays
    pending and fires at the next unmask. The oldest waiter gets a fire; with several threads
    waiting on one IRQ, the source is masked after one is given an interrupt, so a second event
    waits for the next `receive` to begin (nothing is lost; a driver should wait with one thread).
19. **`init`** lives in `root`, has no exit endpoint, and gets handles 1-3 = root, system, users,
    then the devices. Its own objects are charged to `root`; `root` keeps what `system` and `users`
    (and their own pages) do not take.
20. **Page tables** (QUESTIONS 13 says "as allocated"): Sv39's layout; the root is allocated with
    the process (`process_create` charges it to the process's budget); a middle or leaf table is
    allocated when a mapping first needs it and freed when it maps nothing (now the spec's, R11); a
    lent-out page keeps its reservation, so its tables stay. The kernel chooses addresses above
    everything the process has mapped, from `KERNEL_CHOSEN_BASE`.
21. **A process object whose payer is destroyed** goes with it: a notice it held is not sent (now
    the spec's, QUESTIONS 74).
22. **Steward ids** are random (keyed), and session names count per principal (QUESTIONS 18).
23. **Policy numbers** the design leaves open: `PENDING_CAP` 4, `DECLASSIFY_MAX` 256 bytes of
    printable ASCII, `FIELD_CAP` 64 characters, session and lease sizes, a principal's 12 processes
    split evenly between its sub-budgets; a sub-agent gets half its agent's free pages; the approver
    of a request is the requester's own principal; the blame window counts crashes less than ten
    minutes old, the count restarts after a logout, and the lockout lasts ten minutes from the third
    crash; a session's fair share is `PENDING_CAP` divided by the live sessions of its bucket, at
    least one; any unlabelled session of the lease's principal counts as its sponsor.
24. At `MAX_OPEN_CALLS`: now the spec's (calls stay queued; sends and notices are delivered).
25. Withdrawn (badge notices were removed, QUESTIONS 69).
26. **Delivery happens once a step's other effects are done**: an endpoint on which something
    became deliverable is matched with its receivers at the end of the step (and inside a tick,
    after each timeout), so a destruction revokes everything it reaches before anything is
    delivered.
27. **`init`'s PID is 1**, not drawn, so a trace can name it before any result; every other PID is
    drawn at random from 2..=0xffff (Sv39 ASIDs). Running out of PIDs is `OutOfProcesses`.
28. **R2's turns at `MAX_OPEN_CALLS`**: a group whose oldest message is a call offers its oldest
    send instead; a message goes to the first waiting receiver whose process can take it.
29. **A `process_exit` reported `faulted`** (the process held open calls) keeps its code.
30. **The server's endpoint is created by `init`** in the steward's world (INIT.md), so its receive
    right is stamped `root` and the steward can narrow connections to scopes under `users`
    (`mint` only narrows to the default stamp's descendants).

## Spec problems and open questions

Found while building the model; none is worked around silently.

1. I10 and exit notices: now the spec's ("once its processes' exit notices are received or
   dropped").
2. R4 for sends: now the spec's (QUESTIONS 72).
3. R4a at delivery: now the spec's (QUESTIONS 81, 105).
4. I7 and R1's owner: now the spec's (a receive right is never handed across label sets; QUESTIONS
   46).
5. **Page tables "as allocated"** (choice 20): where the kernel places mappings decides the page
   counts, so a replay compares usage exactly only if the kernel does what the model does (R11 now
   says so).
6. Crash blame per account: now by (account, label set) (QUESTIONS 48).
7. Lends of calls in flight to a destroyed endpoint: now the spec's (abandoned, R3).
8. **Open questions the model keeps its behaviour for**: 102 (no `MAX_HANDLES`: a handle table
   grows while its budget pays) and 111 (a handle table costs `ceil(handles / 128)` pages, not the
   pages in use; with holes the kernel and the model disagree until the owner decides).
9. **I8 and `users`.** "`class(child) = class(parent)`" does not hold for `users`, which the kernel
   creates class `user` under `root` (class `system`). The model's I8 exempts `users`; I8 should
   say "for budgets made by `budget_create`".
10. **I15 says "exactly once"**, but a thread that replies to an abandoned call before it receives
    again on that endpoint is never told (the call is gone), and one that never receives there
    again is never told either (QUESTIONS 104). The model checks "at most once, and before the
    holding thread waits on that endpoint again".
11. **Narrowing needs a root-stamped receive right** (choice 30). `mint` narrows only to the default
    stamp or below, so a server whose receive right is stamped with its own (system) budget cannot
    narrow a connection to a scope under `users`: QUESTIONS 80's scopes work only because `init`
    creates the servers' endpoints. INIT.md or CAPABILITIES.md should say so.
12. **Budget ids are one counter** (I12: never reused). No call returns one to user mode, so the
    kernel leaks nothing, but anything that prints them to other principals does: the model's audit
    records name an agent's parent session, not its budget id.
