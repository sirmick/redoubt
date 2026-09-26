# Executable security model

Host specification oracle for the Redoubt kernel and selected steward policy contracts. Recovered
from `674831cf9` and reconciled against [KERNEL-SPEC](../docs/KERNEL-SPEC.md),
[CONTAINMENT](../docs/CONTAINMENT.md) and [CAPABILITIES](../docs/CAPABILITIES.md).
The model is independent of kernel source and has no dependencies or unsafe code (`no_std` + alloc).
Passing model tests establishes internal checks of these abstractions. **No trace has been replayed
against the real kernel here**; WP-C1 and K5 are separate acceptance work.

## Commands and sequence counts

```sh
cargo test --offline --locked -p redoubt-model --release
cargo test --offline --locked -p redoubt-model --release --test current_contracts
cargo test --offline --locked -p redoubt-model --release --test policy_current
cargo test --offline --locked -p redoubt-model --release --test mutations -- --nocapture
cargo test --offline --locked -p redoubt-model --release --test properties -- --ignored million --nocapture
cargo testbench model-host-tests
```

`tests/model-host-tests.toml` permanently registers the default suite. Scoped dev optimization
keeps that host run practical; overflow checks remain enabled in both dev and release. Defaults:
20,000 sequences each for kernel, budget lifecycle, scheduler fairness and steward policy;
10,000 for steward noninterference; 20 flood sequences. The separate ignored `million` test runs
**1,000,000 sequences in each of five families and 1,000 flood sequences** (5,001,000 total).
Flood divides the requested count by 1,000 because each scenario can have 20,000 operations.
`REDOUBT_MODEL_SEQUENCES=N` overrides per-family requests, with the same flood divisor. An ordinary
test run reports the million test ignored; it is not evidence of that separate acceptance run.

The unfiltered mutation suite enumerates `Mutation::ALL`: every R1-R12 rule has a deliberate break.
It first tries focused IPC examples, then up to 20,000 seeds per family (20 flood seeds), reporting
the property and seed that detected each break. `REDOUBT_MODEL_MUTATIONS=...` is a diagnostic filter,
not full acceptance. Dedicated policy tests additionally run 256 connection-lineage sequences of
128 operations and 64 push snapshots, plus explicit negative controls. These are separate from
the million families. Actual executed counts and exits belong in the integration evidence log;
commands above are instructions, not assertions that any particular run completed. See
[recovery validation](VALIDATION.md) for executed checks and review findings.

## Contracts and abstractions

- One flat weighted stride queue (answers 103, 166 and the WP-K5 owner decisions), no priority
  flag or tier (`sched.rs`). Stride weight is a budget's free weight (limit less its carve); runtime
  is folded into the pass at every deschedule and before every weight change, with an exact
  division remainder. A wake sets the pass to `max(own, floor)`, the floor being the queue's
  monotone minimum, kept across an empty queue. Equal passes rank wakers first (later reconcile
  first, then lower id), requeues FIFO. The running thread keeps the CPU until its slice ends, it
  blocks or exits, or a budget deadline fires; a timeout or IRQ only wakes. A child budget enters
  at `max(floor, parent pass)`; when destroyed, its work since entry is added to its parent's lead,
  normalized by weight. Threads within a budget run round-robin in (pid, tid) order. The
  `scheduler_fairness` family draws one scenario per seed (share, gaming, idle gap, exit churn,
  budget churn, carve inflation, debt lift, idempotence, rank oracle, shell); `sched_contracts`
  pins the kernel-side rules (timeout wakes do not preempt, the equal-instant expiry order, the
  free-weight refusals).
- `Ret::Call(CallCompletion)` carries status, lend disposition and an optional committed reply
  independently. A partial reply keeps words and positional handle slots even with `OutOfMemory`.
  `Ret::Replied` reports delivered/discarded and the positional installed mask (answers 167-168).
- `Record` abstracts a backed owned record, unmapped/read-only/borrowed/device records, a late copy
  failure, or `Memory(addr)` checked against modeled mappings. `Op::Record` represents a sibling's
  mapping changes between atomic kernel operations, including while another thread is blocked.
  The output record is checked before call effects and at completion, after restoring a lend.
  Failed output discards the record and rolls back just this reply's installed handles/table pages.
  This abstracts a whole record's validity; it does not emulate byte layouts or hardware races.
- Memory is page frames with one word each. Own mappings, protected borrower aliases and lender
  reservations are separate states. Loads/stores/fetches enforce permissions. Sv39 table topology
  remains an explicit placement abstraction, including when context cost is configured for rv32.
- Process/notice objects are creator-paid and survive exit until notice receipt/drop. Separate
  saved-context frames cost two pages for rv64 or one for rv32 (answer 127); thread IPC pages and
  page tables are charged independently. Final-thread exit uses process-exit(0), computing blame
  before teardown (170). The abstract init has a real modeled process object; this is not a claim
  about temporary loader integration objects in the implementation.
- Handle pages are charged by occupied index groups, not live-handle count or highest index;
  holes never compact. `MAX_HANDLES=4096` is enforced, including delivery and partial replies
  (111/116). The invariant checker independently recomputes occupied groups.
- Logical time advances only on `Tick`; deterministic random PID selection stands in for CSPRNG
  output. This tests accounting and transitions, not unpredictability or real-time latency.
- Policy preserves historical P1-P13 admission/approval/lease/blame/declassification checks.
  Additional 117 connection-share lineage is a separate explicit abstraction; it is not the
  older session-level pending-approval fairness calculation. Answer 125 uses ideal opaque keyd
  audit tokens bound to domain, length, record and signing purpose, not cryptography; no chaining,
  truncation detection or ordering guarantee is claimed. Answer 153 models an owner-created
  frozen one-item push approval, exact-label temporary writer, single consumption, labelled audit
  and confined read-down refusal. It does not prove deployment topology or timing isolation.

`ghost.rs` records source authority, delivered calls, ownership transitions and expected blame;
`invariants.rs` recomputes I1-I9/I11-I15 from objects rather than trusting usage counters.
WP-K5b (answer 173) adds I-DMA: `ghost.rs` arms each DMA frame against every device its holder
could reach and disarms a device only on a reset that genuinely confirms (read from the device
object, not from the kernel's answer), and no frame in the free pool may be armed; a quarantined
device used again (OD6: its handles are swept) is a violation too.
`check::budget_lifecycle` supplies I10's before/after comparison. New IPC examples exercise queued
and taken cancellation, server-thread death, returned same-process lends, partial replies and
late-output rollback. Invalid raw lends report returned without certifying the supplied mapping.

## Trace version 2

Text records retain `boot`, `costs`, `device`, `start`, `do`, memory accesses, `fault`, `irq`,
`tick`, `note` and `wake`. Header: `redoubt-model-trace 2`. `costs` now includes `contexts=N`;
`budget_create` has no obsolete priority argument. `device mmio` takes an optional
`resets=always|first-fails|never` (default `always`; WP-K5b's reset outcome for a DMA device). Names `p:`, `t:`, `h:`, `a:`, `m:` bind chosen
PID/thread/handle/address/message values. Lists use `[a,b,...]`; `-` is none; `forever` is no timeout.
`record p:P t:T owned|unmapped|readonly|borrowed|device|copyfault|ADDRESS` changes record validity.
Call results print `call status=ok|ERROR lend=none|returned|consumed reply=absent|present`, followed
by committed `words=[...] handles=[...]` only when present. Server results print
`ok reply delivery=delivered|discarded mask=N`. Old version 1 traces are rejected.

`trace::record`, `parse` and `check` generate, parse and replay against the model. Hostile trace
inputs must reject without panic. The example lender-death trace is checked in. The random trace corpus is supplemented by explicit partial-reply and current-call/blame
traces, including nonzero `ProcessExit` with explicit account/label blame. Random traces end
with an epilogue that exposes pending timeouts, readable memory, budget usage and revoked handles.
Scheduling and IRQ masking are not fully observable through return-value traces; flood cases
supply open-call-limit pressure. These limits do not establish real-kernel replay.

The atomic homogeneous handle allocator generates prefix reply masks: after capacity is exhausted,
no later slot can succeed within that operation. A separate committed-record witness checks sparse
positional masks and rejects a count-derived prefix; it is not presented as a generated syscall
trace. Focused handle-occupancy, exact accounting and deterministic PID-reuse tests also use direct
model operations (and a private allocator-stream fixture), so they are not complete replay traces.


## Open questions and remaining bounds

Questions 164 (confined mediation topology), 165 (authority-closure claims under same-label
sharing), and 166 (one-slice responsiveness) remain unresolved. No model result decides them.
Question 171 (late-invalid receive output) is pending. Random record-invalidations currently
target blocked calls only; one-shot copy failures are cleared before the next syscall.
`Kernel::step` rejects unsupported record changes to blocked receivers, `CopyFault` receives,
and mapping changes/lends/transfers of a waiting receiver's memory-backed record; trace recording
returns an explicit question-171 error for these events. No
receive-late-invalid trace is claimed as an accepted oracle, and no sequence-count reduction
is hidden by this event-domain bound. Initial receive record checks remain covered.
Endpoint last-handle reclamation (128) remains a documented model convention. `map_device`
length return question 146 remains open: the retained model returns the address and device
metadata supplies size; this is not an approved ABI answer. Mapping geometry and finite test
worlds bound what property runs explore. The model's CPU/record atomicity abstraction cannot prove
kernel locking or simultaneous multi-hart completion races.

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
11. Settled by answers 107/116/167/168: partial replies commit with `OutOfMemory`; installed
    slots survive and the server receives their positional mask. Output failure instead rolls back
    the new handles and returns `InvalidArgument`, absent/discarded.
12. **Lent and transferred pages** are mapped read-write in the receiver.
13. A server dies: now the spec's (R4b).
14. Calls in flight to a destroyed endpoint: now the spec's (abandoned, R3, R10).
15. Open question 128: the retained model convention is that **endpoints** live until their owner budget is destroyed; closing the last handle does not free
    one.
16. Settled by answer 170: **the last `thread_exit`** ends the process as `process_exit(0)` would (so `faulted` if the
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
