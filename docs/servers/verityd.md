# verityd

`verityd` makes a volume verified. It sits between [`blkd`](blkd.md) and the volume's file server,
[`erofsd`](erofsd.md) for the read-only system volume or a [`littlefsd`](littlefsd.md): it holds the
volume's range at `blkd`, checks every block it reads against a hash tree whose root the signed boot
manifest pins, or that the volume's own root block gives, signed under a key the manifest names, and
serves only blocks that check, on `blkd`'s own protocol. So everything the file server parses on a
verified volume, data and metadata alike, is what the image's builder wrote.

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
- One mode ([below](#the-root-block-and-the-two-modes)):
  - pinned: `root=<64 lowercase hex>`, the root the manifest pins, and `blocks=N`, the volume's
    data blocks;
  - signed: `key=<64 lowercase hex>`, the Ed25519 key the volume's root block is signed under, and
    `floor=N`, the lowest version it may carry.
- One named handle, `volume`: the volume's range at its disk's `blkd`.

Anything else, any of them twice, both modes or a part of one, or a block count of 0 stops it before
it serves. It serves one badge, 1, the one `init` mints at its endpoint for the volume's file
server; any other gets `not_permitted`.

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

### The root block, and the two modes

<details><summary>Status: built · tested (11)</summary>

- bench:verity-signed
- bench:verity-bad-signature
- bench:verity-rollback
- host:redoubt-verity::a_root_block_is_its_documented_bytes
- host:redoubt-verity::a_malformed_root_block_is_refused
- host:redoubt-verity::arbitrary_bytes_are_one_root_block_or_none
- host:redoubt-verity::the_root_block_is_the_last_whole_block
- host:redoubt-verityd::a_signed_volume_opens_from_its_root_block_at_or_above_its_floor
- host:redoubt-verityd::a_root_block_that_does_not_verify_is_refused_naming_the_signature
- host:redoubt-verityd::a_root_block_below_the_floor_is_refused_naming_the_version
- host:testbench::a_signed_partition_ends_in_its_root_block_signed_deterministically

</details>

A volume is checked against one of two things, which its manifest entry's `verity` states
([init](init.md#the-boot-manifest)):
- **Pinned:** the manifest gives the root and N. The volume changes only with the bundle.
- **Signed:** the manifest gives a key and a floor, and the volume carries its own N and root in a
  root block, so it can be updated apart from the bundle.

**The root block** is the last whole block of the volume's range, after the tree, defined once in
`libs/verity` (`RootBlock`):
- a magic, `RVOLROOT`; N and a version, each a `u64` little-endian; the root; an Ed25519 signature
  of 64 bytes; then zeros to the end of the block. Any other byte there is malformed, so a block
  has one reading.
- The signature covers the preimage `redoubt_signing::volume_preimage` builds, under the domain
  `"redoubt.volume.v1\0"`, built as the bundle's is: the domain, a fixed-width length, then N,
  the version and the root ([boot](../kernel/boot.md#verified-boot)).
- It is checked with `ed25519-compact`, the loader's and `keyd`'s crate at the same version: one
  Ed25519 on the box.

**At start, signed,** `verityd` reads the root block, parses it, verifies the signature under
`key=`, and refuses the volume if the version is below `floor=`, a rollback; nothing in the block
is used before its signature verifies. Then it takes N and the root from the block and goes on as
pinned: the data and tree must end before the root block, and the top tree block must hash to the
root. A refusal names its reason in the one line [below](#starting): `its root block is
malformed`, `its root block's signature does not verify under its key`, or `its root block's
version 1 is below the floor 2`. A refused signed volume, whose N is not believed, answers `info`
with its range's whole blocks before the root block.

**The key** is the manifest's: 64 hex digits, or `bundle`, the key the loader verified the
bundle with, named so no copy of it can drift. **The floor** is the
manifest's too, so it comes and goes with the bundle ([below](#residual-risks)).

**Signing is on the build host only** ([R35 (key separation)](init.md#r35-key-separation)): the
bench's packer signs a recipe's `sign = { key = PATH, version = N }` with the 32-byte seed in
`PATH` ([the bench](../testbench.md#disks-and-network-cards)). The device never holds the key.

### Starting

<details><summary>Status: built · tested (4)</summary>

- bench:verity-wrong-root
- bench:verity-flipped-tree
- host:redoubt-verityd::a_truncated_range_is_refused_and_still_sized
- host:redoubt-verityd::a_wrong_root_or_top_is_refused

</details>

At start `verityd` calls `info` at `blkd`, for a signed volume checks its root block
([above](#the-root-block-and-the-two-modes)), refuses a range shorter than the data blocks and their
tree, reads the top tree block and checks it against the root. If any of that fails, it says one
line on its console naming the reason (`verityd: the volume is refused: ...`), answers `info`
truthfully, answers every `read` with `failed`, and stays up. So a bad medium is never a restart
loop, and the volume's file server, whose first read then fails, serves the volume as corrupt
([R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)) and stays up too. Answering `info`
matters: a file server that cannot size its range exits, and would be restarted.

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

`verityd` serves [`blkd`'s protocol](blkd.md#messages), unchanged, so a file server cannot tell it from a
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

<details><summary>Status: built · tested (3)</summary>

- bench:boot-profile
- bench:boot-profile-unverified
- host:redoubt-verityd::the_last_block_and_the_tree_cache_save_reads

</details>

- **Memory is fixed,** whatever the volume's size: the top block, pinned at start; a cache of 32
  checked tree blocks and one of 4 checked data blocks, each least recently used out first
  ([below](#a-cache-of-checked-data-blocks)); and a 2-page lend at `blkd`.
- **A block is checked from the top down.** The lowest block on its path already held (the top
  always is) gives the digest the next block down must hash to; a block fetched is checked before
  it is kept. A level-1 block held costs one hash per data block.
- **Per block the file server reads:** one more call and a copy of at most 4 KiB. The tree's reads
  are about 1/128 more, mostly cached. Its weight is ordinary, like the file servers': each request
  is a bounded amount of work, at most 8 blocks.
- **Measured** by bench:boot-profile and bench:boot-profile-unverified: what verification costs
  the boot, verified against unverified, on littlefs and on EROFS, is
  [beamlet's table](../userland/beamlet.md#beamlet-on-redoubt).
- **A signed volume costs one Ed25519 verification at start:** `verityd`'s start on
  `verity-signed`'s volume, root block and top block read, takes about 140 ms in guest time on
  either width under `icount` (`shift=3`).
- **The verifier's code is built for size.** `ed25519-compact`, the box's one Ed25519, is built at
  `opt-level = "z"` (the root `Cargo.toml`): at the workspace's `"s"` its verification inlines
  into about 900 KB on rv32, more than a verifier's budget holds; at `"z"` it is about 20 KB, and
  the check is slower for it (the loader's bundle check by under 7% per byte).
- **Read-ahead is not done** ([below](#a-cache-of-checked-data-blocks)).

### A cache of checked data blocks

<details><summary>Status: built · tested (2)</summary>

- bench:boot-profile
- host:redoubt-verityd::the_data_cache_holds_the_last_blocks_least_recently_used_out

</details>

The last 4 data blocks checked are kept, least recently used out first, in place of the one
last-block buffer: a block asked for again while it is held is neither read nor hashed again.
On EROFS a file's blocks are read once each, in order, but `erofsd` reads the same few blocks
again and again: a directory's blocks at every walk, and the blocks holding the inodes. By its
512th read in the rv64 boot profile (boot-stats build, seed 1) `verityd` was asked for 849 data
blocks: one block held 100 of them (12 %), 4 hold 197 (23 %) and 8 would hold 289 (34 %). 4
blocks, 16 KiB, are what is built.

Read-ahead (8 blocks per `blkd` call, hashed and cached) is not built: `erofsd` already reads a
file's blocks in one call of up to 64 sectors, which `verityd` splits into block reads at `blkd`.

## Authority

Status: built · tested: host:redoubt-verityd::only_the_volumes_badge_is_served, host:redoubt-verityd::writes_are_refused_and_flush_answers_at_once

- `verityd` holds one range at `blkd`, its endpoint, and a console connection. It holds no MMIO,
  interrupt or DMA, mints nothing, and never writes its range.
- The volume's file server holds the one badge it serves.
- The crate forbids `unsafe`.

## Security properties

### R76 (verified volumes)

<details><summary>Status: built · tested (13)</summary>

- bench:userland-boot
- bench:userland-bad-start
- bench:verity-flipped-tree
- bench:verity-wrong-root
- bench:verity-bad-signature
- bench:verity-rollback
- host:redoubt-verityd::a_root_block_that_does_not_verify_is_refused_naming_the_signature
- host:redoubt-verityd::a_root_block_below_the_floor_is_refused_naming_the_version
- host:redoubt-verity::a_flipped_bit_at_each_level_is_refused
- host:redoubt-verityd::a_wrong_root_or_top_is_refused
- host:redoubt-verityd::a_mismatch_is_failed_and_said_naming_the_block
- host:redoubt-verityd::a_failed_block_is_never_kept
- host:redoubt-verityd::arbitrary_media_never_panic

</details>

A reader of a verified volume sees only blocks that hash, through the tree, to the root the signed
manifest gives, or, for a signed volume, to the root its root block gives, signed under the
manifest's key at a version no lower than the manifest's floor. Otherwise it sees a device failure,
which the file server serves as `corrupt`. `erofsd` and `littlefsd` both poison a volume on an I/O
error until they start or mount it again, so one bad block fails closed for the whole volume: later
loads fail too, and a reader keeps what it already has.

## Failure and restart

Status: built · partly tested: the exits on bad arguments and on no `volume` handle (`BAD_ARGS`, `NO_VOLUME` in `servers/verityd/src/bin/verityd.rs`) are read from the code, not attacked; no case restarts `verityd` · tested: host:redoubt-verityd::a_truncated_range_is_refused_and_still_sized

- **Bad arguments or no `volume` handle:** `verityd` exits with a code before serving.
- **The start check fails:** it stays up, refused, as above.
- **A read does not check:** that read is `failed`; `verityd` keeps serving, and the file server
  poisons the volume.
- **`verityd` restarts:** it checks the range again from the root, holding nothing across the
  restart.

## Residual risks

- **Rollback is the bundle's.** The root, or a signed volume's floor, comes with the signed
  manifest, so an older bundle brings its older volume, or a lower floor, back with it;
  [boot](../kernel/boot.md) has no rollback protection, and this is no worse. Nothing on the box
  raises a floor: a higher one arrives with a new bundle, and a monotonic store belongs to
  [M5 (persist, install, share)](../plan/m5-persist.md).
- **A root block names no volume.** Its signature covers N, the version and the root, not which
  volume it is, so two volumes signed under one key at versions over the floor can stand in for
  each other on a disk. A deployment gives each signed volume a key of its own, until a later
  version of the domain signs a name.
- **Writable volumes are unverified.** They keep littlefs's metadata CRC and
  [R49](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash) only.
- **A reader trusts the file server and `verityd`.** Under R76 the reader of a verified volume trusts the
  servers that verify it, as it trusts `consoled` for its console; neither checks the other's
  answers beyond the protocol.
- **One bad block fails the volume** until its file server starts again: detection, not correction.

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
