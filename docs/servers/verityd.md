# verityd

`verityd` makes a volume verified. It sits between [`blkd`](blkd.md) and the volume's
[`littlefsd`](littlefsd.md): it holds the volume's range at `blkd`, checks every block it reads against a hash
tree whose root the signed boot manifest pins, and serves only blocks that check, on `blkd`'s own
protocol. So everything littlefs parses on a verified volume, data and metadata alike, is what the
image's builder wrote.

## Purpose

The userland disk holds code: the modules and application resources the system resolves by name.
The disk itself is not signed, and a hostile or damaged medium must not be able to change what
runs. The boot bundle is signed ([R15 (verified boot)](../kernel/boot.md#r15-verified-boot)); `verityd`
extends that signature to a whole volume, one block at a time, so a reader of the volume needs no
check of its own. It is one small process holding one range and no device, written so it can be
read whole.

## Interface

### Arguments

<details><summary>Status: built · tested (2)</summary>

- host:redoubt-verityd::arguments_are_inits_and_nothing_else
- host:redoubt-verityd::only_the_volumes_badge_is_served

</details>

`init` gives every argument ([init](init.md#the-boot-manifest), the `volumes` entry's `verity`
key); a manifest that sets one itself is refused.
- `endpoint=NAME`, the manifest's name of the endpoint it receives on (`verity:system`).
- `labels=ID[,ID...]`, the volume's label set, absent when it is empty.
- `root=<64 lowercase hex>`, the root the manifest pins, and `blocks=N`, the volume's data blocks.
- One named handle, `volume`: the volume's range at its disk's `blkd`.

Anything else, any of them twice, or a block count of 0 stops it before it serves. It serves one
badge, 1, the one `init` mints at its endpoint for the volume's `littlefsd`; any other gets
`not_permitted`.

### The tree

<details><summary>Status: built · tested (5)</summary>

- host:redoubt-verity::a_known_vector
- host:redoubt-verity::every_level_boundary
- host:redoubt-verity::the_geometry_refuses_no_blocks_and_overflow
- host:redoubt-verity::a_flipped_bit_at_each_level_is_refused
- host:redoubt-verity::the_root_pins_the_block_count

</details>

One definition, `libs/verity`, shared by the packer that writes the tree and `verityd` that checks
it:
- A volume of N data blocks of 4096 bytes (littlefs's block, eight sectors) is followed in its
  range by its tree.
- Level 1 holds one digest per data block, `SHA-256(0x00 ‖ block)`. Each level above holds one
  digest per block of the level below, `SHA-256(0x01 ‖ block)`. A tree block holds 128 digests,
  zero-filled after the last. Levels are stored bottom-up, the top level one block.
- The root is `SHA-256(0x02 ‖ N as u64 LE ‖ top block)`, so it pins the block count too.

The three prefixes keep the kinds apart: no data block can stand for a tree block, nor either for
the root. The geometry is overflow-checked arithmetic in one function. A tree covers every bit of
its volume, so any error in data or tree is detected, which is stronger than a CRC; nothing
corrects one.

### Starting

<details><summary>Status: built · tested (4)</summary>

- bench:verity-wrong-root
- bench:verity-flipped-tree
- host:redoubt-verityd::a_truncated_range_is_refused_and_still_sized
- host:redoubt-verityd::a_wrong_root_or_top_is_refused

</details>

At start `verityd` calls `info` at `blkd`, refuses a range shorter than the data blocks and their
tree, reads the top tree block and checks it against the root. If any of that fails, it says one
line on its console naming the reason (`verityd: the volume is refused: ...`), answers `info`
truthfully, answers every `read` with `failed`, and stays up. So a bad medium is never a restart
loop, and `littlefsd`, whose mount then fails, serves the volume as corrupt
([R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)) and stays up too. Answering `info`
matters: a `littlefsd` that cannot size its range exits, and would be restarted.

### Messages

<details><summary>Status: built · tested (8)</summary>

- bench:userland-boot
- bench:userland-read-only
- host:redoubt-verityd::sub_block_and_multi_block_reads_return_the_volume
- host:redoubt-verityd::a_mismatch_is_failed_and_said_naming_the_block
- host:redoubt-verityd::a_failed_block_is_never_kept
- host:redoubt-verityd::writes_are_refused_and_flush_answers_at_once
- host:redoubt-verityd::the_label_check_runs_on_every_request
- host:redoubt-verityd::arbitrary_media_never_panic

</details>

`verityd` serves [`blkd`'s protocol](blkd.md#messages), unchanged, so `littlefsd` cannot tell it from a
`blkd`:
- **`info`**: N × 8 sectors, read-only. The tree is not part of the volume.
- **`read`** works in whole blocks. It fetches each block the request touches, hashes it, checks
  it through the tree up to a block already checked, and only then copies the requested sectors
  out. A mismatch is `failed`, as `blkd` answers a device error, with one console line naming the
  block (`verityd: block 200 does not match the tree`, or `tree block ...`). A block that failed is
  never kept: the next read checks it again.
- **`write`** is `not_permitted`, as `blkd` refuses one on a read-only disk; **`flush`** answers at
  once.
- **The label check** ([R25 (the label check)](serving.md#r25-the-label-check)) runs on every request against
  `labels=`, as `blkd`'s does: `info` and `read` are reads, `write` and `flush` writes.

### Memory and cost

<details><summary>Status: built · tested (1)</summary>

- host:redoubt-verityd::the_last_block_and_the_tree_cache_save_reads

</details>

- **Memory is fixed,** whatever the volume's size: the top block, pinned at start; a cache of 32
  checked tree blocks, least recently used out first; the last checked data block, so sub-block
  reads within it hash once; and a 2-page lend at `blkd`, as `littlefsd`'s.
- **A block is checked from the top down.** The lowest block on its path already held (the top
  always is) gives the digest the next block down must hash to; a block fetched is checked before
  it is kept. A level-1 block held costs one hash per data block.
- **Per block `littlefsd` reads:** one more call and a copy of at most 4 KiB. The tree's reads are about
  1/128 more, mostly cached. Its weight is ordinary, like `littlefsd`'s: each request is a bounded
  amount of work, at most 8 blocks.
- **Measured** on rv64 under QEMU, the image booted to its prompt and `Enum.sum(1..10)`: 172.9 s
  through `verity:system`, 102.9 s with the same volume attached to `littlefsd` directly. By then `littlefsd`
  had made 65,536 reads, for which `verityd` checked 51,312 data blocks; level-1 blocks were held
  for 99.96 % of them. littlefs reads a block in pieces and alternates between blocks, so the
  last-block buffer saved 22 % of the reads, and each of the ~1,950 blocks the boot loads was
  fetched and hashed about 26 times. The tree is not the cost; the repeated data blocks are.
- **Read-ahead is not done** ([below](#a-cache-of-checked-data-blocks)).

### A cache of checked data blocks

Status: planned · M1 (separation and containment)

A few checked data blocks, least recently used out first, beside the tree cache, would hash a
block `littlefsd` reads in pieces once, and read-ahead (8 blocks per `blkd` call, hashed and cached)
would cut the calls at `blkd` on sequential reads. Both are local to `verityd` and change no
protocol; the measurement above says the timing asks for one.

**Open:** a few-block LRU of checked data blocks, read-ahead, or both, and how many blocks of fixed
memory: a follow-up, with the signed root or on its own.

## Authority

Status: built · tested: host:redoubt-verityd::only_the_volumes_badge_is_served, host:redoubt-verityd::writes_are_refused_and_flush_answers_at_once

- `verityd` holds one range at `blkd`, its endpoint, and a console connection. It holds no MMIO,
  interrupt or DMA, mints nothing, and never writes its range.
- The volume's `littlefsd` holds the one badge it serves.
- The crate forbids `unsafe`.

## Security properties

### R76 (verified volumes)

<details><summary>Status: built · tested (9)</summary>

- bench:userland-boot
- bench:userland-bad-start
- bench:verity-flipped-tree
- bench:verity-wrong-root
- host:redoubt-verity::a_flipped_bit_at_each_level_is_refused
- host:redoubt-verityd::a_wrong_root_or_top_is_refused
- host:redoubt-verityd::a_mismatch_is_failed_and_said_naming_the_block
- host:redoubt-verityd::a_failed_block_is_never_kept
- host:redoubt-verityd::arbitrary_media_never_panic

</details>

A reader of a verified volume sees only blocks that hash, through the tree, to the root the signed
manifest gives. Otherwise it sees a device failure, which `littlefsd` serves as `corrupt`. `littlefsd` already
poisons a volume on an I/O error until it is next mounted, so one bad block fails closed for the
whole volume: later loads fail too, and a reader keeps what it already has.

## Failure and restart

Status: built · partly tested: the exits on bad arguments and on no `volume` handle (`BAD_ARGS`, `NO_VOLUME` in `servers/verityd/src/bin/verityd.rs`) are read from the code, not attacked; no case restarts `verityd` · tested: host:redoubt-verityd::a_truncated_range_is_refused_and_still_sized

- **Bad arguments or no `volume` handle:** `verityd` exits with a code before serving.
- **The start check fails:** it stays up, refused, as above.
- **A read does not check:** that read is `failed`; `verityd` keeps serving, and `littlefsd` poisons the
  volume.
- **`verityd` restarts:** it checks the range again from the root, holding nothing across the
  restart.

## Residual risks

- **Rollback is the bundle's.** The root comes with the signed manifest, so an older bundle brings
  its older volume back with it; [boot](../kernel/boot.md) has no rollback protection, and this is
  no worse.
- **Writable volumes are unverified.** They keep littlefs's metadata CRC and
  [R49](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash) only.
- **A reader trusts `littlefsd` and `verityd`.** Under R76 the reader of a verified volume trusts the
  servers that verify it, as it trusts `consoled` for its console; neither checks the other's
  answers beyond the protocol.
- **One bad block fails the volume** until `littlefsd` mounts it again: detection, not correction.

## Why

- **The block layer, not littlefs.** To find a signed file inside littlefs, `littlefsd` would parse the
  volume's metadata before anything checked it; once a reader takes code from the volume,
  [R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance)'s bound to "that volume's
  data" means nothing. Below
  `littlefsd`, every byte littlefs parses has been checked, and one tree covers data and metadata.
- **Not 9P above `littlefsd`.** `littlefsd` would still parse unverified metadata; a partial read could be
  checked only against a per-file tree; and listings, `stat` and walks would go unchecked.
- **A server, not a layer inside `littlefsd`.** The check runs in its own small process, `littlefsd` and
  littlefs stay untouched, and any client of `blkd`'s protocol can be verified. Its cost is one call
  and a copy per block and one process per verified volume; a layer in `littlefsd` would save the call
  and stays possible if the cost says so.
