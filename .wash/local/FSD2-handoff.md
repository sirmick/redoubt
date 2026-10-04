# FSD2 handoff (wp-fsd2, worktree .worktrees/fsd2)

Tip 28bf55ec6, tree clean. All six commits are final (re-rolled, no WIP):
- afdf1c5e4 littlefs: file_blocks (+3 lines)
- 95eff08d3 wire: no_space row 9, regenerated (+3)
- 5e3a2c2e5 littlefs: read_dir_at hands each pair to a callback; read_dir returns its pair count; diff-oracle test (+11; fsd +4 call site)
- a85446241 fsd: rule 9, the mount walk marks every pair block (bitmap) (+3)
- e6c9c29fb littlefs: set_pair_room (Q2 re-ruling); without_pairs for the repair, the rename source, drop_orphan (+18)
- 28bf55ec6 fsd: quotas (quota.rs ledger, server.rs/typed.rs charging, pages) (+348)
Each commit's message carries its "Size budget:" line. The old WIP tip 2d002f7f7 is the same tree less the littlefs lib.rs doc and the budget.

NOT YET RUN on the re-rolled tip: the focused gates (formatting docs size-budget unsafe-budget fsd-host-tests littlefs-host-tests client-host-tests fsd-build). They were all green on the same code before the re-roll, except size-budget (now raised) and formatting (diff.rs now formatted). Run each as `.wash/local/in-dev cargo testbench <case>`. No whole bench yet: it needs the orchestrator's word.

Rules (all have code and tests):
1. Holds/reserve: tally() and the ledger; audit() in every quota test.
2. Room: Fsd::room/charge/recounted; no allowance; quota refusals are FsError::NoSpace.
3. Rewrite: rewrite() via file_blocks; test a_rewrite_at_the_start_needs_room_for_the_tail.
4. Count at the first mint: minted(); a_root_minted_over_files_counts_them.
5. Carving and disconnect: quota tests (carve, share, the granter's own root, a root above a live root).
6. Renames, and Q3 remove refusal: a_rename_or_remove_never_ends_a_live_root, a_rename_between_two_roots_moves_the_bytes.
7. RESERVE = 0 with its reasoning; the_volume_never_runs_out_while_every_root_is_within_its_quota.
8. no_space: a_copy_past_the_quota_is_no_space.
9. Forged tail, join and loop images, plus split_directories_still_mount.
Q2 re-ruling tests: a_root_at_its_quota_creates_while_its_entries_fit_one_pair, with_room_the_directory_splits_and_the_split_is_charged, a_mkdir_without_room_for_its_pair_changes_nothing, littlefs pair_room_bounds_splits_and_new_directories.

Mutation checks, all caught: room check (10 tests fail), rewrite (1), reserve on disconnect (3), pair marking (the tail test), gate unlimited (the at-quota test). The diff oracle ran by hand after pair_room: 8 passed, 1 ignored; its target/ is removed.

Deviation to report: the quota-0 root still CANNOT create. A root holds its own directory's pair (8 KiB), so a quota of 0 is over from the start. This matches the brief's R48 test and its page line, but not the orchestrator's "quota-0 create now succeeds". The orchestrator must rule; the alternative is to charge a root's first pair to its parent.

Pages written: Quotas (with the re-ruled lines and the No-promise bullet), R48 built, Mounting, both residuals, the status lists. docs/SECURITY.md R48 row changed for doccheck: outside the owned paths, so report it. libs/client needs no change for the new row.

Next: run the focused gates, then the final report (detail in .wash/local/FSD2-report.md, not written yet).

What consumed my context: three rulings and re-rulings on SPLIT_PAIRS (simulations), the re-roll of the commits, and full reads of server.rs and typed.rs.

Note after the orchestrator's messages 661e0e21 and 36b2c8c3, which arrived after this handoff: the Q2 re-ruling is already APPLIED in e6c9c29fb and 28bf55ec6. RESERVE = 0 follows the ruling file's "Rule 7's reserve under the gate": uncharged commits get pair_room 0 inside littlefs, so they cannot split. If the Architect rules otherwise on RESERVE, change only the constant and its comment in server.rs. The split search (.wash/local/FSD2-split-sim.py, FSD2-split-pairs.md) is evidence for the reserve's sizing only.
