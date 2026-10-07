# Handles and objects

A **handle** is how a process holds authority: an index into a table the kernel keeps for that
process, naming one kernel object together with a **badge** and a **stamp**. The kernel has four
kinds of object (budgets, endpoints, devices and processes), each one page of RAM charged to one
budget. Handles carry no rights bits: whoever holds one may use it in full. A server narrows a
grant by minting a handle whose badge means less, and a grant is revoked by destroying the budget
its handle is stamped with.

## Purpose

Apart from its own memory and threads, the clock and random numbers, a process reaches
everything through a handle: a server through an endpoint, resources through a budget, a child
through a process handle, hardware through a device. So a process's handle table is the whole of
what it may do beyond itself, and three questions decide whether that is safe. Can a process name
something it was not given? It names only indices into its own table, and only the kernel writes
the table. Can a grant outlive the decision to revoke it? Every handle is stamped with a budget,
and destroying that budget closes every copy. Can holding objects and handles use up memory
someone else needs? Every object and every table page is charged to a budget. This page describes
those three walls and the tests that attack them.

## Interface

### The four object kinds

<details><summary>Status: built · partly tested: that no two objects ever share an id is attacked only in the model for budgets and not at all for the other kinds, because a process cannot see an id · tested (6)</summary>

- bench:budget
- bench:redoubt-ipc
- bench:redoubt-dead
- bench:process
- bench:process-attack
- bench:device

</details>

| Kind | What it is | Made by | Ends when |
| --- | --- | --- | --- |
| budget | limits on pages, processes and CPU weight, with a class, labels, an account and an optional deadline ([budgets](budgets.md)) | `budget_create`; `root`, `system` and `users` by the kernel at boot | it or a budget above it is destroyed, by `budget_destroy` or a deadline |
| endpoint | what clients call and servers receive on; it holds no queue ([IPC](ipc.md)) | `endpoint_create` | its **owner**, the budget of the process that created it, is destroyed |
| device | an MMIO range with a DMA flag, an interrupt line, or the right to power off or reboot ([devices](devices.md)) | the kernel at boot, from the loader's device list; never later | its owner is destroyed, or a DMA device fails its reset |
| process | a program with its address space and threads, and afterwards its exit notice ([processes](processes.md)) | `process_create` | its exit notice is received or dropped, or its creator's budget is destroyed |

Nothing else is an object: no handle names a thread, a page or an open call. Threads are named by
their TID within their process, pages by address, open calls by message id.

Each object lives in one RAM frame of its own. The frame's first word marks its kind, and its
second is the object's **id**, drawn from one 64-bit counter that never repeats, so no two objects
of any kind ever share an id (I12 (ids never reused)). That frame is the page the cost table
charges. No call destroys an endpoint or a device: each dies with the budget it is charged to,
and a DMA device also when its reset fails (Residual risks).

### Handles

<details><summary>Status: built · tested (10)</summary>

- bench:budget
- bench:budget-table-attack
- bench:budget-forge-attack
- bench:budget-destroy-attack
- bench:handle-chain-attack
- bench:handle-chain-fault
- bench:process-chain-fault
- bench:redoubt-ipc
- bench:device
- host:redoubt-sys::malformed_calls_are_refused

</details>

A handle is an index into the calling process's handle table. The slot at that index holds:
- the **object**: its kind, its frame and its id;
- the **badge**, a 64-bit number. For an endpoint, badge 0 is the **receive right** and any other
  badge was chosen by whoever minted the handle ([`mint`](#mint)). Handles to budgets, processes
  and devices carry badge 0;
- the **stamp**, the budget whose destruction revokes the handle ([R9](#r9-stamps)).

Indices start at 1. **Index 0 is never allocated**: in a register or a record slot it means "no
handle", for an optional argument left out or a message slot whose handle was revoked. An index
that is 0 where a handle is required, wider than 32 bits, or not in the caller's table is
`BadHandle`; a handle of the wrong kind for the call is `WrongObject`. A process cannot read its
table: it learns nothing of a handle's object, badge or stamp except by using it.

A handle is four 64-bit words. A slot adds two chain entries of two words each (below), so it
is eight words, 64 bytes, on both widths, and a 4 KiB table page holds `HANDLES_PER_PAGE` (64)
handles; a compile-time check in `kernel/src/handle.rs` confirms that 64 slots fill one page
exactly. Index i is slot (i - 1) mod 64 of table page (i - 1) / 64. A table has at most
`MAX_HANDLES` (4096: 64 pages) handles. A message's copy of a handle is the four words alone.

```mermaid
---
config:
  packet:
    bitsPerRow: 64
---
packet-beta
title one slot: eight 64-bit words, 64 bytes, so 64 slots fill a 4 KiB table page
0-27: "object's frame"
28-55: "stamp's frame"
56-63: "kind"
64-127: "object's id"
128-191: "badge"
192-255: "stamp's budget id"
256-319: "object chain: previous link"
320-383: "object chain: next link"
384-447: "stamp chain: previous link"
448-511: "stamp chain: next link"
```

The kind is 0 for an empty slot, 1 a budget, 2 an endpoint, 3 a device, 4 a process. A link is 0
for none, the head's frame with the top bit set, or the holder's PID and the index.
*Figure: a handle as the kernel stores it. A compile-time check confirms every frame index fits
its 28 bits.*

A handle dies when the object it names goes or when the budget that stamped it does. The two
chain entries let a destruction find it without reading every table
([residual risks](budgets.md#residual-risks)). When a handle arrives in a table, the kernel walks
up from the holder's budget, at most `MAX_DEPTH` steps, and enters the slot:
- in the **object chain** of the budget its object is charged to, if the holder is outside that
  budget's subtree (a budget handle's object is the budget itself); a handle to a process object
  is entered in that object's own chain wherever it is held, since the object can be freed while
  its creator lives;
- in the **stamp chain** of its stamp, if the holder is outside the stamp's subtree.

A handle held inside dies with its holder's table, which goes whole when the holder ends. Each
chain is doubly linked, from a head word in the budget's or process object's frame, so adding or
closing a handle costs a constant and using one costs nothing more. Beside its two handle chains'
heads a budget's frame holds two more heads, of its charged chain and its counted chain of process
objects; each process object carries a pair of links for each, beside
the pair that puts it on its exit endpoint's lists ([what objects cost](#what-objects-cost)).

A new handle takes the **lowest free index**. A table page is a frame of its own, allocated and
charged one page to the process's budget when its first handle arrives, and freed when its last
handle goes. Handles are never moved to fill holes, so an index stays valid until it is closed,
and a table with holes costs one page per table page in use. A table already holding
`MAX_HANDLES` handles refuses one more with `TooLarge`, which a caller can tell apart from its
budget being unable to pay for the next table page (`OutOfMemory`). At delivery, a handle that
would take the receiver past `MAX_HANDLES` is a cost it cannot pay, and the sender gets `Refused`
([R4 (delivery)](ipc.md#r4-delivery)).

```mermaid
flowchart LR
    subgraph T["process P's handle table: index, object, badge, stamp"]
        direction TB
        h1["1: budget B, badge 0, stamp system"]
        h2["2: endpoint E, badge 0, stamp system"]
        h3["3: endpoint E, badge 5, stamp S"]
        h4["4: process Q, badge 0, stamp system"]
        h5["5 to 64: empty"]
    end
    subgraph O["kernel objects"]
        direction TB
        B["budget B"]
        E["endpoint E"]
        Q["process Q"]
    end
    h1 --> B
    h2 --> E
    h3 --> E
    h4 --> Q
```

Table page 0 holds indices 1 to 64 in one frame, charged to P's budget; table page 1 (65 to 128)
has no frame until a handle needs index 65; table page 63 ends at index 4096 (`MAX_HANDLES`).
*Figure: a handle table and the objects it names. Handles 2 and 3 name the same endpoint: 2 is
its receive right, 3 a handle minted with badge 5 and stamped with S, a budget below `system`.*

Every lookup checks the id in the handle against the id in the object's frame, and the stamp's id
against its budget's. A mismatch would mean a handle outlived its object. The chains of
[R10 (destruction)](budgets.md#r10-destruction) rule that out (I2 (revocation is complete)), and
the kernel stops rather than use a frame that may hold something else
(I1 (handles name live objects)).

### Making, copying and closing handles

<details><summary>Status: built · tested (5)</summary>

- bench:budget
- bench:process
- bench:process-attack
- bench:redoubt-ipc
- bench:redoubt-revoke

</details>

| Call | Arguments -> result | The handle |
| --- | --- | --- |
| `endpoint_create` | -> h | the receive right (badge 0) to a new endpoint, owned by and charged to the caller's budget |
| `budget_create` | h(parent), spec -> h | a new child budget ([budgets](budgets.md)) |
| `process_create` | h(budget), h(exit endpoint) -> h | a new process object ([processes](processes.md)) |
| `mint` | source, badge, h(budget) or none -> h | another handle to an existing endpoint, with a non-zero badge ([below](#mint)) |
| `handle_close` | h | removed; the object is untouched |

Each call that makes a handle installs it at the lowest free index of the caller's table,
stamped as [R9](#r9-stamps) says. If the table cannot take it (`TooLarge`, or `OutOfMemory` for
a new table page), the call is undone and costs nothing.

Handles are **copied**, never moved:
- a message carries up to `MAX_MSG_HANDLES` (4) handles, installed in the receiver's table when it
  is delivered while the sender keeps its own ([IPC](ipc.md#messages));
- `process_start` copies up to `MAX_START_HANDLES` (128) of the caller's handles into the new
  process's table, at indices 1 to n in the order given ([processes](processes.md)).

A copy is the same object, badge and stamp at a new index in another table. Copies are equal:
none is the original, and closing one leaves the others. There is no call that copies a handle
within one table.

`handle_close(h)` removes the handle at index h, and frees its table page if it was the page's
last; `BadHandle` if the caller holds no handle at h. It never destroys the object: a budget,
endpoint or device lives on under its own rules, and a process handle closed early leaves the
exit notice pending. The index is free again, and the process's next new handle may take it.
When a process ends, every handle in its table closes. When a process object is freed, every
handle to it closes, in every table.

### What objects cost

<details><summary>Status: built · partly tested: an endpoint's page is attacked only in the model, a device's page by no case; the boot code departs from R6 (charging) for `root`'s own page · tested (11)</summary>

- bench:budget
- bench:budget-table-attack
- bench:budget-mem-churn
- bench:process-attack
- bench:process-review
- bench:process-lifecycle
- bench:thread-limit
- mutation:R6EndpointsFree
- mutation:R6ProcessObjectChargedToBudget
- mutation:R6OwnPageChargedToItself
- mutation:R6OpenCallsFree

</details>

Everything the kernel stores is charged in whole pages to one budget (R6 (charging)):

| What | Pages | Charged to |
| --- | --- | --- |
| budget | 1 | its parent; `root`'s own to `root`, a rule the boot code departs from ([budgets](budgets.md#residual-risks)) |
| endpoint | 1 | its owner, the budget of the process that created it |
| device | 1 | its owner: `system`, charged at boot |
| process object (it holds the exit notice) | 1 | the creator's budget, the budget of `process_create`'s caller |
| process header | 1 | the budget the process runs in |
| thread IPC page | 1 per thread | the budget the process runs in |
| page tables | 1 per page-table page, the root table included | the budget the process runs in |
| handle table | 1 per table page holding a handle | the budget the process runs in |
| open call | 1 while open | the receiving process's budget ([R4a (open calls)](ipc.md#r4a-open-calls)) |

Pages a process maps, lends or is given are charged as [memory](memory.md) and [IPC](ipc.md)
say. The header is the page at `PROCESS_AREA`
([memory layout](memory-layout.md#per-process-kernel-data)), one on both widths; a thread's saved
registers are the last bytes of its IPC page, so a thread costs one page on both widths too. The
header and each IPC page are counted once, and neither is the process object's page.

A budget's own page is its parent's, so the whole of a budget's page limit is usable and a
**revocation scope** (a budget with zero limits, made only to be destroyed) is no special case.
The process object is charged to its creator, not to the budget the process runs in, because it
holds the exit notice, and that notice must outlive the budget it ran in; the page was paid at
`process_create`, so delivering a notice never allocates. Everything the running process needs
(header, page tables, thread pages, handle table) is charged where it runs, and comes back when
it ends.

So a process object belongs to two budgets, and each heads a chain of them, doubly linked through
the objects' own frames from a head word in the budget's: the creator's **charged chain**, of the
objects charged to it, and a budget's **counted chain**, of the objects whose PIDs it counts, which
while a process lives is the budget it runs in. An object joins both when `process_create` makes it and leaves both when
it is freed. A destruction finds the processes it kills, the objects it frees and the PIDs it moves
from the dying budgets' chains alone ([R10](budgets.md#r10-destruction)).

Every charge comes back exactly, with one exception. Closing a handle, unmapping, replying,
receiving an exit notice and destroying a budget each return what they freed; a refused call
charges nothing. The exception is a DMA page quarantined because its device did not confirm a
reset: it is never freed, so its charge stays, and moves to the parent when its budget is
destroyed ([devices](devices.md#quarantine)). A table of
n handles filled without closing any costs exactly ceil(n / 128) pages.

### `mint`

<details><summary>Status: built · partly tested: `Dead` from a message source whose endpoint or stamp has gone is attacked by no case · tested (5)</summary>

- bench:redoubt-ipc
- bench:redoubt-ipc-attack
- bench:redoubt-revoke
- host:redoubt-sys::malformed_calls_are_refused
- mutation:MintFromUnservedMessage

</details>

`mint(source, badge, budget?) -> h` makes a new handle to an endpoint, with a badge the caller
chooses. The source is one of:
- **a receive right** the caller holds: the new handle is to that endpoint, and its **default
  stamp** is the receive right's stamp;
- **the message id of an open call held by the calling thread**: the new handle is to the
  endpoint the call arrived on, and its default stamp is the stamp of the handle the call came
  through. A `send`'s id, another thread's call and an id the thread never received are
  `InvalidArgument`; if the call's endpoint or its stamp has gone meanwhile, `Dead`.

The rules:
- The badge is never 0: 0 is the receive right, and `mint` never makes one. The decoder in
  `redoubt-sys` refuses it, and the kernel checks again
  (I3 (minted badges are non-zero and narrow)).
- Only a receive right mints: a source handle with any other badge is `NotPermitted`. So only a
  holder of the receive right hands out badges for an endpoint
  (I4 (only badge-0 handles receive)).
- With no budget handle, the new handle gets the default stamp. With one, that budget must be the
  default stamp or below it in the budget tree, or the call is `NotPermitted`: a budget handle
  only narrows.
- The new handle is installed like any other: the lowest free index, `TooLarge` or `OutOfMemory`
  if the table cannot take it.

The badge means whatever the server says: a 9P root and its mode, one client's session, one
grant in a typed protocol ([the servers](../servers/README.md)). The kernel only guarantees it:
every message sent through the handle carries it ([R14 (unforgeable sender)](ipc.md#r14-unforgeable-sender)).
Minting from a call is how a server answers a request with a new capability: the handle it
returns is stamped like the handle the request came through, so it dies with the client's grant.
The full order of checks is in the [ABI reference](abi.md#errors-and-the-order-of-checks).

```mermaid
flowchart TD
    root[root] --> system[system]
    root --> users[users]
    system --> S["S: a revocation scope"]
    RR["receive right on E<br/>badge 0, stamp system"]
    RR -- "mint badge 5" --> H5["E, badge 5<br/>stamp system"]
    RR -- "mint badge 6 into S" --> H6["E, badge 6<br/>stamp S"]
    RR -- "mint badge 7 into root<br/>or into users" --> NP["NotPermitted:<br/>not system or below"]
```
*Figure: narrowing. A receive right stamped with `system` mints handles stamped with `system` or a
budget below it, never above or beside it. Destroying S revokes the badge-6 handle and every copy
of it; the badge-5 handle lasts as long as `system`.*

Because `mint` only narrows, a server can mint a handle into a `user`-class budget only from a
receive right whose stamp is that budget or an ancestor of it. A server's own endpoints are stamped
with its own budget, so a `system`-class keeper — the steward, whose sessions and leases are carved
from `users` — cannot mint a session's or lease's handles from them: `users` is beside `system`,
not below it. The receive rights it mints from are therefore ones a process running at or above
`users` created: `init`, which runs in `root`, makes its servers' endpoints stamped `root` (below
which everything sits) and mints the badged handles its manifest names
([init](../servers/init.md#starting-the-servers)); a helper run in `users` itself stamps its
endpoints `users` and so can mint into anything under `users`. That helper must run in `users`
itself, not in a child of it (a child's stamp would be the leases' sibling, not their ancestor),
and it may exit at once, because an endpoint dies only with its owner budget, never with the
process that created it.

## Authority

<details><summary>Status: built · tested (6)</summary>

- bench:redoubt-ipc-attack
- bench:budget-forge-attack
- bench:device
- bench:process-attack
- mutation:ReceiveWithBadgedHandle
- mutation:ExitEndpointBadged

</details>

What each handle lets its holder do:

| Handle | Its holder may |
| --- | --- |
| budget | carve a child from it (`budget_create`), destroy it and everything below it (`budget_destroy`), destroy its children one at a time and keep it (`budget_reap`), read its usage (`budget_usage`, subject to [R1 (flow)](ipc.md#r1-flow)), run a process in it (`process_create`), and name it to narrow a `mint` |
| endpoint, badge 0 (the receive right) | `receive` on it, `mint` handles to it, `call` and `send` through it, and name it as a new process's exit endpoint |
| endpoint, any other badge | `call` and `send` through it; the server sees the badge |
| process | `process_map` into it and `process_start` it, both only before it starts |
| device, MMIO | `map_device`; `dma_alloc` too if it has the DMA flag |
| device, IRQ | `receive` on it |
| device, Reset | `system_reset` |

A handle of another kind gets `WrongObject`. A device handle is the only way to hardware
([R18 (device authority)](devices.md#r18-device-authority)).

- **A process uses only its own table.** An index is looked up in the caller's table and nowhere
  else, so no number a process passes names another process's handle, and a forged index is
  `BadHandle` (I1).
- **No rights bits.** Whoever holds a copy of a handle may do everything in its row. A grant is
  narrowed by the server minting a badge that means less, or by giving a child budget in place of
  a budget, never by a flag on the handle.
- **Authority grows only three ways:** a creating call makes a new object paid from the caller's
  own budget; `mint` makes a new badge on an endpoint whose receive right the caller holds or
  whose call it took; a copy passes on a handle the sender already holds. None gives the caller
  authority over anything it did not already hold or pay for.
- **A budget handle is the right to end that budget.** Any holder may destroy it and everything
  below it, so a budget handle is given only to a party that may end it. A party that needs a
  budget only to narrow a `mint` is given a revocation scope, whose destruction ends no process
  ([budgets](budgets.md)).

## Security properties

### R9 (stamps)

<details><summary>Status: built · partly tested: a handle minted from a call taking that call's stamp is attacked only in the model · tested (7)</summary>

- bench:process-attack
- bench:redoubt-revoke
- bench:budget-deadline
- mutation:R9ReceivedHandleRestamped
- mutation:R9MintStampsCaller
- mutation:R9MsgStampIsSenderBudget
- mutation:R10KeepForeignHandles

</details>

Every handle has a stamp, a budget. Destroying that budget, or any budget above it, closes the
handle and every copy of it, in every table; a copy in a message not yet received arrives as 0
([R10 (destruction)](budgets.md#r10-destruction), I2). Which budget the stamp is:
- `endpoint_create`, `budget_create` and `process_create` stamp the new handle with the
  **caller's budget**, the one its process runs in. A budget handle made by a process in budget A
  dies with A, even though the budget it names sits under another parent and outlives it.
- **A copy keeps its stamp.** A handle delivered in a message or a reply, or placed by
  `process_start`, is stamped as the sender's was. No path restamps a handle.
- **`mint` stamps with the source's default stamp**, or with a budget the caller names at or below
  it ([`mint`](#mint)). The default stamp of a call is the stamp of the handle the call came
  through, not the caller's budget.
- Handles the kernel places at boot, before any process runs, are stamped with `root` for the
  budgets and devices `init` receives ([boot](boot.md)).

So a stamp only moves down the budget tree. A handle minted from another dies no later than its
source, and a handle minted in answer to a call dies no later than the handle the call came
through: a client whose grant is revoked loses what servers minted for it as well.

The stamp is not the object's owner. An object dies with the budget it is charged to; a handle
dies with its stamp; either ends the handle, because R10 closes every handle whose object or stamp
is being destroyed.

## Failure and restart

<details><summary>Status: built · tested (5)</summary>

- bench:budget-destroy-attack
- bench:budget-table-attack
- bench:process
- bench:process-attack
- bench:redoubt-revoke

</details>

- **A process ends:** every handle in its table closes and its table pages return to its budget.
  The objects they named are untouched: an endpoint it received on stays for a restarted server
  ([R4b (a server dies)](ipc.md#r4b-a-server-dies)).
- **A budget is destroyed:** every handle naming it or a budget below it, naming an endpoint,
  device or process object charged to one of them, or stamped with one of them, closes in every
  table, and a copy in a message not yet received arrives as 0 (R10).
- **A process object is freed**, when its notice is received or dropped or its creator's budget
  is destroyed: every handle to it closes.
- **A creating call cannot install its handle:** the object is freed and its charge returned, so
  a refused create costs nothing.
- **A handle whose object has gone** is a kernel bug, not an error a call returns. Every lookup
  compares ids, and on a mismatch the kernel stops rather than use a frame that may hold something
  else (I1). R10's sweeps keep it from happening (I2), and no argument to any call reaches it
  (I14 (no call panics the kernel)); `budget-destroy-attack` reuses frames and indices after a
  destruction to look for one.

## Residual risks

- **No call destroys an endpoint.** An endpoint lives, and costs its owner a page, until the owner
  budget is destroyed; closing every handle to it frees nothing. The owner is always the creating
  process's own budget, so a server that makes an endpoint per client pays for each until that
  budget goes, bounded by the budget's page limit. Servers make one per service instead (Why).
- **A device object is never remade.** None is created after boot; one destroyed with its owner,
  or for failing a DMA reset, is gone until reboot ([devices](devices.md)).
- **Revocation is by budget only.** A grant is revoked by destroying the budget it is stamped
  with, and with it everything else stamped there. A server that means to cut off one client alone
  must have minted that client's handle into a revocation scope of its own. Otherwise it can only
  stop honouring the badge: the handle still reaches the endpoint, and the refusal is the
  server's, not the kernel's.
- **A budget handle cannot be narrowed.** Every holder may destroy the budget and everything
  below it: a handle to `system` is the right to end every system server, and a handle to `root`
  the right to end every process.
- **A stale handle stops the machine.** The kernel's answer to a broken I1 is to stop. A bug in
  R10's chain walk would stop the kernel, and every process with it; it would not let a process use a
  freed object.
- **Slot searches and one sweep scan.** Destroying a budget closes the handles in its chains and
  frees the dying tables whole; it walks the dying subtree, its owner lists and its process
  chains, not the object
  frames or the live tables ([budgets](budgets.md#residual-risks)). Destroying a quarantined
  device still closes its handles in one pass over every table of every process (up to
  `MAX_PROCESS_COUNT` (511) processes of 64 table pages), bounded by compile-time constants.
  Installing a handle searches for the lowest free slot of one table.

## Why

- **No rights bits.** A read-only or non-transferable handle would not hold: its holder can
  proxy for anyone it talks to. So the kernel checks one thing, that the caller's table holds the
  handle, and meaning lives where it is enforced: in the badge, which the server chose and checks
  (a 9P root, read-only), and in the stamp, which bounds how long the grant lasts.
- **Budgets are the only revocation.** There is no per-handle revoker and no derivation tree to
  walk. Every handle already names a budget in its stamp, and destroying a budget is already the
  one way resources come back, so revocation reuses it. A revocation scope, a budget with zero
  limits, makes any grant revocable on its own for one page. Budget ids are never reused, so a
  stale stamp never matches a later budget.
- **Stamps only narrow.** A server answering a call mints under the call's stamp by default, so
  what it hands a client dies with the client's grant, and the server never holds the client's
  budget.
- **An endpoint dies only with its owner.** A server serves every client on one endpoint and
  tells them apart by badge ([the servers](../servers/README.md#connections)), so it makes one
  endpoint per service, not per client, and no server needs to free one on its own. A call that
  did would add a destruction path to the kernel for a cost that already falls only on the
  owner's own budget, bounded by its page limit.
- **Stamp and owner are separate.** Who pays for an object and who may revoke a handle to it are
  different questions. A process in budget A may create a budget under B for someone else; B's
  child lives as long as B, while A's own handle to it dies with A.
- **Every object is a page of its own.** There is no kernel table of fixed size that one budget
  could fill for everyone: creating an object fails only on the caller's own budget
  ([R7 (carving)](budgets.md#r7-carving)). Handle tables work the same way, a page at a time.
- **Lowest free index, never compacted.** An index stays valid until it is closed, so a program
  can keep a number. A table with holes costs a page per page in use, which is what its memory
  costs, and the [model](model.md) charges the same.
- **`TooLarge` at `MAX_HANDLES`**, so a caller can tell a full table from a budget out of pages;
  the limit also bounds every scan of a table.
- **0 means none**, so optional arguments and revoked message slots need no second encoding.
- **The process object is its creator's**, because its exit notice must outlive the budget the
  process ran in; the page was paid at creation, so a notice never allocates.
