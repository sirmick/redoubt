# Kernel specification

Designed, not built; frozen for milestone 1. Owns: the kernel's objects and their costs, system
calls, message shape, errors and the order of checks, constants and invariants, stated precisely
enough to implement from. The executable model (CONTAINMENT.md) implements exactly this: same names,
arguments, errors and invariants; where the two differ, the model changes.
Rationale lives elsewhere: handles and IPC in CAPABILITIES.md, labels in CONTAINMENT.md, budgets and
scheduling in RESOURCES.md.

## Constants
Initial values; changing one is a spec change (HISTORY.md).

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
| `MAX_RANDOM` | 64 | bytes one `random` call returns |
| `WAIT_CAP` | 16 | blocked senders per (account, label set) per endpoint |
| `STRIDE` | 2^20 | stride scheduling numerator |
| `SLICE` | 10 ms | time slice |
| `FOREVER` | `u64::MAX` | a timeout that never expires; as a deadline, none |

Time is monotonic microseconds since boot (`u64`). Timeouts are relative microseconds, added to the
current time with saturation (so `FOREVER` never expires). A budget deadline is absolute
microseconds since boot.

## Objects
Five kinds. Every object is charged in pages to one budget (its owner).

**Budget**

| Field | Type | Rule |
| --- | --- | --- |
| `id` | u64 | never reused, on both widths |
| `parent` | budget | none only for `root` |
| `class` | `system` or `user` | `class(child) <= class(parent)` (`user < system`) |
| `labels` | sorted set of u64, at most `MAX_LABELS` | fixed at creation; `labels(child) ⊇ labels(parent)` |
| `account` | u64, 0 = none | inherited (R8) |
| `deadline` | time or none | when it passes, the kernel destroys the budget |
| `pages` | limit, usage | every object charged here (R6) |
| `processes` | limit, usage | PIDs double as ASIDs |
| `weight` | u32 limit, carved | CPU share; carved by children (R7); weight 0 holds no process |

`root`, `system` and `users` are created by the kernel at boot from the argument block (`system`
gets its reserved share); `root` and `system` are class `system`, `users` is class `user`; all three
have account 0 and no labels. `init` receives handles to all three.

**Process**: PID (= ASID), owning budget, address space, up to `MAX_THREADS` threads, handle table,
exit endpoint handle, an exit slot, a started flag, and its open calls (at most `MAX_OPEN_CALLS`).
The **exit slot** holds the one exit notice; it is charged to the creator's budget by
`process_create`, so a notice never allocates. It is freed when the notice is received or dropped,
or with the creator's budget (R10). Each thread records the account of the message it is serving (0
when none): set when `receive` delivers a message, cleared by `reply`.

An **open call** is a `call` a thread has taken with `receive` and not yet replied to. A thread may
hold several; its process may hold at most `MAX_OPEN_CALLS`. Each costs a page (below) while it is
open.

**Endpoint**: the object clients call and servers receive on. Its **owner** is the budget of the
process that created it (it is charged there). It holds the threads blocked in `send`/`call` on it,
grouped by the sender's (account, label set), and a round-robin cursor over those groups. It
survives the death of processes receiving on it.

**Device**: one of
- MMIO: a physical range, and a DMA flag;
- IRQ: an interrupt number, a `fired` flag and a `masked` flag;
- Reset: the right to power off or reboot (given only to `init`).

The loader creates device objects from the device tree; `init` receives them all.

**Handle** = (object, badge: u64, stamp: budget id), an index into a process's handle table.
**Index 0 is never allocated** and means "no handle" where a handle is optional. For an endpoint,
**badge 0 is the receive right**: only a badge-0 handle may `receive`, and `mint` never creates
badge 0. Handles to budgets, processes and devices carry badge 0.

**What objects cost.** Every object is charged in whole pages (R6):

| Object | Pages | Charged to |
| --- | --- | --- |
| budget | 1 | itself; a revocation scope's to its parent |
| process | 1 | its budget |
| thread | 1 | its process's budget |
| endpoint | 1 | its owner |
| page tables | 1 per page-table page, when allocated | the process's budget |
| handle table | 1 per 128 handles | the process's budget |
| open call | 1 while open | the receiving process's budget |
| exit slot | 1 | the creator's budget (`process_create`'s caller) |

Plus the pages themselves: mapped, lent under R3, or transferred. The handle-table figure assumes a
handle of 24-32 bytes (object reference, 64-bit badge, 64-bit stamp); WP-K1's implementer confirms
it, and a different figure is a change to this table. The exit slot is its own page, not part of
the process's: the process is charged to its budget and dies with it (R10), but its `killed`
notice must outlive it, paid by someone still alive. One page, like every other object, keeps the
accounting uniform.

## Messages
A message carries:
- `WORDS` words;
- up to `MAX_MSG_HANDLES` handles, **copied** (the sender keeps its own), each keeping its stamp;
- at most one buffer: a **lend** (`call` only: page-aligned, at most `MAX_LEND_PAGES`, writable,
  unmapped from the caller until the call ends) or a **transfer** (`send` only: pages whose owner and
  payer both become the receiver).

The kernel attaches, unforgeably: the **badge** of the handle it was sent through, the sender
budget's **account** and **labels**, and a **message id** (for `reply` and `mint`). Message ids are
non-zero and never reused, so a stale `reply` or `mint` cannot reach a later message.

`receive` returns one of:
- a message: `(kind, msg_id, badge, account, labels, words, handles, buffer address, page count)`,
  where `kind` is `call` (a reply is owed; the buffer, if any, is a lend) or `send` (no reply; the
  buffer, if any, is a transfer);
- an interrupt: the IRQ handle fired;
- an exit notice (on an endpoint named as some process's exit endpoint): `(pid, cause, code,
  blamed_account)`, where `cause` is `exited`, `faulted` or `killed` and `blamed_account` is the
  account the faulting thread was serving (0 if none);
- `Timeout`.

## Rules
**R1. Label check.** The receiver is the endpoint's **owner** budget, whichever thread takes the
message, so the check is decided when the message is sent. When both the sender's budget and the
owner are class `user`, a message is delivered only if their label sets are **equal**; otherwise the
sender gets `LabelDenied`. When either side is class `system`, the kernel does not check (system
servers check themselves; CONTAINMENT.md). An exit notice is delivered only if the exit endpoint's
owner is class `system` or its labels ⊇ the exiting budget's; otherwise it is dropped.

**R2. Fair waiting.** Blocked senders on an endpoint are grouped by the sender budget's (account,
label set) and served round-robin by group: each `receive` takes the oldest message of the next
group after the last one served. A group with `WAIT_CAP` senders already blocked on an endpoint gets
`Busy` immediately. (Keyed by label set too, so a vault session and its owner's unlabelled session,
which share an account, share neither a turn nor a cap: CONTAINMENT.md.)

**R3. Lends outlive their lender.** If the caller dies, or its call times out, after the server took
the message, the lent pages stay mapped in the server and are **charged to the server's budget until
`reply`**, then freed. While a budget is over its page limit this way, its new allocations fail with
`OutOfMemory`. A reply to an abandoned call is discarded.

**R4. Transfer opt-in.** A receiver gets transferred pages only if its `receive` named a
`max_transfer` at least the transfer's size, and its process's budget has the free pages to hold
them. Otherwise the pending transfer fails its sender with `Refused`, and the kernel moves on to the
next sender. (`Refused` tells the sender one bit about the receiver's budget, and only a sender the
receiver chose to accept transfers from.)

**R4a. Open calls.** A `receive` on an endpoint while the process holds `MAX_OPEN_CALLS` open calls
gets `Busy`. Taking a `call` opens it and charges its page to the receiving process's budget; if the
budget cannot pay for that page, the message's handles or the page tables to map its lend, the
`receive` gets `OutOfMemory` and the message stays queued. `reply` closes it and frees the page.
`reply` to a `send`'s message id gets `InvalidArgument`: a send is never an open call.

**R4b. A server dies.** When a thread or process exits, faults or is killed holding open calls, each
of their callers gets `Dead` and its lend back; a lend whose caller had already died (R3) is freed.
Senders still blocked on the endpoint keep waiting: the endpoint survives, and a restarted server
receives them (INIT.md).

**R5. Interrupts.** When an IRQ fires, the kernel masks the source and sets `fired`. `receive` on the
IRQ handle unmasks the source when it begins, then returns when `fired` is set (clearing it). There
is no acknowledge call.

**R6. Charging.** Every kernel object (pages, page tables, handle tables, thread contexts, endpoints,
budgets, open calls, exit slots) is charged in pages to its owning budget, as the cost table says.
A budget's own object is charged to itself; a budget with zero limits (a **revocation scope**) is
charged to its parent. A parent's usage counts its **children's limits**, never their live usage.

**R7. Carving.** A child's page, process and weight limits come out of the parent's free limits: the
children never add up to more than the parent. Allocation fails only on the caller's own budget.

**R8. Accounts.** A new budget's account equals its parent's, unless the parent's is 0; then the
creator may set any value (the steward sets one on each principal's top budget).

**R9. Stamps.** `endpoint_create`, `budget_create` and `process_create` stamp the new handle with the
caller's budget. A handle received in a message keeps its stamp. For `mint`, see the table.

**R10. Destruction.** Destroying budget B destroys its descendants first, kills their processes (each
exit notice has cause `killed`), closes every handle stamped with B or a descendant in every
process's table, frees every object charged to them, and returns their carved limits to B's parent.
Calls blocked on a destroyed endpoint, and calls in flight to it, fail with `Dead`.

**R11. Memory.** No mapping is ever writable and executable. Every page is zeroed before a process
first sees it. Userspace never maps RAM by physical address; a DMA driver learns the physical
address of pages the kernel gave it.

**R12. Scheduling.** System-class runnable budgets run before user-class ones. Within a class, one
flat stride queue over budgets with runnable threads: run the lowest pass; at every deschedule, pass
+= runtime x `STRIDE` / weight (never 0: a weight-0 budget holds no process); on wake, pass =
max(own pass, current minimum). Within a budget, threads run round-robin. The timer is always armed
(slice end or the next deadline).

## System calls
`h` is a handle. Every call returns a result or one error from the enum below; no argument can make
the kernel panic.

| Call | Arguments -> result | Checks |
| --- | --- | --- |
| `map_anon` | len, flags -> addr | pages charged; not W+X; zeroed |
| `unmap` | addr, len | own mapping; not currently lent |
| `set_flags` | addr, len, flags | own mapping; not W+X |
| `map_device` | h(MMIO) -> addr | MMIO device handle |
| `dma_alloc` | h(MMIO), npages -> addr, phys | DMA flag; pages charged; contiguous; zeroed |
| `thread_create` | entry, sp, arg -> tid | pages charged; fewer than `MAX_THREADS` |
| `thread_exit` | - | - |
| `process_exit` | code | exit notice `exited` |
| `process_create` | h(budget), h(exit endpoint) -> h(process) | budget's weight not 0; its process and page limits; exit slot charged to the caller |
| `process_map` | h(process), src, dst, len, flags | process not started; src owned by caller; pages move to the child's budget; not W+X |
| `process_start` | h(process), entry, sp, handles | not started; at most `MAX_START_HANDLES` handles, copied into slots 1..n |
| `endpoint_create` | -> h (badge 0) | pages charged |
| `mint` | source, badge, optional h(budget) -> h | see below |
| `call` | h, words, handles, lend, timeout -> reply | endpoint; R1; R2; lend rules |
| `send` | h, words, handles, transfer, timeout | endpoint; R1; R2; rendezvous |
| `receive` | h or none, timeout, max_transfer -> message, interrupt, exit notice | badge-0 endpoint, IRQ, or none (sleep); R4a |
| `reply` | msg_id, words, handles | msg_id is an open call of the caller's thread; returns the lend |
| `handle_close` | h | - |
| `budget_create` | h(parent), pages, processes, weight, class, labels, account, deadline -> h | R6-R8; class and labels below; depth < `MAX_DEPTH` |
| `budget_destroy` | h(budget) | always allowed to a holder |
| `budget_usage` | h(budget) -> counters | caller's budget is class `system`, or its labels ⊇ target's |
| `time_now` | -> µs | - |
| `random` | len -> bytes | `len` at most `MAX_RANDOM`; bytes from the kernel's CSPRNG (seeded at boot, BOOT.md) |
| `system_reset` | h(Reset), kind | Reset device handle |

**`mint(source, badge, budget?)`** creates a handle to an endpoint with `badge != 0`.
- `source` is either a message id the caller is serving (the new handle is to the endpoint the
  message arrived on; default stamp = the stamp of the handle the message was sent through), or a
  badge-0 endpoint handle the caller holds (default stamp = that handle's stamp).
- With no budget handle, the new handle gets the default stamp. With one, the budget must be the
  default stamp or a descendant of it (`NotPermitted` otherwise): a budget handle only narrows.

**`budget_create` class and labels.** Class `system` only if the parent is `system` and the
caller's own budget is class `system`. Labels must be a superset of the parent's; adding labels
needs the caller's own budget to be class `system`. Either class check failing is `ClassDenied`.
Labels are sorted and deduplicated. The deadline is absolute; `FOREVER` means none.

**`budget_usage` counters:** page limit and usage, process limit and usage, weight limit and carved
weight (the children's weight limits; R7), so a holder can see the free weight it may still carve.

User mode may also read the `time` counter directly (`rdtime`).

## ABI
This note owns the calls: their names, arguments, results and errors. The ABI crate `redoubt-sys`
owns how they travel (which registers, which records, which numbers), documented in its crate docs,
and must match this note. These rules of the encoding are part of the spec:
- **One register layout on both widths.** A 64-bit argument or result (ids, badges, accounts, time)
  always takes two registers, each holding a 32-bit half, low half first. Other register values are
  a 32-bit value or one address or length. `redoubt-sys` therefore has no width `cfg`
  (MEMORY-LAYOUT.md).
- **Records** (what does not fit in registers: message bodies, a budget's fields, the
  `process_start` list, what `receive` returns) are arrays of 64-bit little-endian slots at an
  8-byte-aligned address in the caller's memory, the same on both widths.
- **Decoding refuses W+X flags and a `mint` badge of 0**; the kernel's mapping and minting code
  refuse them again (R11, I3), so neither rests on one check.

## Errors and the order of checks
Errors: `BadHandle`, `WrongObject`, `InvalidArgument`, `OutOfMemory` (page limit), `OutOfProcesses`,
`TooManyThreads`, `NotPermitted`, `ClassDenied`, `LabelDenied`, `Busy`, `Refused`, `TooLarge`,
`Timeout`, `Dead`.

A call with several faults returns the first one found, in this order, so the kernel, the
executable model and a replayed trace (WP-C1) agree exactly:
1. **Decoding**: the registers in order (`a1` first), then each record the call passes (its
   alignment and whether it lies in the caller's own memory, readable for input and writable for
   output, then its slots in order). A required handle that is 0 or does not fit in 32 bits is
   `BadHandle`; a list longer than its limit is `TooLarge`; anything else malformed (an unknown call
   number, tag or flag bit, W+X flags, a value too wide for its field, a non-zero unused register or
   slot, a misaligned record) is `InvalidArgument`.
2. **Kernel argument checks**, argument by argument: a handle exists (`BadHandle`) and names the
   right kind of object (`WrongObject`); a size is within its fixed limit (`TooLarge`); a range is
   page-aligned, non-empty, in user space and mapped as the call needs (`InvalidArgument`).
3. **Permission**: `NotPermitted`, `ClassDenied`, `LabelDenied`.
4. **Resources**: `OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`.
5. **At delivery** (`call`, `send`, `receive`, after blocking or not): `Refused`, `Timeout`,
   `Dead`, `OutOfMemory`.

Per call, in the order checked. "Then" lists stages 2-5; "each h" is every handle in a list, in
order. A call that allocates also fails with `OutOfMemory` when the caller's handle table must grow
and its budget cannot pay (last, after the errors listed).

| Call | Decoding | Then |
| --- | --- | --- |
| `map_anon` | flags: `InvalidArgument` | `InvalidArgument` (len 0 or unaligned; flags 0, or W without R), `OutOfMemory` |
| `unmap` | - | `InvalidArgument` (range; a page not the caller's own mapping, or lent out) |
| `set_flags` | flags: `InvalidArgument` | `InvalidArgument` (range; flags; a page not the caller's own mapping) |
| `map_device` | h: `BadHandle` | `BadHandle`, `WrongObject` (not MMIO), `OutOfMemory` (page tables) |
| `dma_alloc` | h: `BadHandle` | `BadHandle`, `WrongObject`, `InvalidArgument` (npages 0), `NotPermitted` (no DMA flag), `OutOfMemory` |
| `thread_create` | - | `TooManyThreads`, `OutOfMemory` |
| `thread_exit` | - | - |
| `process_exit` | code: `InvalidArgument` | - |
| `process_create` | each h: `BadHandle` | `BadHandle`, `WrongObject` (budget), `BadHandle`, `WrongObject` (exit endpoint), `InvalidArgument` (budget weight 0), `OutOfProcesses`, `OutOfMemory` (the budget: process, page tables; then the caller: exit slot) |
| `process_map` | h: `BadHandle`; flags: `InvalidArgument` | `BadHandle`, `WrongObject`, `InvalidArgument` (src range, not the caller's own RAM; dst range, occupied; flags), `NotPermitted` (started), `OutOfMemory` (the child's budget) |
| `process_start` | h: `BadHandle`; count over `MAX_START_HANDLES`: `TooLarge`; list: record, each h `BadHandle` | `BadHandle`, `WrongObject`, `BadHandle` (each h), `NotPermitted` (started), `OutOfMemory` (the child's budget: thread, then table) |
| `endpoint_create` | - | `OutOfMemory` |
| `mint` | source: tag `InvalidArgument`, handle `BadHandle`; badge 0: `InvalidArgument`; budget h: `BadHandle` | source: a message id the caller is not serving `InvalidArgument`, its endpoint or stamp gone `Dead`; or a handle `BadHandle`, `WrongObject`; budget: `BadHandle`, `WrongObject`; `NotPermitted` (a handle source's badge not 0), `NotPermitted` (budget not the default stamp or below), `OutOfMemory` |
| `call` | h: `BadHandle`; lend: exactly one of address and page count 0 is `InvalidArgument`; body: record, count `TooLarge`, each h `BadHandle` | `BadHandle`, `WrongObject` (not an endpoint), `BadHandle` (each h), `TooLarge` (lend over `MAX_LEND_PAGES`), `InvalidArgument` (lend not page-aligned or not the caller's own writable RAM), `LabelDenied` (R1), `Busy` (R2); at delivery: `Timeout`, `Dead`, `OutOfMemory` (the reply's handles do not fit the caller) |
| `send` | as `call`, with the transfer for the lend | as `call` without the lend limit; at delivery: `Refused` (R4), `Timeout`, `Dead` |
| `receive` | h (0 = none): `BadHandle`; record | `BadHandle`, `WrongObject` (not an endpoint or IRQ), `NotPermitted` (badge not 0), `Busy` (R4a); at delivery: `Timeout`, `Dead` (endpoint destroyed), `OutOfMemory` (R4a; the message stays queued) |
| `reply` | body: record, count `TooLarge`, each h `BadHandle` | `InvalidArgument` (msg_id not an open call of the caller's thread, including a `send`'s id), `BadHandle` (each h) |
| `handle_close` | h: `BadHandle` | `BadHandle` |
| `budget_create` | h: `BadHandle`; spec: record, labels over `MAX_LABELS` before deduplication `TooLarge`, class tag or a 32-bit field `InvalidArgument` | `BadHandle`, `WrongObject`, `TooLarge` (the child would be at depth `MAX_DEPTH`), `ClassDenied` (class `system`), `LabelDenied` (labels not ⊇ the parent's), `ClassDenied` (labels added), `OutOfMemory` (pages over the parent's free pages, or no free page in the parent for a scope's object), `OutOfProcesses`, `InvalidArgument` (weight over the parent's free weight), `OutOfMemory` (pages fewer than the budget's own object) |
| `budget_destroy` | h: `BadHandle` | `BadHandle`, `WrongObject` |
| `budget_usage` | h: `BadHandle` | `BadHandle`, `WrongObject`, `LabelDenied` (a user-class caller whose labels ⊉ the target's) |
| `time_now` | - | - |
| `random` | len over `MAX_RANDOM`: `TooLarge` | `InvalidArgument` (the bytes not the caller's own writable memory) |
| `system_reset` | h: `BadHandle`; kind: `InvalidArgument` | `BadHandle`, `WrongObject` (not the Reset device) |

Two exceptions to the stages, both stated in the rows: a weight over the parent's free weight is
`InvalidArgument` (no error names weight), and `mint` from a message whose endpoint or stamp is gone
is `Dead` at the argument stage. Every call can also fail decoding in the general ways of stage 1
(unused registers, values too wide).

## Invariants
Numbered for the executable model and the property tests.
1. A process can use only indices into its own handle table; every live handle names a live object.
2. After budget B is destroyed, no handle stamped with B or any descendant exists anywhere.
3. A minted handle's badge is non-zero, and its stamp is its source's default stamp or a descendant.
4. Only badge-0 endpoint handles receive; every one is `endpoint_create`'s result or a copy of it.
5. For every budget: usage <= limit (except pages charged by R3, at most `MAX_LEND_PAGES` per open
   call held by the budget's processes); the children's limits plus its own objects fit in its
   limits.
6. Labels never change; `labels(child) ⊇ labels(parent)`; only a system-class creator adds labels.
7. Messages between two user budgets flow only between equal label sets; exit notices and usage
   reads flow only to ⊇ label sets or to a system-class budget.
8. `class(child) <= class(parent)`, and only a system-class caller creates a system-class budget;
   `account(child) = account(parent)` unless the parent's is 0.
9. No page is ever mapped writable and executable; every page is zeroed before a process first sees
   it; a lent page is unmapped from its lender until the call ends.
10. Creating and then destroying a budget leaves its parent's usage and free limits unchanged.
11. With k (account, label set) groups blocked on an endpoint, each group's oldest message is taken
    within k receives.
12. Budget ids and message ids are never reused; a message id is never 0.
13. Every blocking call returns by its timeout.
14. No sequence of system calls, with any arguments, panics the kernel.

## Added in milestone 2
`budget_children(h) -> [h]`, so a restarted steward can enumerate and destroy what it created
(CAPABILITIES.md). Nothing else in this spec changes for milestone 2.
