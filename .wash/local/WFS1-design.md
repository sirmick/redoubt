# The writable file system for the SSD: two candidates, one recommendation (Architect, for the owner)

The owner (2026-10-06): "We arguably should pull in the other writable FS that's more
appropriate and not a NAND flash one." Earlier (2026-10-05): "an xv6-style logged filesystem
with per-block hashes … ext2 is the alternative if you'd rather keep a differential oracle than
have crash atomicity." The system volume is moving to EROFS (EROFS1, read-only); littlefs
stays on `/home` and `/vault` today. This note sets the two candidates against the tenets and
the pages, sizes the work, and ends in one question.

## What the volumes need

`/home/<principal>` and `/vault` (the labelled volumes) on the SSD, served one instance per
volume (R47), under the kernel's `blkd` ranges, with quotas per attach root (R48), a hostile
medium read as corrupt and never a crash (R49), and power loss leaving a file before or after a
write, never between (R50). littlefs gives all four today; what it does not give: data
checksums (littlefsd.md's first residual: "littlefs does not checksum data"), and reads that do
not re-walk (the read-cost note, `.wash/local/littlefsd-read-cost.md`). It was chosen for NAND:
wear levelling and erase units are its shape, and a virtio disk or an SSD needs neither.

## Candidate A: an xv6-style logged file system, Redoubt's own format, with per-block hashes

- **Format:** superblock; a write-ahead log of whole blocks; an inode table; a block bitmap;
  data blocks. A transaction writes its blocks to the log, commits with one block, then copies
  them home; recovery at mount replays committed transactions and drops the rest. 4 KiB blocks
  (the SSD's and the kernel's page). Directories are arrays of fixed entries; a file's blocks
  are direct plus one indirect level (sizes to the volumes' needs, stated).
- **Per-block hashes:** a hash region, one SHA-256 (`libs/sha256`, the shared crate) per block
  of the volume (0.8 % of it), written in the same transaction as the block. A read verifies
  its block against the region; a mismatch is `corrupt` (R49's word), never a crash. This is
  detection of a damaged or lying medium, not a signature: a medium that rewrites a block and
  its hash together is not caught, and the page says so. (A hash tree with a sealed root would
  catch that, at the cost of a tree update per write; not proposed for M1.)
- **Crash atomicity:** R50 becomes exact and whole-transaction: a `write`, `create`, `rename`
  or `remove` is one transaction; power loss leaves the volume before or after it, data and
  metadata together, which littlefs's "attributes and data are two commits" residual does not
  give.
- **Oracle:** no C reference exists for a format of our own. The oracle is the one littlefs's
  crate already has in shape: a model (an in-memory file system) driven by random operations
  with handles open and volumes run full, plus a power-loss fuzzer that cuts the block device
  at every write and checks the mounted result against the model's before-or-after set, plus
  image fuzzing against the parser. The format gets a specification page the way wire tables
  have one, so the parser is reviewed against a text.
- **Tenets:** small (a logged FS of this shape is a few thousand lines, less than littlefs's
  crate); nothing C on the target; fails closed; the medium is hostile by construction.
- **Name:** a file server is named for its format (servers/README.md "Naming"). The format
  needs a name of its own; `walfs` (write-ahead-log file system) and `walfsd` for the server
  are the proposal; the owner names it.

## Candidate B: ext2

- **Format:** well known; `mke2fs`, `debugfs` and `e2fsck` on the host are a differential
  oracle as littlefs's C library is, and images move between Linux and the box.
- **What it lacks:** no journal (atomicity needs ext3's, a second format on top), no data or
  metadata checksums (ext4's), and after a crash the volume needs `fsck`, which is C and runs
  on the host only: a box that lost power would mount a possibly inconsistent volume or refuse
  it until a host repairs it. Neither R50 nor the hostile-medium tenet is met by the format
  itself; they would be met by rules on top (mount read-only when the dirty bit is set; check
  what is read), which is the work of candidate A without its guarantees.
- **Size:** an ext2 parser and writer of the subset we need is about littlefs's size; with the
  on-top rules, more than A.
- **Tenets:** interoperability is not a tenet of Redoubt's; a hostile medium and before-or-after
  are. The differential oracle is the one real argument for B, and A's model oracle is the same
  kind of evidence littlefs's crate already rests on beside its C oracle.

## Recommendation: A

A logged file system of Redoubt's own, 4 KiB blocks, whole-transaction atomicity, a hash per
block, a specification page, a model and power-loss oracle; `walfsd` beside `erofsd` and
`verityd` under `blkd`, serving `/home` and `/vault` on the SSD; littlefs retires from the
image once both volumes have moved (the `littlefsd` crate and its cases go with it, which is
the larger part of the cost of carrying two writable formats; the owner may instead keep
littlefs for a raw-flash medium that does not exist today).

## Size and sequence

- **WFS1** (Tier A, size M): the format's specification page, `libs/walfs` (no_std, no unsafe,
  no dependencies but `libs/sha256`), its model and power-loss oracle, image fuzzing, the host
  packer (`mkimage` writes a `walfs` volume). Needs nothing; starts after EROFS1 merges so the
  second file server's pattern (bootfsd, erofsd) is settled and the writer-on-the-host pattern
  is EROFS1's.
- **WFS2** (Tier A, size M): `servers/walfsd` on the file-server skeleton (R47, R48, R49, R50's
  rows restated for it), the image's `data` and labelled volumes on it, the steward's `home`
  and `vault` lines pointing at `walfsd:` handles, `init`'s volumes, the cases (power-loss case
  on the machine, hostile-medium case, quota case, the steward's session cases on the new
  volumes), and littlefs's retirement (`littlefsd`, its cases, its pages) in the same package or
  the one after. Needs WFS1 and STEWARD2.
- **Beside BEAM3:** BEAM3's `beamlet-files` case runs on `littlefsd:data` today and is a
  manifest line away from `walfsd:data`; the two packages do not touch each other's code, so
  WFS1 runs in parallel with BEAM3 and WFS2 lands after both. FSD5 (littlefsd's caches) is then
  not needed if littlefs retires; it stays on the list only if littlefs stays.

## The question for the owner

"For the SSD's writable volumes (`/home`, `/vault`): (1) a logged file system of Redoubt's own
(`walfs`: whole-transaction atomicity, a SHA-256 per block, a model and power-loss oracle, a
specification page) rather than ext2 (a host differential oracle, but no atomicity or
checksums and `fsck` on the host after a crash)? Recommended: the logged file system. (2) Does
littlefs retire from the image once both volumes have moved, deleting `littlefsd` and its cases
(recommended), or stay for a flash medium? (3) WFS1 (format, library, oracle, packer) after
EROFS1 and in parallel with BEAM3; WFS2 (the server, the volumes, the steward's lines, the
retirement) after WFS1 and STEWARD2, in M1?"

## plan_set bodies

WFS1 (parent M1, needs EROFS1, state todo): "Owner (2026-10-06): the SSD's writable volumes
move off littlefs ('not a NAND flash one') to a logged file system of Redoubt's own with
per-block hashes; design note .wash/local/WFS1-design.md (architect-16); the owner's decision
on the format, littlefs's retirement and the sequence is recorded on this node when given.
Tier A, size M: the format's specification page; libs/walfs (no_std, no unsafe, deps:
libs/sha256): superblock, write-ahead log of whole 4 KiB blocks with one commit block,
inodes, bitmap, data, a hash region with a SHA-256 per block checked on read (corrupt, never a
crash); whole-transaction atomicity (R50 exact); a model oracle with random operations, a
power-loss fuzzer cutting the device at every write, image fuzzing; the host packer writes a
volume. Not here: the server (WFS2), the volumes' move, littlefs's retirement."

WFS2 (parent M1, needs WFS1, STEWARD2, state todo): "servers/walfsd on the file-server
skeleton serving walfs volumes one instance per volume (R47, R48, R49, R50 rows); the image's
data and labelled volumes on walfs, init's volumes and the steward's home and vault lines on
walfsd: handles; machine cases: power loss before-or-after, hostile medium corrupt, quota, the
steward's session cases on the new volumes; littlefs's retirement from the image (littlefsd,
its cases and pages) per the owner's answer. Tier A, size M. Brief by the Architect at launch."
