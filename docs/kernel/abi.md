# System call reference

A process reaches the kernel in one way: an `ecall` with a call number in `a0` and its
arguments in `a1`-`a7`. This page is the reference for that interface: the register layout,
which is the same on rv32 and rv64; every call's number, arguments and result; the records that
carry what does not fit in registers; the error codes; and the exact order in which the kernel
checks a call's arguments. The crate `redoubt-sys` (`libs/sys`) is the encoding, shared by the
kernel and every process; what each call does is explained on the page linked from its row.

## Purpose

The system call interface is the whole of the kernel's surface as a process sees it. Every
action an agent takes, hostile or not, arrives as eight registers and perhaps a record. So the
interface has three jobs: give every value exactly one encoding, so nothing can hide in a
register the kernel ignores; refuse every malformed value with an error before the kernel acts
on it (I14 (no call panics the kernel)); and report the same first error that the
[executable model](model.md) reports, so a trace means the same thing to both.

## One register layout on both widths

Status: built · partly tested: that the kernel preserves the registers outside `a0`-`a7` is not attacked by a case · tested: host:redoubt-sys::every_call_round_trips, host:redoubt-sys::every_result_and_error_round_trips, host:redoubt-sys::random_registers, bench:budget-syscall-attack

A call is an `ecall` from user mode with `a0` = the call's number and its arguments in `a1`,
`a2`, ... in the order its row lists them. The kernel answers in the same eight registers: `a0`
= 0 and the result in `a1` onward, or `a0` = an error code and `a1`-`a7` = 0. `call` is the one
exception: its lend and reply dispositions are in `a1` and `a2` on every return, errors
included ([IPC](ipc.md#how-a-call-completes)). The kernel preserves every other register of the
thread.

No register ever holds more than 32 bits or one `usize`, so rv32 carries every encoding and
`redoubt-sys` has no width `cfg` in any of them. `USER_AREA_END`, the end of user space
([memory layout](memory-layout.md)), is its one per-width constant.

| Argument kind | Registers | Rule |
| --- | --- | --- |
| address, length, word (`usize`) | 1 | |
| 64-bit value (ids, badges, accounts, time, `random`'s value, a timeout) | 2: low half, then high half | each half fits in 32 bits |
| small value (`u32`: tid, pid, exit code, count) | 1 | fits in 32 bits |
| mapping flags | 1 | only `READ` (1), `WRITE` (2), `EXECUTE` (4); never `WRITE` with `EXECUTE` |
| handle | 1 | an index, 1 to 2^32 - 1 |
| optional handle | 1 | 0 is none (index 0 is never allocated) |
| message id, badge | 2: low half, then high half | never 0 as an argument |
| optional page run (lend, transfer) | 2: address, then pages | (0, 0) is none; exactly one of them 0 is `InvalidArgument` |
| tag (reset kind, `mint` source) | 1 | numbered from 1; 0 is never a valid tag |

Registers a call does not use must be 0. **Timeouts** are relative microseconds; the kernel
adds one to the current time with saturation, so `FOREVER` (2^64 - 1) never wraps and never
expires ([timer](timer.md), I13 (every blocking call returns by its timeout)). A **budget
deadline** is absolute, in microseconds since boot, and `FOREVER` means none.

```svgbob
      ecall: call                              return from call
    +----+-----------------------------+      +----+--------------------------------+
    | a0 | 0x10e, the number of call   |      | a0 | 0, or the error code           |
    +----+-----------------------------+      +----+--------------------------------+
    | a1 | endpoint handle             |      | a1 | lend: 0 none, 1 returned,      |
    | a2 | body record address         |      |    |       2 consumed               |
    | a3 | lend address, 0 for none    | ---> | a2 | reply: 0 absent, 1 present     |
    | a4 | lend pages, 0 for none      |      | a3 | 0                              |
    | a5 | timeout, low 32 bits        |      | .. |                                |
    | a6 | timeout, high 32 bits       |      | a7 | 0                              |
    | a7 | 0, unused                   |      |    |                                |
    +----+-----------------------------+      +----+--------------------------------+
```
*Figure: register use for `call` and its return. The 64-bit timeout takes two registers on both widths.*

On the process's side, `redoubt_sys::syscall` is the `ecall` itself (the crate's only `unsafe`),
and `redoubt_sys::decode_result` reads the result. It refuses any result the kernel could not
have produced: an unknown code, a non-zero register the result does not use, a value too wide
for its field, and for `call` and `reply` a disposition that contradicts the status
([IPC](ipc.md#how-a-call-completes)).

## Call numbers and arguments

Status: built · tested: host:redoubt-sys::every_call_round_trips, host:redoubt-sys::numbers_and_codes_are_dense_from_one, host:redoubt-sys::malformed_calls_are_refused

A call's number is `NUMBER_BASE` (0x100) plus its place in the table, from 1. The numbers are
dense, and a number once given is never reassigned: a call added to the table takes the next
one. "(2)" marks a 64-bit value in two registers. A field named `..._rec` is the address of a
[record](#records).

| `a0` | Call | Arguments, from `a1` | Result, from `a1` | Explained in |
| --- | --- | --- | --- | --- |
| 0x101 | `map_anon` | len, flags | address | [memory](memory.md) |
| 0x102 | `unmap` | address, len | - | [memory](memory.md) |
| 0x103 | `set_flags` | address, len, flags | - | [memory](memory.md) |
| 0x104 | `map_device` | device handle | address, length | [devices](devices.md) |
| 0x105 | `dma_alloc` | device handle, pages | address, physical address (2) | [devices](devices.md) |
| 0x106 | `thread_create` | entry, stack pointer, argument | TID | [processes](processes.md) |
| 0x107 | `thread_exit` | - | does not return | [processes](processes.md) |
| 0x108 | `process_exit` | code | does not return | [processes](processes.md) |
| 0x109 | `process_create` | budget handle, exit endpoint handle | process handle | [processes](processes.md) |
| 0x10a | `process_map` | process handle, source, destination, len, flags | - | [processes](processes.md) |
| 0x10b | `process_start` | process handle, entry, stack pointer, argument, `handles_rec`, count | - | [processes](processes.md) |
| 0x10c | `endpoint_create` | - | endpoint handle, badge 0 | [objects](objects.md) |
| 0x10d | `mint` | source tag (1 message id, 2 handle), source value (2), badge (2), budget handle or 0 | handle | [objects](objects.md#mint) |
| 0x10e | `call` | endpoint handle, `body_rec`, lend address, lend pages, timeout (2) | `a0` status; lend and reply dispositions | [IPC](ipc.md#the-calls) |
| 0x10f | `send` | endpoint handle, `body_rec`, transfer address, transfer pages, timeout (2) | - | [IPC](ipc.md#the-calls) |
| 0x110 | `receive` | endpoint or IRQ handle or 0, timeout (2), `max_transfer` (pages), `received_rec` | a [receive record](#the-receive-record) | [IPC](ipc.md#what-receive-returns) |
| 0x111 | `reply` | message id (2), `body_rec` | delivered (1) or discarded (0), installed-handle mask | [IPC](ipc.md#how-a-call-completes) |
| 0x112 | `serve` | message id (2) | - | [IPC](ipc.md#the-calls) |
| 0x113 | `handle_close` | handle | - | [objects](objects.md) |
| 0x114 | `budget_create` | parent budget handle, `spec_rec` | budget handle | [budgets](budgets.md) |
| 0x115 | `budget_destroy` | budget handle | -, or does not return if the caller's own budget is in the subtree | [budgets](budgets.md) |
| 0x116 | `budget_usage` | budget handle, `usage_rec` | a usage record | [budgets](budgets.md) |
| 0x117 | `time_now` | - | microseconds since boot (2) | [timer](timer.md) |
| 0x118 | `random` | - | one value from the kernel's CSPRNG (2) | [boot](boot.md) |
| 0x119 | `system_reset` | Reset device handle, kind (1 power off, 2 reboot) | does not return on success | [devices](devices.md) |
| 0x11a | `map_fixed` | address, len, flags | - | [memory](memory.md) |

`thread_create`'s and `process_start`'s argument reaches the new thread unchanged in its first
argument register; the kernel does not check it, nor the entry or stack pointer. A `usize`
result (an address, a length) is one register; a handle or TID is one register holding at most
32 bits.

## Records

Status: built · partly tested: a record at a device mapping is attacked by a case only as a `call` body and as `budget_create` and `budget_usage` records · tested: host:redoubt-sys::records_round_trip, host:redoubt-sys::malformed_records_are_refused, host:redoubt-sys::random_records, fuzz:redoubt-sys/decode, bench:budget-syscall-attack, bench:ipc-outcomes, bench:process-attack

### Layouts

A **record** is what does not fit in seven registers: a fixed-length array of 64-bit
little-endian **slots** in the caller's memory, passed by address, at an 8-byte-aligned
address. Slot `n` is at byte `8n`. The layout is the same on both widths. Unused slots must be
0. A `usize` field must fit the target's `usize`: on rv32 a slot above 2^32 - 1 there is
`InvalidArgument`. A list (labels, handles) is a count, then its capacity's slots, the unused
ones 0.

| Record | Slots | Layout | Used by |
| --- | --- | --- | --- |
| body | 9 (`BODY_SLOTS`) | words 0-3; handle count (at most `MAX_MSG_HANDLES` (4)); handles 0-3 | `call` (request in, reply out), `send`, `reply` |
| receive record | 24 (`RECEIVED_SLOTS`) | [below](#the-receive-record) | `receive` (out) |
| budget spec | 14 (`BUDGET_SPEC_SLOTS`) | pages; processes; weight; label count (at most `MAX_LABELS` (8)); labels 0-7; account; deadline | `budget_create` (in) |
| usage | 6 (`USAGE_SLOTS`) | page limit; pages used; process limit; processes used; weight limit; weight carved to children | `budget_usage` (out) |
| handle list | the call's count, at most `MAX_START_HANDLES` (64) | one handle per slot | `process_start` (in) |

In a body going in, every handle within the count is a handle; a slot of 0 there is
`BadHandle`. In a body coming out (the reply `call` writes back over its request, and a
message's body), a handle slot within the count may be 0 and keeps its place: a handle revoked
while its message was queued ([R10 (destruction)](budgets.md#r10-destruction)), or a reply
handle the caller could not take ([R4 (delivery)](ipc.md#r4-delivery)).

```svgbob
  body: call, send, reply             receive record: what receive writes
  slot                                slot
      +------------------------+          +------------------------+
    0 | word 0                 |        0 | kind                   |
    1 | word 1                 |        1 | msg_id                 |
    2 | word 2                 |        2 | badge                  |
    3 | word 3                 |        3 | account                |
      +------------------------+          +------------------------+
    4 | handle count, 0 to 4   |        4 | label count, 0 to 8    |
      +------------------------+     5-12 | labels 0-7             |
    5 | handle 0               |          +------------------------+
    6 | handle 1               |    13-16 | words 0-3              |
    7 | handle 2               |          +------------------------+
    8 | handle 3               |       17 | handle count, 0 to 4   |
      +------------------------+    18-21 | handles 0-3            |
                                          +------------------------+
  slot n is at byte 8n;                22 | buffer address         |
  every unused slot is 0               23 | buffer pages           |
                                          +------------------------+
```
*Figure: record slot layout of a body and of the receive record, the same on rv32 and rv64.*

### The record check

Before a call uses a record, the kernel checks it whole: the address is 8-byte aligned, and
every slot lies in user space, in a page that is mapped, backed, readable (and writable, for a
record the kernel writes), not either side of a lend, RAM, and credited to the caller in the
kernel's frame ownership table. Anything else is `InvalidArgument`. A page the caller was lent
is its lender's, so it is not a record; nor is a device mapping, which is not RAM. `call`'s body
is checked readable and writable before anything is delivered, because the reply comes back
into it ([R13 (one outcome per call)](ipc.md#r13-one-outcome-per-call)).

**The check never allocates.** A page the caller reserved but never touched is
`InvalidArgument`, not a page the kernel backs and charges in the middle of decoding. The Rust
runtime's records are written stack arrays (`libs/rt/src/sys.rs`, `Record`, 8-byte aligned),
so every record it passes is backed.

A record may overlap the pages a call acts on. The kernel copies a record in before it changes
those pages, and writes results only to memory still the caller's: a `call` body may lie
inside its own lend, and the lend is mapped back before the reply is written into it.

The kernel holds its memory lock from the check through the copy, so no other thread of the
process can unmap or remap a record between the two. A record the kernel writes after a call
has blocked (`receive`'s, or `call`'s reply) is checked again at that point
([IPC](ipc.md#what-receive-returns)).

## The receive record

Status: built · tested: host:redoubt-sys::received_layout, host:redoubt-sys::malformed_records_are_refused, host:redoubt-sys::random_records, fuzz:redoubt-sys/decode

`receive` writes one layout for every result: `(kind, msg_id, badge, account, labels, words,
handles, buffer, pages)`, in the slots of the figure above. Slot 0 is the kind; a field the kind
does not use is 0. What each kind means is on the [IPC page](ipc.md#what-receive-returns).

| Kind | Slot 0 | Slots it fills |
| --- | --- | --- |
| `call` | 1 | all; the buffer is the lend, at the address the kernel chose |
| `send` | 2 | all; the buffer is the transfer |
| `interrupt` | 3 | none |
| `exit` | 4 | 3 (blamed account), 4-12 (blamed labels), 13-15 (pid, cause, code) |
| `abandoned` | 5 | 1 (the abandoned call's message id) |

An exit's cause is 1 exited, 2 faulted, 3 killed ([processes](processes.md#exit-notices)).
`Timeout` is an error, not a record. A record carries no handle kinds: a handle's kind is
checked when it is used, and the wrong kind is `WrongObject`.

`Received::decode` reads the kind first, and refuses any non-zero slot outside the fields that
kind fills, a list's count included, before it reads a field. So a process decoding a record
never mistakes a notice for a message.

## Errors and their codes

Status: built · tested: host:redoubt-sys::numbers_and_codes_are_dense_from_one, host:redoubt-sys::every_result_and_error_round_trips, host:redoubt-sys::error_rows, bench:budget-syscall-attack

One error enum serves every call. Its code travels in `a0`; 0 there is success, so codes start
at 1.

| Code | Error | Means |
| --- | --- | --- |
| 1 | `BadHandle` | a handle that is 0 where one is required, wider than 32 bits, or not in the caller's table |
| 2 | `WrongObject` | the handle names the wrong kind of object for this call |
| 3 | `InvalidArgument` | a malformed or out-of-range value: an unknown number, tag or flag bit, a bad range, a bad record, a message id that is not an open call |
| 4 | `OutOfMemory` | the paying budget's page limit, or no room in the address space, no free frame, no free DMA run |
| 5 | `OutOfProcesses` | a budget's process limit, or no free PID |
| 6 | `TooManyThreads` | the process has `MAX_THREADS` (31) threads |
| 7 | `NotPermitted` | the right object, without the standing: a badged handle where badge 0 is needed, a started process, a `mint` budget outside the default stamp, `dma_alloc` on a device without DMA |
| 8 | `ClassDenied` | labels added by a caller whose budget is not of class `system` |
| 9 | `LabelDenied` | a flow [R1 (flow)](ipc.md#r1-flow) forbids |
| 10 | `Busy` | the sender's group already has `WAIT_CAP` (16) messages queued on the endpoint ([R2 (fair waiting)](ipc.md#r2-fair-waiting)) |
| 11 | `Refused` | the receiver's budget cannot pay for the message (R4) |
| 12 | `TooLarge` | a count over its fixed limit: body handles, labels, the start list, a lend over `MAX_LEND_PAGES` (16), budget depth, `MAX_HANDLES` (4096) |
| 13 | `Timeout` | a blocking call's timeout passed |
| 14 | `Dead` | the object the call needs is gone: its endpoint was destroyed, its server died, or the handle it came through was revoked |

Each call can return only the errors of its row in the table below, plus `InvalidArgument` for
a malformed encoding; `Number::can_return` in `redoubt-sys` is that set. The kernel checks every
error it returns against it in checked builds, so a case built with debug assertions fails on
an error outside its row.

Inside the kernel, the page tables and the frame ownership table report failures in their own
enum (`PageError` in `kernel/src/mem.rs`), which never reaches a process. Each call maps it to
the error its row names, explicitly, at the call's boundary; there is no automatic conversion.
So a lent page and an exhausted budget, which the page layer tells apart, cannot collapse into
one wrong code on their way out.

## Errors and the order of checks

Status: built · partly tested: for most rows the order after decoding is argued from the code rather than pinned by a case; the kernel and the model are compared by reading, not by replaying traces, and differ in two rows (Residual risks) · tested: bench:budget-syscall-attack, bench:syscall-attack, bench:ipc-outcomes, bench:process-attack, host:redoubt-sys::malformed_calls_are_refused, host:redoubt-model::every_call_and_error_is_reached, host:redoubt-model::process_map_destination_validation_precedes_started_state, fuzz:redoubt-sys/decode

A call with several faults returns the first one found, in a fixed order, so that the kernel,
the model and a replayed trace agree exactly. Checks go in stages, and within a stage by
argument position:

1. **Decoding.** The registers in order, `a1` first; then each record the call passes: its
   alignment, then the [record check](#the-record-check) slot by slot, then its slots in order.
   A required handle that is 0 or wider than 32 bits is `BadHandle`; a list longer than its
   limit is `TooLarge`; anything else malformed is `InvalidArgument` (an unknown call number,
   tag or flag bit, W+X flags, a value too wide for its field, a non-zero unused register or
   slot, a record that fails the check, a message id or badge of 0). These three are a
   classification, not a sequence: the first malformed value in register, then slot, order
   wins. Each register is checked whole when it is reached; unused registers come after the
   last argument. This stage never allocates, so it never returns `OutOfMemory`.
2. **Arguments**, one by one: a handle exists (`BadHandle`) and names the right kind of object
   (`WrongObject`); a size is within its fixed limit (`TooLarge`); a range is page-aligned,
   non-empty, inside user space and mapped as the call needs (`InvalidArgument`).
3. **Permission**: `NotPermitted`, `ClassDenied`, `LabelDenied`.
4. **Resources**: `OutOfMemory`, `OutOfProcesses`, `TooManyThreads`, `Busy`. A call that adds a
   handle to its caller's table (`endpoint_create`, `mint`, `process_create`, `budget_create`)
   checks the table last: `OutOfMemory` when it needs a new table page the caller's budget
   cannot pay for, `TooLarge` when it already holds `MAX_HANDLES`.
5. **At delivery** (`call`, `send`, `receive`, whether they blocked or not): `Refused`,
   `Timeout`, `Dead`; for `call` also `InvalidArgument` (the reply record can no longer be
   written) and `OutOfMemory` (reply handles that do not fit); for `receive` also
   `InvalidArgument` (its record can no longer be written). Decoding already refused records
   that were bad from the start; these cover memory that changed while the call waited.

Handle kinds are checked by use: no call reports a handle's kind, and the wrong kind is
`WrongObject` at the first call that needs another. Two checks of stage 1 are made again by the
code that acts on them, so neither rests on one check: W+X flags (by the mapping code,
[R11 (memory)](memory.md#r11-memory)) and a `mint` badge of 0 (by `mint`, I3 (minted badges are non-zero and narrow)).

Per call, in the order checked. "Then" lists stages 2 to 5; "each handle" is every handle of a
list, in order.

| Call | Decoding | Then |
| --- | --- | --- |
| `map_anon` | flags: `InvalidArgument` | `InvalidArgument` (len 0 or not page-aligned), `InvalidArgument` (flags 0, or W without R), `OutOfMemory` (no room in the placement area; then each page and its page tables) |
| `unmap` | - | `InvalidArgument` (range: len 0, not page-aligned, wraps, or past `USER_AREA_END`), `InvalidArgument` (a page that is not a live mapping of the caller's, is either side of a lend, or is RAM not credited to the caller; the whole range before any page goes) |
| `set_flags` | flags: `InvalidArgument` | `InvalidArgument` (range), `InvalidArgument` (flags 0), `InvalidArgument` (a page not the caller's own mapping, as for `unmap`), `InvalidArgument` (W without R) |
| `map_device` | device: `BadHandle` | `BadHandle`, `WrongObject` (not an MMIO device), `OutOfMemory` (no room in the placement area, or the page tables) |
| `dma_alloc` | device: `BadHandle` | `BadHandle`, `WrongObject` (not an MMIO device), `InvalidArgument` (pages 0), `NotPermitted` (the device has no DMA flag), `OutOfMemory` (all 32 of the device's runs in use; then the pages; then no contiguous frames; then the placement area and page tables) |
| `thread_create` | - | `TooManyThreads`, `OutOfMemory` (the thread's page) |
| `thread_exit` | - | - |
| `process_exit` | code wider than 32 bits: `InvalidArgument` | - |
| `process_create` | each handle: `BadHandle` | `BadHandle`, `WrongObject` (budget), `BadHandle`, `WrongObject` (exit endpoint), `InvalidArgument` (the budget's free weight is 0), `NotPermitted` (the exit endpoint's badge is not 0), `OutOfProcesses` (no free PID; then the budget's process limit), `OutOfMemory` (the budget: page tables; then the caller: the process object), handle table |
| `process_map` | process: `BadHandle`; flags: `InvalidArgument` | `BadHandle`, `WrongObject`, `InvalidArgument` (source or destination range), `InvalidArgument` (a source page unmapped, lent, not the caller's own RAM, or a `dma_alloc` page; untouched source pages are backed first), `InvalidArgument` (flags 0, or W without R), `NotPermitted` (the process has ended), `InvalidArgument` (a destination page occupied), `NotPermitted` (started), `OutOfMemory` (the child's budget: page tables, and the pages unless the budget is the caller's) |
| `process_start` | process: `BadHandle`; entry, stack pointer, argument: not checked; count over `MAX_START_HANDLES`: `TooLarge`; handle list: the record, then each slot `BadHandle` | `BadHandle`, `WrongObject`, `BadHandle` (each handle), `NotPermitted` (started or ended), `TooLarge` (the child's table past `MAX_HANDLES`), `OutOfMemory` (the child's budget: its first thread and table pages) |
| `endpoint_create` | - | `OutOfMemory` (the endpoint's page), handle table |
| `mint` | source: each half too wide `InvalidArgument` (both halves are read before the tag), then an unknown tag `InvalidArgument`, a message id of 0 `InvalidArgument`, or a handle of 0 or wider than 32 bits `BadHandle`; badge 0: `InvalidArgument`; budget: `BadHandle` | source: a message id that is not an open call of the caller's thread (a `send`'s id included) `InvalidArgument`, its endpoint or stamp gone `Dead`; or a handle `BadHandle`, `WrongObject` (not an endpoint); budget: `BadHandle`, `WrongObject`; `NotPermitted` (a handle source's badge is not 0), `NotPermitted` (the budget is not the default stamp or below it), handle table |
| `call` | endpoint: `BadHandle`; lend: exactly one of address and pages 0 `InvalidArgument`; body: the record (read and written), handle count `TooLarge`, each handle slot `BadHandle` | `BadHandle`, `WrongObject` (not an endpoint), `BadHandle` (each handle), `TooLarge` (lend over `MAX_LEND_PAGES`), `InvalidArgument` (lend not page-aligned, wraps, unmapped, lent, not the caller's own writable RAM, or a `dma_alloc` page; untouched lend pages are backed first), `LabelDenied` (R1), `Busy` (R2); at delivery: `Refused` (R4), `Timeout`, `Dead`, `InvalidArgument` (the reply record cannot be written; reply handles rolled back, reply absent), `OutOfMemory` (reply handles that do not fit; the reply arrives without them, R4). Dispositions on every return |
| `send` | as `call`, with the transfer for the lend and the body read only | as `call`, with no fixed limit on a transfer; at delivery only `Refused` (R4, a transfer over the receiver's `max_transfer` included), `Timeout`, `Dead` |
| `receive` | handle (0 is none): `BadHandle`; the record (written) | none given: sleep until `Timeout`; `BadHandle`, `WrongObject` (not an endpoint or an IRQ device), `NotPermitted` (badge not 0, I4 (only badge-0 handles receive)); at delivery: `Timeout`, `Dead` (endpoint destroyed), `InvalidArgument` (the record can no longer be written) |
| `reply` | message id 0: `InvalidArgument`; body: the record (read), handle count `TooLarge`, each handle slot `BadHandle` | `InvalidArgument` (not an open call of the caller's thread, a `send`'s id included), `BadHandle` (each handle); nothing after these fails |
| `serve` | message id 0: `InvalidArgument` | `InvalidArgument` (not an open call of the caller's thread) |
| `handle_close` | handle: `BadHandle` | `BadHandle` |
| `budget_create` | parent: `BadHandle`; spec: the record (read), then its slots: processes or weight wider than 32 bits `InvalidArgument`, label count over `MAX_LABELS` `TooLarge`, a label slot past the count non-zero `InvalidArgument` | `BadHandle`, `WrongObject`, `TooLarge` (the child would be at depth `MAX_DEPTH` (8)), `LabelDenied` (the labels, sorted and deduplicated, are not a superset of the parent's), `ClassDenied` (labels added by a caller not of class `system`), `OutOfMemory` (pages plus the budget's own page over the parent's free pages), `OutOfProcesses` (over the parent's free processes), `InvalidArgument` (weight over the parent's free weight, or all of it from a parent that holds a process; [R7 (carving)](budgets.md#r7-carving)), `OutOfMemory` (no free frame for the budget object), handle table |
| `budget_destroy` | budget: `BadHandle` | `BadHandle`, `WrongObject` |
| `budget_usage` | budget: `BadHandle`; counters: the record (written) | `BadHandle`, `WrongObject`, `LabelDenied` (R1: a caller not of class `system` whose labels do not include the target's) |
| `time_now` | - | - |
| `random` | - | - |
| `system_reset` | device: `BadHandle`; kind: `InvalidArgument` | `BadHandle`, `WrongObject` (not the Reset device) |
| `map_fixed` | flags: `InvalidArgument` | `InvalidArgument` (range: address or len not page-aligned, len 0, wraps, or past `USER_AREA_END`), `InvalidArgument` (overlaps any mapping or reservation of the caller's, lent pages included), `InvalidArgument` (flags 0, or W without R), `OutOfMemory` (the pages alone), `OutOfMemory` (the pages and the page tables they need); nothing mapped or charged on failure |

Every call can also fail decoding in the general ways of stage 1 (a non-zero unused register, a
value too wide). "Handle table" is stage 4's last check. Three rows depart from the stages, and
say so: a weight over the parent's free weight is `InvalidArgument`, because no error names
weight; `mint` from a message whose endpoint or stamp is gone is `Dead` at the argument stage;
and `receive` clears the thread's current call before anything else, whatever it returns
([R21 (crash blame)](processes.md#r21-crash-blame)).

`map_fixed`'s order keeps its cost bounded: the overlap walk skips page-table subtrees that are
absent, and the charge for the pages alone comes before the walk that counts page tables, so a
huge length the budget could never pay for is refused by arithmetic
([R22 (range cost)](memory.md#r22-range-cost)).

## Unknown call numbers

Status: built · tested: bench:legacy-gone, bench:budget-syscall-attack, host:redoubt-sys::malformed_calls_are_refused, host:redoubt-sys::numbers_and_codes_are_dense_from_one

Every value of `a0` outside 0x101-0x11a is an unknown number: `InvalidArgument` in `a0` and 0 in
`a1`-`a7`. That includes 0, every number up to and including `NUMBER_BASE`, the first number
past the table, and on rv64 a value with bit 32 or bit 63 set: the whole register is compared,
never a truncated part of it. An unknown number carries no `call` dispositions, and nothing
happens.

The trap handler sends every user-mode `ecall` to one decoder, `redoubt::handle`
(`kernel/src/redoubt.rs`); there is no second dispatcher. The kernel's scheduler enters its own
trap handler with a private tag in `a0` (`SWITCH_TAG`, `kernel/src/sched.rs`), by setting up the
state of a supervisor-mode `ecall` and jumping to the trap vector
(`kernel/src/arch/riscv/syscall.rs`); from user mode that value is an unknown number like any
other.

`bench:legacy-gone` sweeps every number from 0 to `NUMBER_BASE` and the first past the table,
with plausible arguments in `a1`-`a7`, and on rv64 each small number again with bit 32 and with
bit 63 set. It checks every result is `InvalidArgument` with zeros, and that nothing such a call
might ask for happened: no page appears at a fixed address, no callback runs when the console's
interrupt fires, and a child jumping to a fixed kernel return address faults.

## Residual risks

- **The kernel and the model differ in two rows.** `budget_create`: the kernel decodes the spec
  record in slot order, so a spec with a process count wider than 32 bits and more than
  `MAX_LABELS` labels is `InvalidArgument`; the model checks the label count first and says
  `TooLarge`. `process_create`: the kernel checks for a free PID before the budget's process
  limit and before any charge; the model checks it last, so with no free PID and too little
  memory the kernel says `OutOfProcesses` and the model `OutOfMemory`. The table above is the
  kernel's. The model also clears a `receive`'s current call only after the record check,
  checks only a record's first page, and does not model the 32-run limit of `dma_alloc` or the
  size of the placement area. No trace has been replayed on the kernel to find more such
  differences ([model](model.md)). Follow-up: [todo](../todo/abi-model-disagreements.md).
- **The order after decoding is mostly argued from the code.** Cases pin the order of
  decoding, and of the first checks after it for `budget_create`, `budget_usage`, `call`,
  `receive`, `serve` and `process_start`; most later positions in most rows are read from the
  kernel, not attacked. A wrong order is a replay mismatch, not a way past a check: every
  check in a row is still made.
- **A record's frame is checked for RAM by its physical address.** The record check confirms
  every slot is RAM credited to the caller, so a device mapping is refused as a record. A case
  attacks this for a `call` body and for `budget_create` and `budget_usage` records at a device
  mapping, not for `send`, `reply`, `receive` or `process_start` records, which share the same
  check. Follow-up: [todo](../todo/mmio-record-frames.md).
- **An error may leave a page backed.** Stage 1 never allocates, but stage 2 backs untouched
  pages of a lend, a transfer or a `process_map` source before later checks run; if the call
  then fails (`LabelDenied`, `Busy`), those pages stay backed and charged to the caller, as if it
  had touched them ([memory](memory.md)).

## Why

- **One layout on both widths.** A 64-bit value always takes two registers, so no encoding has a
  width `cfg` and one set of tests covers both targets. It costs rv64 an extra register for each 64-bit value.
- **One encoding per value.** Unused registers and slots must be 0, and (0, 0) is the only
  "none", so a malformed value cannot hide where the kernel does not look, and whatever decodes
  re-encodes to exactly its input (the fuzz target checks it). A trace of registers then means
  one thing.
- **Records are slots, not structs.** An array of `u64` slots has one layout on both widths and
  no padding, and the kernel copies it in once and decodes the copy, so another thread cannot
  change a value between its check and its use.
- **Decoding never allocates.** An untouched record page is the caller's error, so decoding can
  never fail for want of memory, never charges a budget halfway through, and needs no
  `OutOfMemory` in its stage.
- **A positional order.** Checking in register order, stage by stage, makes the first error a
  function of the arguments alone, which is what lets the kernel, the model and a replayed trace
  agree exactly. `BadHandle`, `TooLarge` and `InvalidArgument` in decoding sort errors by kind;
  the position decides which one comes first.
- **Kinds checked by use.** No call reports a handle's kind and no record carries one: a query
  call would be more surface, and a process learns the kind the first time it uses the handle
  (`WrongObject`).
- **Numbers from 0x101.** Small numbers are never calls, so a stray or guessed small `a0`, and a
  number from any other interface, lands on "unknown". Numbers are appended, never reassigned,
  so a compiled program keeps working.
- **`call`'s dispositions on every return.** A caller must know whether it still owns its lend
  even when an argument was bad, so the kernel sets them before decoding: `none` for a raw lend
  of (0, 0), `returned` otherwise ([IPC](ipc.md#how-a-call-completes)).
- **A private error enum inside the kernel.** Mapping the page layer's errors to the ABI's by
  hand at each call keeps each row's errors a reviewed choice; an automatic conversion would let
  the wrong code leak out unnoticed.
