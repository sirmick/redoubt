# FSD1 handoff (fsd1-implementer)

State: built, committed, in review (red at medium, simplifier, editor) on 0baabbb4f. Whole bench
on both widths still owed: third in the host's queue (after K21, INIT2); the orchestrator says when.
Detail of gates, rules -> tests, page lines: .wash/local/FSD1-report.md.

## Branch wp-fsd1 (worktree .worktrees/fsd1), base main d45ef88eb

1. `839f5698b wire: fsd's error table gains corrupt`: libs/wire/tables/fsd.md row 8, regenerated
   proto/fsd.rs and elixir/proto/fsd.ex; tests/size-budget.toml libs/wire 3067->3070 with its
   `Size budget:` line.
2. `432fd5d11 redoubt-rt: a typed operation finds the caller's fids as 9P does`:
   `NineServer::fid_node(&self, caller, fid) -> Result<(S::Node, Qid), NineError>` (over the
   private `fid`/`conn_key`), test `a_typed_operation_resolves_only_the_callers_own_fids` in
   ninep_tests.rs; libs/rt ceiling 2824->2827. Nothing else in libs/rt (RT1 owns it; told).
   If the editor wants serving.md's skeleton test list to name the test, add it in THIS commit.
3. `943309106 fsd: ...`: servers/fsd (lib, volume.rs, server.rs, typed.rs, blkd.rs, bin/fsd.rs,
   server_tests.rs, typed_tests.rs, tests/fsd.rs), root Cargo.toml member, Cargo.lock,
   tests/fsd-host-tests.toml, tests/fsd-build.toml (orchestrator: stays), fsd size entry 702 and
   unsafe entry 0/0, docs/servers/fsd.md (Arguments + Mounting bullets, Metadata residual,
   Corruption paragraph, statuses: Volumes built·partly tested (14), Typed built·tested (12),
   their `**Open:** none.` removed for doccheck C1).

To change a commit: commit an `amend! <subject>` (or `fixup!`) and run
`GIT_SEQUENCE_EDITOR=true git rebase -q -i --autosquash d45ef88eb` with a clean tree. No stash.

## Fix round 1 (done)

Read-only ranges (Geometry.read_only from blkd info; Fsd::writable gates every change; Blocks
refuses prog/erase as backstop; blank read-only not formatted); mount-time `ids_are_sound`
(distinct ids, counter above all, no stored id >= TRANSIENT=1<<63, depth <= 255); transient ids
when give_id fails (ReadOnly/NoSpace); code() moved beside nine(); found_child helper;
Fsd::corrupt and Mounted::Files{formatted} deleted; unit noise test renamed
noise_is_never_formatted_and_never_mounted; serving.md lists the rt test (16); fsd 805 lines;
budget lines in the fsd commit. P1a (id write on a read path) HELD for the Architect.

## Design as settled

- Node (A): `Node { path, id, dir }`; path only from client walk/create names. id = fsd attr 0
  (u64 LE), given from the root's counter attr 3 (moved first, never reused); root id 0 with no
  attr. `Fsd::find` re-resolves path and compares id: absent/mismatch -> `removed`. Remove and
  rename-over end other fids; a rename also looks like a remove to fids below it (accepted cost).
- Id on first touch: an entry without attr 0 gets one in `node_at` (walk, create, listing), as
  finishing a create a power cut split (littlefs commits attrs apart from the entry).
- qid version: attr 2 (u32 LE), bumped BEFORE every write and OTRUNC; dirs 0. mtime 0, residual.
- Mount: <4 blocks NoVolume (exit 5); superblock pair all zero -> format; else Corrupt, served,
  attach `corrupt`. Io from blkd -> `Fsd.fs = None`, sticky. No console line (no handle, no print
  facility; status residual, orchestrator agreed).
- Listing skips names that are not UTF-8 valid components (never joined into a path).
- Typed ops: `Typed(&mut NineServer)`; label check via `check` against volume labels; set_attr
  refuses types < 16, > 1022 bytes too_large; failed copy removes its partial destination.

## Tests and mutations

cargo test -p redoubt-fsd: 19 unit (9P via answer_in_place on `Memory` range; typed ops direct) +
7 program (fake blkd process over memory, real bin, client lib unchanged, args, too-small,
noise, failing range, restart, admission). Mutations each fail a test: labels()->&[];
find accepts any id; blank() always true; copy cleanup removed.

## What the Architect's fid ruling could change

If (B) (fids follow renames): `Node` becomes id-keyed with an fsd table id -> (parent id, name)
and a generation per id; `find`/`node_at`/rename in typed.rs change; tests
`a_rename_over_a_file_removes_it` and the client test's "renamed handle reads Rerror" invert.
Pages: Volumes bullet wording. Under (A) nothing changes.

## Traps

- Format only with nightly: `in-dev cargo +nightly fmt -p redoubt-fsd`; stable fmt rewrites all of
  libs/rt (happened once; restored).
- Program tests: `Volume::stop` must ping fsd (Tversion) before destroying endpoints, and stop
  blkd last, or fsd races in mount and exits NO_VOLUME.
- StartupBuilder::new(n) takes the highest handle slot, not the receive slot.

## What consumed my context

Reading ninep.rs/littlefs APIs, the bootfsd model, writing ~1500 lines, and the gate runs.

## Fix round 2 (ruling applied), tip 970f7ddfd

Commits now: 839f5698b wire, 432fd5d11 rt, b000cc1f4 littlefs (mkdir_with_attrs /
open_with_attrs: attrs in the creating commit; crash test
a_create_with_attributes_is_never_seen_without_them fails at write 8 if done in two commits;
budget 1941->1967), 970f7ddfd fsd. give_id/TRANSIENT/first-touch are gone: `next_id` moves the
counter (own commit), then create/copy write ATTR_ID in the creating commit; `node_at` treats a
missing id as corrupt; `ids_are_sound` requires every entry to have a valid name and an id, no
duplicates, counter above highest. New tests: a_higher_reader_writes_nothing (write counter),
a_rename_cut_short_still_mounts (Memory.fail_at), a_volume_whose_ids_do_not_hold_together_is_corrupt,
fids_on_a_renamed_file_or_below_a_renamed_directory_are_removed. fsd 798 lines. Ruling's page
lines in fsd.md (littlefs bullet in the littlefs commit). Reorder trick: GIT_SEQUENCE_EDITOR script
moving a pick line. Not run: littlefs diff/ C oracle (outside the gates).

## Red round 2 fixes, tip e617d004e (2026-10-03)

Commits: 839f5698b wire, 432fd5d11 rt, fa05563ab littlefs (+ diff/ oracle case
creates_with_attributes_read_in_c; oracle run once: 7 passed, 1 ignored), e617d004e fsd.
ids_are_sound: ids checked incrementally (sorted Vec, binary_search -> duplicate = corrupt at
once) and directory visits bounded by blocks/2 (Mounted::Files now carries `blocks`). Test
directories_that_alias_the_root_are_corrupt_at_once forges the root's dirstructs to {0,1}
(point_directories_at_the_root re-signs commits) and bounds reads < 1000; without the incremental
check 4954 reads, without both 296839. The visit bound alone is not separately testable (any
revisit repeats an id). Attributes paragraph states the counter's rule (failed create burns an id
and a write). Never leave libs/littlefs/diff/target behind (untracked; removed).

## Red round 3 fixes, tip d863f2d93

littlefs 36fc112a6 adds DirEntry::attr (read_dir passes each entry's attrs; test
a_directory_read_carries_each_entrys_attributes; budget 1974). fsd: ids_are_sound has no depth rule,
reads ids from DirEntry::attr (no per-child get_attr), sorts once, visits <= blocks/2 dirs.
Tests: a_tree_renames_made_deep_still_mounts (300 deep via renames; depth-255 mutation fails it),
aliasing test renamed directories_that_alias_the_root_are_refused_by_a_bounded_walk (reads < 50 per
block). fsd 793. Residuals for the report: (1) two empty dirs aliasing one pair pass the mount
check (hostile image only): a later create shows under both and removing one frees a pair the other
names; (2) read_dir by path per directory makes a deep chain's mount O(depth^2) pair fetches.

## Red round 4 fixes, tip 836c4f545 (now f8e2e9bdb after round 5: dir case in the cut-rename test)

littlefs ab90b7be2 (subject now covers both): DirRef, root_dir, DirEntry::dir, read_dir_at
(returns pairs read); budget 2001. fsd ids_are_sound walks by DirRef: no paths, total pairs read
<= blocks/2, ids and directory head blocks each checked for repeats (sort once). Test Memory is
sparse (BTreeMap of sectors). Tests: a_directory_aliasing_the_root_is_refused_in_linear_time
(64k blocks, 98314 reads; < 8 reads/block), two_directories_sharing_a_pair_are_corrupt. Both
round-3 residuals closed. fsd 799. C oracle rerun: 7 passed. Trap: a timeout-killed mutation
run leaves its test binary running in the dev container: pgrep and kill it.
