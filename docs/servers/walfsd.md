# walfsd

## Purpose

`walfsd` is to serve a **writable volume** on the SSD in walfs, a write-ahead-log file system of
Redoubt's own: a superblock, a log of whole blocks, a table of inodes, a block bitmap, and a
SHA-256 for every block, checked on every read. littlefs ([littlefsd](littlefsd.md)) is built
for raw flash, with wear levelling and erase units an SSD does not need, and it checksums no
data; walfs makes every operation one transaction, data and metadata together, and finds a
damaged block where it is read. ext2, the alternative, has a host tool chain to test against
but no journal and no checksums, so power loss and a hostile medium would be met by rules on
top rather than by the format.

## Interface

### The format

<details><summary>Status: built · tested (35)</summary>

- bench:walfs-host-tests
- fuzz:walfs/image
- fuzz:walfs/mutate
- host:walfs::a_damaged_hash_block_is_refused_as_itself
- host:walfs::a_directory_sized_as_the_largest_file_is_corrupt_at_its_first_hole
- host:walfs::a_forged_log_header_is_corrupt
- host:walfs::a_forged_orphan_list_ends
- host:walfs::a_forged_size_reads_as_holes_and_allocates_nothing
- host:walfs::an_indirect_entry_outside_the_data_region_is_corrupt
- host:walfs::an_orphan_open_at_unmount_is_freed_at_mount
- host:walfs::a_reserved_bitmap_bit_clear_fails_the_mount
- host:walfs::a_torn_log_is_dropped_or_replayed_never_half_applied
- host:walfs::a_volume_reports_its_data_blocks_and_inode_count
- host:walfs::bad_arguments
- host:walfs::crash_at_every_write_fixed_workload
- host:walfs::crash_at_every_write_random_workloads
- host:walfs::crash_during_recovery
- host:walfs::crash_inside_a_write_of_many_transactions_leaves_a_prefix
- host:walfs::crash_while_freeing_a_large_file
- host:walfs::cycles_and_shared_blocks_are_found
- host:walfs::directories_grow_past_their_direct_blocks
- host:walfs::every_bad_attribute_area_is_corrupt
- host:walfs::every_bad_directory_entry_is_corrupt
- host:walfs::every_flipped_bit_is_corrupt_where_it_is_read
- host:walfs::every_inode_field_out_of_range_is_corrupt
- host:walfs::every_superblock_field_out_of_range_is_corrupt
- host:walfs::full_volume
- host:walfs::generations_and_mtimes
- host:walfs::handles_follow_renames_and_outlive_removal
- host:walfs::large_and_sparse_files_free_every_block
- host:walfs::noise_never_panics
- host:walfs::random_operations_crowded_small_volume
- host:walfs::random_operations_large_volume
- host:walfs::random_operations_small_volume
- host:walfs::the_records_are_the_pages_tables

</details>

`libs/walfs` implements this section and nothing beyond it: a field this page does not have is
a field the crate does not have. Its shape is xv6's (the MIT xv6-riscv tree, `kernel/fs.h` and
`kernel/log.c`): a superblock, a redo log of whole blocks, fixed inodes with direct and
indirect blocks, directories of fixed entries, and a bitmap. The layout is Redoubt's: 4 KiB
blocks, a hash per block, 64-bit file sizes, user attributes. Every integer is little-endian;
a block address is a `u32`, so a volume holds at most 2³² blocks (16 TiB).

#### The constants

| Name | Value | What it fixes |
| --- | --- | --- |
| `BLOCK` | 4096 | bytes per block, the SSD's and the kernel's page |
| `LOG_BLOCKS` | 32 | the most blocks one transaction writes |
| `LOG` | 33 | the log's blocks: the header and `LOG_BLOCKS` |
| `INODE` | 128 | bytes per inode; 32 inodes per block |
| `ATTRS` | 256 | bytes of user attributes per inode; 16 areas per block |
| `HASH` | 32 | bytes per SHA-256 |
| `HASH_SLOTS` | 127 | slots per hash block, whose last `HASH` bytes are its own hash |
| `DIRENT` | 260 | bytes per directory entry; 15 entries per block |
| `NAME_MAX` | 255 | bytes in a name |
| `DIRECT` | 12 | direct block addresses per inode |
| `PER_INDIRECT` | 1024 | block addresses per indirect block |
| `ROOT` | 1 | the root directory's inode number |

A file holds at most 12 + 1024 + 1024² = 1,049,612 blocks, **4,299,210,752 bytes** (4 GiB and
a little over); a write or truncate past that is `FileTooBig`, and a size past it on the medium
is corrupt.

#### Geometry

A volume of `block_count` blocks with `inode_count` inodes is laid out in this order, each
region starting where the one before it ends:

| Region | First block | Blocks |
| --- | --- | --- |
| superblock | 0 | 1 |
| log | 1 | `LOG`: the header, then the logged blocks |
| inode table | 1 + `LOG` = 34 | `inode_count` / 32 |
| attribute table | after the inodes | `inode_count` / 16 |
| hash region | after the attributes | ⌈(`block_count` − `LOG`) / `HASH_SLOTS`⌉: a slot for every block but the log's |
| bitmap | after the hashes | ⌈`block_count` / 32768⌉ |
| data | after the bitmap | the rest, at least one block |

`inode_count` is a multiple of 32, at least 32. Everything in the superblock but
`block_count` and `inode_count` follows from those two, and mount refuses a superblock whose
other fields differ from what they give, or whose `block_count` differs from the device's. A
block address read from the medium outside the region it must lie in (a file's or directory's
block, or an indirect block, outside the data region; a log entry outside the inode table,
attribute table, bitmap and data region) is corrupt.

#### The superblock

Block 0, written by `format` and never again.

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 8 | magic, `walfs\0\0\0` |
| 8 | 4 | version, 1 |
| 12 | 4 | block size, 4096 |
| 16 | 4 | `block_count` |
| 20 | 4 | the log's first block, 1 |
| 24 | 4 | the log's length in blocks, `LOG` |
| 28 | 4 | the inode table's first block, 34 |
| 32 | 4 | `inode_count` |
| 36 | 4 | the attribute table's first block |
| 40 | 4 | the hash region's first block |
| 44 | 4 | the bitmap's first block |
| 48 | 4 | the data region's first block |
| 52 | 4 | the root's inode number, 1 |
| 56 | 4008 | zero |
| 4064 | 32 | SHA-256 of bytes 0 to 4063 |

Mount checks the superblock's own hash first, then its fields, then recovers the log, and only
then checks the block against its slot, in a hash region as the last committed transaction left
it.

#### The log

One transaction at a time, of at most `LOG_BLOCKS` blocks. The log's first block is the
**header**; logged block *k* (from 0) is kept at log block 1 + *k*.

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 8 | magic, `walfslog` |
| 8 | 4 | commit: 0 for an empty log, 1 for a committed transaction |
| 12 | 4 | count of logged blocks, at most `LOG_BLOCKS`; 0 when commit is 0 |
| 16 | 32 × 36 = 1152 | `LOG_BLOCKS` entries of 36 bytes, one per logged block: its home address (4), then the SHA-256 of its 4096 bytes (32) |
| 1168 | 2896 | zero |
| 4064 | 32 | SHA-256 of bytes 0 to 4063 |

A transaction is written in this order, with the device's `sync` at each step:

1. every logged block to its place in the log; `sync`;
2. the header with commit 1 and the blocks' addresses and hashes: **the commit**; `sync`;
3. every block to its home address; `sync`;
4. the header with commit 0 and count 0; `sync`.

**Recovery** at mount reads the header. If its own hash fails, the header was torn by a power
cut in step 2 (nothing went home yet) or step 4 (everything already had), so either way the
transaction is dropped and an empty header written. If it holds a committed transaction, each
logged block is read and checked against its hash in the header, each home address against the
regions, and no address may appear twice; a block that fails is corrupt, since step 1's `sync`
put every logged block on the medium before the header that names it. The blocks are copied
home and the header emptied, as steps 3 and 4. A second cut inside recovery leaves the same
committed header, and copying home again writes the same blocks: recovery is repeated, not
undone.

The `sync` between steps is all the format asks of the device: no order between writes inside a
step, and a torn block write may leave the block in any state (its hash, in the header or the
hash region, catches it). [`blkd`](blkd.md)'s `flush` returns only when the device has made
everything before it durable, which is this `sync`; the host's packer writes a RAM image, where
`sync` does nothing.

A transaction's blocks are home in the inode and attribute tables, the hash region, the bitmap
and the data region; never the superblock or the log. File data goes through the log as
metadata does, so a block is overwritten in place inside a transaction, never copied
elsewhere.

#### Inodes

Inode *i* is at byte (*i* mod 32) × 128 of block `inode_start` + *i* / 32.

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 2 | kind: 0 free, 1 file, 2 directory |
| 2 | 2 | nlink: 1 while a directory entry names it, 0 once removed |
| 4 | 4 | the next inode on the orphan list, 0 for none |
| 8 | 8 | size in bytes |
| 16 | 8 | generation |
| 24 | 8 | mtime, microseconds since the Unix epoch; 0 when the writer has no clock |
| 32 | 4 × 12 | direct block addresses |
| 80 | 4 | the single-indirect block |
| 84 | 4 | the double-indirect block |
| 88 | 40 | zero |

- A block address of 0 is a **hole**, read as zeros; block 0 is the superblock, so no data
  block has that address. An indirect block holds 1024 addresses, every one a hole or in the
  data region. Bytes past the size in the last block are zero, and no address is set past the
  last block the size reaches.
- A **directory's** size is a whole number of blocks, with no holes.
- The **generation** counts the inode's allocations: a free inode keeps the generation it had,
  every other field zero, and allocating it adds 1. A server tells a removed file from a later
  one in the same inode by the pair.
- **Inode 0** is never allocated: its record is zero but for the next-inode field, the head of
  the orphan list. Inode 1 is the root directory, always allocated.
- There are no hard links (9P2000 has none): nlink is 0 or 1, and anything else is corrupt.

**The orphan list** holds the inodes whose blocks are still to be freed, linked from inode 0
through the next-inode field, each at most once (a walk longer than `inode_count` is corrupt).
An inode on it with nlink 0 was removed while open, or its freeing was cut short: all of its
blocks are freed, then the inode. One with nlink 1 was truncated: its blocks past its size are
freed. Mount finishes the list; an open file that is removed stays on it until its last handle
closes. So removing or truncating a file of any size is one transaction, seen before or after,
though freeing its blocks may take several. Between the two, a reader sees the volume as after
the operation in everything but space: the blocks still to be freed are not yet free for
another write, and nothing else differs. A mount between them finishes the freeing before it
serves anything, one transaction at a time from the list's head.

#### User attributes

Inode *i*'s attributes are 256 bytes at byte (*i* mod 16) × 256 of block `attr_start` + *i* /
16: records of a type (1 to 255, one byte), a length (one byte) and that many bytes of value,
one record per type, ended by a type of 0 or the area's end; every byte after the end is zero.
A value is at most 254 bytes, and all of one inode's attributes share the 256. A free inode's
area is zero, and allocating an inode writes its area in the same transaction. Setting one is
a transaction of the area's block and its hash, so a file's attributes and its data are
changed in separate transactions, each whole.

#### Directories

A directory's blocks hold 15 entries each, at byte *k* × 260, then 196 zero bytes:

| Offset | Width | Field |
| --- | --- | --- |
| 0 | 4 | inode number; 0 for a free slot |
| 4 | 1 | the name's length, 1 to 255; 0 for a free slot |
| 5 | 255 | the name, zero past its length |

A free slot is zero throughout. A name is any bytes but `/` and 0, and neither `.` nor `..`; no
two entries of a directory have one name. Entries are unsorted, and a lookup or a listing reads
the directory's blocks in order: linear in its size, which suits the volumes' directories of
tens to hundreds of entries, not of tens of thousands. No `.` or `..` entries are stored, since
9P needs none. A new entry takes the first free slot, or a new block at the directory's end; a
directory never shrinks until it is removed.

#### The bitmap

One bit per block of the volume, block *b*'s at bit *b* mod 8 of byte *b* / 8 of the region: 1
in use, 0 free. The blocks before the data region are always in use, and so are the bits past
`block_count` in the last bitmap block. A data block's bit is 1 exactly when an inode, an
indirect block or a directory reachable from the root or the orphan list holds it: every
transaction that sets or clears an address sets or clears its bit with it. The mount reads the
bitmap whole. The **volume check** (`check`) walks every directory from the root and every inode
on the orphan list, reads every block they hold against its hash, reads every hash block, inode
and attribute area, and names each problem it finds: among them a block held with its bit 0,
one with its bit 1 that nothing holds, a block held twice, an inode named twice or held by
nothing.

#### The hash region

One SHA-256 slot for every block of the volume but the log's, `HASH_SLOTS` to a hash block, and
in each hash block's last `HASH` bytes (offset 4064) the SHA-256 of its first 4064. Slot *i* is
block *i*'s for the superblock (*i* = 0), and block *i* + `LOG`'s for the rest, from the inode
table on; it is at byte (*i* mod `HASH_SLOTS`) × `HASH` of block `hash_start` + *i* /
`HASH_SLOTS`. A slot holds the SHA-256 of its block's 4096 bytes. A transaction that writes a
block writes its slot, so the slot's hash block is in the same transaction, and its own hash is
written last.

A **read of any block is checked** before anything in it is parsed, and a mismatch is `Corrupt`
for that read
([R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)):
a hash block against its own hash, every other block against its slot, and a logged block
against the header that names it. The log has no slots because the header carries a hash for
every block it logs, and a transaction cannot write its own log blocks' slots in itself. A hash
block's own slot (its blocks are past the inode table, so they have one) is zero and never read:
it checks itself, so a damaged hash block is refused as that block, not as the blocks it
covers. A free block's slot is whatever it last held; nothing reads a free block.

This finds a damaged medium, and one that lies about a block: a bit flipped, a block torn or
lost, a block another block's bytes. It does not find a medium that rewrites a block and its
slot, and the slot's block's own hash, together to agree: it is not a signature, and not a tree
under a sealed root.

#### What is corrupt

Each of these is `Corrupt` where it is read, and nothing else is:

- **Any block** that fails its check: the superblock or the log's header against its own hash
  (a header that fails is a torn one and is dropped, not refused), a hash block against its own
  hash, a logged block against its header's entry, every other block against its slot.
- **The superblock:** a wrong magic, or any field, the version, the block size or a zero byte
  among them, other than what `block_count` and `inode_count` give; a `block_count` other than
  the device's.
- **The log's header,** its own hash holding: a wrong magic; a commit other than 0 or 1; a commit
  of 1 with a count of 0, or of 0 with a count above 0; a count above `LOG_BLOCKS`; a byte after
  the entries other than zero; a home outside the inode table, the attribute table, the hash
  region, the bitmap and the data region; a home twice.
- **The bitmap:** a bit 0 among the blocks before the data region or past `block_count`; freeing
  a block whose bit is already 0.
- **An inode:** a kind above 2; a free inode with any field but its generation set (inode 0: but
  its next-inode field); inode 0 allocated; an nlink above 1; a next-inode field past
  `inode_count`; a size past the largest file; a directory's size not whole blocks; an address
  neither a hole nor in the data region; a reserved byte other than zero. The root not an
  allocated directory with nlink 1 fails the mount.
- **The orphan list:** a free inode on it, or a walk longer than `inode_count`.
- **An indirect block:** an address neither a hole nor in the data region.
- **A directory:** a block that is a hole; a free slot with any byte set; an inode number of 0
  with a length, or a length of 0 with an inode number; an inode number past `inode_count`; a
  name the format does not allow, or bytes after it other than zero; a block's last 196 bytes
  other than zero; an entry naming a free inode or one with nlink 0; a name twice, where a lookup
  meets it.
- **An attribute area:** a record that runs past the area's end, a type twice, or a byte after
  the end other than zero.

#### Atomicity

The eight operations that change a volume are `create` (an `open` that creates), `write`,
`truncate` (an `open` that truncates, too), `mkdir`, `remove`, `rename` (over an existing file or
empty directory too), `set_attr` and `remove_attr`. Each is one transaction, data and metadata
together, and a power cut leaves the volume as it was before the operation or after it; a
`write` is one while it fits one transaction's blocks. A write adds a block of data to its
transaction only while 14 of the `LOG_BLOCKS` are left: the data block, a single- and a
double-indirect block and three of the bitmap's blocks, each with its hash block (12), and the
inode with its hash block (2). A larger
`write` is several transactions, each before or after, so a cut inside it leaves the file with
a prefix of the write that ends at a block boundary. Removing or truncating a file is one
transaction that the reader sees; the freeing of its blocks that follows may take more, from
the orphan list, and the next mount finishes it. A `write` that finds the volume full ends at
the last block there was room for and says how much it wrote.

#### Memory

The crate holds the bitmap whole (`block_count` / 8 bytes; 32 KiB for a 1 GiB volume), one
4 KiB buffer per block of the transaction being built (at most `LOG_BLOCKS`), the last hash
block read, a block of scratch for each block being read, and a few words per open handle. A
write goes to the medium in the call that makes it, so no handle holds a block.

#### The oracle

No second implementation exists to test against, so the crate is checked against a model, an
in-memory file system: random operations, valid or not, with handles held open across renames
and removals and volumes run full, each outcome compared with the model's and the volume
remounted, read whole and checked as it goes (`tests/model.rs`). The power-loss harness cuts
the device at every block write of fixed and random workloads, with every write since the last
`sync` landing whole, not at all or torn, and accepts only the model's state before or after the
operation, also when a second cut falls inside the recovery (`tests/crash.rs`). Hostile volumes
(noise, every block's bits flipped in turn, and forged structures whose hashes are made to agree)
must read as before or as corrupt where the damage is read (`tests/hostile.rs`). The bench runs
these as `walfs-host-tests`; the two fuzz targets, `image` (arbitrary bytes mounted, walked and
written) and `mutate` (a valid volume with bytes changed, its hashes forged or not), need
`cargo fuzz` and are run by hand from `libs/walfs`.

### The packer

<details><summary>Status: built · tested (1)</summary>

- host:testbench::a_walfs_partition_is_its_stage_and_two_packs_are_the_same_bytes

</details>

The bench's disk packer writes a walfs volume from a staged directory (`fs = "walfs"` in a disk
recipe), with `libs/walfs`'s own code on a RAM device: `format`, with an inode for every 16
blocks rounded up to whole inode blocks (or, if more, for every staged entry and inodes 0 and 1,
rounded up the same way), then, in the stage's order, a `mkdir` for each directory and a create
and a `write` for each file, each the transactions it takes. It is deterministic: the same tree gives the same bytes, with every mtime 0.

### Serving

Status: planned · M1 (separation and containment)

`walfsd` will serve walfs as `littlefsd` serves littlefs: one instance per volume, under a
`blkd` range, with the same 9P face, labels and typed operations.

**Open:** what a quota counts (blocks or bytes) and which attribute types `walfsd` serves.

## Authority

Status: planned · M1 (separation and containment)

`walfsd` will hold its own endpoint, one `blkd` range and the connections it mints, as
`littlefsd` does.

**Open:** none.

## Security properties

Status: planned · M1 (separation and containment)

`walfsd` is to keep, for its volumes, what `littlefsd` keeps today:
[R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance),
[R48 (a quota per attach root)](littlefsd.md#r48-a-quota-per-attach-root),
[R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)
and [R50 (power loss leaves before or after)](littlefsd.md#r50-power-loss-leaves-before-or-after),
each restated here when the server is built. The format above is what R49 and R50 will rest on.

**Open:** none beyond Serving's.

## Failure and restart

Status: planned · M1 (separation and containment)

A cut transaction is recovered at mount, so a restarted `walfsd` serves the volume as of its
last committed transaction; a corrupt volume will be served as corrupt, not an exit.

**Open:** none.

## Residual risks

- **The hash is not a signature.** A medium that rewrites a block and its slot together is not
  caught; a hash tree with a sealed root would be, at a tree update per write, and is not part
  of the format.
- **A header damaged after a cut is a dropped transaction.** A committed header whose own hash
  fails reads as a commit torn by power loss, so the transaction it held is lost rather than
  refused as corrupt. The blocks it would have written are left as they were, each still
  checked by its slot.
- **A directory lookup is linear** in the directory's size.
- **No wear levelling:** an SSD levels its own wear; walfs on raw flash would wear its log and
  hash region first. littlefs stays the format for raw flash.
- **A large write is several transactions.** Each is before or after, so a power cut inside a
  write of more than one transaction's blocks leaves a prefix of it.

## Why

A format of Redoubt's own gives the volumes what littlefs does not on an SSD: every operation
one transaction, data with metadata, and every block checked where it is read. With no second
implementation to test against, the oracle is a model: an in-memory file system driven by
random operations, a power-loss harness that cuts the device at every write and accepts only
the state before or after the operation, and image fuzzing against the parser. The log is the
simplest thing that makes operations atomic on a block device: a redo log of whole blocks with
one commit block, replayed at mount.
