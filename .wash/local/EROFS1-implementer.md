# EROFS1: `erofsd`, the EROFS read-only volume and its packer; the system volume leaves littlefs

Tier A (a shared server and the packer the image is built with). Size M. Needs VOL1 (merged:
the verifier's range and tree), FSN1 (the rename of `fsd` to `littlefsd`, so every name you
write is final) and BOOT1's step-1 report (the littlefs profile the EROFS boot is compared
against; BOOT1 then profiles EROFS). Don't start until all three.

**The owner's decisions (2026-10-06):** the read-only system volume stops using littlefs
("the wrong tool for the read-only volume"); a read-only EROFS replaces it, built now; littlefs
stays for every writable volume; file servers are named for the format they serve. The design is
docs/servers/erofsd.md, written for this package: read it whole (it is short) and build exactly
it. The 9P face does not change for any client: beamlet's `load_module` walks a name in the
volume's root and reads the file whole, and sees no difference but speed.

Run everything natively on this host under the job pool's rules (docs/testbench.md "On a shared
host"); boot cases share the pool, your `host-tests` runs alone. `mkfs.erofs` and `fsck.erofs`
(erofs-utils 1.9) are host tools: add `erofs-utils` to `scripts/setup.sh`'s one prerequisites
list and GETTING-STARTED's (TOOL1's rule: no other install path; the owner runs the install).
Without `mkfs.erofs` the oracle cases report a **named skip** the way the loopback kind does
(`Unusable`, docs/testbench.md: the bench fails on it unless run with `--allow-skip`), so a host
without it still runs the rest of the bench on request, and a merge gate never passes with the
oracle silently absent.

## Context rules (read these first)

- **Don't read whole files** but the ones named below. `servers/littlefsd` (formerly `fsd`)
  only by symbol (`Range`, `Blocks`, `Args`, the `bin`); `libs/rt/src/server/ninep.rs` only
  `FileServer` and the types it names.
- **The EROFS layout comes from the kernel's documentation** (`Documentation/filesystems/erofs.rst`
  in a Linux tree, and `fs/erofs/erofs_fs.h` for the field widths); read those two, not the
  kernel's implementation. Name the kernel version you read in the crate's doc comment.
- **Don't open `.wash/qa/*.md` or other packages' reports** but BOOT1's step-1 section.
- **Pipe bench output;** boot logs through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1900 bytes,** detail in `.wash/local/EROFS1-report.md`.

## Reading list (only these)

- `docs/servers/erofsd.md` whole; `docs/servers/bootfsd.md` "Serving `/boot`"; `docs/servers/fsd.md`
  "Volumes, connections and labels", "littlefs" (the C-reference oracle pattern), R47, R49;
  `docs/servers/verityd.md` "The tree", "Arguments"; `docs/servers/init.md` "The boot
  manifest" (`volumes`, `servers`), the boot's step 5; `docs/servers/README.md` "Naming".
- `servers/bootfsd/src/server.rs` whole (292 lines: the template for a read-only `FileServer`);
  `servers/littlefsd/src/volume.rs` (`Range`, `Geometry`, `Blocks`), `.../blkd.rs` (the range
  client), `.../bin` (how a volume server starts); `tools/testbench/src/disk.rs` (`Partition`,
  `pack_disk`, `tree`); `libs/verity/src/lib.rs` (`Geometry`, `BLOCK`); `libs/littlefs/diff/`
  (how the C reference is driven as an oracle).
- `.wash/local/BOOT1-report.md`, the step-1 section.

## The design (the page rules; this is how to build it)

1. **`libs/erofs`**, `no_std` + `alloc`, no `unsafe`, no dependencies: the on-disk structures of
   the uncompressed subset, in one crate shared by the parser and the writer.
   - Reading: `Superblock::parse(&[u8])` (magic, block size 4096 only, root nid, meta block
     address, block count against the range), `Inode::parse(&[u8], nid)` (compact and extended;
     mode, size, nlink, layout flat plain or flat inline, raw block address, xattr count: the
     xattr area's size is computed and bounds-checked, never read), `Dirents::parse(block)`
     (count from the first name offset, every name offset inside the block, names sorted:
     `lookup(name)` by binary search, `iter()` in order). Every other layout, an inode outside the
     inode area, a block past the count: `Corrupt`.
   - Writing: `Writer::pack(tree) -> Vec<u8>`: superblock, inodes (extended, for 64-bit sizes
     where needed), flat plain for files, flat inline for tails, sorted dirent blocks, the
     `user.sha256` attribute per file, a fixed timestamp, deterministic.
   - A fuzz target over `Superblock::parse` + `Inode::parse` + `Dirents::parse` (fuzz what
     parses); a differential test on the host: `mkfs.erofs -E ^fragments,^dedupe
     -C 0`-style options that keep the image in the subset (find the exact flags for 1.9 that
     disable compression and chunking; record them), packing the same tree, mounted and read
     by our parser with equal results; and `fsck.erofs` on what our writer packs.
2. **`servers/erofsd`**, on the serving library as `bootfsd` is: `impl FileServer` over
   `libs/erofs` and a `Range`. A node is an inode number plus the inode read once at walk;
   `attach` roots at the root inode; `walk` by component reads the directory's blocks and
   finds the name; `open` for reading only; `read` is one range read of the file's blocks from
   `raw + offset / BLOCK`, cut to the size, with an inline tail copied from the inode's block;
   `stat` and `dir_entry` from the inode (size, type; the qid's path = the nid, version 0);
   `write`, `create`, `remove`, open-for-write: `read-only`. Minted connections rooted by
   `new_connection` as `littlefsd`'s. Labels: the volume's, on every node. Admission with
   `buckets`. A corrupt volume: `attach` refused with `corrupt`, the server up; a read the
   range fails poisons the volume as `littlefsd`'s does (R49). Arguments as `littlefsd`'s
   (`endpoint=`, `buckets=`); the range handle is the volume's as `init` hands it. One block of
   scratch; one inode per open fid.
3. **`init`.** A `servers` entry whose `program` is `erofsd` is a volume server like
   `littlefsd`'s: the same `volume` key, the same range handoff, the same confinement check. No
   new manifest key: the program names the format (the convention on servers/README.md
   "Naming"). The image's system entry becomes `program = "erofsd"`, named `erofsd:system`, and
   beamlet's argument `endpoint=` follows.
4. **The packer.** `tools/testbench/src/disk.rs`: `fs = "erofs"` packs the staged tree with
   `libs/erofs`'s writer; `--pack-disk` and the bench's own staging both. `image/userland.toml`
   says `fs = "erofs"`. The verified volume: the tree after the image, as VOL1's packer writes
   it for any volume. `littlefsd`'s own packer stays for littlefs volumes.
5. **littlefs stays** for every writable volume (`littlefsd:data`, the labelled volumes).
   `littlefsd` and `libs/littlefs` change nothing in this package.

### The rules it keeps

R47 (one volume per instance; the smaller parser), R49 (corrupt, not a crash), R25 and R26
through the serving library, R75/R76 (the system volume stays verified; `erofsd` reads through
`verityd` exactly as `littlefsd` did), R46 (`/boot` is `bootfsd`'s; nothing moves there).

## The cases (both widths; system verdicts)

1. **`userland-boot`**, **`userland-bad-start`**, **`userland-read-only`**, VOL1's `verity-*`
   cases and the confined system-volume case: unchanged expectations, now through `erofsd`
   (the flipped-block refusal comes from `verityd`, the `failed` read from `erofsd`).
2. **`erofs-corrupt`**: the superblock magic flipped: `erofsd` serves corrupt, beamlet parks,
   `init` restarts nothing; an inode naming a block past the count: the same; a directory block
   with a name offset past its end: the same; a compressed-layout inode: the same (refused as
   outside the subset). One boot with several volumes, or several boots.
3. **`erofs-read-only`**: `create`, `write`, `remove` refused `read-only`; a walk to a
   directory lists exactly its children, in order; `..` never leaves the root; a file whose
   tail is inline reads whole and byte-equal.
4. **Host, `erofs`:** every superblock field out of range; both inode sizes; both layouts; the
   xattr skip's bounds; dirent parsing (count, offsets, order, duplicates); `lookup` and `iter`
   against a random tree compared with the staged directory; the writer deterministic (two
   packs of one tree byte-equal); the differential against `mkfs.erofs` and `fsck.erofs`; the
   fuzz target.
5. **Host, `erofsd`:** the `FileServer` through `bootfsd`'s harness pattern: walk, reads at
   block boundaries, past the end, across an inline tail; `stat`; directory reads by page; the
   refusals; the corrupt path; the poisoned-after-range-failure path.
6. **Host, `init`:** an `erofsd` entry is checked as a volume server (the confinement tests
   gain one read-only volume).
7. **The profile:** BOOT1's `boot-profile` on EROFS, both boots, in your report beside BOOT1's
   littlefs numbers (BOOT1 keeps the case; you run it).

## Page lines (exact text in the report)

- **erofsd.md:** every status line to `built · tested` with the cases; Residual risks stand,
  the first naming the exact `mkfs.erofs` options used.
- **fsd.md** (now `littlefsd`'s page, renamed by FSN1): "littlefs" gains one sentence: read-only
  volumes are EROFS, served by [`erofsd`](erofsd.md); "Why" gains the reason in one sentence.
  R47 and R49 status lines name `erofsd`'s cases where they apply.
- **init.md:** "The boot manifest", `servers`: `program` is `littlefsd` or `erofsd` for a
  volume server; step 5 names both.
- **servers/README.md:** the holdings row is in; the server graph gains `erofsd`; the
  trust-tier table's "each `fsd`" row names both file servers.
- **beamlet.md:** the `load_module` row names the system volume's server (`erofsd`);
  "beamlet on Redoubt" status lists the cases.
- **boot.md** R75: the text unchanged; its tests list.
- **testbench.md** "Disks and network cards": the recipe's `fs = "erofs"`; the size budget
  gains `erofsd` and `libs/erofs`. **GETTING-STARTED.md:** erofs-utils in the prerequisites.
- **image/README.md:** the system volume is EROFS.
- **SECURITY.md:** R47's and R49's rows gain the cases; R75's row names `erofsd`.

## Owned paths

- `libs/erofs/**`, `servers/erofsd/**` (new); `tools/testbench/src/disk.rs` (`fs = "erofs"`);
  `image/userland.toml`, `image/manifest.json` (the system entry), `image/boot.toml` if it
  lists programs; `servers/init` (the `erofsd` program as a volume server: the check and the
  handoff, tests); `userland/otp/redoubt` only the `endpoint=` argument; `scripts/setup.sh` and
  GETTING-STARTED.md (erofs-utils); the cases and pages above.

**Not yours:** `servers/littlefsd` and `libs/littlefs` (unchanged); `servers/verityd` (BOOT1's
LRU); beamlet's loading. **Hotspots:** BOOT1 (the profile case and `verityd`: rebase onto
whichever lands first; run its case as case 7); STEWARD2 edits `image/manifest.json`
(principals): rebase.

## Gates

The whole bench on both widths under the pool's rules; the host tests of `erofs`, `erofsd`,
`init`; fmt; the unsafe ratchet (none: `libs/erofs` and `erofsd` have no `unsafe`); the size
budget (two new crates: state them); doccheck. Report each command with its exit code, and the
profile table of case 7.

## Not here

Any change to littlefs or `littlefsd`; compressed or chunked EROFS; extended attributes served
to clients; a second read-only volume on the image (packages are M5's); a writable file system
for an SSD (the owner's later question).

## Checkpoint

After `libs/erofs` with its host tests, the differential against erofs-utils and the fuzz
target (point 1), one progress line with the branch, before the server.

## 2026-10-06: BOOT1's step 2 moves here (architect-15, orchestrator's note on `BOOT1-profile-construction`)

BOOT1 is the measurement only (its step 1 commits and merges after VOL1: the `boot-profile`
case, the `boot-stats` feature, the `[t=N]` milestones; no cache, no target). Its numbers on
littlefs: the prompt at **1016.7 s** of guest time verified and **534.7 s** unverified, about
0.85 M instructions per 4 KiB block read unverified and 1.62 M verified (the `fsd` → `verityd`
→ `blkd` 9P hops, virtio, the copies). EROFS1 adds, after its points 1 to 5:

6. **`verityd`'s LRU of checked data blocks**, sized from the EROFS profile: a file's blocks are
   consecutive, so each block is read once per object and the gain is small; start at 4 blocks,
   report the hit rate, and keep it only if the profile shows re-reads it serves (a cache that
   serves nothing is deleted, not kept). Local to `verityd`, no protocol change; it closes
   verityd.md's `**Open:**` item either way (built, or closed as not needed, with the numbers).
   `servers/verityd` joins the owned paths for this.
7. **The boot-time target**, from EROFS1's measured boot on the pinned seed: the prompt within
   N s of guest time, N = 1.5× the measured verified boot rounded up to 10 s, and the same for
   the unverified boot; gated by a `[t=N]` expectation on beamlet's prompt line in
   `userland-boot`, both widths (rv32's from its own measurement, or "measured, not gated" if its
   spread is wide). Stated on beamlet.md "beamlet on Redoubt" and image/README.md.
8. **The before/after table** on beamlet.md "beamlet on Redoubt": littlefs 1016.7 | 534.7 s
   (verified | unverified) against EROFS measured, both widths, with `bench:boot-profile` named.
9. **The per-read cost, for the next cut.** Report, from `boot-profile` on EROFS, the
   instructions per 4 KiB block read (verified and unverified) beside BOOT1's 0.85 M / 1.62 M,
   and the counts: 9P reads at `erofsd`, `verityd` reads at `blkd`, bytes per read. The package
   after this one (fewer hops, or bigger reads per hop) is cut from those numbers, not here.

The gates gain `userland-boot` with its target on both widths. The report's table is the owner's
view of the gain.
