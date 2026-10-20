# erofsd

## Purpose

`erofsd` serves a **read-only volume** in EROFS, the Enhanced Read-Only File System, written
once on the build host and read whole at every boot. The system volume (OTP, Elixir and the
shell as objects) is one. Its clients see the same 9P face every file server shows
([littlefsd](littlefsd.md)); what changes is underneath: a file is one sequential run of 4 KiB blocks,
found through one inode and one sorted directory, so a read of an object touches each of its
blocks once, and a verifier's tree over the volume is read in order. It is named for the format
it serves ([naming](README.md#naming)); a writable volume on flash, and the data volume, are
littlefs ([littlefsd](littlefsd.md)); the SSD's writable volumes are walfs ([walfsd](walfsd.md)).

## Interface

### The format

<details><summary>Status: built · tested (12)</summary>

- bench:erofs-corrupt
- bench:erofs-host-tests
- bench:erofs-oracle
- fuzz:erofs/image
- host:erofs::both_inode_sizes_parse
- host:erofs::both_layouts_read_whole_and_inline_tails_stay_in_their_block
- host:erofs::directory_blocks_are_counted_bounded_and_ordered
- host:erofs::every_superblock_field_out_of_range_is_corrupt
- host:erofs::extended_attributes_are_sized_and_bounded_never_read
- host:erofs::lookup_and_iteration_agree_with_the_packed_tree
- host:erofs::what_mkfs_erofs_packs_our_parser_reads_as_the_tree
- host:redoubt-erofsd::a_volume_that_is_not_in_the_subset_is_served_as_corrupt

</details>

EROFS is Linux's read-only file system, documented in the kernel tree
(`Documentation/filesystems/erofs.rst`) and written by `mkfs.erofs` (erofs-utils). `erofsd`
reads the **uncompressed subset**, with 4 KiB blocks:

- the **superblock** at byte 1024 of block 0: the magic, the block size, the root inode's
  number, where the inode area begins, and the block count;
- **inodes**, addressed by number as an offset into the inode area, in either of the format's
  two sizes (compact, 32 bytes; extended, 64 bytes): the mode, the size, the link count, the
  data layout and the data's first block, and a count of extended attributes, which `erofsd`
  skips over and never interprets;
- two **data layouts**: *flat plain*, the file's bytes in consecutive blocks from its first
  block, and *flat inline*, the same with the last partial block stored after the inode; every
  other layout (the compressed and the chunk-based ones) is refused as corrupt;
- **directories** as blocks of fixed-size entries (the child's inode number, the offset of its
  name in the block, its type) followed by the names, each block's entries sorted by name, so a
  lookup is a binary search per block and a listing is one pass.

Everything `erofsd` reads is bounded before it is used: the superblock's block count against
the range's size, every inode's offset against the inode area, every data block against the
block count, every name offset against its block, and the entry count against the block size. A
volume that fails any of these, or asks for a layout outside the subset, is **corrupt** and is
served as `littlefsd` serves one
([R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)):
every attach refused with `corrupt`, the server up. The parser is these checks, two inode
sizes, two layouts and a sorted directory: the smallest thing that can stand between a hostile
medium and a client, and under
[R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance) a client who exploits it
reaches that volume's data and nothing else; there is less of it to exploit.

`erofsd` does not hash a file as it serves it: integrity is the verifier's
([verityd](verityd.md)), not the parser's. The packer records each file's SHA-256 as the
extended attribute `user.sha256`, for tools and audits (a volume's listing can be checked
against what was packed); `erofsd` skips it with every other attribute.

### Serving

<details><summary>Status: built · tested (16)</summary>

- bench:erofs-read-only
- host:redoubt-erofsd::a_directory_lists_exactly_its_children_in_order_by_page
- host:redoubt-erofsd::a_minted_connection_sees_only_below_its_root
- host:redoubt-erofsd::a_read_is_one_range_read_and_a_walk_reads_no_more_than_it_needs
- host:redoubt-erofsd::a_session_rooted_below_the_root_sees_only_there
- host:redoubt-erofsd::every_way_of_writing_is_refused_read_only
- host:redoubt-erofsd::files_read_back_byte_for_byte_at_block_edges_past_the_end_and_across_an_inline_tail
- host:redoubt-erofsd::stat_says_the_inode_and_the_name_it_was_walked_by
- host:redoubt-erofsd::the_client_library_reads_the_volume_through_blkd
- host:redoubt-erofsd::the_conformance_vectors_run_against_erofsd
- host:redoubt-erofsd::the_image_entry_holds_its_buckets
- host:testbench::every_manifest_entry_holds_its_buckets_at_their_caps
- host:redoubt-erofsd::the_volumes_labels_are_checked_on_every_node
- host:redoubt-init::an_erofsd_entry_is_a_volume_server_as_a_littlefsd_one_is
- host:testbench::the_file_servers_fid_caps_leave_every_connection_its_root_and_working_share
- bench:lease-code-load

</details>

`erofsd` is started by `init` as `littlefsd` is, one instance per read-only volume, with the same
arguments (`endpoint=`, `buckets=`) and the same range: a `blkd` range for an unverified
volume, a `verityd` range for a verified one ([blkd](blkd.md), [verityd](verityd.md)). Its
clients share the budget its manifest entry gives, the entry's pages less its heap cap and stack,
by the entry rule ([walfsd](walfsd.md#serving)): a bucket at its caps costs 286,720 bytes, and every
session's domain is one, beside the steward's ([the steward](steward.md#authentication-and-sessions));
the image's six (a case's extra client the sixth) are 1,720,320 bytes, 420 pages, which with the
heap cap's 26 and the stack's 128 round to 768 of the image's 1,024, and the entry leaves 870 pages
of budget, twelve such buckets and not a thirteenth.
A bucket holds 28 minted connections: what a manifest lets one account hold at the system volume
at once, whoever minted them ([serving](serving.md#admit)), two per session the steward starts
(its shell's and its relay's) for the console session, the cap of live contexts per label set and
one spare for a context detaching while another attaches, and one per lease, the leases what the
principal's unlabelled share holds in `agent` budgets ([the steward](steward.md#leases)), held by
the bench to at least the largest such sum over the tree's manifests, 2 × (1 + 3 + 1) + 18 under
`steward-latency`'s manifest, 2 × (1 + 3 + 1) + 7 under the image's
([walfsd](walfsd.md#serving), the same derivation). A bucket holds 57 fids, derived from the same
connections ([serving](serving.md#admit)): each holds its root fid for its life, a fixed cost
outside the fair share, and beside it one working fid at a time, a module file walked, opened,
read and clunked by the VM's one reader, so the cap is the 28 roots and a pool of one per
connection and one over, 28 + 29, which the share `max(1, pool / (holders + 1))` divides into
exactly one each with every connection holding. **The account's concurrent VMs at the system
volume** are bounded by this and nothing else on the read path: before the root left the share,
sixteen connections of one account (two sessions' and fourteen leases') at the cap of 32 held one
fid each, the root, and no VM of the account could load a module, so a lease's VM died or
parked by where the refusal landed and the supported count was thirteen leases beside two
sessions, seven for anything the shell's hub needed at the data volume
([walfsd](walfsd.md#serving)); now every connection the manifest lets the account hold has its
root and its working fid, so the supported count is the manifest's own lease term, the sixteen
leases `steward-latency`'s alice carves beside her two sessions and the image's seven with
margin, held by bench:lease-code-load (sixteen agents of one account loading 27 modules each at
once, every one ending by its own exit and none refused, on both widths at one and two harts).
The case runs on the host's clock: under the virtual clock every boot launched and loaded all
sixteen and ended fifteen, the sixteenth destroyed at its own deadline the instant the guest
idled with no nearer timer, the bench's clock finding and not the read path's
([the test bench](../testbench.md#the-case-file)); on the host's clock every boot ends all
sixteen. The read path
is slow under that load, never a death's cause: beside three to eleven others a lease's VM
starts (its boot pack of about 3.8 MB and its start module) in 35–140 s on either width at one
hart and 43–143 s at two, loads at 30–85 KB/s per VM after that, and launches in 1.3–11.5 s
([scheduling](../kernel/scheduling.md#responsiveness)); `verityd` holds about a tenth of the
blocks asked and read-ahead gives about 2.2 blocks a `blkd` read
([verityd](verityd.md#a-cache-of-checked-data-blocks)). A `buckets=N` the budget cannot hold
and `erofsd` does not start. At start it reads the superblock and the root inode, then serves:

- `attach`, `walk`, `open` for reading, `read`, `stat`, directory reads and `clunk`, with the
  serving library's 9P skeleton, admission and label check as every file server
  ([serving](serving.md#the-9p-server-skeleton));
- a `walk` of one component reads the directory's blocks and finds the name; the inode it
  names is read once and kept with the fid;
- a read of a file is one range read of its blocks from its first block plus the offset, cut
  to the file's size, with an inline tail copied from the inode's block;
- `write`, `create`, `remove` and any open for writing are refused with `read-only`; there is
  no quota, since nothing is written, and attributes are not served.

Each connection is rooted where the granting party chose with `new_connection`, as `littlefsd`'s are
([littlefsd](littlefsd.md#volumes-connections-and-labels)); every node reports the volume's labels.
Memory is bounded: one block of scratch for directory and inode reads, and one inode per open
fid.

### The packer

<details><summary>Status: built · tested (7)</summary>

- bench:erofs-oracle
- bench:erofs-read-only
- bench:userland-boot
- host:erofs::the_writer_refuses_what_it_cannot_name
- host:erofs::two_packs_of_one_tree_are_the_same_bytes
- host:erofs::what_our_writer_packs_fsck_erofs_checks_and_extracts_as_the_tree
- host:testbench::an_erofs_partition_is_its_stage_and_each_damage_is_corrupt_where_it_is

</details>

The bench's disk packer writes an EROFS volume from a staged directory (`fs = "erofs"` in a
disk recipe, in place of `fs = "littlefs"`), with Redoubt's own writer in `libs/erofs`: flat
plain and flat inline layouts only, no compression, sorted directories, the `user.sha256`
attribute per file, deterministic (the same tree gives the same bytes; no timestamps but a
fixed one). The build host's `mkfs.erofs` (erofs-utils, installed by the setup script) is a test
oracle, as littlefs's C reference is ([littlefsd](littlefsd.md#littlefs)): a volume our writer packs is
checked by `fsck.erofs`, and one `mkfs.erofs` packs from the same tree is mounted and read by
our parser with equal results. Nothing of erofs-utils runs on the target. The userland disk's
recipe packs its objects this way. A verified volume is followed in its range by the verifier's
tree over its blocks, as any verified volume is ([verityd](verityd.md#the-tree)), and the rest
of its range, zeros, is covered too. A file's blocks are consecutive, so each is read once; the
blocks read again are a directory's and those holding inodes, which
[verityd's cache of checked data blocks](verityd.md#a-cache-of-checked-data-blocks) holds.

## Authority

Status: built · partly tested: that `erofsd` holds nothing but its endpoint and its range is `init`'s placement, checked by the manifest's tests, and not probed from inside as `littlefsd-one-volume` does · tested: host:redoubt-erofsd::the_client_library_reads_the_volume_through_blkd, host:redoubt-init::an_erofsd_entry_is_a_volume_server_as_a_littlefsd_one_is

`erofsd` holds its own endpoint and one range, at `blkd` or at a `verityd`, and nothing else. It
sends no write; `verityd` refuses every write, and `blkd` refuses them only on a read-only disk.
It parses one format's subset, bounded as above.

## Security properties

Status: built · tested: bench:erofs-corrupt, fuzz:erofs/image, host:redoubt-erofsd::a_range_that_fails_makes_the_volume_corrupt_until_erofsd_starts_again, host:redoubt-erofsd::the_volumes_labels_are_checked_on_every_node

`erofsd` claims no rule of its own. It keeps `littlefsd`'s R47 (one volume per instance), R49 (a
hostile medium is corrupt, not a crash) and the serving library's R25 (the label check) and
R26 (admission fairness), each stated on its owning page, and under a verifier the volume's
R76 (verified volumes).

## Failure and restart

Status: built · partly tested: a restart is `init`'s, as for every server, and no case restarts an `erofsd` · tested: bench:erofs-corrupt, host:redoubt-erofsd::a_range_that_cannot_be_sized_is_no_volume, host:redoubt-erofsd::a_range_that_fails_makes_the_volume_corrupt_until_erofsd_starts_again, host:redoubt-erofsd::a_volume_of_noise_or_a_failing_range_is_served_as_corrupt, host:redoubt-erofsd::arguments_it_does_not_understand_or_no_range_stop_it_before_serving, host:redoubt-fileserver::arguments_it_does_not_understand_stop_it_before_serving

A bad argument or a missing range handle is an exit at start; a corrupt volume is served as
corrupt, not an exit. `init` restarts `erofsd` on the same endpoint as any server, and a
restarted `erofsd` reads the same superblock and serves the same files: a read-only volume has
no state to lose.

## Residual risks

- **A subset of a larger format.** EROFS has compressed and chunked layouts, which `erofsd`
  refuses; an image `mkfs.erofs` writes with other options may use them, so the bench packs
  with Redoubt's writer and checks `mkfs.erofs`'s output (erofs-utils 1.9) only with the options
  that stay in the subset: no `-z` (compression), no `--chunksize` (chunks) and no `-E` feature
  that is off by default (`fragments`, `dedupe`, `48bit`, `dot-omitted`), with `-b4096`, `-T` 0
  and `--all-root`, and each of `-Eforce-inode-compact`, `-Eforce-inode-extended`,
  `-E^inline_data` and `--root-xattr-isize=64` (`libs/erofs/tests/oracle.rs`).
- **No hashing on the serving path.** Without a verifier, a read-only volume's bytes are
  trusted as the medium gives them; the per-file digests are checked only by tools. The system
  volume is verified ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)).
- **Directory blocks are checked one at a time.** Each block's names are checked in order, but
  not one block's against the next; a walk binary-searches the blocks by their first and last
  names, so a volume whose blocks are out of order makes a listed name walk to `not_found`. It is
  a wrong answer, not an unsafe one, and a verified volume's blocks are the ones its builder wrote.
- **Extended attributes are skipped, not parsed.** Their count and size are bounds-checked so
  that skipping them cannot leave the inode area; their content is never read.

## Why

On littlefs, a log-structured file system built for flash that is written in place, files are
skip lists of blocks, read a block-sized piece at a time with the list re-walked for each piece,
and directories are metadata pairs replayed at mount. For a volume that is written once on the
build host and read whole, module by module, at every boot, that costs each block many reads
and, under a verifier, many hashes per boot ([verityd](verityd.md#memory-and-cost)). Nothing
littlefs does for a writable medium (wear, power loss, commits)
applies to a volume nothing writes. EROFS is the format the access pattern asks for, with a
standard tool chain on the host, and its uncompressed subset's parser is about a quarter of the
code of the one it replaces (`libs/erofs` is 578 lines with its writer, `libs/littlefs` 2,033),
which under R47 is a smaller thing to trust. Writable volumes keep littlefs, which is built for a
written medium.
