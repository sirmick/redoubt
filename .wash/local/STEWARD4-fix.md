# STEWARD4 train 56 regression fix (steward4-fixer, 2026-10-17)

Head: 12551cdb4 on wp-STEWARD4 (one commit on main 82a026733; b09135def amended, message
kept, the account's Cases sentence gaining one clause). `git status --short | wc -l` = 0.

## Cause

Not the image: image/boot.toml at b09135def already ships `crossing` (`redoubt-crossing`),
and train 56's init-boot transcript shows `init: 4 public entries pushed to bootfsd, and
sealed`. The twelve failures were two consequences of image/manifest.json's `public` gaining
`crossing`:

- boot-profile, boot-profile-unverified, image-disk, sshd-restart-detaches, userland-read-only
  boot image/manifest.json (or its unverified copy, whose `public` also names `crossing`) with a
  `programs` list of their own, which lacked `redoubt-crossing`: `public[3]: names nothing the
  manifest or the bundle holds`.
- init-boot expected `3 public entries pushed`; the image now publishes four.

The Architect's ruling Q2 and the brief say nothing against the image publishing the crossing
(the commit's account already says "the crossing program the image publishes"), so `public`
keeps it and the cases ship it.

## Changes

- tests/{boot-profile,boot-profile-unverified,image-disk,sshd-restart-detaches,userland-read-only}.toml:
  `{ package = "redoubt-crossing", bin = "crossing" }` after consrelay; image-disk's program
  comment names the crossing.
- tests/init-boot.toml: `4 public entries` (the description does not name the count).
- image/README.md: boot.toml's entry list names `consrelay` and `crossing`.

Summaries checked, unchanged: docs/servers/init.md (no count of public entries),
docs/kernel/boot.md (no entry list), docs/testbench.md:1781 (already names the four published
entries). image/manifest.json budgets unchanged; no page states the image's size.

The init-refuses-* cases boot the same manifest but refuse before the public check (they passed
in train 56); init-refuses-bound rerun as the gate asked.

## Sweeps stopped first

`q ls` showed one running job (rv64 steward-latency --sweep 11..20 --jobs 2, pid 3504230, run dir
run-3504230-*) and a waiting bash chain (pid 3504960) that would have started the rv32 sweeps and
the seed-1 replay; killed by pid, chain first. Partial logs stay in .wash/local/steward4-logs/.

## Gates (logs: /home/mcloonan/redoubt/.tmp/steward4-fix/gates.log, gates2.log)

`make -f scripts/jobs.mk -k set CASES='boot-profile boot-profile-unverified image-disk init-boot
sshd-restart-detaches userland-read-only init-refuses-bound' rv64/steward-declassify
rv64/steward-push docs` rc=0: all 14 set cases PASS both widths, steward-declassify rv64 PASS
(10.8 s), steward-push rv64 PASS (23.1 s), docs PASS.
`make -f scripts/jobs.mk -k rv64/formatting rv64/size-budget` rc=0: formatting PASS (21.0 s),
size-budget PASS. (Plain stable `cargo fmt --check` differs on kernel files untouched here: the
project's gate is the nightly `formatting` case.)
