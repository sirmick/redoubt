# PACK1: implementer's report

## Checkpoint 1 (2026-10-06): the pack reads; the measured floor; the target rule gives above 30 s

Branch wp-PACK1 (uncommitted at this checkpoint), base main fa08fe2c8. All runs:
`make -f .wash/local/jobs.mk rv64/boot-profile`, seed 1, verified, icount shift=3, boot-stats.
Consoles kept in /tmp/pack1-consoles/ (bp64-asc.log, bp64-desc16k.log, bp64-asc16k.log).

What works: mkimage (the bench's stager) writes `boot.pack` (1,879,603 bytes, 94 entries: 89
modules and 5 `.app`, the 96 lookups to the prompt, two names asked twice) into the verified
volume. beamlet reads it whole before the VM, checks it, and its lookups take from it:
`beamlet: boot-stats: loads 96 (found 96, of them 96 from the pack, absent 0, refused 0)`, so
the list is exact and the pack is spent at the prompt. The 9P ops and littlefs reads below are
littlefsd's cumulative line at the BootStats call after the prompt (includes Enum.sum's loads).

| Read | 9P reads of the pack | pack read done | prompt | littlefsd 9P total | littlefs reads (words / whole blocks / pieces) | distinct blocks |
| --- | ---: | ---: | ---: | ---: | --- | ---: |
| BOOT1 baseline (no pack) | - | - | 1,016 s | 652 | 77,710 | 673 |
| 64 KiB (MSIZE lend), ascending | 29 | 66.25 s | **74.26 s** | 122 (read 75) | 17,065 (12,363 / 4,114 / 588) | 620 |
| 16 KiB (4-page lend), descending | 115 | 143.95 s | 151.96 s | 208 (read 161) | 27,848 (19,708 / 7,468 / 672) | 620 |
| 16 KiB (4-page lend), ascending | 115 | 144.05 s | 152.05 s | 208 (read 161) | 27,848 (19,708 / 7,468 / 672) | 620 |

The VM after the pack: 8.0 s (pack read done to prompt, incl. decoding 96 modules; 0.12 s in
lookups). Servers to beamlet's start: 0.75 s.

**The lever (reading backwards) does nothing**: identical counts at 16 KiB. littlefsd opens the
file afresh for every 9P read (`on_file`: open, seek, read, close), so each read pays the root
directory walk and a CTZ find from the file's head; the order of the reads changes neither.
Kept out.

Fitting t = a + b x reads over the two read sizes: b = 0.90 s per 9P read (about 70 verified
block reads: the open's root-directory walk, the Architect's ~63), a = 39 s for the bytes
(about 3,000 verified block reads for ~460 data blocks: littlefs's per-boundary find from the
head, ~6.5 per block). **So no read size reaches 30 s on littlefs**: even one read would be
~39 s + 0.9 + 8.7 = ~49 s. The rule: 74.26 s x 1.1 = 81.7, rounded up to 5 s: **target 85 s**
(64 KiB); 152.05 x 1.1 = 167.3: 170 s (16 KiB). Both above 30 s: checkpoint 2.

**The 64 KiB read needs littlefsd:system's heap cap raised**: its heap peak rises 19 -> 29
pages (the 64 KiB message buffer), and `beamlet-footprint` rv64 fails `littlefsd:system: heap
needs 58 pages for twice its 29-page peak, capped at 38`; on rv32 (and rv64 reading backwards)
littlefsd refuses the read itself (`boot.pack not loaded: its file could not be read: other`,
its heap out at the cap). Raising littlefsd:system's `heap_pages` 38 -> 58 (and its budget) is
image/manifest.json and testbench.md's row, outside PACK1's owned paths. The branch now reads
through the 4-page lend module files use (16 KiB, littlefsd unchanged): 152 s.

Memory (64 KiB run, rv64 beamlet-footprint): beamlet's scan 9,369 pages, unchanged (BEAM6's
figure); runtime heap at the prompt held 7,876 (unchanged), peak 8,240 (was 7,897: +343, under
the pack's 459 pages). The pack is freed at the prompt (all 94 entries taken).

### Decision needed (checkpoint 2, owner)

1. Accept the pack with littlefs's floor: 64 KiB reads with littlefsd:system's heap cap 38 -> 58
   pages (+20 per label set; manifest + testbench.md row), target 85 s; or 16 KiB as is,
   target 170 s.
2. The target under 30 s waits for EROFS1 (one lookup, no CTZ): the pack then reads in about
   its ~460 blocks plus one lookup, ~6 s + 8.7 s.

## Built so far (uncommitted)

- tools/testbench/src/userland.rs: `[objects] pack` (names), `pack_fault` (test-only),
  `boot_pack` (sorted, refuses unknown/duplicate names), `pack_entries`; stage writes boot.pack
  and checks each entry equals the file beside it. Host test
  `the_boot_pack_is_deterministic_sorted_and_only_of_the_objects`.
- tools/testbench/src/disk.rs, qemu.rs: `flip` damages every copy of a file the pack also holds.
- userland/otp/redoubt/src/pack.rs: reader, checked whole (magic, version, count, names,
  order, contiguity, each module's first atom); userland.rs: `Disk::with_pack`, pack first, freed
  when spent; lib.rs: boot-stats counts pack hits; bin/beamlet.rs: reads the pack at start,
  parks on refusal, start module from the pack. Host tests in tests/pack.rs.
- Cases: pack-outside-module, pack-bad-truncated, pack-bad-wrong-length, pack-bad-wrong-name
  (rv64 PASS); boot-profile(-unverified) expect the pack line; userland-bad-start's line is the
  pack's (the start module is in the pack, both copies flipped).
- Pages drafted: beamlet.md (lookup row, pack paragraph), boot.md R75 sentence.

## After checkpoint 1: committed at 16 KiB, both widths (seed 1)

| Case | prompt | pack read done | 9P total | littlefs reads |
| --- | ---: | ---: | ---: | ---: |
| boot-profile rv64 | 152.05 s | 144.05 s | 208 | 27,848 |
| boot-profile-unverified rv64 | 76.58 s (BOOT1 536) | 68.58 s | 208 | 27,848 |
| boot-profile rv32 | 148.71 s | 141.36 s | 208 | 27,848 |
| boot-profile-unverified rv32 | 70.60 s | 63.25 s | 208 | 27,848 |

beamlet-footprint at 16 KiB: scan rv64 9,369 / rv32 5,511 (unchanged); prompt peak rv64 8,240
(+343), rv32 5,019 (+397); littlefsd:system heap 17 / 15 of 38 (unchanged); beamlet stack
33,736 bytes rv64 (testbench.md's row says 33,240; 17 pages declared, still twice it).

Commits: c41559b8a (testbench: the pack), d391de2a5 (beamlet-redoubt: limits tests built
against run's seventh argument: broken on main since BEAM6, which made beamlet-lookup-host fail),
8a818a745 (beamlet: the reader), e7f44e0e8 (tests: the pack cases). Docs uncommitted, pending
the boot-time paragraph.

Pages not touched that state BOOT1's figures in the present tense: docs/servers/verityd.md
"Measured" (1,016 s / 536 s), docs/servers/littlefsd.md residual (652 9P ops, 77,710 reads):
left as the Architect ruled (verityd.md untouched); a reviewer may want "before the boot pack".

## Owner's decision (checkpoint 2) and state at the hold

Owner: no littlefs target; keep 16 KiB reads; the target is set on EROFS after rebasing onto
EROFS1's merge. Docs committed: 81c0856b3 (beamlet.md: the pack, the lookup order, one
sentence on littlefs's figure, no target; boot.md R75 sentence; SECURITY.md R75 row; M1
progress; image/README.md).

On EROFS (not measured here; EROFS1 reports 14.9 s verified / 11.1 s unverified): EROFS
removes the directory walks; the pack removes the 96 lookups' per-file 9P traffic (open, reads,
clunk per module) for one sequential read of about 460 data blocks, so its gain there is a few
seconds of the 14.9 s, not an order of magnitude. To be measured after the rebase.

Gates on 81c0856b3 (all `make -f .wash/local/jobs.mk`, exit 0):
- rv64+rv32: pack-outside-module, pack-bad-truncated, pack-bad-wrong-length,
  pack-bad-wrong-name, beamlet-footprint, userland-bad-start, boot-profile,
  boot-profile-unverified, beamlet-boot, userland-boot (verity-flipped-block inside it),
  verity-flipped-tree, verity-wrong-root, userland-read-only, init-boot, ipc-outcomes,
  bench-net-peer (and its -count/-pcap-empty/-twice, matched by prefix).
- formatting, no-cruft, unsafe-budget, size-budget, docs, beamlet-lookup-host: PASS.
- `jobserver bounded cargo test -p testbench`: 122 passed.
- `jobserver bounded cargo test -p beamlet-redoubt --features fake` per target: pack 2,
  console 7, lookup 2, userland 2, limits 7.
Not run: the whole bench (the train's); a seed sweep (after the rebase, on the shipped format).

Summaries checked: README.md, GETTING-STARTED.md (no module-lookup or boot-time claim: no
change); docs/plan/m1-separation.md progress (updated), remaining work (no item: the target
moves with EROFS); image/README.md (updated); image/userland.toml comment (updated);
docs/servers/verityd.md "Measured" and littlefsd.md residuals (BOOT1's figures, left per the
Architect's ruling; flagged above); docs/testbench.md memory row (scan unchanged: not moved;
beamlet's stack peak now 33,736 bytes against the row's 33,240, still under half of 17 pages).

## Departure from the brief: a copy per entry, not decode-in-place

The brief says the pack's entries are "decoded in place by slice (no copy arises)". The
platform boundary hands the VM an owned `Lookup::Found(Vec<u8>)` (userland/otp/vm, not
PACK1's), so `Pack::take` copies the entry's bytes; the copy dies when the loader returns,
exactly as a file read's bytes do (BEAM6: no loader copy kept). The transient is at most one
module's size; the held figure is unchanged (scan 9,369 / 5,511) and the prompt peak +343 /
+397 pages, under the pack's own 459. Decoding from a borrowed slice needs `Lookup` to carry
one: a VM change (BEAM8's area), not made here.
