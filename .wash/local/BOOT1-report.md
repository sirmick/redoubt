# BOOT1 implementer report

Branch wp-BOOT1, worktree /home/mcloonan/redoubt/.worktrees/BOOT1, base 2f7025638 (VOL1's head).

## Early checkpoint (2026-10-05): step 1 edit list and contradictions

### Step 1, per-file edit list (nothing written yet)

- `servers/init/Cargo.toml`: feature `boot-stats` (off by default; see Q2).
- `servers/init/src/bin/init.rs`: under the feature, `init: started X[, console ID] [t=N]`
  (line 507-517, `time_now` as the restart window already reads it, line 623) and one
  `init: up [t=N]` line once `init` writes anywhere.
- `servers/fsd/Cargo.toml`: feature `boot-stats`.
- `servers/fsd/src/volume.rs` (`Blocks`, under the feature): `bd_read` calls, pointer reads (the
  4-byte `read_pointer` reads `ctz_find` makes, told by length) against data reads, bytes, and a
  bitmap of distinct blocks read (`count / 8` bytes, ~250 B for the image's ~2 K blocks).
- `servers/fsd/src/server.rs` (or wherever 9P is dispatched): counts per `walk`, `open`, `read`,
  `clunk`, `stat`, bytes returned by `read`; one line `fsd: boot-stats ...` at the print points (Q4).
- `servers/verityd/Cargo.toml`, `src/volume.rs`/`src/server.rs`: reads served, data blocks
  checked, tree-cache hits (`Counts` exists), `blkd` calls; one line at the print points.
- `servers/blkd/Cargo.toml`, `src/bin/blkd.rs` (or its server): requests and sectors; one line.
- `userland/otp/redoubt/Cargo.toml`, `src/lib.rs` (`load`, `console_read`): `Found`/`Absent`/
  `Refused` counts, bytes read, guest time spent inside loads (Q5); the prompt stamp (Q3).
- `tests/boot-profile.toml` (+ Q6's unverified twin and its data files).
- `docs/testbench.md` "Checked builds": `boot-stats` beside the other diagnostic features, in
  the commit that adds the feature.

### Contradictions and gaps (blocking questions)

Q1. Stale milestone: beamlet's `objects in /boot/system.index` line no longer exists (VOL1
deleted `system.index`; grep finds it only in old QA). Proposal: the existing
`beamlet: Elixir.Redoubt.Shell read from fsd:system` line is the "shell loaded" milestone instead.

Q2. `init: started X` gaining `[t=N]` always on breaks 51 case files (79 patterns anchored
`...console [0-9a-f]{16}$`), none of them mine. Proposal: the stamps on `init`'s lines only under
`boot-stats` on `redoubt-init` too (the profile case builds `init` with it).

Q3. The banner and the target. The banner is printed by `Redoubt.Shell` (`userland/shell`, not
owned); the brief puts "the banner's stamp" in `beamlet-redoubt` under the feature, but point 9
needs that stamp in `userland-boot`, which boots `image/boot.toml` (a recipe case cannot take
features) and has neither `icount` nor `qemu_seed` (the brief's "the pinned seed of
userland-boot" does not exist): without `icount`, `time_now` is host time. Proposal: (a)
`beamlet-redoubt` prints one always-on line at the VM's first `console_read` (the prompt waiting
for input): `beamlet: at the prompt [t=N]`; (b) `userland-boot` gains `icount =
"shift=3,sleep=off"` and a pinned seed, so N is guest time, with a regex bound on N. Cost: under
icount the guest clock is 125 M instructions/s, so N is guest time, not the 103 s wall figure;
userland-boot's wall time and timeout may move. Alternative: the target lives in `boot-profile`
only (but the brief says it asserts no time).

Q4. Print points. No shutdown reaches the servers (`init` resets only on failure; the shell's
`exit` ends the VM, not the machine), and `redoubt_rt::server::serve` has no receive timeout to
print when quiet (`libs/rt` not owned). Proposal: each of `fsd`, `verityd`, `blkd` prints its
line at each power of two of its requests from 2^12 (about 5 lines each through the boot), and
`fsd` prints once more, exactly at the prompt, on a walk of a fixed name under the feature: the
case types `BootStats.x()` after `Enum.sum(1..10)`, beamlet looks up `Elixir.BootStats.beam`,
`fsd` prints its counts and answers `not_found` as usual. verityd's and blkd's final counts are
then bracketed (and blkd = verityd's calls + fsd's reads unverified), fsd's exact.

Q5. The VM's CPU time: `budget_usage` reports pages, processes and weight, no CPU time; the
scheduling trace needs a checked kernel and its ring output, which would distort the boot.
Proposal: under the feature beamlet sums guest time spent inside each `load` (blocked on
fsd→verityd→blkd), and prints at the prompt: loads, bytes, time in loads, time since start. On
one hart the rest is the VM's own work plus console output (servers idle otherwise).

Q6. The unverified boot needs a manifest without `verity` on `system` and without
`verity:system`, and a userland recipe with `verity = false`: the verified partition holds the
tree past the littlefs volume, so `fsd` on the whole range sees a block count unequal to the
superblock's and serves it corrupt. A case is one boot per width, and a recipe case cannot take
features. Proposal: `tests/boot-profile.toml` and `tests/boot-profile-unverified.toml`, both
listing the image's programs explicitly with `features = ["boot-stats"]` on init, blkd,
verityd, fsd and beamlet; `tests/data/boot-profile/manifest-unverified.json` and
`userland-unverified.toml` (copies of the image's, verity removed). Paths outside the brief's
owned list: these data files and the second case.

Also: the brief says "Needs VOL1 merged. Don't start until it is"; the assignment starts on
VOL1's head, as instructed.

## Step 1, first verified profile (rv64, seed 1, 2026-10-05)

Run A, `icount = "shift=3,sleep=off"` (as ruled): FAILED. `verityd: block 236 could not be read`
right after `init: the boot is done`, then `fsd:system` served the volume as corrupt. With
sleep=off an idle guest's clock jumps to the next deadline; while `blkd` waits for the host's
virtio completion that deadline is its own `REQUEST_TIMEOUT_US` (10 s, servers/blkd/src/virtio.rs),
so the read times out before the host finishes it. No existing icount case has a disk.

Run B, `icount = "shift=3"` (sleep on): reached the prompt, 55, BootStats sentinel. The bench then
waited on two wrong patterns of mine (the prompt has no newline, so beamlet's line shares its
console line and `55` prints alone); the job was stopped at my 1 h limit (build ~35 min of it).
Patterns fixed. Console log: target/testbench/run-3476305-1791264747788384386 (22:32:34-22:36:31
wall: ~237 s of boot wall time for 1016.7 s of guest time).

Milestones (guest µs): init up 38,693; beamlet started 826,595; boot done 865,246; Redoubt.Shell
read (first object) 10,840,218; at the prompt 1,016,702,307.

Counts at the prompt:
- beamlet: 96 loads (96 found, 0 absent, 0 refused), 1,895,790 bytes, 999.29 s in loads,
  1,005.81 s since the platform started.
- fsd (sentinel, exact): 9P 652 (walk 112, open 111, read 318, clunk 111, stat 0), 2,264,058
  bytes read; littlefs reads 77,710 (4-byte words 52,058, whole blocks at offset 0 25,040, pieces
  612), 104,708,366 bytes, 673 distinct blocks.
- verityd at 65,536 reads: 51,313 data blocks checked, 51,295 level-1 hits, 51,332 blkd reads.
- blkd at 32,768 reads: 262,144 sectors (8 per read: every read one 4 KiB block).

## Step 1 report: the boot apportioned (rv64, seed 1, `icount = "shift=3"`, sleep on)

Guest time at shift=3 is 125 M instructions/s; with sleep on, idle time is the host's (small:
the boot is CPU-bound, guest time runs ahead of wall). Verified: target/testbench/
run-3476305-1791264747788384386; unverified: run-3507450-1791266179874430530 (PASS, 150.3 s wall).

### Milestones (guest s)

| milestone | verified | unverified |
| --- | ---: | ---: |
| init up | 0.039 | 0.039 |
| beamlet started (`init: started beamlet`) | 0.827 | 0.755 |
| init: the boot is done | 0.865 | 0.791 |
| first object (`Elixir.Redoubt.Shell read from fsd:system`) | 10.840 | 6.105 |
| at the prompt (`beamlet: at the prompt`) | 1016.702 | 534.733 |
| wall time of the boot (host, loaded pool) | ~237 | 150.3 |

### Gaps

| gap | verified | unverified | what it is |
| --- | ---: | ---: | --- |
| up to beamlet started | 0.83 | 0.76 | the servers' start |
| beamlet started to first object | 10.01 | 5.35 | beamlet's start: attach, then Redoubt.Shell read whole (one load, ~4 9P ops) |
| first object to prompt | 1005.86 | 528.63 | |
| of which in the VM's 96 loads | 999.29 | 522.06 | the I/O chain |
| of which the VM's own work and console | 6.57 | 6.57 | parsing, running, console: the same both ways |

So 1,009.3 s of 1,016.7 (99.3 %) verified, 527.4 of 534.7 (98.6 %) unverified, is reading
modules (loads plus the start module's read). Absent probes: 0 (96 found, 0 absent, 0 refused),
so BL1's order costs no reads, as expected.

### Counts (identical both ways but verityd's)

- beamlet: 96 loads, 1,895,790 bytes found (plus Redoubt.Shell read once more by the bin).
- fsd 9P: 652 ops (walk 112, open 111, read 318, clunk 111, stat 0), 2,264,058 bytes read
  (iounit ~16 KiB: the bin's 4-page lend).
- littlefs reads: 77,710, 104.7 MB, 673 distinct blocks:
  - whole blocks at offset 0: 25,040 (metadata fetches);
  - 4-byte words: 52,058 = 2 × 25,040 revision words (50,080) + 1,978 CTZ pointer reads;
  - data pieces: 612.
  - Metadata (directory lookup): 75,120 reads = 96.7 %. CTZ pointers 2.5 %, data 0.8 %.
  - Every 9P op resolves its path from the root more than once (servers/fsd/src/server.rs): a
    walk 3 times (`stat`, the id attribute, the version attribute), an open 2 (`find`, the
    version), a read 2 (`find`, littlefs's `open` in `on_file`); clunk none. 112 × 3 + 111 × 2 +
    318 × 2 = 1,194 resolutions × ~21.0 metadata fetches each = 25,040. The root directory is
    one chain of pairs and a resolution walks it linearly to the name (about half the chain).
- verityd (verified, at 65,536 reads): 51,313 data blocks checked (78 %: the last-block buffer
  saved 22 %), 51,295 level-1 hits, 51,332 blkd reads.
- blkd: verified 8 sectors per read (verityd fetches whole blocks); unverified 216,044 sectors for
  65,536 reads (3.3 per read: fsd asks only the sectors it needs).

### Derived

| per | verified | unverified | difference (verification) |
| --- | ---: | ---: | ---: |
| load (999.29 / 96, 522.06 / 96) | 10.41 s | 5.44 s | 4.97 s |
| 9P op (1,009.3 / 652, 527.4 / 652) | 1.548 s | 0.809 s | 0.739 s |
| littlefs read (1,009.3 / 77,710, 527.4 / 77,710) | 12.99 ms | 6.79 ms | 6.20 ms |
| in instructions (× 125 M/s) | 1.62 M | 0.85 M | 0.78 M |

Verification costs +482 s (+90 %): one more IPC hop, a whole-block fetch at blkd for every read
(a 4-byte word becomes a 4 KiB read), and a SHA-256 on 78 % of them.

### Which candidates the numbers support

1. **Directory lookup in littlefs: the biggest share, ~96.7 % of the reads, so ~950 s of 1,009
   verified and ~510 of 527 unverified.** Not the CTZ re-walk (2.5 %), and not data (0.8 %).
2. **Per-read cost of the chain (IPC hops, copies, hashing):** 6.8 ms unverified, +6.2 ms
   verified; it multiplies whatever read count remains.
3. **The VM's own work and console: 6.6 s**, ~0.6 % verified. The VM's parse speed and console
   are not where the time goes; the `[t=N]` lines are 15 and cost nothing visible.
4. Absent probes: none.

Gaps the caches can touch: only the loads (and the start module's read). The servers' start
(0.8 s) and the VM's own work (6.6 s) are untouched by any cache.

### What it says for the separate packages

- **EROFS1 (hard-packed read-only format, sorted dirents, one lookup per path): the biggest
  measured share.** If a resolution costs ~1-2 block reads in place of ~63 (21 fetches × 3
  reads), the reads fall from 77,710 to about 1,194 × 2 (resolutions; fewer if fsd resolves
  once per op) + 612 (data pieces) + 1,978 (pointers, if any remain) ≈ 5,000. At today's per-read cost that is ~65 s of I/O verified (5,000 × 13.0
  ms) and ~34 s unverified (× 6.8 ms); with the servers' 0.8 s and the VM's 6.6 s, a prompt at
  roughly 70-80 s verified and 40-45 s unverified (the start module's 10 s read is a lookup and
  shrinks the same way). An estimate, to be measured by this case after EROFS1.
- **verityd's LRU of checked blocks:** today it would pay only if it held the directory chain
  (the current block of each of the root's ~42 pairs, ~168 KiB); a 4-block LRU would not. After EROFS1, with ~5,000 reads over 673
  distinct blocks and fsd reading words and pieces of the same block in turn, a small LRU still
  removes repeat hashes and blkd calls; its gain is then a few tens of seconds at most. Size it
  from this case after EROFS1.
- **Read-ahead (8 blocks per blkd call):** helps only sequential data, 0.8 % of reads today.
- **Fewer modules at boot (BEAM6), a preloaded code archive:** 96 loads; each costs 10.4 s
  verified today, mostly lookup. After EROFS1 a load is about 0.7 s verified; fewer loads
  then scale that.
- **Console / consoled, the VM's parse speed:** 6.6 s total; not worth a package for boot time.

Still running for step 1: the `--sweep 1..5` spread, the rv32 attempt, and userland-boot's wall
time with and without icount (ruling Q3/(a)).

### Spread: `--sweep 1..5`, verified, rv64 (run-3532999-1791266599003116145, 5/5 PASS)

| seed | first object (s) | prompt (s) | in loads (s) | littlefs reads | wall (s) |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1 | 10.856 | 1010.905 | 993.47 | 77,710 | 238.2 |
| 2 | 10.949 | 1022.749 | 1005.23 | 77,710 | 238.4 |
| 3 | 11.056 | 1034.424 | 1016.79 | 77,710 | 242.1 |
| 4 | 10.995 | 1029.787 | 1012.22 | 77,710 | 237.9 |
| 5 | 10.914 | 1019.451 | 1001.96 | 77,710 | 239.3 |

Prompt 1010.9-1034.4 s: spread 23.5 s (2.3 %); seed 1 repeated at 1016.7 then 1010.9 (0.6 %),
the sleep-on idle time. Counts identical on every seed. The spread is narrow enough to carry a
target (bench option (c) not needed).

### Design input for erofsd (EROFS1): resolve a path once, and hold it on the fid

Measured (boot-profile, both boots): `fsd` resolves a path from the root more than once per 9P
op: walk 3 (`stat`, the id attribute, the version attribute), open 2 (`find`, the version), read
2 (`find`, then littlefs's `open` in `on_file`), clunk 0. 112 walks + 111 opens + 318 reads = 541
resolving ops made 1,194 resolutions, ~21.0 metadata fetches each (25,040), and each fetch is 3
littlefs reads (2 revision words + the block): 75,120 of 77,710 reads (96.7 %).

- **One resolution per op** (541 in place of 1,194): 653 fewer resolutions × 21.0 × 3 ≈ 41,100
  fewer reads, 53 % of today's 77,710.
- **One resolution per fid, held across its walk, open and reads** (112, one per walk): 1,082
  fewer resolutions ≈ 68,100 fewer reads, 88 % of today's.
- On today's per-read cost (13.0 ms verified, 6.8 ms unverified) those are ~535 s and ~885 s of
  the verified boot's 1,009 s of reading; ~280 s and ~463 s of the unverified 527 s.
- They multiply with the format's own gain (a resolution in 1-2 reads in place of ~63): with
  both, the reads fall to ~112 × 2 + 612 data pieces ≈ 850 plus the data a hard-packed format
  reads in larger pieces.

`fsd` is not changed for it: it stays littlefs for the writable volume; the read-only path moves
to erofsd, which should take this as a design rule (a node holds what its resolution found, as
`fsd`'s `Node` holds a path and an id today, and every request on the fid uses it).

### userland-boot, wall time without and with icount (rv64, shared pool; ruling Q3/(a))

- Without icount (as on main): PASS, 188.8 s wall; prompt at [t=158354207] (host time).
- With `icount = "shift=3"`, `qemu_seed = 1`: PASS, 237.7 s wall (+48.9 s, +26 %); prompt at
  [t=1017189957] guest. `timeout_secs = 450` holds with ~210 s to spare: not raised.
- No target expectation is added yet: step 9's target comes from the post-cache (now: post-EROFS1)
  profile, per the scope change.

### Size budget (raised, each in its commit with a `Size budget:` line)

servers/blkd 1627 → 1656 (+29), servers/init 2076 → 2086 (+10), servers/fsd 1395 → 1501 (+106),
servers/verityd 488 → 528 (+40): the `boot-stats` counters, compiled only under the feature but
counted by the case as every line is.

### rv32 attempt, boot-profile verified (run-3732574-1791273658615088432): PASS, 237.2 s wall

init up 0.040; beamlet started 0.998; boot done 1.069; first object 11.671; prompt 1064.268 s
guest. 96 loads, 1046.75 s in loads, 1052.54 s since the platform started (VM's own work and
console 5.79 s). Counts identical to rv64 (77,710 littlefs reads, 652 9P ops, 673 distinct
blocks; verityd 51,313 checked at 65,536). rv32 is 4.7 % slower in guest time than rv64.

### rv32 userland-boot with icount: PASS, 293.4 s wall; prompt [t=1044177437] guest
(timeout 450 holds). rv32 without icount was not run on this branch: the wall-time pair is rv64's.

## Step 1 final state (2026-10-06)

Branch wp-BOOT1, base 2f7025638, heads:
- 367e71ae2 servers: a boot-stats build stamps and counts the image's boot, and beamlet says when it reaches its prompt
- f3b5d4881 testbench: boot-profile measures the image's boot to its prompt, verified and unverified
- 77dfcc285 testbench: userland-boot runs in guest time with a pinned seed

Gates run (exit codes): clippy servers rv64/rv32 with and without boot-stats 0; clippy beamlet rv64
both 0; `cargo +nightly fmt --all --check` root 0, userland/otp 0 (after formatting my two
lines); `cargo testbench size-budget` 1 before the raises (blkd, init, fsd, verityd over), raises
committed with their lines (not re-run since); `cargo run -p redoubt-doccheck -- --code` 0, no
findings. Cases: rv64 boot-profile PASS (sweep 1..5 5/5), rv64 boot-profile-unverified PASS,
rv32 boot-profile PASS, rv64 userland-boot PASS without and with icount, rv32 userland-boot PASS
with icount. Not run: whole bench (both widths), host tests of fsd/verityd/blkd/init/beamlet
(no behaviour change outside the feature but the prompt line), unsafe ratchet (no unsafe added).

Summaries checked: docs/servers/verityd.md (Measured bullet corrected; status gains
bench:boot-profile; the Open item stays, step 2 deferred), docs/servers/fsd.md (residual on large
directories), docs/userland/beamlet.md ("beamlet on Redoubt": the prompt line, the measurement,
status), docs/testbench.md (Checked builds: boot-stats and the cases; case file: icount sleep on
a disk case), image/README.md (no change: the target is not set yet), GETTING-STARTED.md and
README.md (no boot-time claim; no change).

### Where the per-read cost goes (an estimate; not measured hop by hop)

Unverified, one littlefs read of up to 4 KiB costs 6.79 ms of guest time, ~0.85 M instructions.
It is one `blkd` `read` call from `fsd`'s `Blocks::read`: the call with its 2-page lend (the
system call, the lend's check and mapping, two context switches, the reply's way back), `blkd`'s
decode and label check, one virtio request (descriptors, an MMIO notify that traps to QEMU, the
completion interrupt's trap into the kernel and its delivery to `blkd` as a receive), and four
copies of the block (DMA buffer, `blkd`'s scratch, the lend, `fsd`'s scratch, littlefs's buffer:
a few tens of thousands of instructions). littlefs's own share is small: a metadata fetch CRCs at
most a block with a 4-bit table, ~14 instructions a byte or ~57 k a block, which over all reads
averages ~18 k. So I expect most of the 0.85 M in the kernel's call and interrupt paths and in
`blkd`'s virtio round trip, and in whatever the scheduler runs while `blkd` waits: with sleep on
the vCPU keeps executing (and counting) while QEMU's I/O thread completes the read, so any
runnable process fills that wait with counted instructions, and a quiet one leaves it as host
idle time. One boot with `[t=N]` at each hop under `boot-stats` (fsd before the call, blkd at
receive, at the notify, at the interrupt, at the reply) would apportion it. After EROFS1 this
cost × ~3,700-5,000 reads is the next ~25-34 s unverified, plus verityd's 6.2 ms a read (~23-31 s)
verified.

### rv32 boot-profile-unverified: PASS 143.2 s wall; first object 6.589 s, prompt 557.959 s guest, 545.53 s in loads.

## Rebased onto bd6f768f6 (VOL1's merge), 2026-10-06

Heads: 999dadb04 (boot-stats; fixups folded: init's stamp only under the feature, fsd's
console_only block, the ceilings), a3defdd51 (cases, pages), 8c3af9890 (userland-boot icount).
Conflicts: tests/size-budget.toml (init: main 2097 + mine 11 = 2108, measured exact);
tests/userland-boot.toml (main's `memory = true` beside my icount and seed: both kept). Docs merged
clean. A first plain `git rebase` replayed VOL1's own commits; aborted, redone with `--onto`.

Release bytes (rv64, no features, base vs head): blkd, verityd, fsd identical; init was +132 bytes
(the Stamp Display), now identical in every section (strings equal but for the build path).

Gates on 11a18d526 (= 8c3af9890 but fsd's ceiling): fmt root 0, otp 0; build-rv64 0, build-rv32 0;
unsafe-budget PASS; no-cruft PASS; docs 0; host tests fsd, verity, blkd, init, beamlet-lookup-host,
littlefs PASS; boot-profile rv64 PASS (prompt 990.28 s guest), rv32 PASS (1027.49 s);
boot-profile-unverified rv64 PASS (521.96 s), rv32 PASS (543.47 s); userland-boot rv64 PASS
(430.7 s wall); size-budget FAIL fsd 1510 > 1501 (the console_only fixup's lines, measured after
my raise): ceiling 1510 folded into 999dadb04, size-budget PASS on 8c3af9890.
userland-boot rv32: timed out at 450.3 s wall waiting for the flipped block's verityd line, having
reached the prompt and 55, beside train-1's whole bench (rv64 cases ran 1.5x their solo wall time
in this round): no verdict under the bench's rule; rerun alone after train-1 (solo with icount
earlier: 293.4 s PASS).

The post-rebase prompts are 2-3 % below the pre-rebase sweep's (990 vs 1011-1034 s rv64 verified):
MEM2/B10 underneath; counts unchanged.

### rv32 userland-boot, solo (`jobserver all`), head 8c3af9890: PASS, 240.7 s wall; prompt [t=1066260809]
The loaded run's timeout is void by the rule. Margin: 450 s / 240.7 s = 1.87x (209 s spare).
testbench.md states no margin rule for timeout_secs (its only rule: a case that only ran out of it
beside other work has no verdict). rv64's icount cost was +26 % wall (188.8 -> 237.7 s solo); the
rv32 pre-icount wall was not measured.

### Sweep 1..5 on the rebased tree (8c3af9890 + pending folds; boot-stats build, rv64, icount shift=3 sleep on): 5/5 PASS
seed: first object / prompt / in loads (s guest): 1: 10.83 / 1014.25 / 996.83; 2: 10.96 / 1024.48 /
1006.93; 3: 11.04 / 1034.46 / 1016.83; 4: 11.02 / 1030.61 / 1013.00; 5: 10.92 / 1019.44 / 1001.94.
Prompt 1014.2-1034.5 s, spread 20.2 s (2.0 %); wall 233.7-241.4 s. The loaded gate run's 990.3 s
(seed 1) sits outside it: sleep-on idle time is the host's, and that run shared the host.
Ruling (b) applied: the line is `beamlet: first console read [t=N]`, under boot-stats only;
userland-boot is main's again (commit dropped).

### Release bytes vs main bd6f768f6, no features (red P3), llvm-size -A
- init, blkd, verityd, fsd: every section identical on rv64 and rv32 (init 284,395 / 280,563 B;
  blkd 91,761 / 84,468; verityd 120,113 / 118,566; fsd 256,531 / 256,599).
- beamlet (built from equal-length paths, since its workspace embeds source paths): rv32 identical
  (3,740,468 B); rv64 .text identical (2,019,520), .rodata 16 bytes smaller (794,979 vs 794,995),
  every string identical. Main built twice from two paths gives the same .rodata, so the 16 bytes
  follow my source: the only changes reaching a default build are moved lines and a removed `use`
  (panic-location data, presumably); no code. My first comparison (different path lengths) showed
  +3.3 KB of paths and a +422 B .text artifact that a fresh build does not reproduce.

## Folded (editor 1-7, red 1-3, ruling b) and rebased onto fb1f3a58f (FSN1, littlefsd), 2026-10-06
Head 724fea323: 06a514ea9 servers: boot-stats (line `beamlet: first console read [t=N]`, under the
feature only), 724fea323 testbench: boot-profile (+ unverified, pages). userland-boot commit dropped.
Rebase conflicts (both sides kept, renamed): littlefsd bin and server.rs, beamlet bin, size-budget
(littlefsd 1510, measured exact), verityd.md and beamlet.md; fsd -> littlefsd through stats.rs's
line, Cargo comment, cases (package, bin, patterns, stage), the unverified manifest (regenerated from
main's), pages, both commit messages (Size budget line servers/littlefsd).
Gates on 724fea323, all rc 0: fmt root/otp; clippy +boot-stats rv64/rv32 (only pre-existing warnings
in sshkey.rs, blkd queue.rs); `cargo test -q -p redoubt-doccheck --test docs`; build-rv64/rv32;
size-budget, unsafe-budget, no-cruft PASS; host tests littlefsd, verity, blkd, init,
beamlet-lookup-host, beamlet-lookup-cli-host, littlefs PASS; boot-profile rv64/rv32 PASS,
boot-profile-unverified rv64/rv32 PASS, userland-boot rv64 195.2 s / rv32 181.5 s PASS (main's case).

### Release bytes after the rebase, vs main fb1f3a58f, no features (red renewal (b)), llvm-size -A
Both trees exported (git archive) to paths of equal length, fresh target dirs, both widths:
- init 284,395 / 280,563 B, blkd 91,761 / 84,468, verityd 120,113 / 118,566, littlefsd 256,563 /
  256,627 (rv64 / rv32): every section identical.
- beamlet: its release sections move with the build path's contents, not only its length (it
  embeds source paths): main's source built at path X gives .text 3,135,518 (rv32) and .rodata
  795,027 (rv64), at path Y 3,134,998 and 795,011; my head gives exactly the same pair, swapped
  with the paths. Built from the same path, base and head are identical in every section on both
  widths. This corrects my earlier "rv64 .rodata -16 B follows my source": main's two builds
  then agreed by coincidence of path.
