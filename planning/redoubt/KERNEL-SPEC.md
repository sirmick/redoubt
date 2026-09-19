# Kernel specification

Designed, not built; frozen for milestone 1. Owns: the kernel's objects, system calls, message
shape, errors, constants and invariants, stated precisely enough to implement from. The executable
model (CONTAINMENT.md) implements exactly this: same names, arguments, errors and invariants.
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
| `WAIT_CAP` | 16 | blocked senders per account per endpoint |
| `STRIDE` | 2^20 | stride scheduling numerator |
| `SLICE` | 10 ms | time slice |
| `FOREVER` | `u64::MAX` | a timeout that never expires |

Time is monotonic microseconds since boot (`u64`). Timeouts are relative microseconds.

## Objects
Five kinds. Every object is charged in pages to one budget (its owner).

**Budget**

| Field | Type | Rule |
| --- | --- | --- |
| `id` | u64 | never reused, on both widths |
| `parent` | budget | none only for `root` |
| `class` | `system` or `user` | `class(child) <= class(parent)` (`user < system`) |
| `labels` | sorted set of u64, at most `MAX_LABELS` | fixed at creation; `labels(child) ⊇ labels(parent)` |
| `account` | u64, 0 = none | inherited; see Accounts |
| `deadline` | time or none | when it passes, the kernel destroys the budget |
| `pages` | limit, usage | every object charged here; see Charging |
| `processes` | limit, usage | PIDs double as ASIDs |
| `weight` | u32 | CPU share |

`root`, `system` and `users` are created by the kernel at boot from the argument block (`system`
gets its reserved share); `root` and `system` are class `system`, `users` is class `user`; all three
have account 0 and no labels. `init` receives handles to all three.

**Process**: PID (= ASID), owning budget, address space, up to `MAX_THREADS` threads, handle table,
exit endpoint handle, one pending exit slot, a started flag. Each thread records the account of the
message it is serving (0 when none): set when `receive` delivers a message, cleared by `reply`.

**Endpoint**: the object clients call and servers receive on. It holds the threads blocked in
`send`/`call` on it, grouped by account, and a round-robin cursor over accounts. It survives the
death of processes receiving on it.

**Device**: one of
- MMIO: a physical range, and a DMA flag;
- IRQ: an interrupt number, a `fired` flag and a `masked` flag;
- Reset: the right to power off or reboot (given only to `init`).

The loader creates device objects from the device tree; `init` receives them all.

**Handle** = (object, badge: u64, stamp: budget id), an index into a process's handle table. For an
endpoint, **badge 0 is the receive right**: only a badge-0 handle may `receive`, and `mint` never
creates badge 0. Handles to budgets, processes and devices carry badge 0.

## Messages
A message carries:
- `WORDS` words;
- up to `MAX_MSG_HANDLES` handles, **copied** (the sender keeps its own), each keeping its stamp;
- at most one buffer: a **lend** (`call` only: page-aligned, at most `MAX_LEND_PAGES`, writable,
  unmapped from the caller until the call ends) or a **transfer** (`send` only: pages whose owner and
  payer both become the receiver).

The kernel attaches, unforgeably: the **badge** of the handle it was sent through, the sender
budget's **account** and **labels**, and a **message id** (for `reply` and `mint`).

`receive` returns one of:
- a message: `(msg_id, badge, account, labels, words, handles, buffer address, lend page count or
  transferred page count)`;
- an interrupt: the IRQ handle fired;
- an exit notice (on an endpoint named as some process's exit endpoint): `(pid, cause, code,
  blamed_account)`, where `cause` is `exited`, `faulted` or `killed` and `blamed_account` is the
  account the faulting thread was serving (0 if none);
- `Timeout`.

## Rules
**R1. Label check.** When both the sender's and the receiver's budgets are class `user`, a message is
delivered only if their label sets are **equal**; otherwise the sender gets `LabelDenied`. When
either side is class `system`, the kernel does not check (system servers check themselves;
CONTAINMENT.md). An exit notice is delivered only if the receiver's labels ⊇ the exiting budget's;
otherwise it is dropped.

**R2. Fair waiting.** Blocked senders on an endpoint are served round-robin by account: each
`receive` takes the oldest message of the next account after the last one served. An account with
`WAIT_CAP` senders already blocked on an endpoint gets `Busy` immediately.

**R3. Lends outlive their lender.** If the caller dies, or its call times out, after the server took
the message, the lent pages stay mapped in the server and are **charged to the server's budget until
`reply`**, then freed. While a budget is over its page limit this way, its new allocations fail with
`OutOfMemory`. A reply to an abandoned call is discarded.

**R4. Transfer opt-in.** A receiver gets transferred pages only if its `receive` named a
`max_transfer` at least the transfer's size; a larger pending transfer fails its sender with
`Refused`, and the kernel moves on to the next sender.

**R5. Interrupts.** When an IRQ fires, the kernel masks the source and sets `fired`. `receive` on the
IRQ handle unmasks the source when it begins, then returns when `fired` is set (clearing it). There
is no acknowledge call.

**R6. Charging.** Every kernel object (pages, page tables, handle tables, thread contexts, endpoints,
budgets) is charged in pages to its owning budget. A budget's own object is charged to itself; a
budget with zero limits (a **revocation scope**) is charged to its parent. A parent's usage counts its
**children's limits**, never their live usage.

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
+= runtime x `STRIDE` / weight; on wake, pass = max(own pass, current minimum). Within a budget,
threads run round-robin. The timer is always armed (slice end or the next deadline).

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
| `process_create` | h(budget), h(exit endpoint) -> h(process) | budget's process and page limits |
| `process_map` | h(process), src, dst, len, flags | process not started; src owned by caller; pages move to the child's budget; not W+X |
| `process_start` | h(process), entry, sp, handles | not started; handles copied into slots 1..n |
| `endpoint_create` | -> h (badge 0) | pages charged |
| `mint` | source, badge, optional h(budget) -> h | see below |
| `call` | h, words, handles, lend, timeout -> reply | endpoint; R1; R2; lend rules |
| `send` | h, words, handles, transfer, timeout | endpoint; R1; R2; rendezvous |
| `receive` | h or none, timeout, max_transfer -> message, interrupt, exit notice | badge-0 endpoint, IRQ, or none (sleep) |
| `reply` | msg_id, words, handles | caller is serving msg_id; returns the lend |
| `handle_close` | h | - |
| `budget_create` | h(parent), pages, processes, weight, class, labels, account, deadline -> h | R6-R8; class and labels below; depth < `MAX_DEPTH` |
| `budget_destroy` | h(budget) | always allowed to a holder |
| `budget_usage` | h(budget) -> counters | caller's labels ⊇ target's |
| `time_now` | -> µs | - |
| `system_reset` | h(Reset), kind | Reset device handle |

**`mint(source, badge, budget?)`** creates a handle to an endpoint with `badge != 0`.
- `source` is either a message id the caller is serving (the new handle is to the endpoint the
  message arrived on; default stamp = the stamp of the handle the message was sent through), or a
  badge-0 endpoint handle the caller holds (default stamp = that handle's stamp).
- With no budget handle, the new handle gets the default stamp. With one, the budget must be the
  default stamp or a descendant of it (`NotPermitted` otherwise): a budget handle only narrows.

**`budget_create` class and labels.** Class `system` only if the parent is `system`. Labels must be a
superset of the parent's; adding labels needs the caller's own budget to be class `system`
(`ClassDenied` otherwise). Labels are sorted and deduplicated.

User mode may also read the `time` counter directly (`rdtime`).

## Errors
`BadHandle`, `WrongObject`, `InvalidArgument`, `OutOfMemory` (page limit), `OutOfProcesses`,
`TooManyThreads`, `NotPermitted`, `ClassDenied`, `LabelDenied`, `Busy`, `Refused`, `TooLarge`,
`Timeout`, `Dead`.

## Invariants
Numbered for the executable model and the property tests.
1. A process can use only indices into its own handle table; every live handle names a live object.
2. After budget B is destroyed, no handle stamped with B or any descendant exists anywhere.
3. A minted handle's badge is non-zero, and its stamp is its source's default stamp or a descendant.
4. Only badge-0 endpoint handles receive; every one is `endpoint_create`'s result or a copy of it.
5. For every budget: usage <= limit (except pages charged by R3, at most `MAX_LEND_PAGES` per thread
   of the charged budget); the children's limits plus its own objects fit in its limits.
6. Labels never change; `labels(child) ⊇ labels(parent)`; only a system-class creator adds labels.
7. Messages between two user budgets flow only between equal label sets; exit notices and usage
   reads flow only to ⊇ label sets.
8. `class(child) <= class(parent)`; `account(child) = account(parent)` unless the parent's is 0.
9. No page is ever mapped writable and executable; every page is zeroed before a process first sees
   it; a lent page is unmapped from its lender until the call ends.
10. Creating and then destroying a budget leaves its parent's usage and free limits unchanged.
11. With k accounts blocked on an endpoint, each account's oldest message is taken within k receives.
12. Budget ids are never reused.
13. Every blocking call returns by its timeout.
14. No sequence of system calls, with any arguments, panics the kernel.

## Added in milestone 2
`budget_children(h) -> [h]`, so a restarted steward can enumerate and destroy what it created
(CAPABILITIES.md). Nothing else in this spec changes for milestone 2.
