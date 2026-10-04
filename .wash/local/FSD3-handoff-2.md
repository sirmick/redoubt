# FSD3 handoff 2 (wp-fsd3, worktree .worktrees/fsd3, on main b21a59493, rebased over RT2)

Tip 95f93d935 (WIP). Every commit below the WIP is final, and each was checked with
`rebase --exec`: it builds, passes the fsd, init and testbench host tests, doccheck, and
size-budget, and carries a measured `Size budget:` line where it raises a ceiling.

Final commits: 711330611 blkd labels; ddbd568ea fsd pack; c89781164 init range and labels;
451ec8bb4 confined disk; 1e4b2f9ce fsd endpoint=; 26fb424fd bench past a reboot; 37b4b8bdb bound
fix and its test; 68c6c8ba6 image disk; 3234a5a8a corrupt line and fsd-corrupt-volume; 181cb4d75
fsd-restart; 3195a966c fsd-quota; 752ecb4cc fsd-one-volume (R47 probe); e8a9bfb58 confine test
with two fsds; d8f38c3d9 fsd-label-check.

WIP 95f93d935 (fold before the merge):
- confine.rs: the users count per FSD3-confined-users-ruling.md.
- init.md: the ruled line.
- New host test confined_counts_only_a_shared_servers_own_label_set, listed in init.md's
  confinement check and R34 (9 each) and in SECURITY.md's R34 row.
- fsd-confined-labelled, with tests/data/fsd/confined.json.
- fsd.md Volumes status: built · tested (31).

Tests whose verdict the new count changed (question a34b1956; detail in the worktree's
.wash/local/FSD3-confine-casualties.md):
- host confined_refuses_a_driver_serving_two_label_sets: expects Device, now Ok.
- host confined_refuses_a_server_instance_serving_two_label_sets: expects Server, now Ok.
- bench init-refuses-confined-server: now boots, and FAILs on rv64.
- Sharing::Server looks unreachable now.
- Proposed: drop the two host tests; re-aim the bench case at Endpoint, which needs a grant.
  Wait for the answer, then fold the WIP into final commits.

Cases, all PASS on rv64 and rv32:
- fsd-boot, fsd-reboot, image-disk, init-boot
- fsd-restart, fsd-corrupt-volume, fsd-quota, fsd-one-volume
- fsd-label-check, fsd-confined-labelled (WIP)

fsd-label-check, per FSD3-label-check-ruling.md: a {L} writer exits 10, the unlabelled outsider's
line follows (refused at attach, so it never holds a fid), then the writer exits 11 (read back).
Labelled clients cannot write the console, so their verdicts are init's exit lines.

Page lines written:
- fsd.md: Volumes built · tested (31), the five ruled bench lines; Mounting "says so on its
  console"; Authority and R47 built and tested; Failure and restart with the brief's text (doccheck
  C1 removed "Open: none"); R48 tested (3) with fsd-quota.
- blkd.md: Started by init.
- serving.md: R25 built · tested (8).
- SECURITY.md: the R25, R47, R48 and R34 rows.
- init.md: the confinement line (WIP).
Owed: none beyond the WIP fold.

Gates, each exit 0 at d8f38c3d9: fsd-, blkd-, littlefs- and init-host-tests, unsafe-budget,
size-budget, docs, fmt. Not run: the whole bench, which waits on the orchestrator's word; the
gates at the WIP.

What consumed context: two rebases with conflict resolution; the per-commit size-budget rewrite;
splitting mixed files into commits; probing confined manifests; the labelled-console discovery.
