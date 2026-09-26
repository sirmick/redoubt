# netd

`netd` is the virtio-net driver. It owns one network device and serves its frames to one client,
[`ipd`](ipd.md): `ipd` transmits through `netd`'s `netif` protocol, and `netd` hands each received
frame to `ipd` as a one-way message carrying one page. It is written like [`blkd`](blkd.md): two
DMA regions the device alone can reach, nothing the device writes believed, and a device that
lies reset and refused for good. What a sender on the wire puts in a frame is content, never a lie.

## Purpose

The network card is driven by code that must trust neither the device nor anyone on the wire, and
that must keep every earlier frame, which may be another principal's traffic, out of every later
one. `netd` does only that: frames in, frames out, for one client, with no addresses, sockets or
policy. Everything above the link is `ipd`'s.

## Interface

### Serving `ipd`

Status: built · tested: bench:netd-host-tests, bench:d3-net-tcp, host:redoubt-netd::info_and_transmit_for_the_client, host:redoubt-netd::anyone_else_is_not_permitted, host:redoubt-netd::a_frame_of_the_wrong_length_is_too_many, host:redoubt-netd::a_full_ring_is_busy_and_a_lie_is_failed_for_good, host:redoubt-netd::a_request_that_does_not_decode_is_malformed, host:redoubt-netd::arguments_are_exactly_one_client_badge, host:redoubt-netd::randomized_requests_reach_the_wire_only_from_the_client, fuzz:redoubt-netd/request

- **One client.** `netd`'s one argument names the badge `ipd`'s handle carries. Any other badge, and
  any labelled caller, gets `not_permitted`. `netd` mints nothing and parks nothing, so a call holds
  nothing after its reply.
- **`info`**: the device's MAC address and the MTU (1500).
- **`transmit(frame)`**: one Ethernet frame of 14 to 1514 bytes (`MIN_FRAME` to `MAX_FRAME`), copied
  out of the caller's lend into a transmit slot. A frame of another length is `too_many`; a full
  transmit ring is `busy`; a broken device is `failed`.
- **Received frames** go to `ipd` as its `frame` message: a `send`, the frame in one transferred
  page of anonymous memory ([ipd](ipd.md)). A frame `ipd` cannot take within `SEND_TIMEOUT_US`
  (50 ms) is dropped, as on any wire.

The table: [libs/wire/tables/netif.md](../../libs/wire/tables/netif.md).

{{#include ../../libs/wire/tables/netif.md}}

### Rings and slots

Status: built · tested: host:redoubt-netd::an_honest_device_comes_up_with_two_features_and_its_mac, host:redoubt-netd::bring_up_refuses_and_resets, host:redoubt-netd::frames_arrive_exactly_and_every_slot_comes_back, host:redoubt-netd::a_transmit_sends_exactly_its_header_and_frame, host:redoubt-netd::an_inflated_length_reads_zeros_not_old_frames, host:redoubt-netd::the_ring_counters_wrap, host:redoubt-netd::spurious_interrupts_deliver_nothing, host:redoubt-netd::a_frame_completed_before_the_first_wait_without_an_interrupt_is_delivered, host:redoubt-netd::a_frame_completed_silently_during_a_drain_is_delivered_before_the_next_wait

- **Two DMA regions**, one per queue, each a run of `REGION_PAGES` pages from `dma_alloc`
  ([devices](../kernel/devices.md#dma_alloc)): the descriptor table, available ring and used ring
  on the first page, then `QUEUE_SIZE` (16) slots of `SLOT_LEN` (2048) bytes. **Every address
  `netd` gives the device is a region's base plus a constant**, checked at compile time
  (`ring::LAYOUT_FITS`), and descriptor i always names slot i
  ([R54 (DMA stays in netd's regions)](#r54-dma-stays-in-netds-regions)).
- **Bring-up** accepts version 2 only and exactly two features, version 1 and the MAC; it
  configures both queues, offers every receive slot, and sets `DRIVER_OK` before the receive thread
  exists. A device that fails any step is reset and refused.
- **Receive.** All sixteen slots are always offered. Each frame is copied out of its slot once, with
  `netd`'s own length, into `netd`'s own memory before any byte is looked at, then into a fresh page
  for `ipd`; the slot is zeroed before it is offered again, so a device that reports more than it
  wrote hands back zeros, never an earlier frame.
- **Transmit.** Up to sixteen frames are with the device at once, reclaimed when the next is sent.
  A transmit's descriptor carries exactly its 12-byte header and its frame, never the whole slot,
  so no byte of an earlier frame goes out behind a later one
  ([R56 (no earlier frame leaks)](#r56-no-earlier-frame-leaks)). The transmit queue asks for no
  interrupts.
- **Interrupts only say when to look.** The receive queue is drained before every wait, so a lost
  interrupt delays nothing and a spurious one delivers nothing.
- **A DMA page never leaves `netd`.** No path lends, transfers or maps one; the kernel refuses to
  anyway ([devices](../kernel/devices.md#dma_alloc)).

```svgbob
 receive region (REGION_PAGES pages)            transmit region (REGION_PAGES pages)
 +------------------------------------+         +------------------------------------+
 | page 0: descriptors | avail | used |         | page 0: descriptors | avail | used |
 |  desc i --> slot i  | (write| (idx,|         |  desc i --> slot i: | (write| (idx,|
 |  (write-only)       |  only)| id,  |         |  header + frame only|  only)| id,  |
 |                     |       | len) |         |  (write-only)       |       | len) |
 +------------------------------------+         +------------------------------------+
 | slot 0  (2048 bytes)               |         | slot 0  (2048 bytes)               |
 | slot 1                             |         | slot 1                             |
 |  ...                               |         |  ...                               |
 | slot 15                            |         | slot 15                            |
 +------------------------------------+         +------------------------------------+
     |  copied out once, slot zeroed                 ^  copied in from ipd's lend
     v                                               |
 +-------------+   send, one page    +-----+        +-----+
 | netd memory | ------------------> | ipd |  ----> |     |  transmit call
 +-------------+                     +-----+        +-----+
```
*Figure: `netd`'s two DMA regions; descriptor i always names slot i, and only the used ring's index, id and length are read.*

### Lies and bad frames

Status: built · tested: fuzz:redoubt-netd/device, host:redoubt-netd::receive_lies_are_refused, host:redoubt-netd::transmit_lies_are_refused, host:redoubt-netd::a_buffer_completed_twice_is_a_lie, host:redoubt-netd::scribbling_devices_change_nothing_netd_believes, host:redoubt-netd::a_scribbled_descriptor_is_rewritten_when_offered_again, host:redoubt-netd::a_device_that_keeps_every_slot_is_busy_then_broken, host:redoubt-netd::a_bad_frame_from_the_wire_is_dropped_never_a_lie, host:redoubt-netd::randomized_hostile_devices

**A lie and a bad frame are different things.**

- **A lie** is a used entry that breaks the ring protocol: the index running past what is
  outstanding, an id out of range, not outstanding or seen twice, a length below the header or above
  the slot, or a header asking for checksum or segmentation that was never negotiated; or a
  transmit slot held longer than `TX_TIMEOUT_US` (10 seconds). A lie ends the device: it is reset
  and every later request is `failed`. The descriptor tables and available rings are written from
  constants and `netd`'s own counters and never read back, so a device that scribbles on them
  changes nothing `netd` believes.
- **A bad frame** has a length outside 14 to 1514 bytes but inside the slot: some sender on the wire
  sent it (a tagged frame, a runt), and an honest device delivers it. It is dropped and counted,
  and the slot is offered again. A packet on the LAN never bricks the NIC
  ([R55 (a bad frame is content, not a lie)](#r55-a-bad-frame-is-content-not-a-lie)).

### Two threads, reset on exit

Status: built · tested: host:redoubt-netd::only_the_receive_threads_report_breaks_the_device, host:redoubt-netd::the_receive_loop_resets_and_reports_a_lie, host:redoubt-netd::the_receive_loop_resets_and_reports_when_the_interrupt_fails, host:redoubt-netd::a_panic_resets_the_device, bench:d3-net-tcp

- **The serving thread** maps the registers, allocates both regions, brings the device up and only
  then starts the receive thread. It owns the transmit queue, answers `netif` calls, and waits on
  nothing but its own `receive`.
- **The receive thread** takes its half out of `netd`'s own memory, so no address travels in a
  message. It waits on the interrupt, drains the receive queue and sends each frame to `ipd`. It
  tells the serving thread only that the device is broken, on a badge drawn at random above 2^63
  that carries no data.
- **Every way out stops the device.** The receive thread resets it and reports on a lie, a fault
  reading the rings, or the interrupt failing; the serving thread resets it on a lie, on that
  report, and before it exits; a panic resets it from the runtime's panic hook. A reset is status 0,
  read back, so the device stops touching its rings before its pages can return to the pool
  ([R57 (the device stops before netd does)](#r57-the-device-stops-before-netd-does)).
- **A device fault never stops `netd`.** A broken device is answered `failed` for good, and `netd`
  stays up, so a lying device cannot make the box restart it in a loop.

### Started by `init`

Status: planned · M1 (separation and containment)

`init` starts `netd` with the network card's MMIO region (DMA allowed) and interrupt, placed by
name from the boot manifest's `devices` list, and its one argument, the badge `ipd`'s handle
carries; it hands `ipd` the matching handle to `netd` and `netd` a handle to `ipd`'s endpoint for
frames ([init](init.md#starting-the-servers)). The net rig (`tests/net/src/rig.rs`) does this in
the bench, finding the card by its virtio device ID.

**Open:** none.

## Authority

Status: built · tested: host:redoubt-netd::anyone_else_is_not_permitted, host:redoubt-netd::randomized_requests_reach_the_wire_only_from_the_client

- `netd` holds one network device's MMIO region and interrupt, its two DMA regions, its endpoint,
  and a handle to `ipd`'s endpoint for frames. It holds no addresses, sockets or policy.
- Only `ipd`'s badge may transmit; nothing reaches the wire from anyone else.
- It is trusted, on a machine without an IOMMU, to name only its DMA regions to the device.
- The only `unsafe` in the crate is in `servers/netd/src/kernel.rs`, the seam to the kernel.

## Security properties

### R54 (DMA stays in netd's regions)

Status: built · tested: host:redoubt-netd::scribbling_devices_change_nothing_netd_believes, host:redoubt-netd::randomized_hostile_devices, host:redoubt-netd::a_transmit_sends_exactly_its_header_and_frame

`netd` never asks the device to touch anything but its two DMA regions, every address it gives is a
region's base plus a checked constant, and no DMA page is ever lent, transferred or mapped out of
`netd`. Without an IOMMU this bounds what `netd` asks for, not what the device does.

### R55 (a bad frame is content, not a lie)

Status: built · tested: host:redoubt-netd::a_bad_frame_from_the_wire_is_dropped_never_a_lie, host:redoubt-netd::receive_lies_are_refused, fuzz:redoubt-netd/device

Nothing a sender on the wire puts in a frame can stop `netd` or break its device: a frame of a
length `netd` does not carry is dropped and counted, and only a used entry that breaks the ring
protocol, which only the device can write, ends the device.

### R56 (no earlier frame leaks)

Status: built · tested: host:redoubt-netd::a_transmit_sends_exactly_its_header_and_frame, host:redoubt-netd::an_inflated_length_reads_zeros_not_old_frames, host:redoubt-netd::frames_arrive_exactly_and_every_slot_comes_back

No byte of an earlier frame reaches a later one in either direction: a transmit tells the device
exactly its header and frame, and a receive slot is zeroed before it is offered again, so a device
that claims more than it wrote hands back zeros.

### R57 (the device stops before netd does)

Status: built · partly tested: a kill or a fault of `netd` runs none of its code, and the device reset then is the kernel's · tested: host:redoubt-netd::a_panic_resets_the_device, host:redoubt-netd::the_receive_loop_resets_and_reports_a_lie, host:redoubt-netd::the_receive_loop_resets_and_reports_when_the_interrupt_fails, host:redoubt-netd::only_the_receive_threads_report_breaks_the_device

On every way out of either thread that `netd` controls, a lie, a failed interrupt, an exit or a
panic, the device is reset and the reset read back before anything else, so the device writes
nothing more into pages that may be returned. A kill or a fault runs none of `netd`'s code; then the
kernel resets the device before its DMA pages are reused
([I16 (DMA pages reset before reuse)](../kernel/invariants.md#i16-dma-pages-reset-before-reuse)).

## Failure and restart

Status: built · tested: host:redoubt-netd::a_full_ring_is_busy_and_a_lie_is_failed_for_good, host:redoubt-netd::bring_up_refuses_and_resets, host:redoubt-netd::a_panic_resets_the_device

- **No device or bad arguments:** `netd` exits with a code before serving.
- **The device lies or times out:** it is reset, every later request is `failed`, and `netd` stays up.
- **`netd` panics:** the panic hook resets the device, and the runtime reports and exits
  ([serving](serving.md#failure-and-restart)).
- **`ipd` is slow:** received frames it cannot take within 50 ms are dropped.

## Residual risks

- **`netd` is trusted without an IOMMU.** A compromised `netd`, or a device that ignores its
  addresses, can write anywhere in RAM.
- **A frame flood costs `netd`'s CPU** at its large manifest weight, and its drops fall on every
  connection `ipd` serves.
- **A reset stops the device for everyone.** One lie ends the network for every principal until the
  device is brought up again; `netd` is not restarted to do that.
- **`netd` does not boot under `init` in the bench.** The net rig launches it through the stub in
  place of `init`.

## Why

- **One client.** Every policy about who may send what belongs to `ipd`; a driver with one client
  has none to get wrong.
- **Lies and bad frames apart.** If a wrong-length frame counted as a device lie, anyone on the LAN
  could reset the NIC with one packet.
- **Exact transmit lengths and zeroed slots.** A slot reused without clearing carries its last
  frame; telling the device exactly what to read, and zeroing what it may write, keeps one
  principal's traffic out of another's.
- **Reset before exit.** A device that keeps writing after its driver is gone writes into whatever
  its pages become next.
- **Stay up on a fault.** A driver that exits on a lie hands the attacker a restart loop; one that
  stays up and answers `failed` does not.
