# SHELL6a report: the editor (without highlighting and the file manager)

Branch `wp-SHELL6`, worktree `.worktrees/SHELL6`, base origin/shell 80c703534 (SHELL5 in). Not
pushed. Commits:

1. `1a5c3cdb1`: the editor's files. A cherry-pick of main's package 14d5d2766
   (`wp-SHELL6-files`), carried as a dependency until it lands on main. Drop it at rebase once
   main has it.
2. `47cf2cc5a`: `Redoubt.Editor.Buffer`, the pure text model.
3. `314349273`: `Redoubt.Editor` (`ed/1`, the screen) and `Redoubt.Editor.View` (line runs), the
   pages.

## What it does

- **`ed(path)`:** a missing path is a new file; a non-UTF-8 file opens read only, with bytes
  shown as `<FF>`; a file over `Files.max_bytes` (2 MiB) is refused, naming the size and the
  limit.
- **Keys** (micro's): Ctrl+S, Ctrl+Q, Ctrl+F, Ctrl+N, Ctrl+R (`/re/` for a regex), Ctrl+L,
  Ctrl+Z, Ctrl+Y, Ctrl+X, **Alt+C copy**, Ctrl+V, Ctrl+A, Shift to select, Ctrl and arrows for
  words, Ctrl+Home and Ctrl+End, Ctrl+O to open another file, Alt+, and Alt+. to switch, F10 or
  Alt and a letter for the menus (File, Edit, Search).
- **Ctrl+C** still ends the screen; the help page and the page say so (ruling 2b).
- **The paste rule** (ruling 3): a save, close or open whose key arrives with other messages
  already queued (`Process.info(:message_queue_len) > 0`, a burst) opens a confirm dialog, and
  keys arriving within 300 ms of it are dropped. Tested: a pasted Ctrl+S with Enter behind it
  saves nothing, and the person's later Enter does save. The gap is named in the Open list of
  shell.md's paste section; the editor section is built, and doccheck forbids Open there.
- **Saving:** goes through `Files.save`, with the digest from the read; a file changed on disk
  asks before overwriting. Closing with changes asks; Yes closes only after a successful save.
- **The view:** a tab goes to the next stop of 4; a control or bidi character is drawn through
  `Text.visible`; the cursor and the selection are theme roles; half a wide character becomes a
  blank. Horizontal and vertical scroll follow the cursor.

## Tests

- buffer_test (9), editor_test (8 on both VMs, plus a beamlet-only screen test: a file with OSC
  and BEL drawn visibly, typing, Ctrl+S, Ctrl+Q), files_test (6).
- Full `./test-shell`: rc=0. beamlet 223 passed, 0 failures; BEAM 194 passed, 29 skipped.
- doccheck rc=0, formatting clean.

## Footprint (prebuilt, jobs.mk, PASS both widths)

| | rv64 | rv32 |
| --- | ---: | ---: |
| origin/shell 80c703534 | 5,433 | 5,253 |
| wp-SHELL6 head | 5,435 | 5,252 |
| accounted at the prompt, both | 3,867 | 3,802 |

Modules at the prompt: 90 on both. The editor loads when `ed` is called. The pages keep 5,432 /
5,253; the difference is run-to-run noise.

## Pages

- shell.md "The editor": now built (partly tested), with what is not built (highlighting, the file
  manager); the Alt+C departure; the paste rule.
- shell.md "The editor's files": its status now names only the file manager as unbuilt.
- shell.md paste section: the Open item names the gap.
- m2-usable-shell.md Progress: the editor.
- README and GETTING-STARTED have nothing on the editor. The intro's `ed("notes.txt")` is now true;
  its `fm("project")` is not yet.

## Next

6b (highlighting), then 6c (the file manager), on this branch, each its own merge.

## Fold of b22-red's verdict and the orchestrator's asks (2026-10-08)

wp-SHELL6 rebased onto origin/shell 94845840e (which now carries Files, 62db903c4, through its
merge), so the branch is the two 6a commits: 93aa34a17 Buffer, 172dded2f Editor (both amended,
messages updated).
- BLOCK (paste ending in a guarded key): a key is in a burst when keys are queued behind it, or
  when it comes within @burst_ms (300) of a key that had keys queued behind it (state.burst_at,
  monotonic ms). A paste's last key meets an empty queue but follows its queued predecessor by a
  draw's time, so Ctrl+S, Ctrl+Q and Ctrl+O at a paste's end ask. Test: for each of the three, a
  paste "x" with its last key queued, then the guarded key with nothing behind: the guard dialog,
  file unchanged; and the same key 1 s later acts at once. The screen test now types Ctrl+S once
  the X is drawn plus 300 ms, as a person would. Moduledoc and page say the rule.
- Undo bound (orchestrator: in 6a). Measured unbounded on the BEAM, 500 steps each after a
  top/bottom jump on a 2 MiB file: ~350 MiB heap (64-byte lines), ~2.6 GiB (8-byte lines).
  Buffer now counts per step the lines its lists were rebuilt over since the step before
  (`relinked`: cursor crossings and inserted lines, capped at the file's size; replace_all counts
  the whole file); record keeps the newest step always, then older ones while within 500 steps
  and @undo_lines = 1,000,000 lines (~2-2.5 words a line on the BEAM, so ~2.5M words, under a
  sixth of a screen's 16M-word max_heap_size). Test (buffer_test): 2 MiB of 15-byte lines
  (139,810 lines), 250 rounds of top+insert, bottom+insert (500 edits): older steps hold
  <= undo_lines, newest <= size, 2 < steps kept < 500, the last two undos undo the last two
  edits; on the BEAM the process holding the buffer after GC is <= 8M words (half a screen's
  heap; it measured 5.19M total_heap_size, slack included). Runs in seconds on beamlet.
- Ctrl+C copies (SHELL10's ctrl_c: :key): ed runs its screen with ctrl_c: :key; {"c",[:ctrl]}
  is :copy; Alt+C gone; hint, menu, help, moduledoc and page say Ctrl+C copies and Ctrl+\ ends
  the editor. The screen test sends 0x03 through the driver: the editor stays and says
  "nothing selected".
- The leftover unused `asked` field in the editor's state is gone.

Gates on head 172dded2f (base origin/shell 94845840e): ./test-shell (full) rc=0, every stage
passed (the earlier fake-kernel console flake, an_end_of_input_already_waiting..., did not
recur; no userland/otp change here); docs rc=0; format clean; make prebuilt rc=0;
beamlet-footprint PASS rv64 heap 5,448 / stack 35,880 B, rv32 heap 5,266 / stack 28,960 B of
11,885 pages: base rv64 5,447 / rv32 5,265, +1 page, the prompt unmoved. (Corrected: the first
report gave the widths the other way round.)
Summaries: docs/userland/shell.md's editor section (keys, burst, undo) updated; README.md,
docs/README.md, docs/userland/README.md, docs/plan/m2-usable-shell.md name the editor as M2's
goal with no key or undo claims: no change.

## Fold of b22-red's notes on 172dded2f (2026-10-08): head 18476976d

Rebased onto origin/shell 44d970ab4 (SHELL4), no conflicts. 15cc885e5 Buffer, 18476976d Editor.
- Undo counts bytes: a step's cost adds div(byte_size(cursor line), 16), a line's worth per 16
  bytes, for the copy of the cursor's line each step keeps. Test: one 1 MiB line, 500 x (type a
  key, move left): older steps <= undo_lines, < 100 steps kept (~15), the last undo right; on the
  BEAM the process's off-heap binaries <= 32 MiB (~16 MiB kept; 500 MiB unbounded).
- Found while testing: String.split_at walked the whole line even to cut at column 1; put/2 now
  cuts walking only the graphemes before the cursor (String.next_grapheme_size).
- Page: the undo paragraph gains the byte term and the long-line figures; the burst paragraph
  notes the 300 ms are between handled keys, so a paste of keys each slower than that could
  outrun it, bounded by the screen's heap limit.
- Report widths corrected above (rv64 5,448 / rv32 5,266 on 94845840e).
Gates on 18476976d: full ./test-shell rc=0; docs rc=0; format clean; prebuilt rc=0;
beamlet-footprint PASS rv64 heap 5,455 / stack 35,880 B, rv32 heap 5,274 / stack 28,960 B;
base origin/shell 44d970ab4 measured the same (5,455 / 5,274): +0.
6b (52aa9b31b) is parked on local branch wp-SHELL6b, to be rebased onto this head.
