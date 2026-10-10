# B45 report: a long printed line no longer ends the session

Branch `wp-B45` (worktree /home/mcloonan/redoubt/.worktrees/B45), on origin/main 3206c43b4.
Head 46d8c6284, one commit. Not pushed. Strict track: the escaping and drawing path
(Redoubt.Term, Redoubt.Term.Text, the driver).

## What died, and why

- Reproduced on main, rv64: IO.puts(String.duplicate("z", 70_000)) in alice's session over SSH
  gave {'EXCEPTION',exit,killed}, and the VM exited.
- The driver is the VM's top process (Redoubt.Shell.start runs Driver.run). It was killed at the
  VM-wide max_heap_words, a sixteenth of the session's 11,008 pages = 352,256 words.
- beamlet's VM limit counts `usage.total_words()`: the heap plus the off-heap binaries a process
  holds (vm.rs over_memory), each binary once. A process's own max_heap_size counts only its heap
  unless include_shared_binaries is set, which is why the first host repro, with only an own
  limit, missed the 1 MiB case.
- What the driver held:
  - Text.visible's list, one element per byte;
  - advance_text's list of graphemes;
  - the whole line's drawing, held beside the line;
  - a copy of the binary group sent, made by characters_to_binary.
- On the host with no limit, beamlet drew 70 KB in 4.2 s.

## The fix (46d8c6284)

- Term.draw_text makes text visible and measures it a piece of at most 4096 bytes at a time:
  - each piece is cut before a newline or where a UTF-8 sequence starts;
  - Text.visible/2 carries the tab column across pieces;
  - advance_text walks graphemes with String.next_grapheme, building no list.
- Term.slices/3: text printed with no line open and past 64 KiB becomes put_chars pieces, cut at
  character starts. The driver draws and writes each before the next, so it holds the text and
  one slice's drawing. A line open is not sliced: its text goes above the line, which is redrawn
  below it.
- Term.text no longer copies a unicode binary (for any binary the result was already the same
  bytes).
- The encoder keeps `tab`, the code points of the printed line so far. Tab stops now continue
  across cuts and across prints; before, each print restarted them at 0.
- A grapheme cut between two 4096-byte pieces is measured as its parts. This is a residual,
  stated on the page.

## Tests

- driver_test (beamlet and BEAM): the driver runs under max_heap_size 352,256 words with
  include_shared_binaries, as on the machine.
  - A 70,000-byte line drawn on the model, followed by :seventy.
  - 1 MiB of z, and 1 MiB of ESC drawn as ^[ with no raw ESC, each followed by its value; the
    driver stays alive.
  - Both failed with killed before the fix.
- term_test:
  - a line longer than a piece, with é at the cut and a tab after it, is drawn as one line;
  - drawing the slices one after another gives the same screen and cursor as drawing the whole
    (this caught the tab-stop restart);
  - an open line and other requests are not sliced.
- shell-long-output (new case, rv64 and rv32): 70,000 bytes and 1 MiB printed in a session over
  SSH, each followed by its value, then {:still, 2}; forbids EXCEPTION and killed.

## Gates on 46d8c6284

- `q run -- ./test-shell`: exit 0, every stage.
- prebuilt: 0.
- `make -k -j -f scripts/jobs.mk set` with shell-cases origin/main, shell-long-output, no-cruft,
  size-budget and formatting: 38 cases, exit 0, 73 PASS.
  - shell-long-output: rv64 167 s, rv32 186 s.
  - beamlet-footprint passed on both widths.
- doccheck: 0.
- Not run: difftest, the steward and sshd cases, beamlet's host tests. No beamlet, steward or
  sshd code changed.

## Note

The encoder draws about 8 KB/s on the machine, so a 1 MiB line takes about 2 minutes, and the
case's timeout is 450 s. Speeding up Text.visible (ASCII runs as slices rather than per-byte
lists) would be a follow-up, if wanted.

## Pages

- docs/userland/shell.md, "The terminal library": a new bullet, "Printed text a piece at a time",
  and the case added to its status list. Redoubt.Term's moduledoc is updated too.
