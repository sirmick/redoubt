# steward-red review, filed by the orchestrator from the reviewer's transcript (its Wash calls were refused)

Merge verdict: OK with notes

Base main db6575a3f; head bf01f9828 (wp-NSCR2). Read in .worktrees/NSCR2: userland/native/cells/src/lib.rs; userland/otp/screen/src/{lib,buffer,draw}.rs, buffer/tests.rs, tests/natives.rs, tests/src/screen_probe.erl; shell lib/redoubt/screen/native.ex, screen.ex, shell/driver.ex, term/{buffer,frame,cells,text,width}.ex; shell test screen/{draw,read}_test.exs, term/draw_vectors_test.exs; tests/nscr-frame-rate.toml, programs screen-stage.rs; docs/userland/{shell,beamlet}.md, docs/plan/m2-usable-shell.md; tools/testbench/src/qemu.rs (shown/forbidden); .wash/local/NSCR2-report.md.

Verified: header checks in the Elixir order (version, flags, size, count, truncated), then the buffer's size compare before any clear or put, as reference/3; cell checks in order, seen bitmap sized from the checked header, left/at guarded, no reachable panic (put's expect holds). Joined symbol returned unput with its position recorded; resume only at the recorded at, else badarg; a refusal leaves nothing drawable. Draw writes only CSI 0m/2J/H and numeric SGR; visible/1 parity (tabs, ^X, ^?, <U+..>); held by draw_test (every point <U+3000, edges, hostile, styles, partial frames) and draw.json with forced controls; non-owner draw writes no cell; the driver draws only the registered buffer of the pid in front, synchronously, so no torn frame; frame-rate forbids judged on the escape-stripped line too. Heap: bytes() counts both grids + bitmap, charged at new/resize.

P2-1 The report ends at commit 1: no gates, numbers or summaries recorded for 9430cfbe5 or head bf01f9828 (nscr-frame-rate both widths, ./test-shell incl. draw tests, beamlet-footprint at head, docs). Content fit to merge; acceptance needs the head's gate evidence recorded.
P3-1 draw's cursor after a multi-grapheme symbol from a trusting put follows columns(); Frame.draw sums graphemes: misplacement only, documented; fine.
P3-2 draw's transient Vec (~12 MB worst case) is outside the heap count, as diff's encode was; no regression.
