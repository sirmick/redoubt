# blkd

`blkd` is the virtio-blk driver. It owns one disk, reads its GPT partition table once, and serves
each partition to one [`fsd`](fsd.md) as a range of sectors, named by a badge. A client's sector
numbers are relative to its range, the device only ever sees `blkd`'s own DMA region, and a device
that lies is refused for good rather than believed.

## Purpose

A file server must see only its own partition, and the disk device must be driven by code that
trusts neither its clients nor the device. `blkd` does both in one small process: it turns a
badge into a window on the disk, copies every byte between a client's lend and a DMA buffer the
device alone can reach, and treats every value the device writes as a claim to check. On a
machine where nothing confines DMA it is inside the trusted base, and it is written so that what
it asks the device to touch can be checked.

## Interface

### Ranges and badges

Status: built · tested: bench:blkd-host-tests, host:redoubt-blkd::a_block_round_trips_through_a_range_badge, host:redoubt-blkd::info_describes_the_range_not_the_disk, host:redoubt-blkd::a_full_run_round_trips, host:redoubt-blkd::a_range_cannot_name_a_sector_outside_itself, host:redoubt-blkd::a_badge_that_names_no_range_is_refused, host:redoubt-blkd::roots_have_one_slot_per_gpt_entry, host:redoubt-blkd::a_gap_in_the_table_does_not_renumber_the_volumes_after_it, host:redoubt-blkd::a_labelled_caller_may_read_but_not_write, host:redoubt-blkd::one_clients_read_never_carries_anothers_bytes

- **A range is a badge.** The root badge of GPT entry i is i + 1, counting every entry of the
  array, used or not. So a launcher mints each volume's range from the manifest's entry number
  without asking `blkd`, a restarted `blkd` gives the same badges the same meaning from the same
  disk, and a badge naming an unused entry is refused like one past the end. Counting only used
  entries would renumber every volume after a gap.
- **Sector numbers are relative to the range**, so no number a client can write names a sector
  outside it. Turning one into a disk sector is one overflow-checked function (`range::Range`).
- **`blkd` mints nothing and remembers nothing.** There is no `grant` and no `release`: every range
  comes through a badge its launcher minted, and a client can make `blkd` hold nothing beyond its
  request, so there is no admission to keep. A flood of requests is bounded by the kernel's fair
  waiting ([R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting)) and by `MAX_SECTORS` per request.
- **The label check** runs on every request against the range's labels
  ([R25 (the label check)](serving.md#r25-the-label-check)): reading is a read, writing and
  flushing are writes. A badge that names no range and a caller who fails the check get the same
  `not_permitted`.

### Messages

Status: built · tested: host:redoubt-blkd::a_request_over_the_bound_is_refused_and_one_of_no_sectors_is_malformed, host:redoubt-blkd::a_read_whose_reply_would_not_fit_the_lend_is_refused_before_the_disk_is_touched, host:redoubt-blkd::malformed_requests_are_refused, host:redoubt-blkd::arbitrary_requests_never_panic, host:redoubt-blkd::a_read_only_device_refuses_writes, host:redoubt-blkd::a_broken_read_only_device_does_not_answer_ok_to_a_flush, fuzz:redoubt-blkd/request

`blkd` serves a typed protocol, not 9P: four operations on a range, and no namespace.

- **`info`**: the range's size in sectors, the sector size (512) and whether the disk is read-only.
- **`read(sector, count)`**: at most `MAX_SECTORS` (64) sectors, 32 KiB, into the caller's lend. A
  read whose reply would not fit the lend is refused before the disk is touched; a count of 0 is
  malformed.
- **`write(sector, data)`**: at most `MAX_SECTORS` sectors, a whole number of them.
- **`flush`**: returns only when the device says its flush completed, so what was written before
  it is durable. The device must offer flush at negotiation; one that does not is refused at
  bring-up.

Errors: `not_permitted`; `out_of_range` for sectors outside the range; `too_many` for a request
over the bound; `failed` for a device error or a broken device; `malformed` for a request that
does not decode.

This is the block-device contract littlefs's power-loss safety rests on
([fsd](fsd.md#power-loss)): a write overwrites whole sectors, requests complete in order (one is
outstanding at a time), and `sync` is a `flush` that waits for the device.

The table: [libs/wire/tables/blkd.md](../../libs/wire/tables/blkd.md).

{{#include ../../libs/wire/tables/blkd.md:tables}}

### The DMA region

Status: built · tested: host:redoubt-blkd::a_broken_device_is_never_handed_a_clients_write_payload, host:redoubt-blkd::a_short_write_reads_back_as_zeros_not_as_the_last_requests_bytes, host:redoubt-blkd::impossible_requests_never_reach_the_device, host:redoubt-blkd::a_hundred_thousand_random_liars_never_panic_and_never_stray

The device can reach one contiguous run of `DMA_PAGES` pages from `dma_alloc`
([devices](../kernel/devices.md#dma_alloc)), laid out by constants: the descriptor table, the
available ring, the used ring, one request header and one status byte in the first page, and one
data buffer of `MAX_SECTORS` sectors after it. **Every address `blkd` writes into a descriptor is
the region's base plus one of these constants**, checked at compile time to lie inside the region
(`queue::LAYOUT_FITS`), and the only other addresses it gives the device are the three queue base
registers, which point into the same region. A client's lent pages are never named to the device:
a write is copied from the lend into the data buffer before the request is offered, and a read is
copied out of the data buffer once, with a length `blkd` chose, before anything looks at a byte of
it ([R51 (DMA stays in its region)](#r51-dma-stays-in-its-region)).

```svgbob
 DMA region: DMA_PAGES pages from dma_alloc, the only memory the device is told about

 base + 0                                                       base + PAGE_SIZE
 +------------------+-----------------+--------------+--------+--------+-------+
 | descriptor table | available ring  |  used ring   | header | status | (pad) |
 | (written, never  | (written, never | (3 values    |        |  byte  |       |
 |  read back)      |  read back)     |  read,checked|        |        |       |
 +------------------+-----------------+--------------+--------+--------+-------+
 base + PAGE_SIZE
 +------------------------------------------------------------------------------+
 | data buffer: MAX_SECTORS x 512 bytes                                          |
 +------------------------------------------------------------------------------+
        ^                                                   |
        | copied in before a write                          | copied out once after a read
 +------+------+                                     +------v------+
 | client lend |  never named to the device          | blkd memory |
 +-------------+                                     +-------------+
```
*Figure: the DMA region and where a client's bytes go.*

### Device lies

Status: built · tested: fuzz:redoubt-blkd/device, host:redoubt-blkd::rewriting_the_rings_changes_nothing_the_driver_believes, host:redoubt-blkd::a_device_that_lies_about_a_completion_is_refused_and_never_spoken_to_again, host:redoubt-blkd::a_lying_device_becomes_failed_and_stays_failed, host:redoubt-blkd::lies_that_are_not_protocol_violations_are_still_harmless, host:redoubt-blkd::a_reported_io_error_does_not_break_the_device, host:redoubt-blkd::a_device_that_is_not_one_is_refused_at_bring_up, host:redoubt-blkd::only_the_features_we_need_are_accepted, host:redoubt-blkd::an_interrupt_storm_is_bounded_even_when_the_clock_has_failed, host:redoubt-blkd::an_interrupt_storm_with_a_working_clock_ends_at_the_deadline, host:redoubt-blkd::every_completion_acknowledges_the_interrupt, host:redoubt-blkd::avail_flags_is_written_again_before_every_request, host:redoubt-blkd::both_completion_paths_work

- **Nothing the device writes is believed.** The descriptor table and the available ring are
  written from constants for every request and never read back, so a device that rewrites them (a
  looping chain, a `next` out of range, an overflowing length) changes nothing `blkd` believes. Of
  the used ring three values are read, and each is checked: `idx` must be exactly one more than
  the last seen; the entry, read from the slot `blkd`'s own counter names, must name the one
  descriptor `blkd` submits; `len` must not exceed what the device was given. No device value is
  ever an index or a length.
- **Bring-up** checks the magic, version 2, the block device ID, and accepts only the features it
  needs (version 1, flush, read-only).
- **A lie is permanent.** A device that breaks the protocol, or misses `REQUEST_TIMEOUT_US`
  (10 seconds), is marked broken and every later request gets `failed`: carrying on after a
  timeout would mean a completion for a request no longer tracked, and carrying on after a lie
  would mean trusting the liar. A device that merely reports a failure (`IOERR`, `UNSUPP`) fails
  that one request.
- **Interrupt storms are bounded**, by the deadline, and even when the clock has failed.

The fake device (`servers/blkd/src/fake.rs`) is deliberately hostile, and the host tests run the
whole driver against it, including a hundred thousand random liars
([R52 (a lie is a failure, never corruption)](#r52-a-lie-is-a-failure-never-corruption)).

### The partition table

Status: built · tested: fuzz:redoubt-blkd/gpt, host:redoubt-blkd::a_good_table_reads_back, host:redoubt-blkd::header_fields_sit_where_the_specification_says, host:redoubt-blkd::crc32_matches_the_published_check_value, host:redoubt-blkd::unused_entries_are_skipped_and_indices_are_the_entry_s, host:redoubt-blkd::a_disk_with_no_signature_is_refused, host:redoubt-blkd::a_header_crc_that_does_not_match_is_refused, host:redoubt-blkd::an_entry_array_crc_that_does_not_match_is_refused, host:redoubt-blkd::hostile_header_numbers_are_refused, host:redoubt-blkd::hostile_partition_entries_are_refused, host:redoubt-blkd::adjacent_partitions_are_allowed, host:redoubt-blkd::arbitrary_header_bytes_never_panic, host:redoubt-blkd::arbitrary_entry_arrays_never_panic

The GPT (UEFI 2.10, section 5.3) is the one on-disk structure `blkd` parses, once, at start.

- **The medium is hostile.** Every length, LBA and count is checked before use; the entry array is
  sliced with checked access and its size bounded (at most 128 entries of 128 to 512 bytes, at
  most 64 sectors) before it is allocated; every partition's first and last LBA are checked
  against the disk's capacity and the header's usable range; both CRCs must match. A malformed
  table yields an error, never a panic.
- **Overlaps are refused whole.** Two partitions sharing a sector would let two volumes alias each
  other's bytes ([R53 (a filesystem sees only its partition)](#r53-a-filesystem-sees-only-its-partition)).
- **Primary header only.** `blkd` never writes a partition table, so a table that does not check out
  is a disk to refuse, not damage to repair: no fallback to the backup header, and the protective
  MBR is not read.

### Started by `init`

Status: planned · M1 (separation and containment)

`init` starts `blkd` with two named handles in its startup block, `disk` (the virtio MMIO region,
with DMA allowed) and `disk-irq` (its interrupt), placed from the boot manifest's `devices` list
([init](init.md#the-boot-manifest)). `blkd` parses no device tree and hardcodes no address;
without both handles it does not start. `init` mints each `fsd`'s range badge from the manifest's
`volumes` entry, and hands it to that `fsd` alone.

**Open:** none.

## Authority

Status: built · tested: host:redoubt-blkd::a_range_cannot_name_a_sector_outside_itself, host:redoubt-blkd::impossible_requests_never_reach_the_device

- `blkd` holds one disk's MMIO region and interrupt, its DMA region, and its endpoint. It makes no
  calls to other servers.
- It is trusted, on a machine without an IOMMU, to name only its DMA region to the device; nothing
  in it can make a device that ignores those addresses stay inside them.
- A client's badge is its range and nothing more: it reads and writes those sectors, under the
  label check.
- The crate denies `unsafe` everywhere but `servers/blkd/src/kernel.rs`, the one seam to the
  kernel, which holds the volatile register and DMA accesses.

## Security properties

### R51 (DMA stays in its region)

Status: built · tested: host:redoubt-blkd::a_broken_device_is_never_handed_a_clients_write_payload, host:redoubt-blkd::a_hundred_thousand_random_liars_never_panic_and_never_stray, host:redoubt-blkd::impossible_requests_never_reach_the_device

`blkd` never asks the device to touch anything but the pages `dma_alloc` gave it: every address it
gives the device is the region's base plus a constant checked to lie inside it, and a client's lend
is never named. On a platform whose hardware confines DMA this bounds the device; without it, it
bounds what `blkd` asks for, not what the device does.

### R52 (a lie is a failure, never corruption)

Status: built · tested: fuzz:redoubt-blkd/device, host:redoubt-blkd::rewriting_the_rings_changes_nothing_the_driver_believes, host:redoubt-blkd::a_device_that_lies_about_a_completion_is_refused_and_never_spoken_to_again, host:redoubt-blkd::a_lying_device_becomes_failed_and_stays_failed, host:redoubt-blkd::a_hundred_thousand_random_liars_never_panic_and_never_stray, host:redoubt-blkd::one_clients_read_never_carries_anothers_bytes

Nothing a device writes can corrupt `blkd`'s memory, panic it, or make it return one client's bytes
to another: no device value is an index or a length, the rings `blkd` writes are never read back,
the three used-ring values it reads are checked, and a device that breaks the protocol is refused
for good.

### R53 (a filesystem sees only its partition)

Status: built · tested: fuzz:redoubt-blkd/gpt, host:redoubt-blkd::a_range_cannot_name_a_sector_outside_itself, host:redoubt-blkd::hostile_partition_entries_are_refused, host:redoubt-blkd::a_badge_that_names_no_range_is_refused, host:redoubt-blkd::a_gap_in_the_table_does_not_renumber_the_volumes_after_it

A client's badge names one partition, its sector numbers are relative to it, and no partition table
with overlapping entries is accepted. So no client can read or write a sector outside its own
partition, and no two partitions share one.

## Failure and restart

Status: built · partly tested: the restart itself is `init`'s and planned; the device reset before reused DMA pages is the kernel's · tested: host:redoubt-blkd::a_lying_device_becomes_failed_and_stays_failed, host:redoubt-blkd::a_disk_with_no_signature_is_refused

- **No device handles, or no disk, or no valid partition table:** `blkd` exits with a code
  before serving.
- **The device lies or times out:** every later request gets `failed` until `blkd` is restarted,
  which resets the device from the beginning.
- **`blkd` restarts:** it reads the same table and gives the same badges the same ranges, holding
  nothing across the restart. Its DMA pages are never reused while the device can still reach them:
  the kernel resets the device before it frees them
  ([I16 (DMA pages reset before reuse)](../kernel/invariants.md#i16-dma-pages-reset-before-reuse)).

## Residual risks

- **`blkd` is trusted without an IOMMU.** On QEMU no hardware confines DMA, so a compromised `blkd`,
  or a device that ignores its addresses, can write anywhere in RAM: a compromised kernel.
- **A slow or lying device holds the disk** for up to `REQUEST_TIMEOUT_US` (10 seconds) before it is
  marked broken; with one request outstanding, every client waits meanwhile.
- **Wrong bytes are not detected.** A device that returns wrong data within the protocol is believed;
  neither `blkd` nor littlefs checksums data. Disk encryption with authentication is beyond M5.
- **`blkd` does not boot in the bench.** It is attacked by host tests against a hostile fake device
  (`blkd-host-tests`); `blkd-build` only builds it for both widths.

## Why

- **Our own virtqueue.** The existing Rust virtio crate is thirteen thousand lines of device classes
  Redoubt does not have, behind six dependencies, `unsafe` at every call site; the queue and register
  code here is about five hundred lines, and with one request outstanding there is no descriptor
  state to shadow.
- **Copy through a DMA buffer.** Naming a client's lend to the device would let the device, or a
  race, reach memory the client still holds; a copy makes the device's reach one fixed region.
- **Badges by entry position.** A restarted `blkd` means the same thing by the same badge without
  storing anything, and a gap in the table renumbers nothing.
- **Broken stays broken.** A device that lied once may lie again in a way no check catches;
  refusing it until a reset is the closed failure.
