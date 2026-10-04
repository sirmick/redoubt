# FSD1 report (fsd1-implementer)

Branch `wp-fsd1` (worktree .worktrees/fsd1), base main d45ef88eb. Three commits:
- `839f5698b wire: fsd's error table gains corrupt` (table row 8, regenerated Rust + Elixir;
  `Size budget: libs/wire` +3 generated lines)
- `94e3a1349 redoubt-rt: a typed operation finds the caller's fids as 9P does`
  (`NineServer::fid_node` + test; `Size budget: libs/rt` +3)
- `0baabbb4f fsd: one littlefs volume served over 9P, with its labels and typed operations`
  (servers/fsd, workspace member, Cargo.lock, tests/fsd-host-tests.toml, tests/fsd-build.toml,
  size + unsafe budget entries, fsd.md lines and statuses)

## Gates (each `/home/mcloonan/redoubt/.wash/local/in-dev cargo testbench <case>`)

| case | exit |
| --- | --- |
| fsd-host-tests | 0 |
| fsd-build (rv64, rv32) | 0 |
| littlefs-host-tests | 0 |
| client-host-tests | 0 |
| rt-host-tests | 0 |
| wire-host-tests | 0 |
| r4-host-tests | 0 |
| formatting | 0 |
| docs | 0 |
| size-budget | 0 (fsd 702 lines, new; libs/rt 2824->2827; libs/wire 3067->3070) |
| unsafe-budget | 0 (fsd 0 unsafe, 0 undocumented; `#![forbid(unsafe_code)]`) |
| whole bench, both widths | NOT RUN: needs the orchestrator's word |

`cargo test -p redoubt-fsd`: 19 unit + 7 program tests, all pass. Clippy clean for fsd.

## Rules -> code -> test

1. Args/handles: `bin/fsd.rs` (`volume` handle, `buckets=`, `parse_labels` in server.rs;
   anything else = BAD_ARGS before serving) -> `arguments_it_does_not_understand_stop_it_before_serving`.
2. Blocks: `volume.rs` (BLOCK 4096 = 8 sectors, count = sectors/8, <4 = NoVolume::TooSmall,
   exit NO_VOLUME) -> `a_blank_range_is_formatted_and_only_a_blank_one`, `a_range_too_small_is_no_volume`.
3. Mounting: `volume::mount` (blank superblock pair -> format; else unmountable -> Corrupt;
   served, attach = `corrupt`) -> `a_range_of_noise_is_served_as_corrupt` (unit and program;
   bytes unchanged), `a_blank_range_is_formatted_and_only_a_blank_one`. No console line (agreed).
4. corrupt: `server::nine`, `typed::code`; `Fsd::with` drops the fs on Io -> sticky ->
   `a_device_that_fails_makes_the_volume_corrupt_until_it_is_mounted_again`,
   `a_failing_range_answers_corrupt`, `removed_and_corrupt_are_the_tables_answers`.
5. Remove: node = path + id (attr 0; counter attr 3 on root, moved before the id is written);
   `Fsd::find` -> `removed` -> `a_removed_files_other_fids_get_removed` (read/write/stat removed,
   clunk ok, a new file under the same name is not the old fid's), `a_rename_over_a_file_removes_it`.
6. Resolver: `NineServer::fid_node` -> `a_typed_operation_resolves_only_the_callers_own_fids`,
   `a_strangers_fid_is_not_found`.
7. No metering, no init: `limits()` and `minted` default (accepts any quota).
- qid version (attr 2, bumped before every write/truncate) -> `writes_and_truncations_move_the_qid_version`.
- id-less entry gets an id when first reached -> `an_entry_without_an_id_gets_one_when_first_reached`.
- Labels -> `the_volumes_labels_are_checked_on_every_request`, `typed_operations_check_the_volumes_labels`.
- Typed ops -> `rename_moves_within_the_volume_and_keeps_the_files_id`,
  `a_directory_is_not_renamed_into_itself`, `copy_file_copies_and_counts_the_bytes`,
  `a_copy_that_does_not_fit_leaves_nothing`, `attributes_set_and_get_with_fsds_own_types_refused`.
- Client library unchanged vs real fsd -> `the_client_library_works_against_fsd`, `files_survive_a_restart`.
- Admission -> `fids_are_bounded_and_disconnect_frees_them` (a full bucket gives a new session 0
  fids; after `disconnect` of the holder, a new session opens again).
- 9P vectors -> `the_conformance_vectors_run_against_fsd`.

## Mutation checks (each applied, test run, reverted)

- label check: `labels()` returns `&[]` -> `the_volumes_labels_are_checked_on_every_request` FAILS.
- removed generation: `find` accepts any id -> `a_removed_files_other_fids_get_removed` FAILS.
- mount rule: `blank()` always true -> 4 tests FAIL (noise formatted, etc.).
- copy cleanup removed -> `a_copy_that_does_not_fit_leaves_nothing` FAILS.

## Page lines (docs/servers/fsd.md)

- Arguments and Mounting bullets after "One instance per volume", exactly as the brief.
- Metadata bullet: + "A file's qid version moves with every write and truncation, so a client
  caching it sees the change. mtime is 0 until a clock reaches `fsd`."
- Typed operations: + the "Corruption." paragraph exactly as the brief.
- Volumes status: "built · partly tested: one instance per volume under `init`, and the line a
  volume served as corrupt prints there, are not built until `fsd` runs under `init` · tested (14)"
  (package IDs kept off the page). Typed operations: "built · tested (12)". Both sections lost
  their `**Open:** none.` (doccheck C1 forbids Open outside a planned section).
- Quotas, Authority, R47, R48, Failure and restart: still planned.

## Departures / things to check

- tests/fsd-build.toml is not in the brief's owned paths; added as bootfsd-build's twin so the
  bench checks rv32 (implementer rule 5). Drop it if unwanted.
- serving.md's skeleton status list does not name the new rt test (not my page).
- Residual risks: under (A) a rename looks like a remove to other fids and to a connection rooted
  below it; a hostile medium can hold duplicate ids or a low next-id counter (contained to that
  volume, one fsd per volume); id assignment writes on a first walk even for a read-only caller;
  directory listing is O(n^2) in entries (one read_dir per entry); every write costs one extra
  commit (the qid version).
- libs/littlefs: no bug found, no change.
