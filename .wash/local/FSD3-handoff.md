# FSD3 handoff (wp-fsd3, worktree .worktrees/fsd3, base main 665ec84c3)

Tip bb95c3b8a. All final (no WIP): 968073dd7 blkd labels.P=; faa0a0c41 fsd pack() + littlefs diff
C-reads case (hand run PASS, target/ removed); ffe3d6a5a init mints range badge, labels args,
volume checks, fsd-boot; fc207ea6a confine Q1(b); 203b9a408 fsd endpoint= Q2(a); bb95c3b8a bench
reporter re-read after reboot + fsd-reboot.

UNCOMMITTED (the owner rejected my commit attempt; I did not retry, no WIP commit). Tested:
- bound.rs/check.rs: range badge counted in Counts.handed; init.md boot step 2 + Authority lines.
- Q3 (A, answer 747e9bd4): tools/testbench/src/disk.rs (Recipe, gpt_disk moved from qemu.rs,
  pack_disk), case.rs [disk] recipe/stage + validation, qemu.rs uses it, main.rs --pack-disk,
  Cargo.toml dep redoubt-fsd; mkimage packs disk.img and copies target/testbench/last/
  interactive.tar (it was broken by B7's per-run dirs); image/{manifest.json (+data volume,
  fsd:data), boot.toml (+fsd), disk.toml, README.md}; init-boot boots recipe disk (7 servers, 3
  badges, 5 consoles); image-disk case + tests/data/fsd/{image.json,stage/}; fsd-client `read`
  mode; init tests adapted (without_volumes(), numbers, fuzz ENTRIES +fsd); testbench.md.
  PASS: image-disk, init-boot rv64+rv32; redoubt-init tests; testbench tests (after case.rs
  recipe assertion fix, not rerun); docs; ./mkimage. Split into 2 commits as I planned
  (bound fix; image disk), owner's word first.

Cases (both widths): fsd-boot PASS/PASS; fsd-reboot PASS/PASS; image-disk PASS/PASS (uncommitted);
fsd-restart, fsd-corrupt-volume, fsd-quota, fsd-one-volume, fsd-confined-labelled: not started.
Rules: 1 done; 2 done; 3 Q1(b) applied, case owed; 4 done (uncommitted mkimage part); 5 Open item
page line owed with fsd-restart; 6 done.
Q1(b), Q2(a): applied per instruction 4a8c7af4. Q3: applied, uncommitted.

Page lines written: blkd.md label check; init.md status, servers row, Volumes bullet, confinement
lines (exact), example args; fsd.md Arguments (exact); testbench.md; image README/disk.toml.
Owed: fsd.md Volumes status (drop partly tested, name cases), Authority+R47 (fsd-one-volume),
Failure and restart (exact text in brief); blkd.md "Started by init" loses partly tested.

fsd-restart: netd-restart's pattern (feature triggered once). fsd-corrupt: noise partition via a
recipe or stage. Size-budget lines in commit messages are estimates: verify with size-budget.
Pending: rebase over RT2 (fsd.rs/blkd.rs loops become redoubt_rt::server::serve); my changes in
those bins are before the loop only. Gates not yet run (whole bench needs orchestrator's word).

What consumed my context: init manifest test churn from the image's fsd, reading init/blkd/
testbench code, a boot log grep that hit the tar's binary (use the .log file only).
