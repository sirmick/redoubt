# BOOT1: a boot profile of the image, then the two read caches, and a boot-time target

Tier A (`fsd`, `verityd` and `libs/littlefs` are the trusted base's servers and their library).
Size M: step 1 S (a measurement), step 2 S (two local caches). Needs VOL1 merged. Don't start
until it is.

**The owner's question:** the image boots to the prompt in 102.9 s unverified and 172.9 s
verified on QEMU rv64 (VOL1's report, point 6). Why so long unverified, and what do two caches
buy? Step 1 answers the first with numbers before step 2 changes anything. The owner has
decided both caches land in the near term; step 1 decides their sizes and shows their effect,
it does not decide whether.

Run everything natively on this host under the job pool's rules (docs/testbench.md "On a shared
host"): the boot cases are guest-time cases (icount, pinned seed) and share the pool; your
`host-tests` run alone.

## Context rules (read these first)

- **Don't read whole files.** `libs/littlefs/src/file.rs`: `read`, `read_content`, `read_ctz`,
  `ctz_find` only; `servers/fsd/src/volume.rs`: `Blocks` only; `servers/verityd`: the read path
  and the tree cache only. VOL1's report: its point-6 section only (lines near "Boot to the
  prompt").
- **Don't open `.wash/qa/*.md` or other packages' reports** but VOL1's point 6.
- **Pipe bench output;** boot logs through `grep` or `tail` (they begin with hex dumps).
- **Never print per-read:** VOL1's first try printed every 128 reads and never reached the
  prompt in 450 s. Counters print once, at the end.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1900 bytes,** detail in `.wash/local/BOOT1-report.md`.

## Reading list (only these)

- `.wash/local/VOL1-report.md`, point 6; `.wash/local/VOL1-implementer.md`, point 6.
- `docs/servers/verityd.md` (VOL1's page): the cost paragraph and its `**Open:**` item.
- `docs/servers/fsd.md`: Interface (the volume as a block device) and Residual risks.
- `docs/userland/beamlet.md`: the `load_module` row; the "beamlet on Redoubt" status line.
- `docs/userland/packages.md#profiles-and-upgrades` (the code path's search order, BL1).
- `docs/testbench.md`: "What a case passes on", "Checked builds" (the `stats`-like features
  pattern: `sched-trace`, `walk-trace`), "On a shared host".
- `libs/littlefs/src/file.rs` (the symbols above); `servers/fsd/src/volume.rs` (`Blocks`);
  `servers/verityd/src` (read path); `tests/userland-boot.toml`.

## The design

### Step 1: measure first

1. **Milestones in guest time.** Every milestone is a console line that already exists or is
   added once, carrying `time_now` (µs since boot, the kernel's clock) in a fixed form
   `[t=N]`: `init` up; each server launched (`init: started X` gains the stamp); beamlet
   started; beamlet's `objects in /boot/system.index` line (OTP's index read); the shell's banner
   (the prompt). `init` prints `time_now` already (`bin/init.rs`, the restart window): reuse it.
   Guest time under `icount` is the same on any host, so the profile is a verdict-grade
   measurement, not a wall-clock one.
2. **Counters, printed once.** A Cargo feature `boot-stats` (off by default, like `sched-trace`;
   never in the image's build) on `fsd`, `verityd` and `blkd`, each keeping counts and printing
   one line when the case's final input is answered (the bench's last `[[input]]` is a line the
   servers cannot see: print on the console at a fixed point instead: `fsd` and `verityd` on
   their 2^16th read as VOL1 did, and once more at power-off, which `init`'s shutdown reaches):
   - `fsd`: 9P operations by type (`walk`, `open`, `read`, `clunk`, `stat`), bytes read, littlefs
     block reads (`bd_read` calls) and how many were pointer (CTZ) reads against data reads, and
     distinct blocks read (a bitmap of the volume's blocks: ~2 K bits);
   - `verityd`: reads served, blocks checked, tree-cache hits, `blkd` calls;
   - `blkd`: requests and sectors.
   - beamlet, under the same feature in `beamlet-redoubt`: module loads (`Found`, `Absent`,
     `Refused`), bytes read, and the VM's own CPU time (`budget_usage`, or the scheduling
     trace's charge to its budget) at the prompt, so the VM's work is told from its waiting.
3. **Where the wall time goes.** From 1 and 2, apportion the boot to: the kernel IPC hops per
   block (three calls per block verified, two unverified: `fsd` → `verityd` → `blkd` → virtio,
   each a copy), littlefs's re-reads (the 26× refetch: `read_ctz` calls `ctz_find` for every
   block-sized piece, and `ctz_find` reads pointer words from block heads each time, so a
   whole-object read of 16 blocks re-reads pointer blocks over and over, and the per-piece
   alternation defeats `verityd`'s last-block buffer), the VM's own loading and parsing (CPU time
   under TCG), the code path's `Absent` probes (BL1's order: an index lookup in memory, no read;
   confirm from the counts), and console output (the banner and `init`'s lines; the `[t=N]`
   stamps themselves must stay under 40 lines).
4. **The case.** `boot-profile` (rv64, the image's manifest, `memory_mib = 1024`, the pinned
   seed of `userland-boot`, both the verified image and the same manifest without `verity`):
   boots to the prompt, types `Enum.sum(1..10)`, and expects the milestone lines, the counter
   lines and `55`; it asserts no time. The bench records it as a measurement: the report's
   table is milestone times and counts for both boots. Run it under `--sweep 1..5` once to show
   the spread (guest time: expect none beyond the seed's PID draw).
5. **Report before step 2:** which candidate causes the numbers support, with the arithmetic
   (reads × per-call cost against the milestone gaps). **Checkpoint here.** Say which of the
   following step 1 revealed as separate packages, not BOOT1's: fewer modules loaded at boot
   (BEAM6's lazy-loading lever, M2); a preloaded code archive (one object holding the boot set,
   so one walk/open/read instead of ~2,000: a packer and beamlet change); read-ahead in `fsd` or
   `verityd` (up to 8 blocks per `blkd` call, VOL1's deferred option); fewer console lines or a
   faster `consoled` path; the VM's parse speed. Name the one with the biggest measured share.

### Step 2: the two caches

6. **`fsd`'s read cache is littlefs's.** The amplification is inside `libs/littlefs`, between
   9P reads of up to 65,512 bytes and `blkd` requests of up to 32 KiB: `read_ctz` fetches one
   block-sized piece at a time and re-walks the skip list for each. Two local changes in
   `libs/littlefs`, no on-disk change:
   - a **read cache** as the C reference's `rcache`: a configured number of whole blocks
     (`Config::read_cache_blocks`), checked before `bd_read`; the default 1 block, and the
     object-sized 16 blocks (64 KiB, one object) if step 1 shows pieces still alternating
     between blocks within one object read: choose from the counts, state the choice;
   - a **CTZ cursor per open file handle**: the (block, offset) the last read ended at, so a
     sequential read continues without `ctz_find` re-walking from the head; invalidated on
     seek and on any write through the handle.
   `fsd` sets the cache size per volume (the image's read-only volumes: the chosen size; writable
   volumes: 1 block, so a writer's view is never stale: the cache is invalidated on `prog` and
   `erase` of its block, and the host tests prove it). Memory: 16 blocks × 4 KiB per volume, in
   `fsd`'s budget; state the new total on fsd.md.
7. **`verityd`'s LRU of checked data blocks.** Beside the tree cache: `DATA_CACHE` whole data
   blocks (start at 4; report the hit rate) that passed their hash, keyed by block number, LRU;
   a read served from it is not re-hashed and makes no `blkd` call. Local to `verityd`, no
   protocol change: it closes verityd.md's `**Open:**` item. A block enters the cache only
   after its hash matched, so nothing unverified is ever served from it (say so on the page).
8. **Measured effect:** `boot-profile` again, both boots, after each cache alone and both
   together: the table in the report and on the pages.
9. **The target.** From the post-cache profile on the pinned seed, state the image's boot-time
   target on QEMU rv64: the prompt within N s of guest time, N = 1.5× the measured
   verified boot rounded up to 10 s, and the same for the unverified boot. It is checked by
   `userland-boot` (it already boots the image): add one expectation that the banner's `[t=N]`
   stamp is under the target, so a regression fails the case. The target is stated on
   docs/userland/beamlet.md's "beamlet on Redoubt" (the case's page) with its measurement, and
   on image/README.md.

### The rules it keeps

R75/R76 (a cached block in `verityd` is one that passed its hash: the cache never widens what
is served); R47 and R49 (littlefs's parsing and corruption handling unchanged; the cache is
below the parser, above the medium); `fsd`'s label check and quota (untouched); the
Responsiveness targets (untouched: this is boot time, not wake latency).

## The cases (rv64; rv32 attempted for `boot-profile` and reported)

1. **`boot-profile`** (point 4), both boots, a measurement with no time assertion.
2. **`userland-boot`** gains the target expectation (point 9), both widths; the rv32 target is
   stated from its own measurement or marked "measured, not gated" if rv32's spread is wide.
3. **Host, `littlefs`:** the read cache serves a second read of a block without `bd_read`; a
   `prog`/`erase` of a cached block invalidates it (a write then a read sees the new bytes); the
   CTZ cursor continues a sequential read with no `ctz_find` (count the pointer reads through
   the test `Storage`); a seek or write resets it; the inline-file path is unchanged. Fuzz: the
   existing littlefs fuzz target runs with the cache on.
4. **Host, `verityd`:** a cached block is served without a `blkd` call and not re-hashed; a
   block that failed its hash never enters the cache; LRU eviction order; the cache is dropped
   when the volume turns corrupt.
5. **Host, `fsd`:** the per-volume cache size, 1 block on a writable volume.

## Page lines (exact text in the report)

- **fsd.md:** Interface, the volume as a block device: the read cache and the CTZ cursor, their
  size per volume and the writable rule; Residual: the memory per volume.
- **verityd.md:** the cost paragraph gains the data LRU and the measured hit rates and boot
  times; the `**Open:**` item on the data cache is closed (the section's status allows it: a
  built section has no `**Open:**`).
- **beamlet.md** "beamlet on Redoubt": the boot-time target with its measurement table (both
  boots, before and after), `bench:boot-profile` and the `userland-boot` expectation listed.
- **image/README.md:** the target, one line.
- **testbench.md** "Checked builds": `boot-stats` beside the other diagnostic features.
- **libs/littlefs's `lib.rs` doc:** the "Differences from the C reference" list: the read cache
  is the reference's `rcache`; the CTZ cursor is ours.

## Owned paths

- `libs/littlefs/src/{file,fs}.rs` (the cache and cursor; no format change), its tests and
  fuzz target; `servers/fsd/src/volume.rs` (the size per volume) and the `boot-stats` counters;
  `servers/verityd/**` (the data LRU, counters); `servers/blkd` (counters only); `servers/init`
  (the `[t=N]` stamps on existing lines); `userland/otp/redoubt` (the beamlet counters and the
  banner's stamp, under the feature); `tests/boot-profile.toml`, `tests/userland-boot.toml`; the
  pages above.

**Not yours:** beamlet's loading order or module set (BEAM6), the packer's objects (a preloaded
archive is its own package), `consoled`, the kernel, any Responsiveness target. **Hotspots:**
VOL1's merge (rebase onto it); BEAM6 (M2) measures the VM's memory, not its time: share the
`boot-stats` feature's name with its brief when written.

## Gates

The whole bench on both widths under the pool's rules; the host tests of `littlefs`, `fsd`,
`verityd`; fmt; the unsafe ratchet (none expected); the size budget (the caches add bytes to
`fsd` and `verityd`: state them); doccheck. Report each command with its exit code, and the
two profile tables.

## Not here

Read-ahead across blocks in `verityd` or `fsd`; fewer or lazier module loads; a preloaded code
archive; console output changes; any change to what `verityd` verifies.

## Checkpoint

After step 1's report (point 5): stop and send it with the branch, before any cache code. The
sizes in step 2 are chosen from it.

## 2026-10-06: rulings from the early checkpoint (architect-15, QA `BOOT1-profile-construction`)

- **Q1.** The index milestone is gone with `system.index`; the milestone is the first object
  read: `beamlet: Elixir.Redoubt.Shell read from fsd:system`, stamped.
- **Q2.** `init`'s `[t=N]` stamps are under `boot-stats` only (the feature on `redoubt-init`
  too); the image's build and every other case see today's lines. The profile case builds `init`
  with the feature.
- **Q3** (ruled again after the red found the always-on line misdraws the shell's first
  prompt). The stamp is under `boot-stats` (the feature on `beamlet-redoubt` too, as Q2 put it
  on `redoubt-init`); the image's build and every other case see today's console. The line is
  `beamlet: first console read [t=N]`, named for what it marks: for the shell that is the prompt
  drawn and waiting, and `boot-profile`'s description says so, with the misdrawn first line as
  that case's known cost. The boot-time target is a regex bound on N in `boot-profile` (built
  with the feature, `icount = "shift=3,sleep=off"` and `qemu_seed`, so its `time_now` is guest
  time and its run repeats). `userland-boot` is untouched: no icount, no seed, no stamp. The
  icount cost is measured, not guessed: report `boot-profile`'s wall time with and without
  icount on this host, and set its `timeout_secs` by that measurement, stated in the report.
  rv32 likewise.
- **Q4.** Yes: `fsd`, `verityd` and `blkd` print their counts at each power of two of requests
  from 2^12 (about five lines each), and `fsd` prints exact counts once more on a walk of a
  fixed name that exists in no volume (`BootStats.x()` typed after the `55`; the walk stays
  `not_found`), under the feature only. Fifteen lines do not move the boot; VOL1's 600 did.
- **Q5.** Yes: beamlet sums guest time spent inside `load_module`/`load_app` and prints loads,
  bytes, time-in-loads and time-since-start at the prompt. The remainder is apportioned by the
  milestone gaps (init to beamlet started: the servers; beamlet started to the first object: the
  VM's own start; first object to the prompt, less time-in-loads: the VM's parsing and the
  console). Say which gap the caches can touch and which they cannot.
- **Q6.** Granted: `tests/boot-profile.toml`, `tests/boot-profile-unverified.toml` (programs
  with `features = ["boot-stats"]`), `tests/data/boot-profile/{manifest-unverified.json,
  userland-unverified.toml}` (the manifest without `verity` and the recipe with
  `verity = false`, so `fsd` sees no tree blocks). They join the owned paths.

## 2026-10-06: refocused by the owner's decision (architect-15): the system volume leaves littlefs

The owner decided the read-only system volume stops using littlefs: a packed read-only format,
served by `erofsd`, replaces it (design docs/servers/erofsd.md; package EROFS1, which needs your
step-1 report). What changes here:

- **Step 1 stands, unchanged, and is the measurement the owner compares.** Finish it on the
  littlefs system volume as planned and report at the checkpoint. The `boot-profile` case stays
  yours and runs again on the pack when EROFS1 has it (EROFS1 runs it as its case 7 with your
  case unchanged; the before/after table goes on erofsd.md's "Why" and beamlet.md).
- **Step 2, point 6 (littlefs's read cache and CTZ cursor) is dropped.** littlefs stays only on
  the writable volumes (`fsd:data`, the labelled volumes), whose reads are a session's files, not
  a boot's 2,000 objects: no measurement says a cache pays there, so none is built; say so in
  the report as a residual on fsd.md ("littlefs reads a block in pieces; a read cache is worth
  building only when a writable volume's read pattern asks for it").
- **Step 2, point 7 (`verityd`'s LRU of checked data blocks) stays**, sized from the profile: on
  the pack, a file's blocks are consecutive, so the LRU sees each block once per object read and
  a small cache (4) suffices; measure on the pack, not littlefs, once EROFS1 lands, and state the
  hit rate. If EROFS1 is not merged when your step 1 is reported, build the LRU against littlefs
  (it is local to `verityd` and format-blind) and EROFS1 re-measures.
- **Point 9, the target,** is set from the pack's profile, not littlefs's: state it when the
  pack's numbers exist (EROFS1's case 7); until then the `[t=N]` expectation in `userland-boot`
  is not added.

**After your step-1 report:** stop at the checkpoint as the brief says. The orchestrator then
either lets you continue with point 7 (the LRU, against littlefs if EROFS1 is not in) or holds
BOOT1 until EROFS1 merges; the pages' lines for the cache are unchanged either way.

## 2026-10-06, later (architect-15): BOOT1 is the measurement only

Step 2 (the `verityd` LRU) and the target move to EROFS1, where the profile they are sized and
set from exists. BOOT1 commits and merges step 1 after VOL1: the `boot-profile` case, the
`boot-stats` feature and the `[t=N]` milestones, with the littlefs numbers in the report
(1016.7 s verified | 534.7 s unverified to the prompt; 0.85 M / 1.62 M instructions per 4 KiB
block). No cache, no target, no `userland-boot` expectation here.
