EROFS1 handoff (2026-10-06), implementer erofs1.

## Branch state
Worktree /home/mcloonan/redoubt/.worktrees/EROFS1, branch wp-EROFS1, rebased onto main fc5714880 (B16 merged). Tip 23105516e, 9 commits:
a0c312f81 testbench: host-tests `tools` key (named skip)
b140d367a erofs: libs/erofs parser+writer (+ named field offsets, size 578)
ac1a2282d erofsd: server
ae31b0c05 init: erofsd volume-server test (+ Size budget init 2109)
95821263e testbench: disk recipes fs=erofs + damage; erofs-corrupt, erofs-read-only; pages built; todo page docs/todo/file-server-arguments-and-range-client.md linked from erofsd.md residuals
d7cbe2cc2 erofsd: boot-stats
9cdb3afe2 image: system volume EROFS (erofsd:system), beamlet endpoint=, beamlet's 4 cases now fs=erofs/erofsd:system
ca95eb698 verityd: 4-block data LRU (DATA_CACHE doc says why 4)
23105516e testbench: boot-time target (<30 s / <20 s) in boot-profile(-unverified); both now in the whole run (whole_run removed)
Working tree clean. doccheck --code 0 on every commit of this tip.

## Reviews
Editor OK with notes (all folded). Red OK with notes (P2 1-4 folded). Simplifier OK with notes: (1) named offsets DONE (libs/erofs lib.rs `field::{sb,inode,dirent}`, LAYOUT_*, S_IF*, XATTR_HEADER; used by read.rs, write.rs `put`, testbench disk.rs damage); (3) DONE: endpoint= kept (brief), beamlet's 4 cases moved to EROFS, rv64 rerun beamlet-boot/console/heap-flood/budget-flood all 0; (2) DONE as docs/todo page (no code); (4) DONE. Red renews on the parser delta (the named offsets).

## Exits since the last short gate (on the pre-B16 tip, then this one)
erofs (8) + oracle (2) host tests 0; testbench disk:: 0; erofsd tests 0; fuzz crate check 0; clippy erofs/erofsd both targets clean; size-budget libs/erofs 578 (ceiling raised in the erofs commit). Running at handoff (background, /tmp/erofs1/profile-rv64_erofs-host-tests_...out): erofs-host-tests, erofs-oracle, erofsd-host-tests, size-budget, formatting, no-cruft, unsafe-budget, erofs-corrupt rv64+rv32, erofs-read-only rv64, userland-boot rv64+rv32 (the rv32 scan now under B16), build-rv64/rv32, cargo test -p testbench. Report these exits with the tip.

## Numbers (for the report)
Profile on main 051a2f86c: rv64 16.0/12.5 s, rv32 15.7/12.1 s (littlefs 1016.7/534.7, 1044.2/558.0). Detail in .wash/local/EROFS1-report.md; logs .wash/local/erofs1-profile/. Evidence of the paint duplicate: .wash/local/evidence-erofs1-paint-twice/.

## Traps
- Pool runs: /tmp/erofs1/profile.sh <targets> (make -k, env set). Never -j.
- Folding: git commit --fixup=<sha>, then GIT_SEQUENCE_EDITOR=true git rebase -q -i --autosquash main (main moves often: check git log main.. after). Messages: --fixup=amend:<sha> with GIT_EDITOR="cp file".
- doccheck binary copy at /tmp/erofs1/doccheck for per-commit checks.
- beamlet-lookup-host fails on main (userland/otp/redoubt/tests/limits.rs lacks report_memory): not ours.
- The bench keeps only its last 8 run dirs: copy logs out at once.
- size-budget.toml edits touch several commits' lines: edit the one line per fixup.

## Next
When the background checks end: report the tip (23105516e or later if main moved) with their exits to the orchestrator; the red renews on the parser delta.
