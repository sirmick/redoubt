# WFS1 report: walfs, its format page, library, oracle, fuzzers and packer

Branch wp-WFS1, head f7fc42b22, on main f7ce1e9b6 (rebased after B19; one conflict set in
tools/testbench/src/disk.rs, its Cargo.toml, docs/testbench.md and tests/formatting.toml, resolved
keeping both sides).

## Commits
- f7fc42b22 testbench: a disk recipe's partition may be fs = "walfs"
- 1a5c343db walfs: fuzz targets for arbitrary images and mutated volumes
- a8907558d walfs: a hostile volume is corrupt where it is read, never a crash
- 744affaa8 walfs: power cut at every block write leaves before or after
- 102b84595 walfs: the format in pure Rust, checked against an in-memory model
- 02a51c7f2 walfsd: the page specifies walfs, the writable SSD volumes' format

## Delivered (paths)
- docs/servers/walfsd.md (new); docs/SUMMARY.md; docs/servers/README.md (Naming, the capability
  table's row, both graphs); docs/servers/littlefsd.md (one Purpose sentence); docs/testbench.md
  (Hostile inputs paragraph; Disks section: `walfs` in the fs list, the packer's test, "a volume may
  be verified/generate"); docs/plan/m1-separation.md (progress bullet; "Not built: ... `walfsd`").
- libs/walfs/** (src: lib, layout, fs, file, ops, check; tests: model, crash, hostile, common/{mod,
  ops, exercise}; fuzz: image, mutate, Cargo.lock, rust-toolchain nightly-2026-05-11).
- tools/testbench/src/disk.rs (fs = "walfs", `pack_walfs`, its test), tests/walfs-host-tests.toml,
  tests/size-budget.toml (libs/walfs max_lines 1705), tests/unsafe-budget.toml (walfs, 0), root
  Cargo.toml (member; fuzz excluded), Cargo.lock.
- Beyond the brief's owned paths, needed by the above: tools/testbench/Cargo.toml (the walfs
  dependency), tests/formatting.toml (the fuzz workspace's root), docs/plan/m1-separation.md and
  testbench.md's Disks section (pages move with the code).

## Format decisions (on the page)
- LOG_BLOCKS 32; max file 4,299,210,752 bytes (12 direct + 1024 + 1024^2 blocks of 4 KiB); u32
  block addresses (16 TiB volumes); 128-byte inode; 256-byte attribute area per inode in a parallel
  table (TLV, value <= 254); 260-byte entries, 15 per block, names <= 255.
- Hash region as the orchestrator decided (option b): 127 slots and the block's own SHA-256 per hash
  block; no slots for the log (logged blocks are checked against per-entry hashes in the
  self-hashed header); a hash block's own slot is zero, it checks itself.
- Commit: log blocks, sync, header (commit), sync, home, sync, cleared header, sync. A header whose
  own hash fails is a torn commit or clear: dropped. A committed one is checked whole, then replayed;
  replay is repeatable.
- Orphan list (inode 0 holds its head) so remove/truncate of any size is one visible transaction,
  freeing finished over later transactions or at mount; the page says what a reader sees between.
- Writes go to the medium in the call (no per-handle buffer); a write that fills the volume ends at
  the last block with room and returns the count.
- SHA-256 from sha2 0.11 (default features off), as libs/verity: libs/sha256 is not on main.
- xv6 reference: mit-pdos/xv6-riscv commit 06aad25c735fd3159bdfae5680be4eab7b1668b2 (kernel/fs.h,
  kernel/log.c); the crate's doc names it by branch and date, since the docs checker refuses
  hashes in comments (C11).

## Tests and gates (head f7fc42b22; via scripts/q and scripts/jobs.mk)
- `q run --cores 4 -- cargo test -p walfs --release`: exit 0 (1 layout + 10 model + 6 crash + 11 hostile).
- `cargo +nightly fmt -p walfs -p testbench --check`: exit 0.
- `q run --cores 8 -- cargo build -p walfs --release --target riscv64gc-unknown-none-elf`: exit 0;
  same for riscv32imac-unknown-none-elf: exit 0.
- jobs.mk: prebuilt rc=0; build-rv64 rc=0; build-rv32 rc=0; rv64/walfs-host-tests PASS rc=0
  (364.9 s); rv64/host-tests PASS rc=0 (quiet; testbench's tests incl.
  a_walfs_partition_is_its_stage_and_two_packs_are_the_same_bytes); docs PASS rc=0;
  rv64/formatting PASS rc=0; rv64/size-budget PASS rc=0; rv64/unsafe-budget PASS rc=0 (walfs 0);
  rv64/no-cruft PASS rc=0; smoke: init-boot, userland-boot, ipc-outcomes, bench-net-peer, each PASS
  rc=0 on rv64 and rv32. make exit 0. No rerun under --quiet was needed.
- Fuzz (by hand, cargo-fuzz, which I installed into ~/.cargo/bin; on the format code of this head,
  before the rebase, which changed only a doc comment in walfs): image 300 s, 72,475,772 runs, no
  findings (cov 37: random bytes never pass the superblock's own hash, by design); mutate 300 s,
  111,369 runs, cov 1196, ft 3562, corpus 633, no findings.
- Hand mutations, reverted: commit header written before the logged blocks -> all 6 crash tests
  fail; no sync before the header is cleared -> all 6 fail; the read hash check disabled ->
  every_flipped_bit_is_corrupt_where_it_is_read fails.
- Attack cases' verdicts come from the system: the hostile and crash tests mount the volume with
  the crate's own code and compare against the model or demand Corrupt; the forged cases rewrite
  hashes as an attacker would, and the crate's parser refuses them.

## Budgets
- Size: libs/walfs 1705 lines (my count: non-blank, non-// lines, the layout test module out); the
  size-budget case accepted it. Unsafe: 0, `forbid(unsafe_code)`.

## Summaries checked
- README.md, GETTING-STARTED.md: no claim about writable volumes or file systems affected; unchanged.
- docs/plan/m1-separation.md: updated (walfs host-tested; walfsd not built).
- docs/servers/README.md: updated (Naming, table row, graphs). littlefsd.md Purpose: updated.
- docs/testbench.md: updated. docs/SECURITY.md: no rows, nothing serves the format yet (as briefed).
- docs/servers/erofsd.md: says writable volumes stay littlefs; still true today, unchanged.

## Page lines (exact)
### walfsd.md Purpose
## Purpose

`walfsd` is to serve a **writable volume** on the SSD in walfs, a write-ahead-log file system of
Redoubt's own: a superblock, a log of whole blocks, a table of inodes, a block bitmap, and a
SHA-256 for every block, checked on every read. littlefs, which serves the writable volumes
today ([littlefsd](littlefsd.md)), is built for raw flash, with wear levelling and erase units
an SSD does not need, and it checksums no data; walfs makes every operation one transaction,
data and metadata together, and finds a damaged block where it is read. ext2 was the other
candidate, with a host tool chain to test against, but it has no journal and no checksums, so
power loss and a hostile medium would be met by rules on top rather than by the format.

### walfsd.md Serving
### Serving

Status: planned · M1 (separation and containment)

`walfsd` will serve walfs as `littlefsd` serves littlefs: one instance per volume, under a
`blkd` range, with the same 9P face, labels and typed operations.

**Open:** what a quota counts (blocks or bytes) and which attribute types `walfsd` serves.

### walfsd.md Authority, Security properties, Failure and restart
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

### walfsd.md Residual risks and Why
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

### littlefsd.md Purpose (the new sentence)
littlefs serves a flash medium and the data volume until walfs takes the SSD's writable volumes ([walfsd](walfsd.md)).

### servers/README.md Naming (the change)
`littlefsd:data` is the data volume; `walfsd` serves walfs.

### SUMMARY.md
  - [walfsd](servers/walfsd.md)

### testbench.md Hostile inputs (new paragraph)
The file systems' parsers are also fuzzed, by hand with `cargo fuzz` and outside the bench, each
from its crate's own `fuzz/` workspace: littlefs's `image` and `mutate`
([littlefsd](servers/littlefsd.md#the-medium-is-hostile)), and beside them walfs's `image`, arbitrary
bytes mounted, walked and written, and `mutate`, a valid volume with bytes changed and its hashes
forged or not ([walfsd](servers/walfsd.md#the-oracle)).

## Open risks
- The hash is not a signature (on the page). A committed header damaged after a cut is dropped, not
  refused (on the page). The volume check reads every in-use block: its cost is the volume's size.
- Format and packer are host-only; nothing in the image uses walfs (WFS2).


## After review (head 0f782d894, on main 8f51851d9)
Folded into the owning commits (the six commits rebuilt from trees, then rebased; each touches only
its own files):
- Editor (8): per-structure corrupt list; bitmap invariant and the volume check defined; littlefsd,
  erofsd and servers/README say the same of littlefs and walfs; 32 × 36 = 1152; LOG = 33 and
  HASH_SLOTS = 127 by name; no time words; the eight operations counted against ops.rs; Naming
  rewrap, both graphs mark walfsd planned.
- Simplifier (13): make_room in write; slot_of through hash_block; blocks_of gone (bmap per block);
  check_holes folded into check_data, count params and too_many_arguments gone; SINGLE/DOUBLE_BASE
  in layout; attr_target, named, touch helpers in ops; named inode and superblock offsets; tests:
  one attrs, one walk(fs, cap), tests/common/forge.rs shared with the fuzz target, one populated;
  INODE, HASH, DIRENT, DIRECT, PER_INDIRECT, MAX_FILE_BLOCKS pub(crate); unmount and geometry()
  deleted; the packer sentence.
- Red (7): directory scans lazy, Corrupt at the first hole, read_dir streamed per block, hostile
  a_directory_sized_as_the_largest_file_is_corrupt_at_its_first_hole; image fuzz target seals block
  0 and its slot (cov 37 -> 53); recover re-checks each logged block's hash before writing it home;
  alloc's `end - b >= 8`; tx.bits cleared after write_through succeeds; page: mount order (fields,
  recovery, then the slot check, since the slot is read from a hash region recovery may have
  rewritten), the packer's transactions and inode-count rule; tests: the freeing crash test also
  cuts the recovering mount (every seventh cut, at each of its writes),
  an_indirect_entry_outside_the_data_region_is_corrupt, a_reserved_bitmap_bit_clear_fails_the_mount.
- Size: libs/walfs 1689 lines (row lowered from 1705 in the crate commit). Unsafe 0.

Page changes after the model was written (the orchestrator's question): the hash region became
127 slots plus the block's own hash with no log slots (orchestrator's decision); the Atomicity
section, including the full-volume write ending at the last block that fits (the model's
full_volume test found the earlier code dropped the whole last transaction); the Memory bullet
(last hash block read); the orphan list's reader-visible state (orchestrator's ask); Security
properties and Open lines (docs checker); the reviews' notes above. The field tables did not change.

Gates on 0f782d894, all exit 0: fmt check; cargo test -p walfs (31 tests); walfs rv64/rv32 target
builds; prebuilt; build-rv64; build-rv32; walfs-host-tests (458.0 s); host-tests (quiet); docs;
formatting; size-budget; unsafe-budget; no-cruft; smoke: init-boot, userland-boot, ipc-outcomes,
bench-net-peer, each on rv64 and rv32. Fuzz 300 s each: image 30,934,283 runs, cov 53, no
findings; mutate 109,466 runs, cov 1214, no findings.
