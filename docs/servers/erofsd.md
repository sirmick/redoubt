# erofsd

## Purpose

`erofsd` serves a **read-only volume** in EROFS, the Enhanced Read-Only File System, written
once on the build host and read whole at every boot. The system volume (OTP, Elixir and the
shell as objects) is one. Its clients see the same 9P face every file server shows
([littlefsd](littlefsd.md)); what changes is underneath: a file is one sequential run of 4 KiB blocks,
found through one inode and one sorted directory, so a read of an object touches each of its
blocks once, and a verifier's tree over the volume is read in order. It is named for the format
it serves ([naming](README.md#naming)); writable volumes stay littlefs ([littlefsd](littlefsd.md)).

## Interface

### The format

Status: planned · M1 (separation and containment)

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

**Open:** none.

### Serving

Status: planned · M1 (separation and containment)

`erofsd` is started by `init` as `littlefsd` is, one instance per read-only volume, with the same
arguments (`endpoint=`, `buckets=`) and the same range: a `blkd` range for an unverified
volume, a `verityd` range for a verified one ([blkd](blkd.md), [verityd](verityd.md)). At
start it reads the superblock and the root inode, then serves:

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

**Open:** none.

### The packer

Status: planned · M1 (separation and containment)

The bench's disk packer writes an EROFS volume from a staged directory (`fs = "erofs"` in a
disk recipe, in place of `fs = "littlefs"`), with Redoubt's own writer in `libs/erofs`: flat
plain and flat inline layouts only, no compression, sorted directories, the `user.sha256`
attribute per file, deterministic (the same tree gives the same bytes; no timestamps but a
fixed one). The build host's `mkfs.erofs` (erofs-utils, installed by the setup script) is a test
oracle, as littlefs's C reference is ([littlefsd](littlefsd.md#littlefs)): a volume our writer packs is
checked by `fsck.erofs`, and one `mkfs.erofs` packs from the same tree is mounted and read by
our parser with equal results. Nothing of erofs-utils runs on the target. The userland disk's
recipe packs its objects this way. A verified volume is followed in its range by the verifier's
tree over its blocks, as any verified volume is ([verityd](verityd.md#the-tree)); because a
file's blocks are consecutive, which suits a cache of checked data blocks if
[verityd](verityd.md#a-cache-of-checked-data-blocks) gets one.

**Open:** none.

## Authority

Status: planned · M1 (separation and containment)

`erofsd` holds its own endpoint and one range, at `blkd` or at a `verityd`, and nothing else. It
sends no write; `verityd` refuses every write, and `blkd` refuses them only on a read-only disk.
It parses one format's subset, bounded as above.

**Open:** none.

## Security properties

Status: planned · M1 (separation and containment)

`erofsd` claims no rule of its own. It keeps `littlefsd`'s R47 (one volume per instance), R49 (a
hostile medium is corrupt, not a crash) and the serving library's R25 (the label check) and
R26 (admission fairness), each stated on its owning page, and under a verifier the volume's
R76 (verified volumes).

**Open:** none.

## Failure and restart

Status: planned · M1 (separation and containment)

A bad argument or a missing range handle is an exit at start; a corrupt volume is served as
corrupt, not an exit. `init` restarts `erofsd` on the same endpoint as any server, and a
restarted `erofsd` reads the same superblock and serves the same files: a read-only volume has
no state to lose.

**Open:** none.

## Residual risks

- **A subset of a larger format.** EROFS has compressed and chunked layouts, which `erofsd`
  refuses; an image `mkfs.erofs` writes with its defaults may use them, so the bench packs with
  Redoubt's writer and checks `mkfs.erofs`'s output only with the options that stay in the
  subset.
- **No hashing on the serving path.** Without a verifier, a read-only volume's bytes are
  trusted as the medium gives them; the per-file digests are checked only by tools. The system
  volume is verified ([R75 (verified userland)](../kernel/boot.md#r75-verified-userland)).
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
standard tool chain on the host, and its uncompressed subset's parser is a fraction of the one
it replaces, which under R47 is a smaller thing to trust. Writable volumes keep littlefs, which
is built for a written medium; a writable file system for an SSD is a later question.
