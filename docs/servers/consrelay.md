# consrelay

`consrelay` is a context's console relay. The steward starts one beside each SSH context's VM,
in the context's budget ([the steward](steward.md#contexts)). It serves the VM's `/dev/cons`,
and forwards it to whichever SSH channel the context is attached to. While none is, it keeps the
VM's newest output, a bounded amount, and replays it to the next channel. So closing a terminal
detaches the context instead of ending it.

## Purpose

A session's `/dev/cons` used to be its channel's own console file in `sshd`, which dies with the
channel. A context that outlives its channel needs a console that outlives it too, served by
something that is neither the channel nor the VM: the VM cannot keep its own console across
channels, and the steward must not hold a person's output on its own heap. The relay runs in the
context's budget, so what it keeps is charged to the context, and its writes to a channel carry
the context's labels.

## Interface

### `/dev/cons` for the VM

Status: built · tested: bench:consrelay-build, bench:consrelay-host-tests, host:redoubt-consrelay::output_kept_while_detached_is_replayed_after_the_note_and_input_reaches_the_vm, host:redoubt-consrelay::a_channel_let_go_is_gone_to_its_threads_and_a_takeover_takes_its_input, host:redoubt-consrelay::typed_input_outlives_a_detach, host:redoubt-consrelay::input_read_before_a_detach_and_given_after_it_is_the_vm_s, host:redoubt-consrelay::a_takeover_s_detach_drops_the_old_channel_s_input, host:redoubt-consrelay::the_last_line_typed_reaches_the_vm_after_the_detach, host:redoubt-consrelay::the_size_is_the_attached_channel_s_and_each_attach_is_a_change, host:redoubt-consrelay::the_vm_s_size_is_the_channel_s_and_an_attach_redraws

One file over 9P, with the semantics of `sshd`'s channel console: a read with nothing typed
parks, a write goes to the channel, `stat` says length 0, and the file carries the context's
labels. The VM reaches it only through the connection the relay minted for it at its start; an
attach through any other badge is refused. On the same endpoint the relay serves `consol`'s `size`
and `resize` ([consoled](consoled.md#the-consol-protocol)): `size` answers the attached channel's
size as `sshd` last gave it, 80 by 24 before any channel has; a `resize` is parked until the size
changes, then answered with it, and each attach's first size counts as a change, the same size or
not, so a VM waiting is told its new terminal and redraws. What was typed on a channel and not yet read stays
for the VM when the terminal closes, and so does what the reader read before the steward's
`detach` reached the relay, until another channel is attached: a last `exit` typed just before
the terminal closed still counts. A takeover's `detach`, which carries a note, drops what the old
channel typed and refuses what its reader brings in after.

### The bound

Status: built · tested: host:redoubt-consrelay::a_write_never_waits_while_detached_and_the_newest_bytes_are_kept, host:redoubt-consrelay::an_attach_cuts_to_a_line_says_what_was_dropped_and_replays, host:redoubt-consrelay::nothing_dropped_means_no_cut_and_no_count, host:redoubt-consrelay::an_attached_write_waits_for_room_and_goes_out_in_order, host:redoubt-consrelay::a_broken_channel_keeps_output_as_if_detached, host:redoubt-consrelay::input_is_bounded, host:redoubt-consrelay::a_flood_while_detached_is_counted_and_the_rest_replayed, bench:consrelay-footprint

- **While attached,** the VM's output waits in a buffer of 64 KiB (`KEEP`) for the writer thread, and
  a write with no room waits, as on `sshd`'s console.
- **While detached,** or while the attached channel's calls fail, a write never waits: the oldest
  kept bytes go to make room, and are counted.
- **On attach,** if any were dropped, the kept bytes are cut forward to the end of their first line,
  so no half line or half escape sequence leads them, and the channel gets the steward's note, a
  line `[N bytes of output dropped while detached]`, then the kept output.
- **Input** held for the VM's reads is at most 4 KiB; past that the reader thread's call waits,
  which leaves the rest in `sshd` and SSH's window.
- The output and input buffers are allocated at the start, so a VM that later spends its budget's
  pages cannot starve the relay. Measured once it serves (`consrelay-footprint`), its heap holds
  22 pages and its first thread's stack peaks at 8,360 bytes on rv64; the steward launches it with
  a 44-page heap cap and a 5-page stack, its helpers with 4 pages each, and a session's budget holds 128 pages for it beside the
  VM's share ([budgets](../kernel/budgets.md)).

### Four threads

Status: built · tested: host:redoubt-consrelay::output_kept_while_detached_is_replayed_after_the_note_and_input_reaches_the_vm, host:redoubt-consrelay::the_reader_is_given_a_channel_once_the_writer_opened_it, host:redoubt-consrelay::a_console_is_closed_once_no_thread_is_on_it

- **The serving thread** owns the state and the 9P skeleton, and serves the VM, the steward's
  control calls and the three helpers' calls. It makes no call that waits on anything but its own
  endpoint, so neither the VM nor the steward ever waits on a channel through it.
- **The writer thread** opens each channel's console as it is attached, asks for output and writes
  it there.
- **The reader thread** reads what is typed on the channel the writer opened and hands it over.
- **The sizer thread** asks that channel's console its size, then keeps one `resize` waiting there
  and hands over each answer; `sshd` answers the last one when the channel's session ends, and
  refuses any after, so the sizer leaves a channel that has ended.

A helper names the channel's generation in each call, so one still on a channel the steward has let
go is told so and leaves it. A channel's console handle is closed once no helper is on it.
The helpers and their stacks are made once, at the start.

### The `consrelay` protocol

Status: built · tested: host:redoubt-consrelay::the_vm_cannot_attach_or_detach, host:redoubt-consrelay::a_control_call_without_its_console_is_malformed, host:redoubt-consrelay::a_detach_waits_until_its_note_is_written, host:redoubt-consrelay::a_detach_waits_for_nothing_it_cannot_write, host:redoubt-consrelay::a_note_is_capped, host:redoubt-steward-server::a_login_to_an_attached_context_takes_it_over

At its start the relay makes its endpoint in its own budget, mints the control badge and the VM's
console on it, and sends both to the steward with `hello` on the badge its startup block names
`hello`; the steward waits at most 2 s for it. `attach` gives the relay a channel's console with a
note to write there first; `detach` has it write a note to the attached channel, answered once the
note is written (the steward waits at most 1 s), and let the channel go. Both are accepted only on
the control badge; through the VM's badge they are malformed, as any opcode that is not 9P is. A
note is at most 384 bytes.

The table: [libs/wire/tables/consrelay.md](../../libs/wire/tables/consrelay.md).

{{#include ../../libs/wire/tables/consrelay.md:tables}}

## Authority

Status: built · tested: host:redoubt-consrelay::the_vm_cannot_attach_or_detach, host:redoubt-sshd::a_login_s_two_consoles_and_only_the_steward_s_ends_the_channel

- The relay holds its endpoint, the hello badge until its hello, and the console of each channel it
  is attached to: `sshd`'s second connection to that channel, which can read and write it but not
  end it ([sshd](sshd.md#sessions-over-ssh)). It holds nothing else, and calls nothing but the channel's
  console.
- It runs in the context's budget, so its writes to a channel carry the context's labels and meet
  `sshd`'s label check ([R25 (the label check)](serving.md#r25-the-label-check)), and what it keeps is charged to the context.

## Security properties

Status: built · tested: host:redoubt-consrelay::a_channel_let_go_is_gone_to_its_threads_and_a_takeover_takes_its_input, host:redoubt-steward-server::a_login_to_an_attached_context_takes_it_over

The relay owns no rule of its own. It is how the steward keeps
[R80 (one channel per context)](steward.md#r80-one-channel-per-context): it forwards to one
channel at a time, the one the steward last attached, and a helper still on a channel let go is
told so before it can write there again.

## Failure and restart

Status: built · partly tested: a relay that dies alone is read from the code: no test kills one · tested: host:redoubt-consrelay::a_detach_waits_for_nothing_it_cannot_write

- **A channel's call fails, or its input ends:** output is kept as if detached until the steward
  says what happened, which it does when `sshd` reports the channel closed.
- **The relay dies:** the VM's `/dev/cons` calls fail and the shell ends; the steward watches the
  relay's exit as a session's and ends the context.
- **No hello within 2 s:** the steward's launch step fails, and the session's budget, the relay with
  it, is destroyed.
- **The context ends:** destroying its budget ends the VM and the relay together.

## Residual risks

- **What the VM wrote while detached may be cut.** Only the newest 64 KiB are kept; the count says
  how many went, not what they were.

## Why

- **A process in the context's budget, not the steward.** The steward's heap would otherwise grow
  with every person's detached output, and a bug in forwarding would sit in the trusted base.
- **Helper threads, not blocking calls in the serving thread.** A slow or stuck channel then holds
  one thread of the context's own, never the VM's console or the steward's control call.
