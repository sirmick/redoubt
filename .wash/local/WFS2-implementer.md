# WFS2: `walfsd` serves the SSD's writable volumes; littlefs stays for a flash medium

Tier A (a new file server on the serving skeleton, `init`'s volumes, the image, the steward's
two lines, the cases), size M. Needs WFS1 (in review at f7fc42b22: the walfsd.md specification
page, `libs/walfs`, the packer) and, for its last commit, STEWARD2. Start from `main` once WFS1
is on it. The owner's decision (4e1539fe): the logged format; **littlefs stays in the image for
a flash medium**, serving no default-image volume unless a case asks; WFS1 and WFS2 both in M1.
Run everything through `q run` (`.wash/local/RESUME-q.md`).

## Context rules (read these first)

- **Don't read whole files** but the ones named below. `servers/littlefsd/src/server.rs` is the
  template: read `impl FileServer for Littlefsd` (:826-1100) and `volume.rs`, `blkd.rs`,
  `quota.rs`, `typed.rs`, the `bin`; not `pack.rs`, not the tests but their names.
  `libs/walfs/src/lib.rs`'s public surface (`BlockDevice`, `Error`, `Metadata`, `DirEntry`,
  `Filesystem`'s methods: `format`, `mount`, `open`, `create`, `read`, `write`, `seek`,
  `truncate`, `sync`, `close`, `mkdir`, `remove`, `rename`, `read_dir`, `stat`, `get_attr`,
  `set_attr`, `remove_attr`, `set_time`, `check`, `unmount`) by signature.
- **The page is the spec.** `docs/servers/walfsd.md` "The format" is built; this package writes
  "Serving", "Authority", "Security properties" and "Failure and restart" from planned to built
  and closes Serving's Open line. Read the page whole once.
- **Don't open `.wash/qa/*.md` or other packages' reports.**
- **Pipe bench output;** boot logs through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1,900 bytes,** detail in `.wash/local/WFS2-report.md`.

## Reading list (only these)

- `docs/servers/walfsd.md` whole; `docs/servers/littlefsd.md` "Volumes, connections and
  labels", "Typed operations", "Quotas", R47-R50, "Failure and restart"; `docs/servers/erofsd.md`
  "Serving" (the second server's page as built); `docs/servers/init.md` "The boot manifest"
  (`volumes`, `servers`: "a volume's server has `program` `littlefsd`, or `erofsd` …") and "The
  confinement check"; `docs/servers/blkd.md` "Ranges and badges" and its wire table
  (`libs/wire/tables/blkd.md`: whether a flush exists); `docs/servers/steward.md` "The boot
  manifest's steward lines" (`home`, `vault`) and `docs/userland/sessions.md`'s namespace
  example; `docs/servers/README.md` "Naming" and the server table.
- `tests/littlefsd-{boot,quota,corrupt-volume,reboot,restart,one-volume,label-check,confined-labelled}.toml`
  descriptions (the case shapes to mirror); `tools/testbench/src/disk.rs` (`fs = "walfs"`, WFS1's).
- `.wash/local/WFS1-design.md` (candidate A) and WFS1's report for anything the page defers.

## The design (the page rules; this is how to build it)

1. **`servers/walfsd`**, on the serving library as `littlefsd` is: `impl FileServer` over
   `libs/walfs::Filesystem` and a `Range` (the `BlockDevice` is the `blkd` range client, one
   block per sector run as `littlefsd`'s `volume.rs`; `sync` is a flush). A node is the inode
   number and its generation (walfsd.md "Inodes": a removed file is told from a later
   allocation by the generation, so a stale fid gets `removed`, never another file). `attach`
   roots at the root inode or the minted root; `walk` by component through `read_dir`; `open`,
   `create`, `read`, `write` (one transaction each, the format's R50), `remove`, `rename`,
   `stat`, `dir_entry` (the qid's path = inode number, version = generation), the typed
   operations `littlefsd` serves (`set_attr`/`get_attr`/`remove_attr` over the inode's 256-byte
   attribute area, `copy_file`, times), minted connections rooted by `new_connection` as
   `littlefsd`'s, labels the volume's on every node, admission with `buckets`. A corrupt
   volume (mount fails, or a block's hash fails on a read) is served as corrupt: `attach`
   refused with `corrupt`, the server up, every read failing, until it starts again (R49, as
   `littlefsd`'s poisoning). Arguments as `littlefsd`'s (`endpoint=`, `buckets=`); the range
   handle is the volume's as `init` hands it. Memory bounded as the page's Memory bullet says,
   plus one node per open fid; `heap_pages` measured by the memory scan, six runs.
2. **Quotas (R48), closing the page's Open line:** a quota is in **bytes**, as R48 and the
   steward's `quota` argument say, counted as the blocks a root's files and directories hold
   times the block size plus one inode's share per entry (the page states the formula), carved
   from the granter's at `new_connection`; a write that would pass the root's quota is refused
   before the transaction begins. The ledger is `littlefsd`'s `quota.rs` shape, reused if its
   types fit, copied not shared if not (say which).
3. **The `sync` barrier.** The format's atomicity needs the logged blocks on the medium before
   the commit header, and the header before the copies home. Establish what `blkd` offers: if
   its protocol has no flush, add one typed operation `flush` to `blkd` (wire table row,
   blkd.md "Ranges and badges" sentence, a host test), a virtio-blk `FLUSH` on the device, and
   state the bench's QEMU disk cache mode in the case description; `walfsd` calls it where the
   page's "Transactions" section requires. If `blkd` already writes through, say so with the
   evidence and call nothing.
4. **`init`.** A `servers` entry whose `program` is `walfsd` is a volume server like `littlefsd`'s
   and `erofsd`'s: the same `volume` key, the same range handoff, the same confinement check; no
   new manifest key. init.md's sentence gains `walfsd`.
5. **The image.** `image/disk.toml`: the `data` partition `fs = "walfs"`, packed by WFS1's packer
   from the same stage; `image/manifest.json`: `walfsd:data` with `program = "walfsd"` in place
   of `littlefsd:data`, and every argument that named `littlefsd:data` (`beamlet`'s `endpoint=`
   if it names the data volume; the steward's lines, below). The labelled volume
   (`alice-secrets`, STEWARD2's manifest) likewise on walfs, served by `walfsd:alice-secrets`.
   littlefs stays as a format the packer and `littlefsd` serve: `littlefsd` keeps its crate,
   its cases (they build their own partitions) and its pages; the image has no littlefs volume
   unless a case's manifest asks for one.
6. **The steward's lines** (`home "P" handle=walfsd:data path=/home/alice`, `vault "P"
   labels=[…] handle=walfsd:alice-secrets`): a rename of the handle names in STEWARD2's manifest
   and cases; nothing in the steward's code names a format.

### The rules it keeps

R47-R50 are restated on walfsd.md as its own rows, each with its cases (below); SECURITY.md's
four rows gain `walfsd` and the cases. R49 and R50 rest on the format's hash region and log
(WFS1's proofs), and the server adds: a hash failure on a read is `corrupt` for the volume, and
a cut transaction is recovered at mount (the restart case). R25 and R26 are the serving
library's, unchanged. The confinement check (R34) treats `walfsd` as it treats `littlefsd`.

## What can be built before STEWARD2 lands, and what waits

Commits 1-4 need only WFS1: the server and its host tests; `init`'s program; the image's `data`
volume on walfs with `walfsd:data`; the cases below that are `walfsd`'s own. Commit 5 is the
steward's: the labelled volume, the `home` and `vault` handle names, and the steward's session
cases on the new volumes; it is written against STEWARD2's branch and rebased onto `main` when
STEWARD2 merges. A launch before STEWARD2's merge is honest with that split stated: the report
says which commits are on `main`'s base and which on STEWARD2's. BEAM3's `beamlet-files` case
names the data volume's server: coordinate the name with BEAM3's implementer (whichever lands
second renames one line).

## The cases (both widths; system verdicts)

Mirroring `littlefsd`'s, each description naming walfsd.md:
- `walfsd-boot` (format a blank partition, write, read, rename, remove, a directory),
  `walfsd-one-volume` (R47, the one-volume probe feature), `walfsd-quota` (R48: two roots of 64
  KiB, one refused at its quota, the other still writing), `walfsd-corrupt-volume` (R49: a
  partition of noise served as corrupt, the server up), `walfsd-flipped-block` (R49: one bit
  flipped in a data block after the pack; the read of that file is `corrupt`, naming nothing
  else; other files read), `walfsd-reboot` (files and qids survive a reboot on the same disk),
  `walfsd-restart` (a killed server is restarted by `init`; an old connection gets `Dead`; a
  fresh one reads).
- **`walfsd-power-loss`** (R50, the machine's proof beside WFS1's host fuzzer): a test-only
  feature `cut-after-write` makes the server exit after its N-th block write, N from the case's
  seed, during a client's write that spans several blocks and a rename; `init` restarts it;
  recovery runs at mount; the client then finds the file either as before the operation or as
  after it, by content and by `stat`, never between, and the `check` the server runs under the
  feature finds the volume consistent; run as a sweep over seeds. The case description says
  that a process exit after a completed device write is the same medium state as a cut after
  that write, and why that is the right proxy.
- `walfsd-label-check` and `walfsd-confined-labelled` (the volume's labels on every node; the
  confinement check with a labelled walfs volume), both from `littlefsd`'s shapes.
- Commit 5: STEWARD2's `steward-two-sessions` and `steward-vault-session` running on the walfs
  volumes, unchanged in their verdicts; `image-disk` with the new recipe.
- The memory scan (`init-boot`, `userland-boot`, `beamlet-footprint`) with `walfsd:data`'s
  rows: `heap_pages` and `stack_pages` from six runs, in the manifest and testbench.md's table.

## Page lines (exact text in the report)

- `walfsd.md`: "Serving", "Authority", "Security properties" (the four R rows, each a `### R47
  (one volume per instance)`-style heading restated for walfsd with its status list, since C7
  needs a SECURITY row per `### R` heading: write them as walfsd's own rows and add the
  SECURITY rows, or keep one row per rule naming both servers: the checker decides, say which),
  "Failure and restart" to built; Serving's Open line gone; Residual risks: the hash's limits,
  a linear directory lookup, the quota's granularity.
- `SECURITY.md` rows; `init.md`'s volume-server sentence; `servers/README.md`'s table and graph
  (walfsd beside littlefsd and erofsd); `littlefsd.md` Purpose (serves littlefs for a flash
  medium and for its cases; the image's SSD volumes are walfs's); `blkd.md` if `flush` is added;
  `steward.md` and `sessions.md` where a handle name appears; `image/README.md` (the disk's
  volumes); `testbench.md`'s memory table. No dates, package IDs or review history.

## Owned paths

- `servers/walfsd/**` (new); `servers/init` (the `walfsd` program as a volume server: check,
  handoff, tests); `image/disk.toml`, `image/manifest.json` (the `data` and labelled entries,
  the handle names), `image/README.md`; `servers/blkd` and `libs/wire/tables/blkd.md` only for
  `flush` if point 3 needs it; `tests/walfsd-*.toml` and `tests/walfsd-programs/**`;
  `tests/size-budget.toml`, `tests/unsafe-budget.toml` (the new crate's rows, `unsafe` 0); the
  pages above; commit 5: STEWARD2's manifest lines and case files for the handle names.

**Not yours:** `libs/walfs` (WFS1's; a format change is a design question), `servers/littlefsd`
and `libs/littlefs` (unchanged but the Purpose sentence), `servers/erofsd`, the steward's code,
the serving library. **Hotspots:** STEWARD2 (the manifest and the steward's lines: commit 5),
BEAM3 (`beamlet-files`'s volume name), PACK1 (the system volume: untouched here).

## Gates

The short gate (both builds; host tests of `walfsd`, `init`, `blkd` if touched; the docs
checker, `cargo fmt --check`, the size budget with the new rows, the `unsafe` ratchet at 0 for
the crate, the no-cruft gate; the cases above on both widths, the power-loss sweep over at
least eight seeds; the smoke set). The whole bench is the train's. Report each command with its
exit code and the memory-scan rows.

## Not here

Any change to the format (WFS1's page rules); littlefs's retirement (the owner keeps it); a
hash tree or sealed root; a block cache in `walfsd` (measure first: the format has no CTZ
walks, so the read-cost note's problem may not exist here; report the reads per data block from
a `boot-stats`-style counter if one is cheap); file transfer; BEAM3's client.

## Checkpoints

1. After the server's host tests and `walfsd-boot` on one width, before the image moves: one
   progress line with the branch, the quota formula as written, the `sync` finding (point 3),
   and the reads per data block for a sequential read.
2. Before commit 5 if STEWARD2 has not merged: say so and continue on STEWARD2's branch.
