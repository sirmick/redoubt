# B96 report: beamlet.md's boot-profile prompt row

Base 3e6f29443, head 72cce08fd on wp-B96 (one commit). Tier B, docs and measurement; reviewer
b96-editor. Ruling on the target: orchestrator, thread B96-target, option B.

## Delivered

- docs/userland/beamlet.md: the table's last row now reads 10.3 / 8.5 / 11.8 / 9.9 s (rv64 v/u,
  rv32 v/u). The old row was 13.1 / 10.9 / 14.7 / 12.4 s. The sentence under it gives the
  milestones at main, the cause by mechanism and the seed spread. The target is now 15 s, with
  its arithmetic.
- tests/boot-profile.toml and tests/boot-profile-unverified.toml: the description says 15 s; the
  comments name the figures and the rule; the stamp regex is
  `([0-9]{1,7}|1[0-4][0-9]{6})` (< 15,000,000 µs, checked at 14999999 → match and
  15000000 → no match).

## Measurements at 3e6f29443 (first prompt drawn, guest µs, icount shift=3, sleep on)

| | rv64 v | rv64 u | rv32 v | rv32 u |
| --- | ---: | ---: | ---: | ---: |
| seed 1, set run | 10,306,814 | 8,485,365 | 11,762,226 | 9,867,071 |
| seed 1, sweep | 10,314,022 | 8,513,348 | 11,781,563 | 9,895,787 |
| seed 2 | 10,382,609 | 8,520,689 | 11,791,686 | 9,888,467 |
| seed 3 | 10,333,953 | 8,503,326 | 11,787,717 | 9,910,323 |
| seed 4 | 10,344,333 | 8,520,147 | 11,754,810 | 9,911,486 |
| seed 5 | 10,888,272 | 8,824,048 | 12,263,382 | 10,242,369 |

Each sweep ran 4 boots at once (`cargo testbench --exact --arch W CASE --sweep 1..5 --jobs 4`
under `q run --cores 4 --share qemu`). Seed-1 milestones (rv64 v): the steward started at
1,580,119, the pack was read by 4,387,887, the driver read at 8,376,416 and the prompt came at
10,306,814. The other widths and cases are on the page. Console logs are in
.tmp/B96/main-1 and .tmp/B96/main-sweep.

Whole-run figures, per the ruling: only train 88's two unverified consoles survive, in
.worktrees/train-20/target/testbench: 8,507,979 on rv64 and 9,893,586 on rv32, within 0.03 s
of the quiet seed-1 runs. Trains 86–88's verified consoles were pruned (run-* keeps the newest
few), and the train copies (.tmp/train-8x) keep only PASS/FAIL lines. So no whole-run verified
figure exists. Applying the rule on the evidence available: the worst is seed 5, rv32
verified, 12.26 s; x 1.1 = 13.49, so 15 s. A whole run would have to add 1.4 s to the
prompt to change that. Whole-run unverified moved it by 0.03 s.

## Bisect (rv64 boot-profile, seed 1, first-parent merges between e921fc66f and main)

e921fc66f (the commit that last set the row): 13,637,278. I measured 13.64 s there, not the
page's 13.1. That is presumably a measurement taken before rebase. There is no page line for it,
since the row was replaced.

| merge | prompt | note |
| --- | ---: | --- |
| 76fd2b9bd | 13,956,162 | |
| db09d019f | 14,133,470 | |
| bf3fa75d2 | 11,764,385 | audits/fault reports stop holding other harts; zeroing one check: −2.37 s (rv32 15,629,497 → 13,299,877) |
| 311ca9b48 | 11,760,848 | |
| 22195522e | 11,831,519 | |
| 6b46c8c86 | 11,582,676 | beamlet fewer instructions per reduction: −0.25 s |
| 59e9f4601 | 11,648,279 | |
| a4c093827 | 11,619,898 | |
| c7efc9b67 | 11,635,295 | |
| 2afa4e38d | 11,966,320 | |
| 1fcdd656d | 12,114,280 | |
| db6575a3f | 10,422,629 | a slice's end pays for its work: −1.69 s (rv32 13,807,172 → 11,787,705) |
| d9f1d906c | 10,435,466 | |
| 543717fbe | 10,419,059 | |
| fbaaf088b | 10,486,269 | |
| 3e6f29443 | 10,306,814 | main |

Each of the two steps is measured against its own first parent. Point logs are in
.tmp/B96/points{,32}. The scratch worktree has been removed.

## Gates (at 72cce08fd)

`make -k -f scripts/jobs.mk -j4 set CASES="boot-profile boot-profile-unverified docs formatting"`:
both widths of boot-profile and boot-profile-unverified PASS rc=0; docs PASS rc=0; formatting
PASS rc=0.

## Affected summaries checked

- docs/servers/verityd.md "Memory and cost" (l.197): links to beamlet's table, with no figures. No change.
- docs/testbench.md l.987–990 (boot-stats, the target link): no figures. No change.
- README.md, GETTING-STARTED.md: no boot-time or target figure (grep for 20 s / boot-profile / first prompt). No change.
- beamlet.md's earlier breakdown paragraph (14.7 s / 1.9 / 5.8 / 11.8 s) belongs to the
  previous row ("16-page lends"), not this one. Unchanged, as for every older row.
- beamlet.md l.677 says "the verified prompt is the same across seeds 1 to 5". That belongs to the
  boot-pack row's build, and is left unchanged. Today seed 5 differs, which the new text says.

## Fold round 1 (head 930aacb77, amended from 72cce08fd)

- P2: the breakdown paragraph now states the boot as it is: the steward at 1.6 s, the pack read by
  4.4 s, the driver at 8.4 s, the prompt at 10.3 s, with the unverified and rv32 points. The
  superseded 14.7/1.9/5.8/11.8 s points and the pre-16-page-lend figures are gone. The lend's
  reason (cost per call, not per byte; the 32 KiB add) stays, as present text. The 0.6 s
  servers-up point was not re-measured and is dropped. Same for "about 2.5 ms a call".
- P3: the claim "the verified prompt is the same across seeds 1 to 5" (boot-pack sentence) is
  removed. The spread is stated once, in the breakdown paragraph.
- P3: "under 0.07 s" is now "under 0.08 s": rv64 verified spans 10.307–10.383 s, including the set run.
- The commit message says 0.08 s too.
- Gates at 930aacb77: `make -k -f scripts/jobs.mk -j2 set CASES="docs formatting"`: docs PASS
  rc=0, formatting PASS rc=0. The cases and tomls are unchanged since 72cce08fd. The
  boot-profile runs stand.
