# Kernel specification

Target contract, frozen for milestone 1. Owns: the kernel's objects and their costs, system
calls, message shape, errors and the order of checks, constants and invariants, stated precisely
enough to implement from. The executable model (CONTAINMENT.md) must implement exactly this: same names,
arguments, errors and invariants; where the two differ, the model changes.
Rationale lives elsewhere: handles and IPC in CAPABILITIES.md, labels in CONTAINMENT.md, budgets and
scheduling in RESOURCES.md.

**Current conformance.** Budgets/handles, IPC, device objects and process/thread creation and
exit have implementations on the new syscall path. Real-kernel cases exercise native lifecycle
and loan teardown; timer-driven timeouts/preemption and legacy removal remain work packages.
This note is not a claim that the whole contract runs in the primary checkout. The answers
**167-168 completion encoding, lend ownership, output rollback and delivery-aware server
bookkeeping are present in this checkout**.
This is not WP-IPC1 acceptance: executable-model conformance, K5 timer-dependent cases and
simultaneous multi-hart completion-race acceptance remain outstanding. Native server startup
and the shared-server serving path's terminal-failure fallback still need boot integration;
host checks of that fallback and native syscall teardown tests do not establish it.
SWARM.md's Claims table owns package state; STATUS.md summarizes integration evidence.

There are also explicitly unresolved target differences: the current `map_device` ABI returns
`addr, len`, while the target row below still awaits question **146**; BOOT.md describes the
implemented `Devs`/`Ctrl` handoff while device-policy question **143** remains open. Neither current
behavior is owner approval of its proposal. Questions **164-165** respectively qualify confined
mediation and authority-closure wording under permitted delegation; their recommendations are
not amendments to R1/R9.

## Constants
Initial values; changing one is a spec change (ANSWERS.md).

| Name | Value | Meaning |
| --- | --- | --- |
| `WORDS` | 4 | machine words in a message |
| `MAX_MSG_HANDLES` | 4 | handles carried by one message |
| `MAX_LEND_PAGES` | 16 | pages in one lend (= the 9P `msize`, 64 KiB; WIRE.md) |
| `MAX_THREADS` | 31 | threads per process |
| `MAX_LABELS` | 8 | labels per budget |
| `MAX_DEPTH` | 8 | budget tree depth, root = 0 |
| `MAX_OPEN_CALLS` | 64 | open calls per process (taken by `receive`, not yet replied to) |
| `MAX_START_HANDLES` | 64 | handles in one `process_start` list |
| `MAX_HANDLES` | 4096 | handles in one process's table (32 table pages) |
| `WAIT_CAP` | 16 | queued messages per group (R2) per endpoint |
| `STRIDE` | 2^20 | stride scheduling numerator |
| `SLICE` | 10 ms | time slice |
| `FOREVER` | `u64::MAX` | a timeout that never expires; as a deadline, none |

Time is monotonic microseconds since boot (`u64`). Timeouts are relative microseconds, added to the
current time with saturation (so `FOREVER` never expires). A budget deadline is absolute
microseconds since boot.

## Objects
Four kinds (a device object takes one of three forms). Every object is charged in pages to one
budget (the cost table says which).

**Budget**

| Field | Type | Rule |
| --- | --- | --- |
| `id` | u64 | never reused, on both widths |
| `parent` | budget | none only for `root` |
| `class` | `system` or `user` | inherited from the parent; trust, never order (below) |
| `labels` | sorted set of u64, at most `MAX_LABELS` | fixed at creation; `labels(child) ⊇ labels(parent)` |
| `account` | u64, 0 = none | inherited (R8) |
| `deadline` | time or none | when it passes, the kernel destroys the budget; the kernel sets no maximum (leases are the steward's: CAPABILITIES.md) |
| `pages` | limit, usage | every object charged here (R6) |
| `processes` | limit, usage | the processes running in it |
| `weight` | u32 limit, carved | CPU share: the free weight (limit less the children's carve) is the stride weight (R7, R12); free weight 0 holds no process |

`root`, `system` and `users` are created by the kernel at boot from the argument block (`system`
gets its reserved share); `root` and `system` are class `system`, `users` is class `user`; all
three have account 0 and no labels. `init` receives handles to all three.

**Class is trust, not order.** A budget's class decides three things and nothing else: a flow into
a system-class budget is not label-checked (R1), a system-class reader of `budget_usage` is not
label-checked either, and only a system-class creator adds labels (`budget_create`). Because class
is inherited, a handle to a system-class budget is also the authority to create more of them, which
is why only `init` and the steward ever hold one (INIT.md). Scheduling never looks at class: every
budget shares one stride queue by weight (R12, RESOURCES.md).

**Process**: PID (= ASID), the budget it runs in, address space, up to `MAX_THREADS` threads, handle
table, exit endpoint handle, a started flag, and its open calls (at most `MAX_OPEN_CALLS`). PIDs are
drawn at random from the free ASIDs. The **process object is charged to its creator's budget** (the
budget of `process_create`'s caller), not to the budget it runs in, and it holds the process's one
exit notice. When the process exits, faults or is killed, its threads, address space, handle table
and open calls are freed, and it stops counting against its budget's `processes` usage at once, but
the object outlives it until its exit notice is received or dropped; its PID is freed with the
object, so no PID is reused while a notice still names it. Destroying the creator's budget frees
the object (R10), killing the process first if it still runs; then there is no notice. So a notice
never allocates.

`thread_exit` with surviving siblings frees only that thread and its IPC state, including
the R4b cleanup of its open calls; it produces no process-exit notice. The last thread's
`thread_exit` is equivalent to `process_exit(0)`. Determine whether open calls remain and
snapshot current-call blame before cleanup, then apply the normal process teardown and
notice/PID lifetime above. Thus a started process cannot remain alive with zero threads
(answer 170).

An **open call** is a `call` a thread has taken with `receive` and not yet replied to. A thread may
hold several; its process may hold at most `MAX_OPEN_CALLS`. Each costs a page (below) while it is
open, and its lend is charged to the receiving process's budget too (R3). A `send` is never an open
call. The kernel records each open call's sender account and labels, and an **abandoned** flag
(R3). A thread's **current call** is the open call it is working on, or none: `receive` sets it to
the call it takes, and to none when it returns anything else; `serve(msg_id)` sets it to another
open call of the thread; replying to the current call sets it to none. A fault blames the current
call (Messages, exit notices).

**Endpoint**: the object clients call and servers receive on. Its **owner** is the budget of the
process that created it (it is charged there). It holds the threads blocked in `send`/`call` on it,
grouped as R2 says, and a round-robin cursor over those groups. It survives the death of processes
receiving on it.

**Device**: one of
- MMIO: a physical range, and a DMA flag;
- IRQ: an interrupt number, a `fired` flag and a `masked` flag;
- Reset: the right to power off or reboot (given only to `init`).

The loader creates device objects from the device tree; `init` receives them all.

**Handle** = (object, badge: u64, stamp: budget id), an index into a process's handle table.
**Index 0 is never allocated** and means "no handle" where a handle is optional. For an endpoint,
**badge 0 is the receive right**: only a badge-0 handle may `receive`, and `mint` never creates
badge 0. Handles to budgets, processes and devices carry badge 0. A handle carries no kind a
receiver can read: using one of the wrong kind gets `WrongObject`. A process's table holds at most
`MAX_HANDLES` handles: a call that would add one past that gets `TooLarge`, which a caller can tell
apart from its budget running out of pages (`OutOfMemory`). At delivery the limit is a cost like
any other (R4).

**What objects cost.** Every object is charged in whole pages (R6):

| Object | Pages | Charged to |
| --- | --- | --- |
| budget | 1 | its parent (`root`'s is the kernel's) |
| process | 1 | the creator's budget (`process_create`'s caller) |
| saved process contexts | `PROCESS_IMPL_PAGES` (rv32: 1; rv64: 2) | the process's execution budget |
| thread IPC state | 1 per thread | its process's budget |
| endpoint | 1 | its owner |
| page tables | 1 per page-table page, including the root, when allocated | the process's budget |
| handle table | 1 per table page holding a handle (128 handles a page, at most `MAX_HANDLES`) | the process's budget |
| open call | 1 while open | the receiving process's budget |

Saved contexts are distinct frames from the creator-paid process/notice object and each
thread's IPC page (answer 127). Charge all context frames from address-space creation until
process teardown, including for initial loader processes. There is no global per-PID context
reservation and no subtraction of one context frame for the separately allocated notice.
Each of these storage frames is counted once. Loader-started processes that lack process/notice
objects during boot integration incur no unbacked charge for such an object.

Plus the pages themselves: mapped, lent (charged to both sides while the call is open, R3), or
transferred. The handle-table figure assumes a handle of 24-32 bytes (object reference, 64-bit
badge, 64-bit stamp); the kernel implementer confirms it, and a different figure is a change to this
table. **A table page is charged while it holds any handle**, holes and all: closing a handle frees
its page only when that page holds no other, and handles are never moved to compact the table, so
what is charged is what the table actually costs (the executable model charges the same: question
111). The process object is charged to its creator, not to the budget it runs in, because its
`killed` notice must outlive that budget (R10), paid by someone still alive. A budget's own page is
its parent's, so a budget's whole page limit is usable and a revocation scope (zero limits) needs no
special case.

## Messages
A message carries:
- `WORDS` words;
- up to `MAX_MSG_HANDLES` handles, **copied** (the sender keeps its own), each keeping its stamp;
- at most one buffer: a **lend** (`call` only: page-aligned, at most `MAX_LEND_PAGES`, writable,
  unmapped from the caller until the call ends) or a **transfer** (`send` only: pages whose owner and
  payer both become the receiver).

The kernel attaches, unforgeably: the **badge** of the handle it was sent through, the sender
budget's **account** and **labels**, and a **message id** (for `reply`, `serve` and `mint`). Message
ids are non-zero and never reused within the receiving process, so a stale `reply` or `mint` cannot
reach a later message; they are not drawn from a counter shared with other processes, so they
reveal nothing of anyone else's traffic.

`receive` returns one of:
- a message: `(kind, msg_id, badge, account, labels, words, handles, buffer address, page count)`,
  where `kind` is `call` (a reply is owed; the buffer, if any, is a lend) or `send` (no reply; the
  buffer, if any, is a transfer);
- an interrupt: the IRQ handle fired;
- an exit notice (on an endpoint named as some process's exit endpoint): `(pid, cause, code,
  blamed_account, blamed_labels)`, where `cause` is `exited`, `faulted` or `killed`. A
  `process_exit` while the process holds open calls (a Rust panic, say) is reported `faulted`, like
  a fault; this includes the final thread's `thread_exit`, equivalent to `process_exit(0)`.
  A server that means to exit replies to every open call first. For `faulted`,
  `blamed_account` and `blamed_labels` are the account and labels of the sender of the current call
  of the thread that faulted or called `process_exit`. If that thread has no current call, nobody
  is blamed, even when other threads of the process hold open calls. When nobody is blamed, and for
  `exited` and `killed`, `blamed_account` is 0 and `blamed_labels` empty;
- an **abandoned-call notice** `(msg_id)`: the open call `msg_id`, held by the receiving thread, was
  abandoned (R3); the thread replies to it to free it;
- `Timeout`.

Notices are returned before messages when both are pending. An abandoned-call notice is returned
once, by the next `receive` of the thread that holds the call on the endpoint the call arrived on;
a thread that never receives there again never gets it, so every serving thread keeps receiving
(the shared server library does this: CONTAINMENT.md). It needs no label check: it follows a call
that R1 already allowed.

## Rules
**R1. Flow.** Information flows from budget A to budget B only if B is class `system` or
`labels(B) ⊇ labels(A)`. A message is a flow from the sender's budget to the endpoint's **owner**
(whichever thread takes the message, so the check is decided when the message is sent) and,
because it is answered or refused, a flow back: between two user budgets the label sets must
therefore be **equal** (`LabelDenied`); when either is class `system` the kernel does not check
(system servers check themselves; CONTAINMENT.md). An exit notice is a flow from the exiting budget
to the exit endpoint's owner, a usage read (`budget_usage`) from the budget read to the reader; a
notice that fails the rule is dropped, a read gets `LabelDenied`.

**R2. Fair waiting.** Blocked senders on an endpoint are grouped by the sender budget's (account,
label set), and for account 0 (system callers) by its budget id as well, and served round-robin by
group: each `receive` takes the oldest message of the next group after the last one served. A group
with `WAIT_CAP` messages already queued on the endpoint gets `Busy` immediately. Only queued messages
count (sent, not yet taken): a taken call waiting for its reply is bounded by the server's open
calls (R4a). (Keyed by label set too, so a vault session and its owner's unlabelled session, which
share an account, share neither a turn nor a cap; and by budget for system callers, so one busy
system server cannot fill another's cap: CONTAINMENT.md.)

**R3. Lends and abandoned calls.** A lent page stays charged to the caller, and taking the call
charges it, with the page tables that map it, to the receiving process's budget as well, until
`reply` returns it. A call is **abandoned** when its caller dies, times out, or is failed by
revocation or by its endpoint's destruction (R10) after the server took it. The caller's charge
then ends, and the lend stays mapped in the server, charged only there, until the server replies to
the call; the reply is discarded (its handles are dropped). The kernel sets the open call's
abandoned flag, and the holding thread gets an abandoned-call notice (Messages). An abandoned call
stays open, and counts against `MAX_OPEN_CALLS`, until that reply.
The caller's return reports the lend's disposition independently of its status; `Timeout` or
`Dead` alone does not identify its ownership (IPC completion, below; answer 167).

**R4. Delivery.** A message is delivered only if the receiving process's budget can pay for
everything it brings: the handle-table pages for its handles, a call's open-call page, its lent or
transferred pages and the page tables to map them; and a transfer only if the `receive` named a
`max_transfer` at least its size. Handles that would take the receiver past `MAX_HANDLES` are a
cost it cannot pay, like any other. Otherwise its sender gets `Refused`, and the kernel moves on to
the next sender. A `receive` never fails for want of pages. (`Refused` tells the sender one bit
about the receiver's budget, which a sender with a clock already has.)

A **reply** is never refused: its caller is blocked and has nowhere else to put the error. Handles
a reply carries that do not fit the caller — its budget cannot pay the table pages, or they would
take it past `MAX_HANDLES` — are dropped, each 0 in its slot as a revoked handle is (ABI), the
reply is delivered without them, and the caller's `call` returns `OutOfMemory` (questions 107 and
116). This partial reply is a committed reply, not an absent one: its words and surviving handle
slots remain available even though `call` returns an error. `reply` reports delivery and the mask
of installed slots to the server (IPC completion; answers 167-168).

**R4a. Open calls.** Taking a `call` opens it and charges its page to the receiving process's
budget; `reply` closes it and frees the page and the receiver's charge for the lend. A process that
holds `MAX_OPEN_CALLS` open calls takes no more calls: they stay queued, R2's turns skip them, and
its `receive` still delivers sends, interrupts and notices, and never returns `Busy` for the limit
(question 105). `reply` to a `send`'s message id gets `InvalidArgument`: a send is never an open
call.

**R4b. A server dies.** When a thread or process exits, faults or is killed holding open calls, each
of their callers gets `Dead` and its lend back; the lend of an abandoned call (R3) is freed. Senders
still blocked on the endpoint keep waiting: the endpoint survives, and a restarted server receives
them (INIT.md). A server that means to exit replies to every open call first; exiting with open
calls is reported `faulted` (Messages).

### IPC completion (answers 167-168)
`call` returns three independent facts: status, lend disposition (`none`, `returned`, `consumed`),
and reply disposition (`absent`, `present`). A `present` reply means the complete output record was
committed, not merely that a server attempted a reply. Only then may the caller decode it. The
status may still be `OutOfMemory` under R4. A `returned` lend remains owned by the caller;
`consumed` means its mapping and charge have ended and it must not be accessed or unmapped again.
These outcomes do not change R3's transfer on abandonment or R4b's return on server death.

| Completion | Caller status | Lend, if supplied | Reply record | Server's successful `reply` result |
| --- | --- | --- | --- | --- |
| Rejected before delivery, including queued timeout/revocation | Existing error (`Timeout`/`Dead` for cancellation) | `returned` | `absent` | No call was taken |
| Taken call times out or is revoked/its endpoint destroyed | `Timeout` or `Dead` | `consumed` | `absent` | Later `discarded`, mask 0 |
| Server dies while caller still waits | `Dead` | `returned` | `absent` | No reply |
| Normal reply | Success | `returned` | `present` | `delivered`, installed-slot mask |
| R4 partial reply (some handles do not fit) | `OutOfMemory` | `returned` | `present` | `delivered`, installed-slot mask |
| Caller output record cannot be committed | `InvalidArgument` | `returned` | `absent` | `discarded`, mask 0 |
| Caller dies after receipt | No return to caller | Consumed as R3 | No caller record | Later `discarded`, mask 0 |

With no lend, every returning row reports `none`. On any rejection before delivery, `returned`
means no supplied memory was consumed by this attempt; it does not certify an invalid input range
as mapped or owned. A decoding failure does not acquire, alter or validate that memory.

**Output validity and rollback.** A `call`'s body record is both input and output: validate its
readability and writability during decoding, before delivery effects, and recheck ownership and
writability at completion. Restore a returning lend before writing output: the record may lie in
that lend. If output cannot be committed, close every caller handle newly installed by this reply
attempt and release its otherwise-unused table pages under R6; do not close the server's original
handles or any pre-existing caller handles. Completion validation, output copying, handle
installation or rollback, and outcome publication are one kernel completion, protected together
against relevant handle-table operations, mapping changes (`unmap`, remap, `set_flags`) and
lifecycle teardown or abandonment. A completion cannot validate one mapping and write through a
replacement, nor both commit a reply and consume its lend by abandonment. Serialization or
equivalent validated-frame pinning with completion arbitration must preserve these same outcomes;
pinning alone does not decide which lifecycle transition wins. The caller gets `InvalidArgument`,
`reply = absent`, and its lend back; that output error takes precedence over an attempted partial
reply's `OutOfMemory`. The server successfully closes the call and receives `discarded`, mask 0.
An absent record is unspecified and must never be decoded, even if some bytes were written. This
does not undo server side effects, and committing a record does not prevent another caller thread
from changing or unmapping it afterward. Safe wrappers own and protect their records and buffers.

**Server disposition.** A successful `reply` always closes the open call and releases the server's
open-call/lend charges. It reports `delivered` only when the caller's complete record commits;
otherwise (abandonment or failed output commit) it reports `discarded`. The installed-handle mask
uses the reply's positional slots: a bit is set only for a handle installed for that committed
reply; missing/revoked slots have clear bits. It records installation at completion, not future
revocation or an acknowledgement that caller application code used the reply. Discarded replies
have mask 0. Invalid reply arguments remain errors and do not close the call or assert delivery;
the server still owes a valid reply or the R4b exit handling. Provisional server resources follow
CONTAINMENT.md's transaction rule. The ABI below carries both sides' outcomes outside user memory.

**R5. Interrupts.** When an IRQ fires, the kernel masks the source and sets `fired`. `receive` on the
IRQ handle unmasks the source when it begins, then returns when `fired` is set (clearing it). There
is no acknowledge call.

**R6. Charging.** Every kernel object (pages, page tables, handle tables, threads, endpoints,
budgets, processes, open calls) is charged in pages as the cost table says: a budget's own page to
its parent, a process object to its creator's budget, a lent page to both sides while its call is
open (R3), and everything else to the budget that owns it. A parent's usage counts its **children's
limits** and their own pages, never their live usage.

**R7. Carving.** A child's page, process and weight limits come out of the parent's free limits: the
children never add up to more than the parent. Allocation fails only on the caller's own budget.
A budget's stride weight (R12) is its **free weight**, its weight limit less what its children
carved: carving moves share to the child and never duplicates it. A carve that would leave a budget
that holds a process with free weight 0 is refused, as is `process_create` into a budget with free
weight 0 (both `InvalidArgument`; the rows below).

**R8. Accounts.** A new budget's account equals its parent's, unless the parent's is 0; then the
creator may set any value (the steward sets one on each principal's top budget).

**R9. Stamps.** `endpoint_create`, `budget_create` and `process_create` stamp the new handle with the
caller's budget. A handle received in a message keeps its stamp. For `mint`, see the table.

**R10. Destruction.** Destroying budget B destroys its descendants first, kills their processes (each
exit notice has cause `killed`), closes every handle stamped with B or a descendant in every
process's table and in every message not yet received (such a handle arrives as 0), frees every
object charged to them (a process object among them first kills its process, with no notice), and
returns their carved limits and own pages to B's parent. Calls and sends blocked on a destroyed
endpoint, calls in flight to it, and receives waiting on it, fail with `Dead`; a call in flight that
the server had taken is abandoned (R3). Revocation reaches messages already sent: a queued message
sent through a handle stamped with B or a descendant fails its sender with `Dead`; a taken call sent
through one fails its caller with `Dead` at once and is abandoned (R3).

**R11. Memory.** No mapping is ever writable and executable, and none is writable without being
readable: the privileged architecture reserves that page-table encoding, so every call that
installs or changes user permissions (`map_anon`, `map_fixed`, `set_flags`, `process_map`) refuses it with
`InvalidArgument`, and a writable user page is always readable. Every page is zeroed before a process
first sees it. Userspace never maps RAM by physical address; a DMA driver learns the physical
address of pages the kernel gave it. A page-table page is allocated when a mapping first needs it
and freed when it maps nothing. The kernel chooses the addresses `map_anon`, `map_device` and
`dma_alloc` return; nothing may depend on them. `map_fixed` is the one call that puts new pages
at an address the caller names, so that the loader stub can place a program's segments at their
link addresses from inside the started process (PACKAGES.md); unlike POSIX `MAP_FIXED` it never
replaces a mapping, so it can neither discard nor alias a page (answer 172).

**R12. Scheduling.** **One flat stride queue over every runnable budget**, of either class, with no
priority above it: run the lowest pass; at every deschedule, pass += runtime x `STRIDE` / weight,
the weight being the budget's free weight (R7; never 0: a budget with free weight 0 holds no
process); on wake, pass = max(own pass, current minimum). `init`, the steward and the drivers are
scheduled by weight like everyone else, with the large weights the boot manifest gives them
(RESOURCES.md, INIT.md). Within a budget, threads run round-robin in (pid, tid) order. The timer is
always armed (the running thread's slice end, or the next timeout or budget deadline). Precisely
(the WP-K5 owner decisions):
- **Preemption points.** Only slice end and a *budget* deadline. A timeout expiring or an interrupt
  firing only wakes (makes a thread runnable); the running thread keeps the CPU.
- **Current minimum.** The lowest pass among runnable budgets, the running one included at its
  last-charged pass, floored by a monotone **floor** that holds while the queue is empty. A waking
  budget's pass is max(own pass, floor). A wake is a budget going from no runnable thread to one.
- **Ties** (the rank at an equal pass): wakers ahead of requeued budgets; a later kernel entry's
  wake ahead of an earlier one's; within one entry the lower budget id first (ids are the
  kernel's); requeued budgets in the order they were requeued.
- **Charging.** Runtime is counted in timebase ticks, at least 1 per deschedule, at most 2^40 per
  charge, with an exact per-budget division remainder, so split runs cost what one run would.
  Runtime is charged at the weight it ran at: a weight change (a carve, or a carve returned)
  charges what ran before it.
- **Inheritance.** A new budget enters at e = max(floor, parent's pass), kept as its entry. When
  it is destroyed (bottom-up, R10), its work since entry,
  W = (pass - max(e, floor))+ x its weight + its remainder, is added to its parent's lead:
  parent's pass = max(parent's pass, floor) + W / parent's weight (taken after the child's carve
  returns), the remainder carried. A child's debt is only work it did after its entry, so creating
  and destroying a budget that never ran moves nothing. The top of a destruction returns its carve
  to its parent first, before any of the destruction's own work is charged, so the parent (often
  the caller of `budget_destroy`) pays for it at the weight it has once the child is gone; the
  budgets below return theirs at their own bottom-up step.
- **Running while carved down** (a stated behaviour, accepted for WP-K5). A budget that runs while
  most of its weight is carved away accrues its lead at the small weight it kept, and that lead is
  not rescaled when the weight returns; a destroyed child's lead is lifted at the child's weight at
  its destruction, whatever it had kept while it ran. Both only over-charge the budget that carved,
  never under-charge: no share is gained by carving. Rescaling is to be revisited once real shell
  and steward carve patterns exist.
Wakeup is prompt but not bounded: a wake waits out the running thread's slice, a waking budget may
retain a larger pass, and no deadline follows from weight; RESOURCES.md states the measured
responsiveness target (answer 166, revising 103), and the WP-K5 latency bench measures it.

## System calls
`h` is a handle. Every call returns a status (success or one error from the enum below); its result
is valid as specified. `call` also returns ownership/output disposition on errors, and its committed
partial reply remains valid on `OutOfMemory`. No argument can make the kernel panic.

| Call | Arguments -> result | Checks |
| --- | --- | --- |
| `map_anon` | len, flags -> addr | pages charged; not W+X; zeroed |
| `unmap` | addr, len | own mapping; not currently lent |
| `set_flags` | addr, len, flags | own mapping; not W+X |
| `map_device` | h(MMIO) -> addr | MMIO device handle |
| `dma_alloc` | h(MMIO), npages -> addr, phys | DMA flag; pages charged; contiguous; zeroed |
| `thread_create` | entry, sp, arg -> tid | pages charged; fewer than `MAX_THREADS` |
| `thread_exit` | - | siblings survive without a process notice; last thread is `process_exit(0)` |
| `process_exit` | code | exit notice `exited`, or `faulted` while the process holds open calls |
| `process_create` | h(budget), h(exit endpoint) -> h(process) | budget's free weight not 0 (R7); exit endpoint's badge 0; the budget's process and page limits; process object charged to the caller |
| `process_map` | h(process), src, dst, len, flags | process not started; src owned by caller; pages move to the child's budget; not W+X |
| `process_start` | h(process), entry, sp, arg, handles | not started; at most `MAX_START_HANDLES` handles, copied into slots 1..n; `arg` reaches the first thread unchanged, like `thread_create`'s (the startup page's address, 0 = none: INIT.md) |
| `endpoint_create` | -> h (badge 0) | pages charged |
| `mint` | source, badge, optional h(budget) -> h | see below |
| `call` | h, words, handles, lend, timeout -> status, lend disposition, reply disposition; committed reply in body record | endpoint; R1; R2; lend rules; R4; IPC completion |
| `send` | h, words, handles, transfer, timeout | endpoint; R1; R2; rendezvous; R4 |
| `receive` | h or none, timeout, max_transfer -> message, interrupt, exit notice, abandoned-call notice | badge-0 endpoint, IRQ, or none (sleep); R4a |
| `reply` | msg_id, words, handles -> delivered/discarded, installed-handle mask | msg_id is an open call of the caller's thread; closes the call; returns a live caller's lend, frees an abandoned one; IPC completion |
| `serve` | msg_id | msg_id is an open call of the caller's thread; it becomes the thread's current call |
| `handle_close` | h | - |
| `budget_create` | h(parent), pages, processes, weight, labels, account, deadline -> h | R6-R8; class inherited; labels below; depth < `MAX_DEPTH` |
| `budget_destroy` | h(budget) | always allowed to a holder |
| `budget_usage` | h(budget) -> counters | R1: a flow from the target to the caller's budget |
| `time_now` | -> µs | - |
| `random` | -> u64 | from the kernel's CSPRNG (seeded at boot, BOOT.md) |
| `system_reset` | h(Reset), kind | Reset device handle |
| `map_fixed` | addr, len, flags | as `map_anon`, at exactly `addr`: range in user space and overlapping none of the caller's mappings; nothing mapped on failure. Last in the table so earlier call numbers keep their values (ABI) |

**`mint(source, badge, budget?)`** creates a handle to an endpoint with `badge != 0`.
- `source` is either the message id of an open call of the caller's thread (the new handle is to
  the endpoint the call arrived on; default stamp = the stamp of the handle the call was sent
  through), or a badge-0 endpoint handle the caller holds (default stamp = that handle's stamp).
- With no budget handle, the new handle gets the default stamp. With one, the budget must be the
  default stamp or a descendant of it (`NotPermitted` otherwise): a budget handle only narrows.

**`budget_create` labels.** The child's class is its parent's; the call takes no class, no priority
and no scheduling flag of any kind (R12). Labels must be a superset of the parent's (`LabelDenied`
otherwise); adding labels needs the caller's own budget to be class `system` (`ClassDenied`
otherwise). Labels are sorted and deduplicated. The deadline is absolute; `FOREVER` means none.

**`budget_usage` counters:** page limit and usage, process limit and usage, weight limit and carved
weight (the children's weight limits; R7), so a holder can see the free weight it may still carve.

User mode may also read the `time` counter directly (`rdtime`).

## ABI
This note owns the calls: their names, arguments, results, errors and the normative encoding rules
below. The ABI crate `redoubt-sys` implements and documents their register/record layouts and
numbers, and must match this note. In particular, the completion encoding for `call` and `reply`
is specified here; it is not an independent choice by the crate. These encoding rules are part of
the spec:
- **One register layout on both widths.** A 64-bit argument or result (ids, badges, accounts, time,
  `random`'s value) always takes two registers, each holding a 32-bit half, low half first. Other
  register values are a 32-bit value or one address or length. `redoubt-sys` therefore has no width
  `cfg` (MEMORY-LAYOUT.md).
- **Records** (what does not fit in registers: message bodies, a budget's fields, the
  `process_start` list, what `receive` returns (a notice's kind and fields included),
  `budget_usage`'s counters) are arrays of 64-bit little-endian slots at an 8-byte-aligned address
  in the caller's memory, the same on both widths. **A record's pages must already be backed:**
  decoding never allocates, so a record in a page the caller reserved but never touched is
  `InvalidArgument`, not a page the kernel backs and charges mid-decode. The runtime touches a
  record's buffer before it passes it (question 115).
- **`receive`'s record** has one layout for every result: `(kind, msg_id, badge, account, labels,
  words, handles, buffer, pages)`. `kind` is `call`, `send`, `interrupt`, `exit` or `abandoned`, and
  a field a kind does not use is 0 or empty. A message fills every field (Messages). An interrupt
  fills only `kind` (it arrives only on the IRQ handle `receive` named). An exit notice puts `pid`,
  `cause` and `code` in words 0-2, `blamed_account` in `account` and `blamed_labels` in `labels`. An
  abandoned-call notice puts the call's id in `msg_id`. `Timeout` is an error, not a record.
  A handle that R10 revoked while its message was queued is 0 in its slot, so a message's handle
  slots keep their positions (WIRE.md names handles by slot).
- **Decoding refuses W+X flags and a `mint` badge of 0**; the kernel's mapping and minting code
  refuse them again (R11, I3), so neither rests on one check.

### IPC return registers
The following is the exact encoding of answers 167-168 on both rv32 and rv64. Arguments, call
numbers, body-record layouts and existing error codes do not change. `a0` is 0 for success or the
existing `Error` code. All values here fit in 32 bits; high bits and unused registers are zero.

| System call | `a1` | `a2` | `a3..a7` |
| --- | --- | --- | --- |
| `call`, on success **and every error** | lend: 0 `none`, 1 `returned`, 2 `consumed` | reply: 0 `absent`, 1 `present` | 0 |
| `reply`, on success | delivery: 0 `discarded`, 1 `delivered` | installed-handle mask, bits 0 through `MAX_MSG_HANDLES - 1` | 0 |
| `reply`, on error | 0 | 0 | 0 |

For a recognized `call` number, initialize the disposition before argument decoding: if the raw
lend address/page-count pair is `(0, 0)`, report `none`; otherwise initialize `returned`. This
retention outcome remains defined even if an earlier argument fails, and does not validate the
pair. Only R3 abandonment after receipt changes it to `consumed`. Initialize the reply disposition
to `absent`; change it to `present` only at successful record commit. Unknown call numbers retain
the ordinary error return with all payload registers zero.

`present` is valid only with status success or R4 `OutOfMemory`, never with a consumed lend.
`consumed` implies `absent` and `Timeout` or `Dead`. A successful `call` has `present`. A successful
`reply` with `discarded` has mask 0; with `delivered`, bits outside its supplied handle count are 0,
and each set bit corresponds to a nonzero installed handle in the caller's committed record. The
mask may be 0 even on delivery (no handles requested, or all missing). Result decoders check these
combinations and reject unknown tags, excess bits or nonzero unused registers; `a0 != 0` on `call`
must not discard `a1/a2` or assume the output record is absent. All other syscalls retain their
existing return encoding. A wrapper must inspect disposition before interpreting the error or
dropping a supplied buffer (CAPABILITIES.md, IPC).

## Errors and the order of checks
Errors: `BadHandle`, `WrongObject`, `InvalidArgument`, `OutOfMemory` (page limit), `OutOfProcesses`,
`TooManyThreads`, `NotPermitted`, `ClassDenied`, `LabelDenied`, `Busy`, `Refused`, `TooLarge`,
`Timeout`, `Dead`.

A call with several faults returns the first one found, in this order, so the kernel, the
executable model and a replayed trace agree exactly. Within a stage, checks go by argument
position, as the rows below list them:
1. **Decoding**: the registers in order (`a1` first), then each record the call passes (its
   alignment, whether it lies in the caller's own memory, readable for input and writable for
   output, and whether its pages are backed, then its slots in order). A required handle that is 0
   or does not fit in 32 bits is `BadHandle`; a list longer than its limit is `TooLarge`; anything
   else malformed (an unknown call number, tag or flag bit, W+X flags, a value too wide for its
   field, a non-zero unused register or slot, a misaligned record, a record page the caller
   reserved but never touched, a message id or badge of 0) is `InvalidArgument`. Each register is
   checked in full when it is reached, a list's count against its limit included; unused registers
   come after the last argument. **This stage never allocates** (ABI, Records), so `OutOfMemory`
   never arises in it and no row lists it there.
2. **Kernel argument checks**, argument by argument: a handle exists (`BadHandle`) and names the
   right kind of object (`WrongObject`); a size is within its fixed limit (`TooLarge`); a range is
   page-aligned, non-empty, in user space and mapped as the call needs (`InvalidArgument`).
3. **Permission**: `NotPermitted`, `ClassDenied`, `LabelDenied`.
4. **Resources**: `OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`.
5. **At delivery** (`call`, `send`, `receive`, after blocking or not): `Refused`, `Timeout`, `Dead`,
   `OutOfMemory`; also `InvalidArgument` for a `call` whose output record cannot be committed
   (IPC completion). Decoding already rejected initially invalid output records; this late failure
   covers memory that changed while the call waited.

Per call, in the order checked. "Then" lists stages 2-5; "each h" is every handle in a list, in
order. A call that allocates also fails with `OutOfMemory` when the caller's handle table must grow
and its budget cannot pay, and with `TooLarge` when the new handle would be past `MAX_HANDLES`
(last, after the errors listed).

| Call | Decoding | Then |
| --- | --- | --- |
| `map_anon` | flags: `InvalidArgument` | `InvalidArgument` (len 0 or unaligned; flags 0, or W without R), `OutOfMemory` |
| `unmap` | - | `InvalidArgument` (range; a page not the caller's own mapping, or lent out) |
| `set_flags` | flags: `InvalidArgument` | `InvalidArgument` (range; flags 0, or W without R; a page not the caller's own mapping) |
| `map_device` | h: `BadHandle` | `BadHandle`, `WrongObject` (not MMIO), `OutOfMemory` (page tables) |
| `dma_alloc` | h: `BadHandle` | `BadHandle`, `WrongObject`, `InvalidArgument` (npages 0), `NotPermitted` (no DMA flag), `OutOfMemory` |
| `thread_create` | - | `TooManyThreads`, `OutOfMemory` |
| `thread_exit` | - | - |
| `process_exit` | code: `InvalidArgument` | - |
| `process_create` | each h: `BadHandle` | `BadHandle`, `WrongObject` (budget), `BadHandle`, `WrongObject` (exit endpoint), `InvalidArgument` (budget free weight 0), `NotPermitted` (exit endpoint's badge not 0), `OutOfProcesses`, `OutOfMemory` (the budget: page tables; then the caller: the process object) |
| `process_map` | h: `BadHandle`; flags: `InvalidArgument` | `BadHandle`, `WrongObject`, `InvalidArgument` (src range, not the caller's own RAM; dst range, occupied; flags 0, or W without R), `NotPermitted` (started), `OutOfMemory` (the child's budget) |
| `process_start` | h: `BadHandle`; `arg`: not checked; count over `MAX_START_HANDLES`: `TooLarge`; list: record, each h `BadHandle` | `BadHandle`, `WrongObject`, `BadHandle` (each h), `NotPermitted` (started), `OutOfMemory` (the child's budget: thread, then table) |
| `endpoint_create` | - | `OutOfMemory` |
| `mint` | source: tag `InvalidArgument`, message id 0 `InvalidArgument`, handle `BadHandle`; badge 0: `InvalidArgument`; budget h: `BadHandle` | source: a message id that is not an open call of the caller's thread (a `send`'s id included) `InvalidArgument`, its endpoint or stamp gone `Dead`; or a handle `BadHandle`, `WrongObject`; budget: `BadHandle`, `WrongObject`; `NotPermitted` (a handle source's badge not 0), `NotPermitted` (budget not the default stamp or below) |
| `call` | h: `BadHandle`; lend: exactly one of address and page count 0 is `InvalidArgument`; body: record (read and written), count `TooLarge`, each h `BadHandle` | `BadHandle`, `WrongObject` (not an endpoint), `BadHandle` (each h), `TooLarge` (lend over `MAX_LEND_PAGES`), `InvalidArgument` (lend not page-aligned or not the caller's own writable RAM), `LabelDenied` (R1), `Busy` (R2); at delivery: `Refused` (R4), `Timeout`, `Dead`, `InvalidArgument` (reply output cannot commit; rollback newly installed reply handles, absent reply), `OutOfMemory` (reply handles do not fit; the committed reply arrives without them, R4); disposition accompanies every return |
| `send` | as `call`, with the transfer for the lend and body input-only | as `call` without the lend limit; at delivery only: `Refused` (R4), `Timeout`, `Dead` |
| `receive` | h (0 = none): `BadHandle`; record | `BadHandle`, `WrongObject` (not an endpoint or IRQ), `NotPermitted` (badge not 0); at delivery: `Timeout`, `Dead` (endpoint destroyed) |
| `reply` | msg_id 0: `InvalidArgument`; body: record, count `TooLarge`, each h `BadHandle` | `InvalidArgument` (msg_id not an open call of the caller's thread, including a `send`'s id), `BadHandle` (each h) |
| `serve` | msg_id 0: `InvalidArgument` | `InvalidArgument` (msg_id not an open call of the caller's thread) |
| `handle_close` | h: `BadHandle` | `BadHandle` |
| `budget_create` | h: `BadHandle`; spec: record, labels over `MAX_LABELS` before deduplication `TooLarge`, a 32-bit field `InvalidArgument` | `BadHandle`, `WrongObject`, `TooLarge` (the child would be at depth `MAX_DEPTH`), `LabelDenied` (labels not ⊇ the parent's), `ClassDenied` (labels added), `OutOfMemory` (pages, plus the budget's own page, over the parent's free pages), `OutOfProcesses`, `InvalidArgument` (weight over the parent's free weight, or all of it from a parent that holds a process; R7) |
| `budget_destroy` | h: `BadHandle` | `BadHandle`, `WrongObject` |
| `budget_usage` | h: `BadHandle`; counters: record (written) | `BadHandle`, `WrongObject`, `LabelDenied` (R1: a user-class caller whose labels ⊉ the target's) |
| `time_now` | - | - |
| `random` | - | - |
| `system_reset` | h: `BadHandle`; kind: `InvalidArgument` | `BadHandle`, `WrongObject` (not the Reset device) |
| `map_fixed` | flags: `InvalidArgument` | `InvalidArgument` (range: addr or len unaligned, len 0, not in user space, or overlapping a mapping of the caller's; flags 0, or W without R), `OutOfMemory` (pages, then page tables) |

Two exceptions to the stages, both stated in the rows: a weight over the parent's free weight is
`InvalidArgument` (no error names weight), and `mint` from a message whose endpoint or stamp is gone
is `Dead` at the argument stage. Every call can also fail decoding in the general ways of stage 1
(unused registers, values too wide).

## Invariants
Numbered for the executable model and the property tests.
1. A process can use only indices into its own handle table, which holds at most `MAX_HANDLES`
   handles; every live handle names a live object.
2. After budget B is destroyed, no handle stamped with B or any descendant exists anywhere, in a
   table or in a message not yet received.
3. A minted handle's badge is non-zero, and its stamp is its source's default stamp or a descendant.
4. Only badge-0 endpoint handles receive; every one is `endpoint_create`'s result or a copy of it.
5. For every budget: usage <= limit; the children's limits and own pages, plus its own objects, fit
   in its limits.
6. Labels never change; `labels(child) ⊇ labels(parent)`; only a system-class creator adds labels.
7. Every flow obeys R1 (messages, exit notices, usage reads), a message's compared with the
   endpoint's owner: handing a receive right to another budget is delegation, and a receive right
   is never handed across label sets (CONTAINMENT.md).
8. `class(child) = class(parent)`; `account(child) = account(parent)` unless the parent's is 0.
9. No page is ever mapped writable and executable; every page is zeroed before a process first sees
   it; a lent page is unmapped from its lender until the call ends.
10. Creating and then destroying a budget leaves its parent's usage and free limits unchanged, once
    its processes' exit notices are received or dropped.
11. With k groups (R2) blocked on an endpoint, and the receiving process below `MAX_OPEN_CALLS`,
    each group's oldest message is taken within k receives.
12. Budget ids are never reused; a message id is never 0 and never reused within its receiving
    process.
13. Every blocking call returns by its timeout: its `Timeout` is committed, and its thread made
    runnable, at the first kernel entry at or after the timeout, and the always-armed timer bounds
    when that entry comes by timer latency. When the thread then runs is R12's.
14. No sequence of system calls, with any arguments, panics the kernel.
15. Every abandoned call is reported to the thread holding it exactly once, and stays open until
    that thread replies; its reply reaches nobody. The report is an abandoned-call notice, or, when
    the holder's reply enters before it has seen one (the caller timed out or died just before),
    the reply's own result: `discarded`, mask 0, and no notice follows.

## Added in milestone 2
`budget_children(h) -> [h]`, so a restarted steward can enumerate and destroy what it created
(CAPABILITIES.md). Nothing else in this spec changes for milestone 2.
