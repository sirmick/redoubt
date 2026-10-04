# Blocking IPC and the thread per outstanding I/O: assessment (architect-14, 2026-10-03)

The owner: "damn that's expensive to change, but we must." This is an assessment, not a cut.

## Today

- `call` blocks its thread until the reply. `send` blocks until a receiver takes it: nothing is
  queued without a thread behind it. `receive` waits on one endpoint, one IRQ, or nothing.
  (ipc.md, "The calls").
- So beamlet (beamlet.md, "Asynchronous underneath") spends one thread per waiting call: 246 of
  its 255 (`MAX_THREADS`, held to the u8 TID field by HW1) go to user code's waiting calls, and
  past them `system_limit`.
- The 246 are mostly **client calls**: TCP reads on `/net` data files, the console read, the
  parked `resize`. Some are **receives**: a `serve` loop per served endpoint, an exit notice per
  job.

Raising `MAX_THREADS` is not a fix: the bound is a hardware-checked field, and each thread costs
an IPC page and a stack.

## (A) One thread waits on several endpoints

**Shape.** A kernel **endpoint set** object: a bounded list (say 64) of badge-0 endpoints and IRQ
handles, made and edited by its holder, charged to its budget. `receive` accepts a set where it
accepts an endpoint today, and returns the record with the member it came from.

**R2 across the set.** Each endpoint keeps its own groups and turns, unchanged. The set picks
which member to serve by the same rule one level up: the member served least recently, among
those with something deliverable, ties to the lower slot. So I11 (fair turns) extends: with m
members and k groups on one, that group's oldest message is taken within m x k receives. R4a
still skips calls while the process is at `MAX_OPEN_CALLS`.

**IPC3 is the prerequisite.** IPC3 makes delivery follow per-endpoint receiver lists, one link
per waiting thread, in the thread's frame words (`IPC3-layout-ruling.md`). A thread waiting on a
set must sit on every member's list at once. That needs one link node per membership, held in
the set object, not in the thread. Done before IPC3 it would be designed against the walk IPC3
deletes. After it, it is an extension of IPC3's lists:
- a wake on any member delivers to the set's waiter and unlinks it from the rest;
- an endpoint's or a set's destruction unlinks its nodes (a step in R10's walk, bounded by the
  set's size).

**Size.** M+, Tier A:
- the object and its calls;
- `receive`'s row (abi.md);
- R2 and I11 restated;
- the model and new mutations (a set that serves one member twice ahead of another; a stale
  link after a member's destruction);
- destruction;
- cases.

**What it buys.** Only the receive side: the `serve` loops and the job exit notices collapse onto
one or a few threads. The client calls, most of the 246, still hold a thread each, because a
`call` blocks its thread whatever the receiver does.

## (B) The client side: many outstanding requests, one thread

**The convention as first framed does not fit today's ABI.** A request by `send`, returning at
the take, with the server answering by `send` to a reply endpoint the client handed it: the
answering `send` blocks the server's thread until the client takes it. A client that stops
receiving would then stall its server, a denial of service that today's `reply` cannot cause
(R4: "a reply is never refused", and it never blocks the replier). With a timeout of 0, a reply
sent while the client's one thread is busy is lost. It would also hand servers a send capability
into each client, a new edge under R1 and R34 that today's model does not have.

**What does fit today's ABI: requests by `send`, completions by one long-poll `call`.**
- **Requests.** The client sends each request with `send`, tagged (9P's tag is exactly this),
  small requests in the four words and larger ones as a transfer. The server's receive loop takes
  sends even at `MAX_OPEN_CALLS` (R4a). It admits the request and parks it, so the client's
  `send` returns at the take, almost at once.
- **Completions.** The client keeps one thread in a long-poll `call` per connection: "give me my
  next completions", lending a buffer. The server parks that one call and replies when any of the
  client's requests completes, packing one or more tagged results into the lend.
  - The reply path is today's, never refused and never blocking the server.
  - A client that stops polling only stops its own completions; the server holds them, bounded by
    admission.
- **Threads.** One per server connection (`fsd`, `ipd`, `consoled`, `bootfsd`: a handful), not
  one per outstanding request.

**What it costs.**
- **Lend semantics (ABI2's ownership rules).** A request cannot lend: `send` carries only a
  transfer, so a write's data becomes a transfer, pages given away for good (the server pays at
  the take, R4). A read's data comes back copied into the completion call's lend, one copy as
  today. Page churn for writes, and a transfer per large request, are the price. No ownership rule
  changes.
- **The serving library.** Parked calls become parked requests: admission per request in the
  same `Admission`; the parked deadline per request; one parked completion call per connection.
  New pieces:
  - a tag table per connection;
  - a cancel request (9P's `Tflush`);
  - **abandonment by the connection.** No request is an open call, so a client's death is
    learned when its completion call is abandoned (I15's notice), and the server then drops that
    client's requests. Today it learns per call.
- **The client library.** A multiplexed connection: a send per request, a completion thread, and
  callers waiting in-process on their tag.
- **What does not change.** R1 (requests flow client to server as calls do), R2 (requests are
  messages in the client's group; `WAIT_CAP` bounds a burst, so a burst past 32 waits on `send`'s
  timeout), R3/R13 (one call per connection keeps them).

**Size.** M, userland only (`libs/rt` serving and client, a 9P transport mode, host tests, one
machine case), no kernel change.

## (C) The full asynchronous shape (io_uring)

Submission without a thread, a completion queue, the kernel holding in-flight messages with no
thread behind them. What it breaks:
- **"A message occupies its sender's thread."** Today a queued message is its blocked sender, so
  every queue is bounded by threads and walked by thread. In-flight messages become kernel
  objects charged to a budget, with their own lists, caps and accounting: R2's caps per group
  restated over objects, and R4/R4a's charges restated.
- **R10's destruction.** The walk finds a dying budget's in-flight work by its threads today. It
  would also have to find and fail kernel-held messages and completions, and stay inside 30 ms at
  the gate's fill with them added.
- **Billing.** The kernel's work on a submission no running thread asked for: whose time it is
  (K20's rule, per entry) must be decided again.
- **The model and its mutations.** IPC's model state is per thread. The R2/R3/R4/R13 mutations
  are written against blocked senders and callers, and would largely be rewritten.
- **The gate.** New latency and destruction targets, swept again.
- **ABI.** New calls (submit, a completion queue object or region), and ABI2's ownership rules
  extended to buffers owned by the kernel between submission and completion.

**Size.** XL: several Tier A packages (objects and queues; destruction and billing; the model;
the gate), an M-stage project.

## (D) Sequencing, and the no-MMU variant

**Recommended:**
1. **BEAM3 proceeds now, with its pool behind one interface.** One I/O trait inside beamlet's
   platform: submit a request, take completions. BEAM3 implements it with today's thread pool,
   unchanged in its rules (the split, `system_limit`). Then the switch is internal to the
   platform, and BEAM3 is not held for kernel work.
2. **(B) next, as a userland package** (after BEAM3, needs nothing in the kernel): multiplexed
   requests for the 9P servers. The platform's pool implementation becomes one completion thread
   per connection, and `system_limit` moves from threads to admission. That removes most of the
   246.
3. **(A) after IPC3** (a kernel package, M+), for the receive side: `serve` loops and exit notices
   on one thread. It also lets one thread poll several connections' completions, if (B)'s
   completion calls are ever replaced by sends into a set.
4. **(C) not planned.** Revisit only if, after (B) and (A), a measurement shows the thread or copy
   cost still binding.

**Alternative:** hold BEAM3 until (B) exists. This saves BEAM3 building a pool that (B) then
replaces, but delays files and the async platform by a package. The interface in step 1 makes the
pool's code small and the switch internal, so holding buys little.

**No-MMU variant.** Its backend is cooperative.
- (B) maps directly: a `send` is an enqueue and a switch; the long-poll is a blocked task.
- (A) is a wait set it implements trivially in its scheduler.
- (C) would also be natural there, but nothing above needs it.

Neither (A) nor (B) adds anything the seam (`Transport`, ABI1) cannot carry.

## Risks

- **(B)** moves request lifetime out of the kernel. Abandonment, timeouts and cancellation are the
  server library's to get right, and are tested there: no kernel invariant (I15 per call) backs
  each request any more, only the connection's. Write the rule on serving.md as a property, with
  its attack tests (a client that never polls; a client that dies with requests parked; a flush
  racing a completion).
- **(A)** adds a kernel object on the delivery path that destruction must unlink, the same class
  of work as K17 and IPC3, and checked by the same gate.
- **Both.** The shell's 246-thread rule changes meaning: beamlet.md's "Asynchronous underneath"
  is rewritten when (B) lands.

## Addendum: the owner's shape, "one thread non-kernel side to deal with AIO"

Per process, one reply endpoint and one or two I/O threads:
- a **submitter** sends each request, with the reply endpoint's badge, and a transfer in place of
  a lend;
- a **receiver** blocked on the reply endpoint dispatches each reply (a `send` from the server)
  by badge or tag.

No kernel change.

**The client half is sound, and one thread could even do both jobs.** Once a `send` is taken,
the pages are the server's until the reply transfers them back. Under ABI2's rules that is the
lend's own rule, kept with transfers, so the page rule holds.

Still, two threads, not one: a `send` blocks until it is taken, and at a busy server (`WAIT_CAP`,
a full receive loop) the one thread would stop receiving replies meanwhile. A `send` with
timeout 0 and a retry queue avoids that, at the cost of polling. So: one submitter and one
receiver.

**The server half is the problem: a reply by `send` holds the server's thread until the client
takes it.** Each way out costs a stated property.
- **A send timeout and a dropped reply.** The client sees a timeout, but it cannot tell whether a
  write happened. Every non-idempotent request (write, create, remove, rename) becomes
  ambiguous.
- **A reply thread per client in the serving library.** Per badge is unbounded. Per bucket (at
  most 32) bounds it, but a stalled client then blocks the other badges of its bucket.
- **Either way, R26 (admission fairness).** "One client cannot use up a shared server that serves
  others." A server thread held by a client that does not receive is exactly that. Today `reply`
  never blocks the replier (R4), which is why R26 holds.

A kernel wait set (A) does not fix this: it is the server's `send` that blocks. Only a kernel
`send` that queues without a thread behind it would make the shape safe as stated. That is (C)'s
core, the "message occupies its sender's thread" invariant, in a smaller form: bounded mailboxes
charged to the receiver, with R10's walk, billing and the model all restated. M-L in the kernel,
not proposed now.

**The same goal, with the server's reply kept as `reply`: one completion call per connection.**
- The owner's submitter stays as described: requests by `send`, tagged, writes as transfers.
- The owner's single receiver becomes **one long-poll `call` per server connection**: "my next
  completions", with a lend the server fills with one or more tagged results, then `reply`.
- The server parks that one call (the serving library's existing `Parked`) and never waits on
  the client. A client that stops polling stops only its own completions, which stay bounded by
  its admission under R26.

**Threads per process: one submitter, plus one completion thread per server connection.** For
beamlet that is a handful: `fsd` per volume, `ipd`, `consoled`, `bootfsd`. That is near the
owner's "one thread", without a reply that can stall a server.

**Parked calls become parked requests, inside the serving library.**
- `consoled`'s read and `resize`, and a TCP read at `ipd`, are requests parked under the same
  `InFlight` admission and per-request deadline.
- New in the library: a tag table per connection, a cancel (9P's `Tflush`), and a client's death
  learned when its completion call is abandoned (I15's notice), at which point its parked
  requests are dropped.
- beamlet's own `serve` loops and job exit notices are receives, not calls. They are unchanged:
  still one thread each, until a wait set.

**What is left for a kernel wait set (A), after IPC3, optional.** Only processes that *receive* on
many endpoints:
- beamlet's `serve` loops and exit notices;
- `init`'s watcher thread per server (one each today, counted in its bound).

Servers do not need it: each receives on one endpoint and tells clients apart by badge.

**BEAM3's 255-thread split** becomes:
- the schedulers;
- the submitter;
- one completion thread per connection;
- the session's reserved receives (its `serve` loop);
- one thread per job's exit notice, until (A).

`system_limit` moves from "a thread per waiting call" to the servers' admission and the number
of connections.

**Size: M+, Tier A.** It is in the trusted serving library and every 9P server's skeleton, though
the ABI is unchanged:
- `libs/client`: a multiplexed connection (the submitter, the completion thread, the tag table,
  callers waiting on their tag);
- `libs/rt`'s serving library: parked requests, the completion call, cancel, abandonment by
  connection;
- R26 and R28 restated over requests, on serving.md;
- host tests, and attack cases: a client that never polls, a client that dies with requests
  parked, a cancel racing a completion, a flood of sends at `WAIT_CAP`;
- one machine case with many outstanding reads on one thread.

**Should BEAM3 be built on it from the start? Yes, if it is cut now.** It needs nothing from the
kernel, and can run beside BEAM2's tail; its hotspots are `libs/rt` and `libs/client`, and ABI2
has merged. BEAM3 then builds on the multiplexed connection, and the 246-thread pool and its
`system_limit` rule never exist. If it cannot start before BEAM3 would, BEAM3 goes ahead behind
the one submit/complete interface, as recommended above, and switches later.
