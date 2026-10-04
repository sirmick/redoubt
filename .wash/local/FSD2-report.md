# FSD2 report (fsd2-implementer-2)

Branch wp-fsd2, tip **d4cb77a39** on main ccf648bad (rebased clean after review round 1, which was 80bf426d9; first report 533553e0b). Focused gates on d4cb77a39: all 9 exit 0; fsd 1138/1138, littlefs 2033/2033, wire 3073/3073. Tree identical to 28bf55ec6
(`git diff 28bf55ec6 533553e0b` empty). The only change is the tip's message, which now names
docs/SECURITY.md's R48 row as outside the owned paths (doccheck needs it), as ruled.

## Commits (all final)
- afdf1c5e4 littlefs: a file's size says how many blocks it holds (+3)
- 95eff08d3 wire: fsd's error table gains no_space (+3)
- 5e3a2c2e5 littlefs: a directory read hands over each pair it reads (+11; fsd +4)
- a85446241 fsd: no metadata pair is named twice, within one directory's chain or across two (+3)
- e6c9c29fb littlefs: an operation makes new metadata pairs only within the room it is given (+18)
- 533553e0b fsd: a byte quota per attach root (+348)

Each one's message has its own "Size budget:" line.

## Gates (each `.wash/local/in-dev cargo testbench <case>` from the worktree, on the tip's tree)
| case | exit |
|---|---|
| formatting | 0 |
| docs | 0 |
| size-budget | 0 (servers/fsd 1154/1154, libs/littlefs 2033/2033, libs/wire 3073/3073) |
| unsafe-budget | 0 (fsd 0 unsafe, littlefs 0; no change) |
| fsd-host-tests | 0 |
| littlefs-host-tests | 0 |
| client-host-tests | 0 |
| --arch rv64 fsd-build | 0 |
| --arch rv32 fsd-build | 0 |

Logs are in .wash/local/gates/. The whole bench has not been run; it waits for the orchestrator's word.

## Rules: code and tests
These are as in FSD2-handoff.md, "Rules". In short:
1. Holds/reserve use tally() and the ledger in quota.rs, with audit() in every quota test.
2. Room uses Fsd::room/charge/recounted, and a refusal is FsError::NoSpace.
3. Rewrite uses file_blocks. Test: a_rewrite_at_the_start_needs_room_for_the_tail.
4. Minting at the first connection counts what is there. Test: a_root_minted_over_files_counts_them.
5. Carving and disconnect have the carve, share, granter's own root and root-above-live-root tests.
6. Rename and remove: a_rename_or_remove_never_ends_a_live_root, a_rename_between_two_roots_moves_the_bytes.
7. RESERVE = 0. Test: the_volume_never_runs_out_while_every_root_is_within_its_quota.
8. Typed no_space. Test: a_copy_past_the_quota_is_no_space.
9. Forged tail, join and loop images are corrupt. Test: split_directories_still_mount.

Q2 gate tests:
- a_root_at_its_quota_creates_while_its_entries_fit_one_pair
- with_room_the_directory_splits_and_the_split_is_charged
- a_mkdir_without_room_for_its_pair_changes_nothing
- littlefs pair_room_bounds_splits_and_new_directories

## Attack cases (R48), verdict from the system
- a_write_past_one_roots_quota_is_refused_while_another_still_writes
- a_root_with_quota_0_cannot_create_but_can_read_and_remove

In both, fsd's own reply (NoSpace or refused) is the verdict, and audit() recounts every root from
the medium.

## Mutation checks (predecessor's, all caught)
- room check: 10 tests fail
- rewrite: 1 fails
- reserve on disconnect: 3 fail
- pair marking: the tail test fails
- gate unlimited: the at-quota test fails

The diff oracle was run by hand after pair_room: 8 passed, 1 ignored, and its target/ was removed.

## Page lines as written (docs/servers/fsd.md)
- "Quotas": the brief's text with the re-ruled split sentence on "A root holds…".
- The re-ruled "No promise the disk cannot keep" bullet.
- The "A quota of 0 means nothing" bullet is kept.
- The Q3 "A rename or remove never ends a live root" bullet.
- "Mounting": "...and no metadata pair is named twice, within one directory's chain or across two".
- "Residual risks": "A refused rename or remove says a live root is there"; the mint-walk sentence is appended to "A shared fsd is shared state".
- R48 is built and tested.
- wire table row 9 is `no_space`.
- libs/client needs no hand change.

## Rulings and deviations
- **Quota 0:** settled and withdrawn. The orchestrator's earlier "quota-0 create now succeeds" was
  wrong. A root holds its own directory's pair, so a quota of 0 reads and removes only. The brief's
  R48 test and page line stand, with no code change.
- **RESERVE = 0:** this follows the ruling's "reserve under the gate", and is with the Architect for
  confirmation. If the Architect rules otherwise, only the constant and its comment in server.rs
  change.
- **docs/SECURITY.md:** the R48 row is outside the owned paths, and the commit body names it.

## Open risks
None new. The whole bench is not yet run.

## Next
1. Reviewers' findings go into the commits that own the code.
2. Rebase onto main ccf648bad on the orchestrator's word.
3. Run the whole bench on both widths on the orchestrator's word.

## Review round 1 (tip 80bf426d9), each change in its owning commit
- **Architect:** `one_commit_splits_only_as_far_as_its_room` is added to libs/littlefs/src/tests.rs, in 9ff079562 (the pair-room commit).
  - Setup: 4 KiB blocks. At pair_room 0, /d gets 7 inline files of 280 bytes, then 49 empty ones (every entry under 300 bytes).
  - Halving moves 28, then 14, then 7 entries.
  - Repeated set_attr commits run until /d compacts. Unbounded it ends at 4 pairs; with pair_room 1 it ends at 2. fsck passes and all 56 entries are listed.
  - Mutation: with the countdown in new_pair removed, it fails (4 != 2).
  - It is listed under fsd.md's littlefs status (15 -> 16), and the commit body says so.
- **Editor (1):** the exact count is 16, not 18.
  - read_dir_at_reports_every_pair_of_a_split_directory is the differential split test. It lives in libs/littlefs/diff (crate littlefs-diff), which the workspace excludes and which runs by hand.
  - doccheck's C2 resolves host: names only in workspace members (tools/doccheck/src/lib.rs `crates`), so listing it would fail the docs gate.
  - pair_room_bounds_splits_and_new_directories was already under Quotas.
- **Editor (2):** "a root with quota 0 reads and removes, but cannot create."
- **Simplifier (1):** RESERVE is deleted. Its reasoning is one comment at the volume root's room in Fsd::new.
- **Simplifier (4):** recounted(root, need, dirs, change) sets the pair room itself, and gate is gone.
  - create and copy_file now take next_id before recounted. Before, the gate ran after next_id, so the id counter's commit had pair room 0.
  - That is still so: room is 0 outside recounted. The root directory is therefore no longer recounted there, since that commit can make no pair.
- **Simplifier (5):** charge, holder, holds_live and cost are gone. Call sites use the ledger and cost(fs, ..).
- **Simplifier (2), taken:** Root.quota and Root.reserve are gone. quota(i) sums the connections at the root (the volume root's is the room), and reserve(i) sums the quotas of the live roots nearest below.
  - All fsd tests and audit() pass unchanged.
  - Mutations caught: not returning held at the last disconnect fails 2 tests; an empty reserve fails 12.
  - quota.rs is +3 lines; the gain is one source of truth, not lines.
- **Simplifier (3), declined:** the medium is re-read on every walk, and nothing keeps what the mount verified.
  - A medium whose bytes change after the mount (R49: whatever bytes it holds) can form a directory cycle only a later tally sees: mint's count, holds, or a rename's or remove's. Without the bound that tally loops forever.
  - mint and holds would still need a subtree walk, so merging tally into ids_are_sound would leave two walks anyway.
- **Size:** servers/fsd is measured at 1138; the ceiling goes from 1154 to 1138. The tip's line is 332, not 348, with the forwarders and stored totals gone.

Gates on 80bf426d9, each exit 0: formatting, docs, size-budget, unsafe-budget, fsd-host-tests, littlefs-host-tests, client-host-tests, --arch rv64 fsd-build, --arch rv32 fsd-build.

## Red reviewer (on 28bf55ec6)
- **P1, with the Architect:** in Ledger::mint's new-root branch the parent can shed held bytes. The new root's whole count leaves the parent's held, but the parent's reserve rises only by the quota. A quota-0 mint over Q bytes, repeated k times, holds k*Q within every root's quota. a_root_minted_over_files_counts_them asserts this gain.
  - Red's two fixes, one to be ruled: the parent keeps max(quota, count + child reserve) in reserve until the last disconnect, or such a mint is refused.
  - This is not fixed yet; it waits for the ruling.
- **Note:** a failed copy whose cleanup remove also fails leaves a partial file uncharged. This is rare, and it is a residual.
- **Note:** inline files are counted twice. This is conservative: it can refuse early, never overshoot.

## Rule 5 amended (red's BLOCK), tip 8a1267f12 on ccf648bad, in the quota commit
- **Charge:**
  - Ledger::charge(j) = max(quota(j), held(j) + reserve(j)), and reserve(i) sums the charges of the live roots nearest below i.
  - Both are computed on demand. "Pass the difference up" therefore needs no code: every change below is seen at the next check.
  - tally's live list carries charges, via Ledger::charges().
  - Carve checks:
    - A new root: left + kept + charge <= the parent's quota.
    - An existing root: the parent must fit the charge's growth.
  - Disconnect is unchanged: held returns to the parent, and the reserve follows by computation.
- **Simplifier (2) kept on its merits:** the amended rule needs no propagation code with it, and stored totals would need it at every charge.
  - Cost: reserve recurses through the live roots, so it is quadratic per level in the number of live roots. That number is bounded by connections.
- **Tests:**
  - a_root_minted_over_files_counts_them: the parent's room is unchanged at quota 32K under a 40K count, and at quota 0.
  - New minting_over_files_frees_no_room: Bob fills, mints quota 0 at x, keeps it, and the same block write is still no space.
  - New minting_above_a_live_root_frees_no_room.
  - Mutation: charge = quota fails all three. Quotas status 17 -> 19.
- **Pages:** "A root holds what lies under it" and "Nothing is stored" are as ruled.
- **Copy cleanup:** once the copy's file was created, the copy is charged holds(to), recounted afterwards. That is the whole file on success, nothing once the cleanup remove ran, and what is there if it failed.
  - There is no direct test. A cleanup remove fails only on Corrupt while the volume is still served (an Io error poisons it), and no fault hook makes Corrupt mid-request.
  - The path itself is the one every copy takes, so the copy tests cover it.
- **Inline double count:** not local, so left. An inline file's bytes sit in its directory's pair, which is already counted.
  - Dropping the length would let a quota-0 root grow an inline file within its pair. That breaks R48's "cannot grow" test and the brief's page line.
  - It over-charges and fails closed.
- **Size:** fsd is at 1149 (+11), the ceiling is 1149, and the tip's line is 343.
- **Gates on this tree:** all 9 exit 0.

## Architect's cost finding, tip 063990be6 on ccf648bad, in the quota commit
- Ledger::totals() is one pass. It finds parent[j] = above(path_j), visits the roots deepest path first, and adds charge(j) = max(quota(j), held(j) + acc[j]) into acc[parent[j]].
  - A root's path is strictly longer than its parent's, so every child comes before its parent.
  - reserve(i) reads totals()[i]. charges() and the test-only roots() come from one pass.
  - The mint branch for an existing root takes reserve and charge from one pass. The recursive charge() is gone.
  - Cost: O(n^2·L + n·conns) per call.
- Tests are unchanged, and all 48 fsd tests pass. Mutation: charge = quota in the pass still fails all three minting tests.
- fsd is at 1158 (+9); the ceiling is 1158, and the tip's line is 352.
- Gates on 063990be6: all 9 exit 0.

## Red's note: allocation, tip e5c4f24a8 on ccf648bad, in the quota commit
- totals() allocates through filled(), which uses try_reserve_exact, and returns None when memory runs out. So do reserve() and charges(), which reuses the totals buffer.
- When memory runs out:
  - fits() is false, so the change is no space.
  - spare() is 0, so the pair room is 0.
  - mint is Refused.
  - The minted hook answers QUOTA_REFUSED.
  - audit (tests only) unwraps.
- fsd is at 1172 (+14), the ceiling is 1172, and the tip's line is 366.
- Gates exit 0: formatting, size-budget, fsd-host-tests, docs, and fsd-build on rv64 and rv32.
- Red's review is complete. The whole bench is queued behind RT1, B7 and K21.

## Rebased onto main 387f1639e (RT1, B7): tip 398271201
- No conflicts. The size-budget toml keeps main's lines; this branch changes only wire, littlefs and fsd.
- All 9 focused gates exit 0.

## Rebased onto main f8c1543f3 (K21): tip 85dd4462d; whole bench
- No conflicts. All 9 focused gates exit 0.
- Whole bench: `.wash/local/in-dev cargo testbench --allow-skip`, both widths in one run, log /tmp/fsd2-bench.log.
  - Exit 0: 340 PASS, 0 FAIL, 1 SKIP (163 rv64 and 139 rv32 lines).
  - The skip is bench-ssh-loopback-openssh: podman is not installed.
