# WFS1: walfs, a logged file system of Redoubt's own: the format, its library, its oracle and its packer

Tier A (a new format crate and its packer; the page that specifies the format), size M. Needs
EROFS1 (merged: the second file server's pattern and the host-writer pattern are its). Start
from `main` once EROFS1 is on it. The owner's decision (4e1539fe, 2026-10-06): the logged
format, "the xv6-style fs + log" with per-block hashes; littlefs **stays** in the image for a
flash medium (no retirement); WFS1 and WFS2 both in M1. The design note is
`.wash/local/WFS1-design.md`; this brief is how to build its first half. WFS2 (the server
`walfsd`, the volumes' move, the steward's lines) is not here. Run everything natively on this
host under the job pool's rules.

## Context rules (read these first)

- **Don't read whole files** but the ones named below. `libs/littlefs` is the pattern for the
  crate's shape and oracle, not for the format: read its `lib.rs` header, `tests/model.rs`,
  `tests/crash.rs`, `tests/hostile.rs` and `fuzz/fuzz_targets/{image,mutate}.rs` for how a
  model, a power-loss harness and an image fuzzer are driven; not `fs.rs`, `file.rs` or
  `mdir.rs`.
- **The format is yours to specify before you code it.** The specification page is written
  first and the crate implements the page; a field the page does not have is a field the crate
  does not have. xv6's `fs.h` and `log.c` (the MIT xv6-riscv tree) are the reference for the
  shape; read those two files and name the commit in the crate's doc comment. Nothing of xv6's
  is copied: the layout is Redoubt's (4 KiB blocks, hashes, 64-bit sizes).
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe bench output;** boot logs through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1,900 bytes,** detail in `.wash/local/WFS1-report.md`.

## Reading list (only these)

- `docs/servers/erofsd.md` whole (the page shape for a format with a packer: "The format",
  "Serving", "The packer"); `docs/servers/littlefsd.md` "littlefs", "The medium is hostile",
  "Power loss", R49, R50 (the rules a writable volume must meet; walfs restates them for
  itself in WFS2); `docs/servers/README.md` "Naming"; `docs/servers/wire.md` "Generated
  clients" only for how a table page is included (the format page is prose and tables, not a
  wire table); `docs/testbench.md` "The size budget", "The unsafe budget", "Hostile inputs".
- `tools/testbench/src/disk.rs` (`Partition`, `pack_disk`, `tree`: where a volume's packer is
  called by `fs = "..."`); `libs/verity/src/lib.rs` (`Geometry`, `BLOCK`, and how it hashes:
  `sha2` today); `libs/sha256` if it is on `main` (it lands with STEWARD2).
- `.wash/local/WFS1-design.md` (the candidate A section only).

## The design (the page rules; this is how to build it)

1. **The specification page, `docs/servers/walfsd.md`,** on erofsd.md's shape, with "The
   format" and "The packer" built by this package and "Serving" planned with one **Open:** line
   (WFS2's). SUMMARY.md lists it after erofsd; servers/README.md's table and graph get a
   planned row. The format section states, as tables of fields with widths and byte offsets:
   - **Geometry:** 4 KiB blocks; block 0 the superblock (magic, version 1, block count, the
     log's start and length, the inode table's start and count, the hash region's start, the
     bitmap's start, the data start, the root inode number, a superblock hash over its own
     fields); every region's bounds checked against the block count at mount; a block address
     outside its region is `Corrupt`.
   - **The log:** a header block (a transaction's count and its block list, with a commit flag
     and the header's own hash) followed by the logged blocks; one transaction at a time, at
     most `LOG_BLOCKS` blocks (fixed by the page, enough for the largest operation: a `write`
     of one data block plus its inode, bitmap, directory, indirect and hash-region blocks; a
     write larger than that is several transactions, each before-or-after). Commit = the
     header written with the flag set after every logged block is on the medium; recovery at
     mount copies a committed transaction home and clears the header; an uncommitted one is
     dropped. The device's write ordering is asked for by `sync` between the logged blocks and
     the header, and between the header and the copies home (the `BlockDevice` trait gets
     `sync`, as the kernel's `blkd` protocol allows a flush; say what blkd offers today and
     what the host packer does).
   - **Inodes:** fixed 128-byte records in the inode table: type (file, dir), nlink, size
     (u64), 12 direct block addresses, one single-indirect and one double-indirect (sizes to
     the volumes' needs: say the maximum file size the page gives); mtime (µs since the Unix
     epoch, 0 until a clock exists); a generation number for `removed` detection by the server.
   - **Directories:** arrays of fixed entries (inode number u32, name length, name up to 255
     bytes: the entry size the page fixes), unsorted, linear lookup (the page says so; a
     volume of thousands of entries per directory is not the target); no `.` or `..` entries
     stored (the server synthesises nothing; 9P needs none).
   - **The bitmap:** one bit per block, in its region.
   - **The hash region:** one 32-byte SHA-256 per block of the volume (the superblock, log and
     hash blocks included, each hash covering the block's 4 KiB), 128 hashes per block, so
     the region is block_count / 128 blocks. A write of block b writes its hash slot in the
     same transaction. A read of any block verifies it against its slot before parsing;
     a mismatch is `Corrupt` for that read (R49's word), never a panic, and the page says
     plainly what this detects (a damaged or lying medium) and what it does not (a medium that
     rewrites a block and its slot together; not a signature, not a tree).
   - **Atomicity:** the page states R50 for walfs exactly: every operation (`create`, `write`,
     `truncate`, `rename`, `remove`, `mkdir`, `set_attr` if kept) is one transaction, data and
     metadata together; power loss leaves the volume as before or after it. Attributes: the
     page says whether walfs carries user attributes (littlefsd's typed `set_attr`/`get_attr`
     need a home: an attribute block per inode, or a fixed area in the inode; recommend a
     fixed 256-byte area per inode in a parallel table, decided on the page).
2. **`libs/walfs`**, `no_std` + `alloc`, `forbid(unsafe_code)`, dependencies: `libs/sha256`
   if on `main`, else `sha2` with `default-features = false` as `libs/verity` does, switching
   to `libs/sha256` at the fold if STEWARD2 has merged by then (say which). `BlockDevice` as
   littlefs's (read, write whole blocks, `sync`), so the server's `blkd` range and the host's
   RAM device both fit. `Filesystem::format(device, geometry)`, `mount(device)` (recovery
   runs here), `open`/`create`/`read`/`write`/`seek`/`truncate`/`sync`/`close`, `mkdir`,
   `remove`, `rename`, `read_dir`, `stat`, attributes if the page keeps them, and a `check`
   that walks the volume and reports every refusal by name. Memory bounded: one block buffer
   per open transaction's logged block (at most `LOG_BLOCKS`), one per open file that is
   writing, the bitmap read whole (`block_count / 8` bytes); the page's Memory bullet states
   it. Every parse is bounds-checked against the superblock; corruption is an error value,
   never a panic (`hostile.rs`-style tests prove it).
3. **The oracle,** three parts, the littlefs harness's shape:
   - **`tests/model.rs`:** an in-memory model file system; random operation sequences (create,
     write at offsets, truncate, rename across directories, remove with handles open, mkdir,
     read_dir, volumes run full) checked against the model after every step, on small and
     large block counts.
   - **`tests/crash.rs`, the power-loss fuzzer:** a `BlockDevice` that fails the run after the
     N-th block write, for every N of an operation sequence; after each cut, `mount` runs
     recovery and the result must equal the model's state before or after the cut operation,
     never anything else; also a cut inside the recovery itself (a second cut while copying
     home) must leave the same before-or-after. This is R50 proven, not argued.
   - **`fuzz/fuzz_targets/{image,mutate}.rs`:** random bytes mounted and walked; a valid image
     with random mutations (a flipped bit in any block: the hash region must catch it as
     `Corrupt` on that block's read, and nothing else may change) mounted, walked and written.
   The host-tests case `walfs-host-tests` runs the first two; the fuzz targets are run by
   hand and said so on the page, as littlefs's are.
4. **The packer:** `tools/testbench/src/disk.rs` gains `fs = "walfs"`: a partition packed from
   a stage tree through `libs/walfs`'s own code (format, create every entry, one transaction
   each, `sync`), deterministic from its inputs (a host test packs twice and compares; mtimes
   0). `image/disk.toml` does not change in this package (WFS2 moves the volumes).

### The rules it keeps

R47, R48 and R49's statements stay littlefsd's until WFS2 restates them for walfsd; this
package's page carries no R-rule rows (nothing serves the format yet) and no SECURITY rows. The
no-C-on-the-target tenet, bounded memory, fail-closed parsing: in the crate's tests and the
page's Memory bullet.

## The cases (host; no boot in this package)

`walfs-host-tests` (the crate's tests: model, crash, hostile, attributes, the packer's
determinism, `size_of` asserts for the inode and the superblock layout against the page's
tables); the testbench's own tests for `fs = "walfs"`. Both the fuzz targets built and run once
by hand with the result in the report.

## Page lines (exact text in the report)

- `docs/servers/walfsd.md` new: Purpose (what it is for, why not littlefs here, why not ext2,
  in three sentences from the design note); Interface: "The format" (built; status: the host
  tests and the fuzz targets by name), "The packer" (built), "Serving" (planned, status line
  "planned · M1", one **Open:** line naming what WFS2 decides: the quota's unit and the
  attribute surface, nothing else); Authority (planned, one sentence); Failure and restart
  (planned); Residual risks: what the hash does not catch; a directory lookup is linear; no
  wear levelling (an SSD levels its own); Why: a format of our own, the model oracle in place
  of a C one, the log.
- `docs/SUMMARY.md`, `docs/servers/README.md` (the Naming section's example gains `walfsd`
  serves walfs; the table's planned row); `docs/servers/littlefsd.md` Purpose gains one
  sentence: littlefs serves a flash medium and the data volume until walfs takes the SSD's
  writable volumes (no package names). `docs/testbench.md` "Hostile inputs" lists the two fuzz
  targets beside littlefs's. No dates, package IDs or review history.

## Owned paths

- `libs/walfs/**` (new, with `tests/` and `fuzz/`), `tools/testbench/src/disk.rs` (the new
  `fs` value and its test), `tests/host-tests.toml` (the case), `tests/size-budget.toml` and
  `tests/unsafe-budget.toml` (the new crate's rows: `max_lines` set at the fold from what was
  written, with the number in the report; `unsafe` 0), the root `Cargo.toml` (member), the
  pages above.

**Not yours:** `servers/littlefsd` and `libs/littlefs` (unchanged but the one Purpose
sentence); `servers/erofsd`, `libs/erofs`; `image/**`; `servers/init`; the steward; anything a
server does with the format (WFS2). **Hotspots:** STEWARD2 (`libs/sha256`: rebase onto it if
it merges first and switch the dependency); EROFS1's `disk.rs` changes (start after its merge).

## Gates

The short gate: both builds (the crate is `no_std` and built for rv64 and rv32 as a workspace
member even before a server uses it); host tests of `walfs` and `testbench`; the docs checker,
`cargo fmt --check`, the size budget (the new crate's row), the `unsafe` ratchet (0 for the
crate), the no-cruft gate; the smoke set (`userland-boot`, `init-boot`, `bench-net-peer`,
`ipc-outcomes`) on both widths, unchanged in substance. The whole bench is the train's. Report
each command with its exit code, and the fuzz runs' length and findings.

## Not here

The server (`walfsd`, WFS2); moving `/home` or `/vault`; `init`'s volumes; the steward's lines;
littlefs's retirement (the owner keeps it); compression; a hash tree or a sealed root (a later
question, named on the page as what the hash region is not); hard links and symlinks (9P2000
has none); times from a clock that does not exist yet (mtime 0 until M5).

## Checkpoint

**One, after the specification page and the model** (points 1 and 3's `model.rs`), before the
crash fuzzer, the hostile tests and the packer: a progress line with the branch, the page's
field tables in place, the crate's line count so far, and the `LOG_BLOCKS` and maximum file
size the page fixed, so the format is reviewed once before the parts that depend on it.
