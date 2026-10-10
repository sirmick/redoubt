# SHELL6 Files report (for main, Tier A)

Branch `wp-SHELL6-files`, worktree `.worktrees/SHELL6-files`, base origin/main 415c86ad6, one
commit, 14d5d2766. Not pushed.

## Delivered

- `userland/shell/lib/redoubt/editor/files.ex`: `Redoubt.Editor.Files`.
  - `read/1`: a regular file whole, up to `max_bytes/0` (2 MiB, the one constant). Returns
    `%{bytes, utf8, digest}`; refuses with `{:too_large, size, max}` or `{:not_a_file, type}`.
  - `save/3`: writes `.NAME.saving-<12 hex>` in the same directory (opened `:exclusive`), then
    renames it over the path; removes it on failure. The third argument is the digest read,
    `:absent` or `:any`; a file that doesn't match gets `{:error, :changed}`.
  - `list/1`: entries with `shown` (`Text.visible`), `ok` and `type`. A refused name is never
    stat'ed or joined to a directory.
  - `name_ok?/1`: refuses a name holding `/` or NUL, or empty, `.` or `..`.
  - `copy/3`, `move/3` (`Helpers.mv`, so a rename, or a copy and a removal across volumes),
    `mkdir/2`, `remove/2`: act only on a name in its listed directory. No overwrite (`:eexist`);
    no directory into itself (`:einval`).
- `userland/shell/test/redoubt/editor/files_test.exs`: 6 tests, attack cases with verdicts from
  the file system (the whole tree compared before and after):
  - crafted names (`../outside`, `../outside/keep.txt`, `sub/b.txt`, `/`, `..`, `.`, empty,
    NUL) for all four operations: nothing changes anywhere;
  - names with ESC, RLO and BEL: listed escaped, with no control or bidi character in `shown`,
    and copied or removed as themselves;
  - the pane operations: no overwrite, no directory into itself, `outside` untouched;
  - a save reaches only its path and leaves no temp file, the changed and absent checks hold,
    and a failed save leaves nothing behind;
  - a modeline or escape sequence comes back as its bytes;
  - the size cap and a directory are refused.
- `docs/userland/shell.md`: a new section, "The editor's files", status built (host only). "The
  editor" stays planned.

## Gates

- `./test-shell test/redoubt/editor/files_test.exs`: beamlet 6/6, BEAM 6/6.
- Full `./test-shell`: rc=1. beamlet 161 passed, 0 failures. On the BEAM, 152 of 153 passed:
  driver_test "hostile text a line writes to the console itself never reaches the terminal as a
  control sequence" fails, and sometimes also "a crash report of a process a line spawned...".
  This is on main (415c86ad6, the SHELL8 merge), not this change: the BEAM suite fails the same
  way with files_test.exs left out. It reproduced 4 of 4 runs. Row 1 reads `F")` where the
  escaped line is expected.
- doccheck rc=0; mix format clean.
- `make prebuilt`, then the shell set from the worktree's `./scripts/shell-cases origin/main`
  (14 cases): 28/28 PASS on both widths.

Note: `/home/mcloonan/redoubt/scripts/shell-cases` reads the main checkout's git state, not the
worktree's. Run the worktree's own `./scripts/shell-cases`. For SHELL7 the root's copy picked
beamlet's set (30 cases), a superset of the shell set, so that gate still covered it.

## Not in this package

The editor UI, buffer, highlighting and file manager are SHELL6a, 6b and 6c on `shell`, which
carry this commit as a dependency until it lands on main.

## Fold of red's notes on 14d5d2766 (2026-10-08): head 71746888c

wp-SHELL6-files rebased onto origin/main 5f66a4963; the fold is amended into the one Files commit,
71746888c (message updated).
- P1: save opens the temp, writes with :file.write/2, closes, renames; a write, close or rename
  that fails removes the temp and returns {:error, _}; nothing raises. Tests: a save over a
  non-empty directory (written, rename refused: temp removed, tree unchanged); a directory chmod
  0555 refusing the write (save and save-new both {:error,_}, tree unchanged, no raise), on both
  VMs. A failing write itself (ENOSPC/EROFS after open) cannot be induced on either host VM; it
  takes the same {:error,_} branch as the rename test.
- P2: read and unchanged/2 both read through read_capped/1: at most max_bytes + 1 bytes, looping
  :file.read to EOF or the cap. Test: a file grown past the cap since it was read is :changed and
  left as is.
- P2: open's error (eexist included) returns without File.rm: the temp is not ours.
- Notes: agreed; the page records both as a Residual paragraph (a built section may not carry
  **Open:**): mv's exdev copy left beside the source when removal fails; directory-into-itself
  check by path as written, so a link into the source is followed by cp_r.
- Page: Reading and Saving bullets updated to the capped read and the returned errors.

Gates (worktree, head 71746888c):
- ./test-shell test/redoubt/editor/files_test.exs: rc=0, 8/8 on beamlet and BEAM.
- ./test-shell (full): rc=0, every stage passed.
- make -f scripts/jobs.mk docs: rc=0. mix format clean (test-shell's formatting stage).
- make prebuilt rc=0, then set CASES="$(./scripts/shell-cases origin/main)": origin/main had moved
  to 5151adaf7 by then, so it chose beamlet's set (30 cases, a superset of the shell's 14):
  60/60 PASS, rv64 and rv32.
- beamlet-footprint (on 5f66a4963, before SHELL7's lazy loading; superseded by the rebased
  head's figures below). Files is loaded only when the editor calls it, not at the prompt.

Rebased head 62db903c4 on origin/main 5151adaf7: full ./test-shell rc=0; docs rc=0; prebuilt
rc=0; beamlet-footprint PASS: rv64 heap 5,447 of 11,885 pages, stack 35,880 B; rv32 heap 5,265,
stack 28,960 B: main's figures, Files off the prompt. (Corrected 2026-10-08: an earlier message
gave the widths the other way round.)

Summaries checked: README.md, docs/README.md, docs/userland/README.md, docs/plan/m2-usable-shell.md,
docs/GLOSSARY.md: they name the editor as M2's goal; Files alone builds no editor, so no change.
