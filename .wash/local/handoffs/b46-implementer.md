# Handoff: b48-implementer -> successor, 2026-10-15 (checkpoint at 70 % context)

## Branch state (first)

- **wp-B60** in /home/mcloonan/redoubt/.worktrees/B60, from main 146db8897 (NOT rebased: main is
  now 1dd0acb27 after train 27 with B61 and B51, which touch io.rs/files.rs beside B60; rebase at
  the end). One WIP commit abbc18289: `tests/shell-text-profile.toml`, the measurement case as run
  beside train 27's bench (second attempt). Not for review; the package's commit replaces/drops it.
  A **third draft** with 4 KiB grapheme-path texts is at `.tmp/B60/shell-text-profile.toml`
  (not yet copied in, not run): copy it over tests/, `make -f scripts/jobs.mk prebuilt` (tree
  changed), then `make -k -f scripts/jobs.mk rv64/shell-text-profile` and rv32, on the quiet
  machine (the orchestrator said it is quiet now, message 7d9e195d).
- A run may still be in flight: `rv32/shell-text-profile` under tenant B60 (q ls), at ~2,200 of
  2,400 s at the checkpoint; it times out in the width stage. Its console log is under
  target/testbench/run-247621-*/; copy it to `.tmp/B60/logs/` when done (partly copied).
- wp-B48, wp-B52, wp-B61: merged (trains 25, 26, 27); worktrees B48 removed, B52 and B61 still
  exist at /home/mcloonan/redoubt/.worktrees/{B52,B61} (nothing owed; may be removed). B62 (open)
  carries the hub-wait gap. `.worktrees/B46` is a placeholder dir holding my shell's cwd; delete.
- Report: `.wash/local/B60-report.md` (Plan, Phase 1 so far, the reading of the profile).

## Traps (second)

- `./test-shell` skips its beamlet-redoubt (fake-kernel) stage when the binaries are byte-identical
  to the last pass: `rm userland/otp/target/test-shell-fake-kernel` before each run of a loop
  (B52's third commit documents it in the script header).
- `plan_get` prints the whole tree (huge): use `grep -n -A8 'id = "Bnn"' .wash/plan.toml`.
- A `progress` message is delivered only when you next set waiting; answers/questions wake at once.
- Env exports from `.wash/local/launch-env-2026-10-12.md` in every shell; everything through
  `scripts/q run --cores N`; scratch under `$REDOUBT_TMP/B60`; no edits while prebuilt/cases run
  from the worktree; an untracked file inside the worktree counts as a tree change for prebuilt.
- The docs checker's C4 refuses package IDs (B61 etc.) in pages: write "open" and name the node
  in the report.
- The Edit tool needs a Read of the file in that worktree first (files written by scripts count as
  unread).
- icount cases (shift=3) measure guest time: load beside them stretches wall time only (a 900 s
  deadline timed out under ten jobs); the orchestrator still wants runs beside a bench marked so
  and repeated alone.
- `Fake::last_launched` is global to a test binary: use `Fake::served_child(pid)` (B48).

## B60: what is done

Plan written (report 'Plan'). Phase 1 measurement harness written and run twice beside train 27's
bench (first attempt: timed out at 900 s with nothing printed, least-of-three and a 262k-element
grapheme list; second: one run per stage, printed as it completes):

| beside the bench, ASCII z x 256 KiB | rv64 | rv32 |
| --- | ---: | ---: |
| whole draw (Term.request over slices) | 7.81 s (33.6 KB/s) | 8.65 s (30.3 KB/s) |
| Text.visible alone | 5.35 s | 5.71 s |
| String.next_grapheme walk alone | 13,172 s | 13,605 s |

Logs: `.tmp/B60/logs/` (run-162656-*-rv64, run-247621-*-rv32; job-rv64.log, job-rv32.log;
beside: `.tmp/B60/beside2-*.txt` = ten bench jobs). **Headline:** the per-grapheme step costs ~50 ms
of guest time on the machine (vs ~30 us a byte on the run path), so any text leaving the run path
(CJK, emoji, mark-broken runs) is unusable at size; Text.visible is 2/3 of the ASCII draw. The
width stage, mixed/page texts and the 1 MiB line did not get measured yet (timeouts).

## What is next

1. Run the third draft alone (both widths), get: ascii4k/mixed4k/page4k per-stage, mixed and
   page whole-path at 64 KiB, the 1 MiB line (B/s). Report both sets side by side (the
   orchestrator asked), <=2000 bytes, detail in the report.
2. Phase 2 by the numbers: likely find why `:unicode_util.gc/1` (String.next_grapheme) costs 50 ms
   a step on beamlet (compare 4 KiB vs 256 KiB inputs: an O(n) per-call cost on the remaining
   binary would show as a 64x ratio). Then a run path for wide/combining text with a floor in
   shell-output-rate, or a bounded-window next_grapheme, per what the ratio says. Pages: shell.md
   "The terminal library"/"The rate". Tier A (escaping path): steward-red-2.
3. Rebase wp-B60 onto main (1dd0acb27 or later) before the final gates; drop or keep the harness
   case deliberately; remove the WIP commit's wording.
