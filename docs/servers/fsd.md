# fsd

`fsd` is the file server: one instance per volume, each holding one partition from `blkd` and
serving it over 9P as a littlefs filesystem. A volume has one label set, and every request is
checked against it. Each connection is rooted where its granter chose, with a byte quota carved
from the granter's own. littlefs is Redoubt's own pure-Rust implementation of the littlefs
on-disk format, tested against the C reference on the host.

## Purpose

Sessions need files that survive a reboot and a power cut, on a disk shared by principals who do
not trust each other. `fsd` keeps each volume's parser apart from every other volume's, keeps
labels per volume so a vault's files never share metadata with an unlabelled volume's, and meters
bytes per attach root so one principal filling a shared volume cannot make another's saves fail.
littlefs was chosen for a published format, an independent second implementation to test against,
power-loss safety by design, and a size that can be read.

## Interface

### Volumes, connections and labels

Status: planned · M1 (separation and containment)

- **One instance per volume.** An `fsd` holds one block-range handle, a partition from
  [`blkd`](blkd.md), and no MMIO, interrupt or DMA. An untrusted medium gets its own server
  holding only that medium, so a parser exploit reaches that medium and nothing else
  ([R47 (one volume per instance)](#r47-one-volume-per-instance)).
- **9P**, over the [9P server skeleton](serving.md#the-9p-server-skeleton): one connection per
  client, each rooted where the granting party chose with `new_connection`
  ([wire](wire.md#ninep_common)).
- **Labels are per volume.** Each volume has one label set, from the boot manifest or the steward,
  and `fsd` reports it as every node's labels, so the skeleton's label check runs on every request:
  a read (a qid and a `stat` included) needs the volume's labels to be a subset of the caller's, a
  write needs them equal ([R25 (the label check)](serving.md#r25-the-label-check)). There are no
  per-file labels, owners or permission bits: access is by capability.
- **A remove succeeds while another connection holds a fid on the file.** An "in use" refusal
  would be a channel between connections. The remove frees the file's blocks at once, and every
  other fid on it gets `removed` on its next read, write or stat (the `Rerror` text for 9P, the
  table's `removed` for a typed operation); only a clunk succeeds. So there
  is no orphan to track and no invisible data holding quota, and it tells a fid's holder no more
  than the name vanishing from the directory tells anyone who can walk there. (littlefs itself
  keeps a removed file readable through open handles; `fsd` does not use that.)
- **Removing a file is not revocation.** It ends the file, not anyone's access to the volume;
  revocation is destroying the grant.
- **Admission** is the serving library's, per (account, label set) with a fair share per badge
  ([R26 (admission fairness)](serving.md#r26-admission-fairness)); a `disconnect` frees a
  client's fids.
- **Metadata** lives in littlefs user attributes: what `stat` needs (mtime, qid version) and the
  per-file attributes of `get_attr` and `set_attr`. No access time is kept.

The attack test: after a remove, the file's other fids get `removed` on read, write and stat.

**Open:** none.

### Typed operations

Status: planned · M1 (separation and containment)

`fsd` serves typed messages on its 9P endpoint for what 9P2000 does not express, with the same
label and quota checks as 9P, and exactly these four:

- **`rename(old_dir, old_name, new_dir, new_name)`**: atomic, within one volume; `old_dir` and
  `new_dir` are the caller's fids on directories. Renaming a directory into itself is refused. There
  is no rename across volumes: one `fsd` serves one volume and cannot act on another's files, so
  the client's `File.rename` returns `{:error, :exdev}`, and a move is the caller's own copy and
  remove, which is not atomic.
- **`copy_file(src_fid, dst_dir, dst_name)`**: copies a file within the volume and replies with the
  bytes copied.
- **`set_attr(fid, attr, value)`** and **`get_attr(fid, attr)`**: a file's or directory's user
  attribute `attr`.

The table: [libs/wire/tables/fsd.md](../../libs/wire/tables/fsd.md).

{{#include ../../libs/wire/tables/fsd.md:tables}}

**Attributes.** A value is at most littlefs's `attr_max`, 1022 bytes; a larger one is refused with
`too_large`. Attribute types 0 to 15 are `fsd`'s own (mtime, qid version, and later use), and
`set_attr` refuses them; types 16 to 255 are the user's.

**Open:** none.

### Quotas

Status: planned · M1 (separation and containment)

Each root a connection is minted at has its own **byte quota**, set by whoever granted it in
`new_connection`'s `quota` field and carved from the granter's own quota. `fsd` records it in the
skeleton's `minted` hook, which refuses a quota the granter does not have (`refused`), and gives
it back when the connection is disconnected. A write that would take a root past its quota is
refused. So Bob filling the `data` volume cannot make Alice's saves fail
([R48 (a quota per attach root)](#r48-a-quota-per-attach-root)). The serving library holds no byte
counters; `fsd` is the only server that meters bytes.

- **What a quota counts:** the blocks a root actually holds: whole blocks for a file stored in
  blocks, the byte length for an inline file, and each directory's metadata pair (two blocks),
  charged to the root that created it. littlefs shares no blocks between files, so `copy_file`
  writes new blocks and a copy is charged in full.
- **No promise the disk cannot keep.** The volume root's quota is the usable blocks less a fixed
  reserve for metadata compaction, and carved quotas never exceed it.
- **A quota of 0 means nothing:** the connection can read and remove, but not create or grow. A
  quota is never charged to a parent root, which would reopen a shared pool.

The attack tests: a write past one root's quota is refused while another root still writes; a
root with quota 0 cannot create a file.

**Open:** none.

### littlefs

Status: built · tested: fuzz:littlefs/image, fuzz:littlefs/mutate, host:littlefs::random_operations_small_blocks, host:littlefs::random_operations_large_blocks, host:littlefs::random_operations_tiny_blocks, host:littlefs::random_operations_crowded_small_volume, host:littlefs::directory_split_and_drop, host:littlefs::full_volume, host:littlefs::handles_follow_renames, host:littlefs::bad_arguments, host:littlefs::path_and_handle_rules, host:littlefs::a_failed_write_commits_nothing

`libs/littlefs` implements the littlefs on-disk format, version 2.1, in pure Rust: `no_std` with
`alloc`, no dependencies, no `unsafe`. Images it writes mount in the C reference (v2.11.3) and
the other way round; the C code runs only on the host, as a test oracle (`libs/littlefs/diff/`).
Nothing C runs on the target.

- **`Filesystem`** formats and mounts a volume and provides every operation `fsd` needs: files
  (open, read, write, seek, truncate, sync, close), directories (mkdir, remove, rename, read),
  stat, user attributes on files and directories, and a volume check.
- **Paths** are `/`-separated names relative to the root; `.` and `..` are refused. Names read back
  from the medium are opaque bytes that need not be UTF-8 or nameable by a path (the volume check
  reports those), so `fsd` never joins one into a path it then resolves.
- **Memory** is bounded: one block-sized buffer per metadata fetch, one block per file handle that
  is writing, and an allocation bitmap of `block_count / 8` bytes.
- **Where it departs from the C reference:** open handles follow renames and survive removal (their
  data stays readable); renaming a directory into itself is refused; directory reads return no `.`
  or `..`; CRC-valid commits that make no sense (duplicate names, entries without names, tags out
  of range) are corrupt; the configured block count must equal the superblock's; only on-disk
  version 2.1 mounts; a file's attributes and its data are two commits.
- **Left out on purpose:** wear levelling and bad-block relocation, since a virtio disk's device
  handles both (a failed program or erase is reported, not worked around); growing the
  superblock chain; migration from older versions.

The model tests run random operations against an in-memory model, with handles held open and
volumes run full.

### The medium is hostile

Status: built · tested: fuzz:littlefs/image, fuzz:littlefs/mutate, host:littlefs::corrupted_bytes_never_panic, host:littlefs::noise_never_panics, host:littlefs::duplicate_names_are_corrupt, host:littlefs::nul_names_are_found_and_refused, host:littlefs::geometry_mismatch_is_refused, host:littlefs::tail_list_cycle_is_refused, host:littlefs::directory_chain_cycle_is_refused, host:littlefs::directory_inside_itself_is_found_by_fsck, host:littlefs::file_larger_than_the_volume_is_refused, host:littlefs::file_head_outside_the_volume_is_refused, host:littlefs::skip_list_pointing_at_itself_terminates, host:littlefs::forged_file_sizes_do_not_amplify_allocation, host:littlefs::stale_handle_after_pair_drop_does_not_touch_another_file, host:littlefs::unnameable_names_fail_the_check

Every length, offset, block pointer and tag read from the device is checked before use, and a
malformed image yields `Error::Corrupt`, never a panic. Every walk is bounded: along the list of
metadata pairs at most `block_count / 2` steps, a file's skip list by its size (itself checked
against the volume), a whole-volume walk at most `3 * block_count` blocks. The only recursion is
one level deep. Metadata is checksummed; file data is not
([R49 (a hostile medium is corrupt, not a crash)](#r49-a-hostile-medium-is-corrupt-not-a-crash)).

### Power loss

Status: built · tested: host:littlefs::crash_at_every_write_small_blocks, host:littlefs::crash_at_every_write_tiny_blocks, host:littlefs::crash_at_every_write_large_blocks, host:littlefs::crash_at_every_write_random_workloads, host:littlefs::crash_at_every_write_torn_erases, host:littlefs::crash_during_repair, host:littlefs::io_error_poisons_until_remount

Every change reaches the disk as one metadata commit, or, for renames and directory removal, a
sequence the next mount completes or undoes, so an interrupted operation leaves the volume as it
was before or after. Mounting writes nothing; the first write after a mount first repairs what an
interrupted operation left. An I/O error poisons the filesystem until it is mounted again.

The guarantee holds for a device that keeps littlefs's **block-device contract**, which
[`blkd`](blkd.md) keeps:
- a torn program persists a prefix of whole program units, possibly followed by one partly written
  unit, and nothing after;
- a torn erase leaves the block erased, untouched, or erased in part;
- an erase and later programs of one block reach the medium in the order issued;
- `sync` means durable: when it returns, everything before it survives power loss. littlefs syncs
  before each metadata commit that depends on data blocks, and after each commit.

The crash tests inject exactly these failures at every block write of fixed and random workloads
([R50 (power loss leaves before or after)](#r50-power-loss-leaves-before-or-after)).

## Authority

Status: planned · M1 (separation and containment)

`fsd` holds its endpoint, its one block-range handle at `blkd`, and the connections it minted. It
holds no device, no budget handle and no connection to any other file server. What a client may
reach is the subtree its connection is rooted at, under the volume's labels and its root's quota.

**Open:** none.

## Security properties

### R47 (one volume per instance)

Status: planned · M1 (separation and containment)

Each `fsd` instance serves one volume and holds only that volume's block range. A client who
exploits the filesystem parser through a crafted volume or request reaches that volume's data and
nothing else: no other volume, no other partition, no device.

**Open:** none.

### R48 (a quota per attach root)

Status: planned · M1 (separation and containment)

Every connection's root has a byte quota carved from its granter's, and no write takes a root past
it. So one principal filling a shared volume uses up only its own quota and cannot make another's
writes fail. (How many connections and fids a client may hold is admission's, R26, not the
quota's.)

**Open:** none.

### R49 (a hostile medium is corrupt, not a crash)

Status: built · tested: fuzz:littlefs/image, fuzz:littlefs/mutate, host:littlefs::corrupted_bytes_never_panic, host:littlefs::noise_never_panics, host:littlefs::tail_list_cycle_is_refused, host:littlefs::skip_list_pointing_at_itself_terminates, host:littlefs::forged_file_sizes_do_not_amplify_allocation, host:littlefs::stale_handle_after_pair_drop_does_not_erase_another_files_data

Whatever bytes the medium holds, littlefs refuses them as corrupt rather than panicking, looping
or allocating beyond the volume's size, and a stale handle never touches another file's metadata
or data. So a hostile disk image can make its own volume unreadable, never crash or hang its
`fsd` in the parser.

### R50 (power loss leaves before or after)

Status: built · tested: host:littlefs::crash_at_every_write_small_blocks, host:littlefs::crash_at_every_write_random_workloads, host:littlefs::crash_at_every_write_torn_erases, host:littlefs::crash_during_repair

On a device that keeps the block-device contract, a power cut at any block write leaves every
metadata change either done or not done, and the next mount reads a consistent volume.

## Failure and restart

Status: planned · M1 (separation and containment)

- **`fsd` crashes:** its clients' calls get `Dead`, `init` restarts it on the same endpoint
  ([init](init.md#restarts-and-reboots)), and littlefs's copy-on-write keeps the volume
  consistent. Clients ask for fresh connections.
- **The medium is corrupt:** requests that reach the corruption fail; the volume check reports it.
- **An I/O error from `blkd`** poisons the filesystem until it is mounted again.

**Open:** whether a restarted `fsd` runs the volume check before serving.

## Residual risks

- **littlefs does not checksum data.** A block device that returns wrong data undetected, beyond
  `blkd`'s contract, corrupts file contents silently; only metadata is checksummed.
- **No wear levelling.** On a medium that does not level its own wear (raw flash), littlefs wears
  it out; a virtio disk levels its own.
- **Attributes and data are two commits.** A power cut between them leaves a file's new data with
  its old attributes, or the reverse.
- **Large directories and files scale poorly** in littlefs's format.
- **The littlefs tests are not in the bench.** Its host tests, fuzz targets and the C oracle in
  `libs/littlefs/diff/` run by hand; a change can break them without a bench run noticing.
  Follow-up: [todo](../todo/host-tests-in-bench.md). Three of its hostile tests read images the
  repository does not track, so a fresh clone cannot build them
  ([todo](../todo/littlefs-hostile-images.md)).
- **A shared `fsd` is shared state.** Principals on one volume share one server's memory and
  scheduling; where that matters, each gets its own volume and instance.

## Why

- **One instance per volume.** A filesystem parser is a large surface on hostile bytes; one per
  medium keeps an exploit inside the medium it came from.
- **Labels per volume, not per file.** Per-file labels would put labelled and unlabelled metadata
  in one directory structure, a channel through its layout; a volume per label set has none.
- **Quotas per attach root.** A shared volume without them lets any client fill it; a quota carved
  from the granter's keeps the whole tree of grants within what its root was given.
- **littlefs, reimplemented.** A published format with a second implementation gives a
  differential oracle; the C library wrapped in Rust would put C on the target.
- **No wear levelling.** A virtio disk levels its own wear; the code left out is code that cannot
  be wrong.
