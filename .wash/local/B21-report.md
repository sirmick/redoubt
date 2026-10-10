# B21 report

Branch wp-B21, head ca113af60, one commit rebased onto main ef2ab5a94 (range-diff: unchanged from 68f258bac):
`littlefsd, walfsd: a typed refusal carries the table's name for what it means`.

## What changed

- **The table.** libs/wire/tables/littlefsd.md drops code 3 (`refused`), since no littlefsd
  answer sends it any more. It gains 10 `not_permitted`, 11 `not_supported`, 12 `bad_name`,
  13 `read_only`, 14 `no_memory`, 15 `is_dir` and 16 `not_empty`. The generated Rust and Elixir
  in libs/wire/src/proto/littlefsd.rs and libs/wire/elixir/proto/littlefsd.ex come from
  `cargo run -p redoubt-wire-gen`. wire.md's one name table is unchanged.
- **littlefsd's `code()`** (server.rs) gives each failure the name `nine()` gives it over 9P:
  NoMemory -> no_memory, ReadOnly -> read_only, IsDir -> is_dir, NotEmpty -> not_empty,
  NameTooLong -> bad_name, Invalid -> not_permitted (as `nine()`'s PERMISSION).
- **littlefsd's typed.rs:**
  - label check, live root, the server's own attribute types (0-15) -> not_permitted
  - copy of a directory -> not_supported
  - invalid old_name, new_name or dst_name -> bad_name
- **walfsd** (server.rs and typed.rs) serves the same table with copied code, and is changed
  line for line (orchestrator: same commit, WFS2 rebases).
- **beamlet** `files.rs` `refusal()` maps each new code to its `Name`. POSIX comes only through
  wire.md's last column: not_permitted -> eacces, bad_name -> einval, read_only -> erofs,
  no_memory -> enomem, is_dir -> eisdir, not_empty -> enotempty, not_supported -> enotsup.
  The orchestrator's eperm and enametoolong would change the one table, so they are not used.
- **Tests:**
  - littlefsd and walfsd typed_tests, quota_tests and tests/*.rs, and libs/client/tests/littlefsd.rs,
    assert the new names. The read-only cases now assert `read_only`; the 9P side already said
    ReadOnly, which shows the old inconsistency.
  - New `host:beamlet-redoubt::a_rename_the_volume_refuses_is_eacces`.
  - bench:beamlet-files: its Erlang module renames /home/alice/d into d/inner/d and expects
    `{error,eacces}`.
  - copy_file is not reachable from beamlet: OTP's file:copy is open, read and write, and the
    platform has no copy native. Copying a directory -> not_supported is asserted in both
    servers' host tests.
- **Left as is:**
  - `ninep_common`'s `refused` for a refused new_connection really is a connection's refusal.
  - bootfsd's one-code table only reaches init and the steward natively, never File.
  - consoled and erofsd send no `refused`.

## Gates (on the pre-rebase tree; B22's merge touches no file here; docs rerun after the rebase)

- prebuilt rc=0; build-rv64 rc=0; build-rv32 rc=0
- beamlet-files: rv64 rc=0, rv32 rc=0
- littlefsd cases (rv64), all rc=0: build, boot, confined-labelled, label-check, one-volume,
  large-directory, corrupt-volume, restart, reboot, quota
- walfsd cases (rv64), all rc=0: boot, confined-labelled, corrupt-volume, flipped-block,
  label-check, one-volume, power-loss, reboot, quota
- Host tests, all rc=0: littlefsd-host-tests, walfsd-host-tests, wire-host-tests (includes
  generated_files_are_current), client-host-tests, beamlet-lookup-host (beamlet-redoubt, fake).
  The new test and every_row_of_the_error_table_maps_to_its_posix_error were also run by name:
  both ok.
- docs rc=0 (before and after the rebase); formatting rc=0 (after `cargo +nightly fmt`)

## Docs

- docs/servers/littlefsd.md, Typed operations: rename into itself is `not_permitted`; copying a
  directory is `not_supported`. A new "Refusals" paragraph names each case and says `refused` is
  a connection's refusal. Reserved attributes are `not_permitted`. The minted-quota `refused` is
  ninep_common's and is unchanged.
- docs/servers/walfsd.md, Typed operations: reserved types are `not_permitted`, with the same
  names as littlefsd.
- docs/servers/wire.md, Error names: the example is now not_permitted, not "littlefsd's refused".
- docs/userland/files.md: the server's reasons list now has not_permitted, not refused. The status
  list gains the new host test (19 -> 20).
- Checked, no change: wire.md line 338 (ninep_common's minted `refused`), bootfsd.md line 79
  (bootfsd's own table).

## After the rebase onto ef2ab5a94

range-diff 68f258bac = ca113af60. On ca113af60: prebuilt, beamlet-files rv64 and rv32, build-rv64, build-rv32 and docs, all rc=0. wire.md:169 maps not_permitted to eacces, so a directory renamed into itself reads eacces, and the mapping is as the column says.
