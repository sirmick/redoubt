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

<details><summary>Status: built · partly tested: attacked with a fake UART against the runtime's fake kernel, and in a boot only written to; no test writes as a labelled caller · tested (12)</summary>

- bench:r4-host-tests
- bench:consoled-build
- bench:init-boot
- bench:init-servers
- host:redoubt-consoled::a_refused_typed_request_leaves_no_handle_behind
- host:redoubt-consoled::typing_on_the_uart_reaches_a_ninep_reader
- host:redoubt-consoled::a_read_with_no_input_waits_and_is_freed_when_its_caller_gives_up
- host:redoubt-consoled::a_multiplexed_read_waits_for_input
- host:redoubt-consoled::writes_go_out_of_the_uart_in_order
- host:redoubt-consoled::a_flood_of_input_keeps_what_was_typed_first
- host:redoubt-consoled::the_console_refuses_what_it_is_not
- host:redoubt-consoled::the_conformance_vectors_run_against_consoled

</details>

`/dev/cons` is served over the [9P server skeleton](serving.md#the-9p-server-skeleton) as one file
with nothing below it.

- **A write** sends its bytes out of the UART, in order, each line saying who wrote it
  ([started by `init`](#started-by-init)).
- **A read** returns the input held, from the start of the queue; the offset is ignored, since a
  console is a stream. With no input it **parks** its call with no deadline, because it waits on a
  person, and is served again, unchanged, when a key arrives; a caller that gives up abandons it
  and it is freed at once ([parked calls](serving.md#parked-calls)). A multiplexed connection's
  read waits the same way, as a request in the skeleton, and is answered into its completion call
  when a key arrives ([multiplexed connections](serving.md#multiplexed-connections)).
- **One line, one queue.** Input goes to whichever waiting read has waited longest. At most
  `MAX_INPUT` (1024) bytes are held; a byte beyond that is dropped and counted, so a flood keeps
  what was typed first.
- **`stat`** reports length 0: a console has no size.
- **What it refuses:** opening with `OEXEC` or `OTRUNC`, walking, creating and removing, and a
  write from a labelled caller, through the label check against the console's empty label set
  ([R69 (no write down onto the console)](#r69-no-write-down-onto-the-console)).
- **Beside 9P and `ninep_common`, `consol`'s `size` and `resize`** ([below](#the-consol-protocol)).
  Any other typed opcode, `ended` sent as a call among them, is answered `malformed`, and the
  handles it carried are closed.
- **Admission:** at most 2 parked calls (waiting reads and `resize` calls together),
  `MAX_THREADS` consoles' fids, `MAX_THREADS` connections,
  and 80 multiplexed requests and 2 pages they brought
  ([serving](serving.md#multiplexed-connections)) per (account, label set), across at most
  `buckets=N` of those, sized to fit its 2 MiB budget; a block with no `buckets=N`, or one the
  budget cannot hold, and `consoled` does not start ([init](init.md#the-boot-manifest)). The
  connections, and their fids, scale with
  `MAX_THREADS` because `init` mints every server's console through its one root badge, and
  starts at most `MAX_THREADS - 1` servers ([started by `init`](#started-by-init)): each console,
  and `init`'s own, holds 2 fids, the root it attaches and the `cons` file it opens.

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

<details><summary>Status: built · partly tested: on the host only · tested (5)</summary>

- host:redoubt-consoled::consol_size_is_the_argument_and_a_resize_waits_until_its_caller_gives_up
- host:redoubt-consoled::a_refused_typed_request_leaves_no_handle_behind
- host:redoubt-consoled::a_console_with_no_device_does_not_start
- host:redoubt-sshd::consol_size_is_the_pty_s_and_a_resize_is_due_when_it_changes
- host:redoubt-client::a_session_writes_and_reads_the_console

</details>

Every server of a `/dev/cons` (`consoled`, and `sshd` for each channel) also serves `consol` on
the same endpoint, through the runtime's `server::consol`:

- **`size`** answers the console's columns and rows at once. `consoled` takes its size from its
  arguments, `size=COLS,ROWS`, each from 1 to 1,024; a malformed size, or two, and it does not
  start. A UART cannot know the size of the terminal at its far end, so when the manifest names
  none `consoled` does not guess: it refuses `size` and `resize` (`malformed`), and its clients'
  console is of unknown size, as the image's is. `sshd` answers from the channel's pty request
  ([sshd](sshd.md#sessions-over-ssh)).
- **`resize`** answers the size when it next changes: it parks like a read, with no deadline, and is
  freed when its caller gives up ([parking a typed call](serving.md#parking-a-typed-call)). A UART
  has no window, so on `consoled` a `resize` waits until its caller gives up. A client learns the
  size of its own console only; another channel's waiter stays parked and learns nothing.
- **What a waiter costs.** A waiting `resize` is a parked call of its caller's, in its bucket and
  share, beside its waiting reads and, on a multiplexed connection, its session, which holds one
  of the share too ([multiplexed connections](serving.md#multiplexed-connections)). On
  `consoled` a lone connection's share is one of the bucket's 2: a connection with a `resize`
  waiting has no room for a waiting read, and one with a multiplexed session, as the shell's VM
  has, has no room for a `resize`, so the VM's is refused. On `sshd` the share is two, the
  session's and one waiter's, and the VM's reads and writes wait inside its session, so its
  `resize` starves nothing of its own. One more is refused at once (`malformed`), so however many
  waiters a client asks for, what they cost the server is the bucket's fixed slots and their
  admitted cost, never memory beyond them.
- **`ended`** is the steward's word that a minted console's session is over
  ([started by `init`](#started-by-init)); sent as a call it is malformed.

The table: [libs/wire/tables/consol.md](../../libs/wire/tables/consol.md).

{{#include ../../libs/wire/tables/consol.md:tables}}

### Started by `init`

<details><summary>Status: built · tested (14)</summary>

- bench:init-boot
- bench:init-servers
- bench:init-console-forgery
- bench:bench-init-reporter-forged
- host:redoubt-consoled::the_prefix_is_the_id_in_sixteen_hex_digits
- host:redoubt-consoled::a_root_line_is_bare_and_a_minted_one_carries_its_id_on_every_line
- host:redoubt-consoled::a_line_is_continued_by_its_writer_and_ended_by_any_other
- host:redoubt-consoled::whatever_the_uart_takes_no_line_mixes_writers_or_carries_another_s_id
- host:redoubt-rt::the_file_server_learns_the_id_the_requester_got
- host:redoubt-consoled::a_console_with_no_device_does_not_start
- host:redoubt-consoled::init_s_badge_mints_a_console_for_every_server_init_can_start
- host:redoubt-consoled::every_console_init_mints_attaches_and_opens_in_its_one_bucket
- bench:init-refuses-consoled-handed
- host:redoubt-init::no_server_is_handed_a_root_badge_at_consoled

</details>

`init` starts `consoled` once the manifest and the keys are checked, with the UART's MMIO region
and interrupt as named handles and its endpoint, placed from the boot manifest's `devices` list
([init](init.md#starting-the-servers)). `consoled` parses no device tree; without both handles it
does not start. Only one process ever maps the UART: two would both reach the registers, and two
readers of one FIFO would each take half the line. `init` writes to the UART itself only before
`consoled` starts, and unmaps it first.

**Every line says who wrote it.** A write through `init`'s own connection goes out as it is.
`init` holds the only handle to the root, and it never hands the root to a child: its check
refuses a manifest that hands any server an endpoint `consoled` receives on
([init](init.md#starting-the-servers)). A write through
any connection minted under the root starts each of its lines with `[con N] `, where N is that
connection's id, the one its requester was given, in 16 lowercase hex digits. `init` prints the id
of each child's connection, bare, when it starts the child (`init: started NAME, console N`), so a
reader of the console can tell `init` from every program and each program from the others,
as the bench's log server does today
([rule F](../testbench.md#rule-f-trusted-verdicts)). `ended` on a minted connection releases it:
the steward says so when the console session it started is over. If a write comes from a
different connection than the one that left the last line unfinished, `consoled` ends that line
first. So a line a program writes cannot come out bare, and it cannot carry another connection's
id. A transmitter that takes only part of a prefix leaves the line open, and the rest of the
prefix goes out before any more of the writer's bytes; ended by another writer, such a line holds
the start of its writer's prefix and nothing else.

The rule names the handles `NAME` and `NAME-irq` from one `devices` entry
([init](init.md#the-boot-manifest)); `consoled` takes `uart` and `uart-irq`.

## Authority

Status: built · partly tested: no test writes as a labelled caller · tested: host:redoubt-consoled::the_console_refuses_what_it_is_not

- `consoled` holds the UART's registers and interrupt, its endpoint, and the connections it minted. It
  makes no calls to other servers.
- Any caller with a connection may read the console; only an unlabelled caller may write it.

## Security properties

### R69 (no write down onto the console)

Status: built · partly tested: no `consoled` test writes as a labelled caller; the rule rests on the 9P skeleton's label check, which is attacked in the serving library's tests · tested: host:redoubt-rt::labels_are_checked_on_every_request, host:redoubt-rt::every_write_needs_equal_labels

The physical console carries no labels, so a caller with any label cannot write to it: no labelled
data reaches a screen someone else may be looking at.

### R70 (a stuck UART never hangs the console)

Status: built · tested: host:redoubt-consoled::a_device_stuck_on_data_ready_does_not_hang_the_server, host:redoubt-consoled::a_flood_of_input_keeps_what_was_typed_first

Whatever the UART reports, `consoled` does a bounded amount of work per call and keeps serving: a
drain reads at most the FIFO's depth, and held input is bounded, dropping new bytes rather than
growing.

## Failure and restart

Status: built · partly tested: the restart claims are read from the code, not attacked across a restart · tested: host:redoubt-consoled::a_console_with_no_device_does_not_start, host:redoubt-consoled::a_read_with_no_input_waits_and_is_freed_when_its_caller_gives_up

- **No UART or interrupt handle:** `consoled` exits with a code before serving.
- **A reader gives up:** its parked read is answered and freed at once.
- **`consoled` restarts:** held input is lost, parked reads and `resize` calls get `Dead`, and
  clients ask for fresh connections.

## Residual risks

- **One keyboard, one queue.** A client reading `/dev/cons` takes input another was waiting for; a
  session that must not share a console gets its own from `sshd`.
- **A parked read, or `resize`, has no deadline.** It holds one of its caller's open calls and one
  of `consoled`'s admission slots until a key arrives (for a `resize`, never) or its caller gives
  up.
- **Anyone with a connection reads the console.** What is typed on the physical console is visible to
  every holder of a `consoled` connection.

## Why

- **Park, rather than block or answer empty.** A blocked server serves nobody else; an empty
  answer looks like a closed console; parking costs one slot and nothing more.
- **The interrupt thread touches nothing.** Keeping the device on one thread removes every lock and
  every shared-state bug between the two.
- **Drain at most the FIFO.** A 16550 can hold no more; a device claiming otherwise is broken, and
  bounding the loop turns it into dropped input instead of a hung server.
