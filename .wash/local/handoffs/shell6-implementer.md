shell6-implementer handoff (2026-10-08)

State: nothing open. SHELL6 is complete. Files went to main as 62db903c4 (merged 94845840e); 6a (027d6e392), 6b (3f71a8077) and 6c plus its follow-ups (33a11a2ca) went to shell; batch 2 went to main as 012900114. B44 is on main as 8fa39c5ad. No uncommitted work anywhere.

Worktrees I used (all clean, all merged; safe to remove): .worktrees/SHELL6 (wp-SHELL6), .worktrees/SHELL6-files (detached at 624ab4b08, a baseline), .worktrees/SHELL7 (detached at 01841b74e, a baseline), .worktrees/shell-batch2 (detached at 4562b4e5d, pushed as origin/shell), .worktrees/B44 (wp-B44, merged).

Optional follow-up from B44's steward red: image/README.md:17's "1,794 pages over two sessions" assumes a label set gets exactly half of Alice's 47,624 with no page of its own. State the basis or round. Not assigned.

Reports: .wash/local/SHELL6-files-report.md, SHELL6a/6b/6c-report.md, shell-batch2-report.md, B44-report.md.

Traps learned:
- beamlet doesn't append binaries in place: a loop of acc <> piece is quadratic there, though fast on BEAM. Keep {length, role} and cut once with binary_part.
- String.split_at and String.length walk the whole line; on a 1 MiB line that is too slow on beamlet. Use String.next_grapheme(_size) and walk only as far as needed.
- In a test, helpers that spawn and then receive hang until the 60 s timeout if the spawned process crashes; use Task.async/await. Process.info(self(), :binary) isn't supported on beamlet; guard with Redoubt.Term.Buffer.available?() (true on beamlet).
- The BEAM's :erts_debug.size on large shared structures takes minutes; measure total_heap_size after a GC in a spawned process instead (it includes slack).
- Footprint logs: grep target/jobs/<w>-beamlet-footprint.log per width. Combining both files in one grep -h once swapped the widths in my report. The VM's row report is in target/testbench/run-*/beamlet-footprint-<w>-smp1.log (lines starting "footprint "). Rows rounded to the nearest page sum to the report's total pages.
- Never `pgrep -f "jobs.mk set"` in a wait loop: it matches its own command line.
- A bench's prebuilt goes stale after any tree change; run make prebuilt first or every case fails with "built from this tree before it changed".
- Don't grep test logs where assertions dump large binaries (a 1 MiB line went into my context); cut -c or grep -c.
- Burst/deaf rules: a question raised by a burst stays deaf while dropped keys still have keys queued behind them (re-arm), in both Editor and Manager.
- Doccheck forbids **Open:** in built sections; write "Residual:".
