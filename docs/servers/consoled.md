# consoled

`consoled` is the ns16550 UART driver. It serves the physical console as `/dev/cons`, one file
over 9P: a write goes out of the UART, and a read returns what was typed, or, when nothing has
been, **parks** until a key arrives rather than blocking or answering nothing. The physical
console carries no labels, so any caller may read it and only an unlabelled one may write it.

## Purpose

The serial line is the box's physical console: where the first owner is enrolled, where recovery
happens, and where a panic is reported. Its driver must never stop serving because of one client or
one broken device: a read with no input must wait without holding the server, and a device that
claims data for ever must cost a bounded amount of work. Session consoles are served per SSH
channel by [`sshd`](sshd.md), not here.

## Interface

### `/dev/cons`

Status: built · partly tested: attacked with a fake UART against the runtime's fake kernel; in a boot the console is held by the bench's interim log server, so `consoled` is only built · tested: bench:r4-host-tests, bench:consoled-build, host:redoubt-consoled::typing_on_the_uart_reaches_a_ninep_reader, host:redoubt-consoled::a_read_with_no_input_waits_and_is_freed_when_its_caller_gives_up, host:redoubt-consoled::writes_go_out_of_the_uart_in_order, host:redoubt-consoled::a_flood_of_input_keeps_what_was_typed_first, host:redoubt-consoled::the_console_refuses_what_it_is_not, host:redoubt-consoled::the_conformance_vectors_run_against_consoled

`/dev/cons` is served over the [9P server skeleton](serving.md#the-9p-server-skeleton) as one file
with nothing below it.

- **A write** sends its bytes out of the UART, in order.
- **A read** returns the input held, from the start of the queue; the offset is ignored, since a
  console is a stream. With no input it **parks** its call with no deadline, because it waits on a
  person, and is served again, unchanged, when a key arrives; a caller that gives up abandons it
  and it is freed at once ([parked calls](serving.md#parked-calls)).
- **One line, one queue.** Input goes to whichever waiting read has waited longest. At most
  `MAX_INPUT` (1024) bytes are held; a byte beyond that is dropped and counted, so a flood keeps
  what was typed first.
- **`stat`** reports length 0: a console has no size.
- **What it refuses:** opening with `OEXEC` or `OTRUNC`, walking, creating and removing, and a
  write from a labelled caller, through the label check against the console's empty label set
  ([R69 (no write down onto the console)](#r69-no-write-down-onto-the-console)).
- **No typed protocol of its own.** `consoled` serves 9P and `ninep_common` only; the `consol`
  opcodes are answered `malformed` ([below](#the-consol-protocol)).
- **Admission:** at most 2 parked reads, 4 fids and 4 connections per (account, label set), across
  at most 4 of those (`LIMITS`), sized to fit its 1 MiB budget.

### Two threads and the UART

Status: built · tested: host:redoubt-consoled::a_device_stuck_on_data_ready_does_not_hang_the_server, host:redoubt-consoled::typing_on_the_uart_reaches_a_ninep_reader, host:redoubt-consoled::a_console_with_no_device_does_not_start

- **The serving thread** owns the UART and the 9P skeleton, answers writes, answers reads from the
  input it holds, and parks reads when it holds none, so every other client is still served.
- **The interrupt thread** only receives on the interrupt handle and sends one word to the serving
  thread's endpoint; it touches no register. The UART's registers are not `Sync`, so the split is
  the type system's, not a convention, and neither thread needs a lock. The kernel masks the
  interrupt when it fires and the next receive unmasks it ([R5 (interrupts)](../kernel/devices.md#r5-interrupts)).
- **The FIFO is drained whole** on each wake-up and before a read parks, so two bytes between two
  interrupts are both read and no byte is left behind with a reader waiting. A drain reads at most
  the FIFO's 16 bytes, so a device that says "data ready" for ever costs a bounded number of register
  reads per call instead of hanging the server
  ([R70 (a stuck UART never hangs the console)](#r70-a-stuck-uart-never-hangs-the-console)).
- **The registers** are mapped from the device handle by the runtime and reached through its
  bounds-checked accessors, every access volatile; the crate forbids `unsafe`.

### The `consol` protocol

Status: planned · M2 (usable shell)

Every server of a `/dev/cons` (`consoled`, and `sshd` for each channel) also serves `consol` on
the same endpoint:

- **`size`** answers the console's columns and rows at once. `consoled` takes its size as an argument
  (`cols,rows`, default 80 by 24); `sshd` answers from the channel's pty request.
- **`resize`** answers the size when it next changes: it parks like a read, with no deadline, and is
  freed when its caller gives up ([parking a typed call](serving.md#parking-a-typed-call)). A client
  learns the size of its own console only; another channel's waiter stays parked and learns nothing.

The table: [libs/wire/tables/consol.md](../../libs/wire/tables/consol.md).

{{#include ../../libs/wire/tables/consol.md}}

**Open:** how a typed server says "wait" (open on [the serving library](serving.md#parking-a-typed-call)).

### Started by `init`

Status: planned · M1 (separation and containment)

`init` starts `consoled` first, with the UART's MMIO region and interrupt as named handles and its
endpoint, placed from the boot manifest's `devices` list ([init](init.md#the-boot-manifest)).
`consoled` parses no device tree; without both handles it does not start. Only one process ever
holds the UART: two holders would both reach the registers, and two readers of one FIFO would each
take half the line.

**Open:** the handles' names: `consoled` takes `uart` and `uart:irq`, while `blkd` takes `disk` and
`disk-irq`; one rule for both is open on [init](init.md#the-boot-manifest).

## Authority

Status: built · tested: host:redoubt-consoled::the_console_refuses_what_it_is_not

- `consoled` holds the UART's registers and interrupt, its endpoint, and the connections it minted. It
  makes no calls to other servers.
- Any caller with a connection may read the console; only an unlabelled caller may write it.

## Security properties

### R69 (no write down onto the console)

Status: built · tested: host:redoubt-consoled::the_console_refuses_what_it_is_not

The physical console carries no labels, so a caller with any label cannot write to it: no labelled
data reaches a screen someone else may be looking at.

### R70 (a stuck UART never hangs the console)

Status: built · tested: host:redoubt-consoled::a_device_stuck_on_data_ready_does_not_hang_the_server, host:redoubt-consoled::a_flood_of_input_keeps_what_was_typed_first

Whatever the UART reports, `consoled` does a bounded amount of work per call and keeps serving: a
drain reads at most the FIFO's depth, and held input is bounded, dropping new bytes rather than
growing.

## Failure and restart

Status: built · tested: host:redoubt-consoled::a_console_with_no_device_does_not_start, host:redoubt-consoled::a_read_with_no_input_waits_and_is_freed_when_its_caller_gives_up

- **No UART or interrupt handle:** `consoled` exits with a code before serving.
- **A reader gives up:** its parked read is answered and freed at once.
- **`consoled` restarts:** held input is lost, parked reads get `Dead`, and clients ask for fresh
  connections.

## Residual risks

- **One keyboard, one queue.** A client reading `/dev/cons` takes input another was waiting for; a
  session that must not share a console gets its own from `sshd`.
- **A parked read has no deadline.** It holds one of its caller's open calls and one of `consoled`'s
  admission slots until a key arrives or its caller gives up.
- **Anyone with a connection reads the console.** What is typed on the physical console is visible to
  every holder of a `consoled` connection.
- **`consoled` does not run in a boot.** The bench's console is an interim log server holding the
  same UART; the two must never run together.

## Why

- **Park, rather than block or answer empty.** A blocked server serves nobody else; an empty
  answer looks like a closed console; parking costs one slot and nothing more.
- **The interrupt thread touches nothing.** Keeping the device on one thread removes every lock and
  every shared-state bug between the two.
- **Drain at most the FIFO.** A 16550 can hold no more; a device claiming otherwise is broken, and
  bounding the loop turns it into dropped input instead of a hung server.
