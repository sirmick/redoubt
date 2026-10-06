# EROFS1 implementer report

## Checkpoint 1 (2026-10-06): libs/erofs, its host tests, the erofs-utils differential, the fuzz target

Branch wp-EROFS1 (worktree .worktrees/EROFS1), base d1872a5cd, two commits:

- 661cdd01c testbench: a host-tests case names the host tools its oracle runs
  (`tools = [...]` on a host-tests case: each must be an executable on PATH, else the case
  fails naming it, SKIP with --allow-skip; host:testbench::an_oracles_tools_must_be_on_the_path;
  docs/testbench.md table row + paragraph + status line)
- 202ee3a37 erofs: EROFS's uncompressed subset, parsed and written, checked against erofs-utils
  (libs/erofs: lib.rs, read.rs parser, write.rs writer, tree.rs host walk; no deps, forbid unsafe;
  tests/erofs-host-tests.toml, tests/erofs-oracle.toml; size budget libs/erofs 541; fmt root
  libs/erofs/fuzz; workspace exclude)

Kernel docs read: Linux v7.0 `Documentation/filesystems/erofs.rst` and `fs/erofs/erofs_fs.h`
(named in the crate doc).

The mkfs.erofs 1.9 options that stay in the subset (recorded in libs/erofs/tests/oracle.rs; for
erofsd.md's first residual): no `-z`, no `--chunksize`, no off-by-default `-E` feature
(fragments, dedupe, 48bit, dot-omitted); `-b4096 -T 0 --all-root`; variants
`-Eforce-inode-compact`, `-Eforce-inode-extended`, `-E^inline_data`, `--root-xattr-isize=64`.
mkfs 1.9 by default sets only compat features (sb_csum, mtime), no incompat bit.

Subset decisions: any incompatible feature bit, extra devices or dirblkbits != 0 is corrupt;
i_format bits above 4 corrupt; bit 4 on a compact file = nlink 1; startblk_hi ignored (no 48bit);
the superblock checksum is not checked (integrity is verityd's). Writer: compact inodes
(extended only for sizes > 4 GiB or nlink > 65535), each inode + xattrs + inline tail inside one
block, root first (nid 36), user.sha256 raw 32 bytes (sha256 fn passed in by the caller: the
crate has no deps), superblock with no features, timestamp 0.

Commands and exit codes:
- make jobs.mk rv64/erofs-host-tests 0 (8 tests), rv64/erofs-oracle 0 (2 tests: 3 seeds x 5 mkfs
  variants read by our parser; fsck.erofs --xattrs, --extract equal to the tree, dump.erofs
  Xattr size 56)
- erofs-oracle with erofs-utils off PATH: FAIL "mkfs.erofs is not on the path ..." (exit 1);
  with --allow-skip: SKIP
- fuzz:erofs/image, built by hand with cargo-fuzz's sancov flags (cargo-fuzz is not installed on
  this host): 180 s, 114,778,315 runs, no crash
- rv64/size-budget 0, rv64/formatting 0, rv64/no-cruft 0, doccheck --code 0
- clippy -p erofs (host all-targets, riscv64, riscv32) clean; testbench's own warnings predate

Pages: erofsd.md "The format" moves to built with erofsd (step 2), "The packer" with disk.rs
(step 3). erofs-utils was already in scripts/setup.sh and GETTING-STARTED (no change).

## Final report (2026-10-06): steps 1-3, short gate

Branch wp-EROFS1, rebased onto main 673be7afc, 9 commits:
- f068000c6 testbench: a host-tests case names the host tools its oracle runs
- c82606f5b erofs: EROFS's uncompressed subset, parsed and written, checked against erofs-utils
- 12fe162cf erofsd: one read-only EROFS volume, served over 9P
- abbdc1698 init: an erofsd entry is checked as a volume server, as littlefsd's is
- a70e22b29 testbench: disk recipes pack EROFS volumes, and erofsd boots on them
- 7bb338030 erofsd: a boot-stats build counts its 9P and its reads of the volume
- 4b83d9fed image: the system volume is EROFS, served by erofsd
- 6dd385ce1 verityd: the last four checked data blocks are kept, least recently used out
- 60388f0e4 testbench: the boot profile holds the image to its boot-time target

### Delivered
- libs/erofs (no deps, forbid unsafe): parser (Superblock, Inode, Dirents), writer (pack), host
  walk (read_tree); fuzz target libs/erofs/fuzz (image).
- servers/erofsd: FileServer over libs/erofs; node = inode read once at walk + name; walk = binary
  search over directory blocks; read = one range call per 32 KiB (64 sectors) + one for an inline
  tail; listing reads on from a cursor; one block of scratch caching the last directory block;
  corrupt volume served as corrupt, range failure poisons; read-only refusals `read-only volume`.
  Lend 9 pages. boot-stats feature (src/stats.rs).
- init: no code change needed (volume servers are keyed on `volume`, not the program); a test.
- testbench: `tools` key on host-tests (named skip); disk recipe `fs = "erofs"` and `damage`.
- image: system volume EROFS, erofsd:system (stack 5, heap 24); boot.toml gains erofsd; beamlet
  takes `endpoint=NAME` (the image's erofsd:system; beamlet's own cases keep littlefsd:system).
- verityd: 4-block LRU of checked data blocks (was 1 block), counts requests/held, boot-stats from
  2^7 reads; verity:system heap cap 94 -> 100 (peak 50).
- Target: boot-profile (verified) bounds the prompt at < 30 s, boot-profile-unverified < 20 s, both
  widths (the brief said userland-boot; the assignment said boot-profile, and only the boot-stats
  build stamps [t=N]).

### Profile (seed 1, icount shift=3 sleep on, guest s) — on main 051a2f86c (512 MiB, SMP1)
| | rv64 verified | rv64 unverified | rv32 verified | rv32 unverified |
| littlefs (BOOT1) | 1016.7 | 534.7 | 1044.2 | 558.0 |
| EROFS | 16.0 | 12.5 | 15.7 | 12.1 |
Loads: rv64 7.15 s verified, 3.72 s unverified (rv32 7.23 / 3.77). First object 0.99 / 0.89 s.
Logs: .wash/local/erofs1-profile/.

Counts (identical every run): 9P 652 (walk 112, open 111, read 318, clunk 111), 2,264,058 bytes;
erofsd volume reads 641 (inodes 112, directory blocks 243, data runs 286); range calls 642,
3,348,480 bytes (5.2 KiB a call; a 9P read 7.1 KiB). verityd at its 512th read: 849 data blocks
asked, 197 held (23 %), 652 checked, 638 level-1 hits, 667 blkd reads (4 KiB each).
Per-read cost (item 9): per range call 11.1 ms verified (1.39 M instr), 5.8 ms unverified (0.72 M);
per 4 KiB of range bytes (817.5 blocks) 8.7 ms verified (1.09 M), 4.5 ms unverified (0.57 M),
beside BOOT1's 1.62 M / 0.85 M per littlefs read. The VM's own work and console ~6.5 s are now
~45 % of the verified boot.

Cache sizing (rv64, before SMP1): data cache 1 / 4 / 8 blocks: held 100 / 197 / 289 of 849;
prompt 14.92 / 14.37 / 13.82 s. 4 kept per the brief; 8 is the next cut (+16 KiB).

### Memory (twice the largest peak; init-boot, userland-boot, userland-read-only, both widths)
erofsd:system stack peaks 9,704 (rv64) / 8,064 (rv32) bytes -> 5 pages; heap 12 pages -> 24.
verity:system heap 50 -> cap 100.

### Short gate (pool, make -k; on 051a2f86c, then fixes)
build-rv64 0, build-rv32 0; host: erofs-host-tests 0, erofs-oracle 0, erofsd-host-tests 0,
init-host-tests 0, verity-host-tests 0, memory-host-tests 0, cargo test -p testbench 0 (after
the image-recipe test fix), beamlet-lookup-host 1 (pre-existing on main: userland/otp/redoubt/
tests/limits.rs calls run() without main's new report_memory argument; file identical on main);
formatting 1 then 0 after fmt fix; no-cruft 0, size-budget 0, unsafe-budget 0, doccheck 0.
Both widths, all 0: erofs-corrupt, erofs-read-only, boot-profile, boot-profile-unverified,
userland-boot, userland-bad-start, userland-read-only, verity-flipped-tree, verity-wrong-root,
beamlet-footprint, init-boot, bench-net-peer, ipc-outcomes, beamlet-boot, beamlet-console,
beamlet-heap-flood, beamlet-budget-flood, init-refuses-* (10), image-disk (1 then 0 after adding
erofsd to its programs). rv32 userland-boot: scan "stack paint unit 6763 found twice" in 2 of 3
runs (passed in the gate run); ruled the scanner's (B16); evidence in
.wash/local/evidence-erofs1-paint-twice/. Not run: the whole bench.

### Size budget
libs/erofs 541 (new), servers/erofsd 515 (new, 398 + boot-stats), servers/verityd 528 -> 541,
servers/init 2108 -> 2109 (ENTRIES). Unsafe: 0 in libs/erofs and erofsd (budgets added).

### Pages / summaries checked
Changed: erofsd.md (all sections built; residual names the mkfs options), littlefsd.md (littlefs
sentence, Why, R47/R49 statuses), init.md (servers row, volume servers, step 5), servers/README.md
(graphs, trust tier, placement, Naming built), verityd.md (file server generic; cache built;
measured), blkd.md, boot.md (R75 explanatory text names erofsd; rule unchanged — brief said text
unchanged), beamlet.md (load_module row, handle, table, target), testbench.md (tools key, erofs
partitions/damage, memory table), image/README.md, SECURITY.md (R47, R49 rows).
Checked, no change: GETTING-STARTED.md and scripts/setup.sh (erofs-utils already there), README.md
(no system-volume claim). Not mine, stale: userland/otp/redoubt/src/userland.rs doc comments still
say littlefsd (outside the endpoint= path).

### Open risks
- Every committed file read in full except the large pre-existing testbench sources and pages,
  read by diff and changed functions (all changes mine).
- cargo-fuzz not installed: fuzz run by hand with sancov flags (180 s, 115 M runs).
- The binary search over directory blocks trusts the volume's block order; a hostile order gives
  wrong lookups, not unsafety.
