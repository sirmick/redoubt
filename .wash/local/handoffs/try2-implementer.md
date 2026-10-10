try2-implementer handoff (2026-10-09). TRY2 is merged (main 22195522e). B46 is reported, waiting on steward-red's renewal.

## Branch state
- wp-B46 in /home/mcloonan/redoubt/.worktrees/B46: one commit, 3fd9684e4, on main 6b46c8c86 (BEAM19 merged). Clean tree. Not pushed.
- The commit message is kept in /home/mcloonan/redoubt/.tmp/B46/msg. Edit it and run `git commit --amend -F` on it, so the message stays in step with the code. Its numbers are current.
- Report: /home/mcloonan/redoubt/.wash/local/B46-report.md, with sections for the fix round and the BEAM19 rebase.
- All gates rc 0 at 3fd9684e4:
  - ./test-shell, every stage;
  - prebuilt;
  - shell-output-rate, shell-long-output and beamlet-footprint on both widths;
  - formatting;
  - docs.

## Traps
- Never edit the B46 worktree while prebuilt or any case runs ("tree changed"). Commit first, then run.
- Use `q run` for everything. Typed console lines under icount stay under ~580 characters. A later [[input]] must trigger on a printed line, never a prompt (prompts end without a newline).
- beamlet: `:binary.match` is O(bytes x patterns) and copies the haystack on every call; comparing long lists costs per element. Never key a cache on a list compare.
- BEAM driver_test runs with a heap limit and include_shared_binaries. Large `characters_to_list` or 4 KiB windows grew the heap, and the 1 MiB test got killed, which also broke the crash-report test after it. That is why the windows are 128..512 bytes and caret runs build one binary.
- beamlet-footprint rv64 sits at 5,490 pages of the 5,494 the rule allows (4 pages of peak, 9 of cap). Any prompt-time growth in the shell's code moves the session size a 128-page step. The 64-code-point Erlang prefix keeps the banner off the matcher path; don't lower it.
- `pkill -f` with a path in the pattern kills your own shell, and can leave a stale index.lock. Use `pgrep` first.
- The docs checker reads "C0" (or another letter followed by digits) as a package ID (rule C4). Write "ASCII control characters".

## How the floors were set
- shell-output-rate times the encoder alone (Term.slices plus request, no output) in alice's console session, icount shift=3, guest time.
- On BEAM19's main, the lowest of two runs per width:
  - rv32 ASCII 25,727 B/s, Cyrillic 29,627 B/s;
  - rv64 ASCII 28,381 B/s, Cyrillic 31,892 B/s;
  - redraw (least of 3) 56.6 ms on rv64, 65.4 ms on rv32.
- Floors: 12,000 B/s for each text (25,727 / 12,000 = 2.1) and 250 ms for the redraw (3.8).
- Main's encoder, the same case: 599 / 1,207 (rv32), 1.84 s redraw.
- Timeout 50 s: four times the slowest pass alone (~12 s), rounded up.

## What the red still owes
- steward-red's renewal on the delta from c4ffc0d3b to 3fd9684e4: the P1 window-edge tests (text_test, width_test, term_test n of 70, 130 and 600), the P3 post-draw peak, the BEAM19 rebase and the re-measured numbers.
- If it asks for more, the next step is a fix round on wp-B46, amended into the one commit.
- CTX2 carves 128 relay pages from the session; whichever lands second redoes the session arithmetic. On this head: 11,008 pages, heap cap 10,989, prompt peak 5,490 (rv64) and 5,308 (rv32).
- Post-draw peak: 6,073 / 5,851 on this head, 6,536 with main's encoder. That's the workload's, worth a note to whoever owns the session budget.
