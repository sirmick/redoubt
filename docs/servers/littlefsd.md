# littlefsd

`littlefsd` is the littlefs file server: one instance per volume, each holding one partition from
`blkd` and serving it over 9P as a littlefs filesystem. A volume has one label set, and every
request is checked against it. Each connection is rooted where its granter chose, with a byte quota
carved from the granter's own. littlefs is Redoubt's own pure-Rust implementation of the littlefs
on-disk format, tested against the C reference on the host.

## Purpose

Sessions need files that survive a reboot and a power cut, on a disk shared by principals who do
not trust each other. `littlefsd` keeps each volume's parser apart from every other volume's, keeps
labels per volume so a vault's files never share metadata with an unlabelled volume's, and meters
bytes per attach root so one principal filling a shared volume cannot make another's saves fail.
littlefs was chosen for a published format, an independent second implementation to test against,
power-loss safety by design, and a size that can be read. littlefs is the file system of every
**writable** volume, and of those only: its format is built for a written medium (commits, power
loss). A read-only volume is to be EROFS, served by [`erofsd`](erofsd.md) (planned); a writable
file system for an SSD is a later question.

## Interface

### Volumes, connections and labels

<details><summary>Status: built · tested (35)</summary>

- bench:littlefsd-boot
- bench:littlefsd-confined-labelled
- bench:littlefsd-corrupt-volume
- bench:littlefsd-label-check
- bench:littlefsd-large-directory
- bench:littlefsd-one-volume
- host:redoubt-littlefsd::attach_walk_open_read_write
- host:redoubt-littlefsd::files_and_directories_survive_a_remount
- host:redoubt-littlefsd::a_removed_files_other_fids_get_removed
- host:redoubt-littlefsd::the_volumes_labels_are_checked_on_every_request
- host:redoubt-littlefsd::a_blank_range_is_formatted_and_only_a_blank_one
- host:redoubt-littlefsd::a_range_of_noise_is_served_as_corrupt
- host:redoubt-littlefsd::noise_is_never_formatted_and_never_mounted
- host:redoubt-littlefsd::a_read_only_range_is_never_written
- host:redoubt-littlefsd::a_read_only_range_is_served_read_only
- host:redoubt-littlefsd::a_volume_whose_ids_do_not_hold_together_is_corrupt
- host:redoubt-littlefsd::a_higher_reader_writes_nothing
- host:redoubt-littlefsd::a_rename_cut_short_still_mounts
- host:redoubt-littlefsd::a_directory_aliasing_the_root_is_refused_in_linear_time
- host:redoubt-littlefsd::two_directories_sharing_a_pair_are_corrupt
- host:redoubt-littlefsd::a_tail_that_is_another_directorys_pair_is_corrupt
- host:redoubt-littlefsd::two_chains_joining_at_one_pair_are_corrupt
- host:redoubt-littlefsd::a_chain_looping_back_to_its_head_is_corrupt
- host:redoubt-littlefsd::split_directories_still_mount
- host:redoubt-littlefsd::a_tree_renames_made_deep_still_mounts
- host:redoubt-littlefsd::a_device_that_fails_makes_the_volume_corrupt_until_it_is_mounted_again
- host:redoubt-littlefsd::writes_and_truncations_move_the_qid_version
- host:redoubt-littlefsd::the_conformance_vectors_run_against_littlefsd
- host:redoubt-littlefsd::files_survive_a_restart
- host:redoubt-littlefsd::arguments_it_does_not_understand_stop_it_before_serving
- host:redoubt-littlefsd::a_range_too_small_is_no_volume
- host:redoubt-littlefsd::fids_are_bounded_and_disconnect_frees_them
- host:redoubt-littlefsd::listing_a_directory_reads_it_once_per_window
- host:redoubt-littlefsd::a_change_between_reads_refills_the_window
- host:redoubt-littlefsd::one_reply_costs_a_bounded_number_of_passes

</details>

- **One instance per volume.** A `littlefsd` holds one block-range handle, a partition from
  [`blkd`](blkd.md), and no MMIO, interrupt or DMA. An untrusted medium gets its own server
  holding only that medium, so a parser exploit reaches that medium and nothing else
  ([R47 (one volume per instance)](#r47-one-volume-per-instance)).
- **Arguments.** `littlefsd` gets one named handle, `volume`, its range at `blkd`, or at a
  [`verityd`](verityd.md) for a verified volume, and the arguments
  `endpoint=NAME`, the manifest name of the endpoint it receives on (`littlefsd:data`),
  `labels=ID[,ID...]`, the volume's label set, and `buckets=N`. littlefs blocks are 4096 bytes,
  eight of `blkd`'s sectors, so the volume's block count is its range's sectors divided by 8.
- **Mounting.** At start `littlefsd` mounts its range. A range whose first two blocks are all zero
  has never been written, and `littlefsd` formats it. Any other range that does not mount is served
  as corrupt: every attach is refused with `corrupt`, `littlefsd` says so on its console, and it stays
  up, so a damaged or hostile medium never becomes a restart loop. `littlefsd` never formats a range
  that holds anything.
  A range that mounts is then checked, without writing: every file and directory carries its id,
  no two the same, and the id counter is above the highest, and no metadata pair is named
  twice, within one directory's chain or across two; a volume that fails is served as corrupt
  too. So `littlefsd` serves only volumes it wrote.
- **Read-only ranges.** A range `blkd` reports read-only is served read-only: every change is
  refused before it reaches `blkd`, so a refused write never makes the volume corrupt, and a
  blank read-only range is not formatted.
- **9P**, over the [9P server skeleton](serving.md#the-9p-server-skeleton): one connection per
  client, each rooted where the granting party chose with `new_connection`
  ([wire](wire.md#ninep_common)).
- **A fid is a path and an id.** Every file and directory has an id, `littlefsd`'s own attribute and
  its qid path, from a counter on the root that never gives one twice. The counter moves in one
  commit and the entry is created with its id in the next, so a power cut skips an id and never
  leaves an entry without one, and no read writes the volume. Every operation on a fid finds its
  path again and checks the id: an entry gone, or with another id, is `removed`. So a remove or a
  rename over a file ends every other fid on it, and a fid on a renamed file, or on anything
  below a renamed directory (a connection's root included), is `removed` as well: fids do not
  follow renames.
- **A directory is listed a window at a time.** `littlefsd` serves a directory read from a window of up
  to 64 entries with their stats, filled by one pass over the directory and dropped by any change
  to the volume. Listing n entries costs about n / 64 passes and no lookup per entry, and one
  request costs at most 22 passes over the directory it reads, 22 fixed by the largest message.
- **Labels are per volume.** Each volume has one label set, from the boot manifest or the steward,
  and `littlefsd` reports it as every node's labels, so the skeleton's label check runs on every request:
  a read (a qid and a `stat` included) needs the volume's labels to be a subset of the caller's, a
  write needs them equal ([R25 (the label check)](serving.md#r25-the-label-check)). There are no
  per-file labels, owners or permission bits: access is by capability.
- **A remove succeeds while another connection holds a fid on the file.** An "in use" refusal
  would be a channel between connections. The remove frees the file's blocks at once, and every
  other fid on it gets `removed` on its next read, write or stat (the `Rerror` text for 9P, the
  table's `removed` for a typed operation); only a clunk succeeds. So there
  is no orphan to track and no invisible data holding quota, and it tells a fid's holder no more
  than the name vanishing from the directory tells anyone who can walk there. (littlefs itself
  keeps a removed file readable through open handles; `littlefsd` does not use that.)
- **Removing a file is not revocation.** It ends the file, not anyone's access to the volume;
  revocation is destroying the grant.
- **Admission** is the serving library's, per (account, label set) with a fair share per badge
  ([R26 (admission fairness)](serving.md#r26-admission-fairness)): 32 fids, 8 minted
  connections, 2 multiplexed connections' completion calls, 128 of their requests and 32 pages
  they brought ([serving](serving.md#multiplexed-connections)) per bucket, sized to fit a 2 MiB
  budget; a `disconnect` frees a client's fids.
- **Metadata** lives in littlefs user attributes: what `stat` needs (mtime, qid version) and the
  per-file attributes of `get_attr` and `set_attr`. No access time is kept. A file's qid version
  moves with every write and truncation, so a client caching it sees the change. mtime is 0
  until a clock reaches `littlefsd`.

The attack test: after a remove, the file's other fids get `removed` on read, write and stat.

### Typed operations

<details><summary>Status: built · tested (14)</summary>

- host:redoubt-littlefsd::rename_moves_within_the_volume_and_keeps_the_files_id
- host:redoubt-littlefsd::a_directory_is_not_renamed_into_itself
- host:redoubt-littlefsd::a_rename_over_a_file_removes_it
- host:redoubt-littlefsd::copy_file_copies_and_counts_the_bytes
- host:redoubt-littlefsd::a_copy_that_does_not_fit_leaves_nothing
- host:redoubt-littlefsd::a_read_only_volume_refuses_every_typed_change
- host:redoubt-littlefsd::fids_on_a_renamed_file_or_below_a_renamed_directory_are_removed
- host:redoubt-littlefsd::attributes_set_and_get_with_littlefsds_own_types_refused
- host:redoubt-littlefsd::a_strangers_fid_is_not_found
- host:redoubt-littlefsd::typed_operations_check_the_volumes_labels
- host:redoubt-littlefsd::removed_and_corrupt_are_the_tables_answers
- host:redoubt-littlefsd::the_client_library_works_against_littlefsd
- host:redoubt-littlefsd::a_failing_range_answers_corrupt
- host:redoubt-rt::a_typed_operation_resolves_only_the_callers_own_fids

</details>

`littlefsd` serves typed messages on its 9P endpoint for what 9P2000 does not express, with the same
label and quota checks as 9P, and exactly these four:

- **`rename(old_dir, old_name, new_dir, new_name)`**: atomic, within one volume; `old_dir` and
  `new_dir` are the caller's fids on directories. Renaming a directory into itself is refused. There
  is no rename across volumes: one `littlefsd` serves one volume and cannot act on another's files, so
  the client's `File.rename` returns `{:error, :exdev}`, and a move is the caller's own copy and
  remove, which is not atomic.
- **`copy_file(src_fid, dst_dir, dst_name)`**: copies a file within the volume and replies with the
  bytes copied.
- **`set_attr(fid, attr, value)`** and **`get_attr(fid, attr)`**: a file's or directory's user
  attribute `attr`.

The table: [libs/wire/tables/littlefsd.md](../../libs/wire/tables/littlefsd.md).

{{#include ../../libs/wire/tables/littlefsd.md:tables}}

**Attributes.** A value is at most littlefs's `attr_max`, 1022 bytes; a larger one is refused with
`too_large`. Attribute types 0 to 15 are `littlefsd`'s own (the id, mtime, qid version, the root's id
counter, and later use), and `set_attr` refuses them; types 16 to 255 are the user's. The id
counter moves before each create, in a commit of its own, so a create that then fails (the name
exists, or the volume is full) still uses up an id and a write; ids are never reused, and a 64-bit
counter does not run out.

**Corruption.** An operation that meets a corrupt volume, or an I/O error from `blkd`, answers
`corrupt`, for 9P and the typed operations alike, so a client can tell a broken volume from a
refusal.

### Quotas

<details><summary>Status: built · tested (19)</summary>

- host:redoubt-littlefsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes
- host:redoubt-littlefsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove
- host:redoubt-littlefsd::a_mint_the_room_cannot_take_is_refused_and_disconnect_gives_it_back
- host:redoubt-littlefsd::two_connections_at_one_directory_share_it
- host:redoubt-littlefsd::a_connection_at_its_granters_own_root_carves_nothing
- host:redoubt-littlefsd::a_root_minted_over_files_counts_them
- host:redoubt-littlefsd::a_root_minted_above_a_live_root_holds_its_quota_in_reserve
- host:redoubt-littlefsd::minting_over_files_frees_no_room
- host:redoubt-littlefsd::minting_above_a_live_root_frees_no_room
- host:redoubt-littlefsd::a_rewrite_at_the_start_needs_room_for_the_tail
- host:redoubt-littlefsd::a_rename_or_remove_never_ends_a_live_root
- host:redoubt-littlefsd::a_rename_between_two_roots_moves_the_bytes
- host:redoubt-littlefsd::a_copy_past_the_quota_is_no_space
- host:redoubt-littlefsd::the_volume_never_runs_out_while_every_root_is_within_its_quota
- host:redoubt-littlefsd::a_root_at_its_quota_creates_while_its_entries_fit_one_pair
- host:redoubt-littlefsd::with_room_the_directory_splits_and_the_split_is_charged
- host:redoubt-littlefsd::a_mkdir_without_room_for_its_pair_changes_nothing
- host:littlefs::file_blocks_counts_what_a_file_holds
- host:littlefs::pair_room_bounds_splits_and_new_directories

</details>

Each root a connection is minted at has its own **byte quota**, set by whoever granted it in
`new_connection`'s `quota` field and carved from the room of the live root above it. `littlefsd`
records it in the skeleton's `minted` hook, which refuses a quota that room does not have
(`refused`), and gives it back when the connection is disconnected. A change that would take a
root past its quota is refused with `no space`. So Bob filling the `data` volume cannot make
Alice's saves fail ([R48 (a quota per attach root)](#r48-a-quota-per-attach-root)). The serving
library holds no byte counters; `littlefsd` is the only server that meters bytes.

- **A root holds what lies under it:** whole blocks for a file stored in blocks, the byte length
  of an inline file, and each directory's metadata pairs, less what lies under the live roots
  minted below it; for each of those it holds in reserve the larger of that root's quota and what
  that root holds, so minting a root never frees room. A change is charged to the nearest live
  root above it, whichever connection made it. The volume root is always live, and every
  attached connection is at it. A directory is split into another metadata pair only when the
  root above it has room for one; otherwise littlefs keeps it in the pairs it has, so a split
  never takes a root past its quota.
- **Nothing is stored.** `littlefsd` counts a root by walking its directory when the first connection
  is minted there, and keeps the count while one is live, so the medium holds no counter to
  trust or to lose in a power cut. Connections minted at one directory share its count, and its
  quota is the sum of theirs; one minted at its granter's own root is that root and carves
  nothing. A root minted over more than its quota can read and remove only, until it is under.
- **A rewrite counts what it writes.** littlefs rewrites a file from the first block written to
  its end before it commits, so a write needs room for those blocks as well as any growth.
  `copy_file` writes new blocks and is charged in full.
- **No promise the disk cannot keep.** The volume root's quota is the usable blocks less a fixed
  reserve for the blocks littlefs takes outside any root, and carved quotas never exceed it. A
  commit splits a directory only into room its change's own root has; any other directory it
  touches (the id counter's, a rename's source) is compacted in the pairs it already has.
- **A quota of 0 means nothing:** the connection can read and remove, but not create or grow. A
  quota is never charged to a parent root, which would reopen a shared pool.
- **A rename or remove never ends a live root.** Moving a live root or a directory holding one,
  removing a live root's directory, or renaming over it is refused: it would end that root's
  connections and carry its count away. A rename between two roots' parts of the tree moves the
  bytes and needs room in the second.

The attack tests: a write past one root's quota is refused while another root still writes; a
root with quota 0 reads and removes, but cannot create.

### littlefs

<details><summary>Status: built · tested (16)</summary>

- fuzz:littlefs/image
- fuzz:littlefs/mutate
- host:littlefs::random_operations_small_blocks
- host:littlefs::random_operations_large_blocks
- host:littlefs::random_operations_tiny_blocks
- host:littlefs::random_operations_crowded_small_volume
- host:littlefs::directory_split_and_drop
- host:littlefs::full_volume
- host:littlefs::handles_follow_renames
- host:littlefs::bad_arguments
- host:littlefs::path_and_handle_rules
- host:littlefs::a_failed_write_commits_nothing
- host:littlefs::a_create_with_attributes_is_never_seen_without_them
- host:littlefs::a_create_refuses_attributes_set_attr_would
- host:littlefs::a_directory_read_carries_attributes_and_pairs
- host:littlefs::one_commit_splits_only_as_far_as_its_room

</details>

`libs/littlefs` implements the littlefs on-disk format, version 2.1, in pure Rust: `no_std` with
`alloc`, no dependencies, no `unsafe`. Images it writes mount in the C reference (v2.11.3) and
the other way round; the C code runs only on the host, as a test oracle (`libs/littlefs/diff/`).
Nothing C runs on the target.

- **`Filesystem`** formats and mounts a volume and provides every operation `littlefsd` needs: files
  (open, read, write, seek, truncate, sync, close), directories (mkdir, remove, rename, read),
  stat, user attributes on files and directories, which a create can write in its own commit
  and a directory read passes along, directory reads by a directory's pair as well as by path,
  and a volume check.
- **Paths** are `/`-separated names relative to the root; `.` and `..` are refused. Names read back
  from the medium are opaque bytes that need not be UTF-8 or nameable by a path (the volume check
  reports those), so `littlefsd` never joins one into a path it then resolves.
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

<details><summary>Status: built · tested (16)</summary>

- fuzz:littlefs/image
- fuzz:littlefs/mutate
- host:littlefs::corrupted_bytes_never_panic
- host:littlefs::noise_never_panics
- host:littlefs::duplicate_names_are_corrupt
- host:littlefs::nul_names_are_found_and_refused
- host:littlefs::geometry_mismatch_is_refused
- host:littlefs::tail_list_cycle_is_refused
- host:littlefs::directory_chain_cycle_is_refused
- host:littlefs::directory_inside_itself_is_found_by_fsck
- host:littlefs::file_larger_than_the_volume_is_refused
- host:littlefs::file_head_outside_the_volume_is_refused
- host:littlefs::skip_list_pointing_at_itself_terminates
- host:littlefs::forged_file_sizes_do_not_amplify_allocation
- host:littlefs::stale_handle_after_pair_drop_does_not_touch_another_file
- host:littlefs::unnameable_names_fail_the_check

</details>

Every length, offset, block pointer and tag read from the device is checked before use, and a
malformed image yields `Error::Corrupt`, never a panic. Every walk is bounded: along the list of
metadata pairs at most `block_count / 2` steps, a file's skip list by its size (itself checked
against the volume), a whole-volume walk at most `3 * block_count` blocks. The only recursion is
one level deep. Metadata is checksummed; file data is not
([R49 (a hostile medium is corrupt, not a crash)](#r49-a-hostile-medium-is-corrupt-not-a-crash)).

### Power loss

<details><summary>Status: built · tested (7)</summary>

- host:littlefs::crash_at_every_write_small_blocks
- host:littlefs::crash_at_every_write_tiny_blocks
- host:littlefs::crash_at_every_write_large_blocks
- host:littlefs::crash_at_every_write_random_workloads
- host:littlefs::crash_at_every_write_torn_erases
- host:littlefs::crash_during_repair
- host:littlefs::io_error_poisons_until_remount

</details>

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

Status: built · tested: bench:littlefsd-one-volume

`littlefsd` holds its endpoint, its one block-range handle at `blkd`, the console `init` gave it, and
the connections it minted. It holds no device, no budget handle and no connection to any other
file server. What a client may reach is the subtree its connection is rooted at, under the
volume's labels and its root's quota.

## Security properties

### R47 (one volume per instance)

<details><summary>Status: built · tested (2)</summary>

- bench:littlefsd-one-volume
- host:redoubt-init::no_server_is_handed_a_badge_at_blkd

</details>

Each `littlefsd` instance serves one volume and holds only that volume's block range. A client who
exploits the filesystem parser through a crafted volume or request reaches that volume's data and
nothing else: no other volume, no other partition, no device.

### R48 (a quota per attach root)

<details><summary>Status: built · tested (3)</summary>

- bench:littlefsd-quota
- host:redoubt-littlefsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes
- host:redoubt-littlefsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove

</details>

Every connection's root has a byte quota carved from its granter's, and no write takes a root past
it. So one principal filling a shared volume uses up only its own quota and cannot make another's
writes fail. (How many connections and fids a client may hold is admission's, R26, not the
quota's.)

### R49 (a hostile medium is corrupt, not a crash)

<details><summary>Status: built · tested (8)</summary>

- fuzz:littlefs/image
- fuzz:littlefs/mutate
- host:littlefs::corrupted_bytes_never_panic
- host:littlefs::noise_never_panics
- host:littlefs::tail_list_cycle_is_refused
- host:littlefs::skip_list_pointing_at_itself_terminates
- host:littlefs::forged_file_sizes_do_not_amplify_allocation
- host:littlefs::stale_handle_after_pair_drop_does_not_erase_another_files_data

</details>

Whatever bytes the medium holds, littlefs refuses them as corrupt rather than panicking, looping
or allocating beyond the volume's size, and a stale handle never touches another file's metadata
or data. So a hostile disk image can make its own volume unreadable, never crash or hang its
`littlefsd` in the parser.

### R50 (power loss leaves before or after)

<details><summary>Status: built · tested (4)</summary>

- host:littlefs::crash_at_every_write_small_blocks
- host:littlefs::crash_at_every_write_random_workloads
- host:littlefs::crash_at_every_write_torn_erases
- host:littlefs::crash_during_repair

</details>

On a device that keeps the block-device contract, a power cut at any block write leaves every
metadata change either done or not done, and the next mount reads a consistent volume.

## Failure and restart

Status: built · tested: bench:littlefsd-restart, bench:littlefsd-corrupt-volume

- **`littlefsd` crashes:** its clients' calls get `Dead`, `init` restarts it on the same endpoint
  ([init](init.md#restarts-and-reboots)), and littlefs's copy-on-write keeps the volume
  consistent. Clients ask for fresh connections.
- **The medium is corrupt:** requests that reach the corruption fail; the volume check reports it.
- **An I/O error from `blkd`** poisons the filesystem until it is mounted again.

A restarted `littlefsd` mounts as it does at boot, with the same checks over every metadata pair, then
serves; it reads no file's blocks first. A power cut leaves the volume consistent
([R50](#r50-power-loss-leaves-before-or-after)), and damage in a file's blocks is `corrupt`
wherever a request meets it ([R49](#r49-a-hostile-medium-is-corrupt-not-a-crash)).

## Residual risks

- **littlefs does not checksum data,** except on a verified volume
  ([R76 (verified volumes)](verityd.md#r76-verified-volumes)). A block device that returns wrong data undetected,
  beyond `blkd`'s contract, corrupts file contents silently; only metadata is checksummed.
- **No wear levelling.** On a medium that does not level its own wear (raw flash), littlefs wears
  it out; a virtio disk levels its own.
- **Attributes and data are two commits.** A power cut between them leaves a file's new data with
  its old attributes, or the reverse.
- **Large directories and files scale poorly** in littlefs's format; a listing is linear in the
  directory.
- **A listing across a change may skip or repeat an entry.** A directory read goes on by entry
  index, and a change between two reads refills the window from the directory as it is then, so
  an entry created or removed before that index shifts the rest by one (9P allows it). And a
  window is filled whole: an entry `littlefsd` cannot read, up to 63 places past the one asked for,
  fails the earlier read as `corrupt`, as it would within one reply
  ([R49](#r49-a-hostile-medium-is-corrupt-not-a-crash)).
- **littlefs's fuzz targets and C oracle run outside the bench.** Its host tests run in
  `littlefs-host-tests`; the differential run against the C library (`libs/littlefs/diff/`) is
  its own workspace with a C toolchain, and the fuzz targets need `cargo fuzz`, neither of which a
  host-tests case runs, so both are run by hand.
- **A shared `littlefsd` is shared state.** Principals on one volume share one server's memory and
  scheduling; where that matters, each gets its own volume and instance. A mint walks its root's
  directory once, so minting in a loop costs `littlefsd` that walk each time.
- **A refused rename or remove says a live root is there.** A connection that tries to move or
  remove a directory learns whether some connection is rooted at or under it.

## Why

- **One instance per volume.** A filesystem parser is a large surface on hostile bytes; one per
  medium keeps an exploit inside the medium it came from.
- **Labels per volume, not per file.** Per-file labels would put labelled and unlabelled metadata
  in one directory structure, a channel through its layout; a volume per label set has none.
- **Fids do not follow renames.** Following them needs a table of every file a client has walked
  to, keyed on a number littlefs does not keep; a path and an id need no memory beyond the fid.
- **Quotas per attach root.** A shared volume without them lets any client fill it; a quota carved
  from the granter's keeps the whole tree of grants within what its root was given.
- **littlefs, reimplemented.** A published format with a second implementation gives a
  differential oracle; the C library wrapped in Rust would put C on the target.
- **No wear levelling.** A virtio disk levels its own wear; the code left out is code that cannot
  be wrong.
