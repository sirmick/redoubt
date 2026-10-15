# B64 report (b64-implementer, 2026-10-15)

Node B64: the shell's text run path starts at a piece's first byte. Worktree
`/home/mcloonan/redoubt/.worktrees/B64`, branch `wp-B64`. Tier A (escaping path; reviewer
steward-red-2).

**Base** main 47b0d98ea. **Head** wp-B64 = **87ba3378e** (amended from 27ae944a8 after review), one commit, tree clean.

## The diff's paths

- `userland/shell/lib/redoubt/term/text.ex`: `printable/1` calls `matched/4` at once; the
  `@steps` prologue (64 code points one at a time in Erlang) and its three clauses are gone.
- `userland/shell/lib/redoubt/term/width.ex`: `run/1` calls `matched/4` at once; the same
  prologue gone. The matchers, their windows (128, then ×4 up to 512) and classes are unchanged.
- `userland/shell/test/redoubt/term/text_test.exs`, `width_test.exs`: the window-edge tests take
  the text's start as an edge too (`edges/0`: 0, 128, 640, 1152 bytes, five each side; no
  negative offsets); their reference is the matchers over one unit at a time (fills no window),
  stated in the comments; test names no longer say "past the first 64".
- `tests/shell-output-rate.toml`: a third input draws 64 KiB of shell-text-profile's page
  (82-byte lines, one CJK word, one emoji, one accented name) and the same page in ASCII, floors
  6,500 and 12,000 B/s, verdict line `pages: mixed N B/s, ascii N B/s: at the floors` (expect;
  `below the floors` already forbidden); the redraw input follows it; timeout 50 → 60 s (the
  rv32 pass is ~20 s of guest time); description updated.
- `docs/userland/shell.md` "The terminal library": "Printed text in runs" (the matchers from
  the first byte; a step ≈ two bytes of matcher; a one-code-point run costs a call), "The rate"
  (the numbers; the two page floors and why they stand close), "The profile" (the page's and
  the ASCII page's rates, what the lone graphemes cost, what remains of a line's cost as a
  piece, the open question restated as carrying runs past the newline).

Not changed: `term.ex` (`advance_text`'s first-byte guard still decides when `Width.run` is
asked), the matcher classes, escaping or width semantics. No native crate changed
(`tests/size-budget.toml` not applicable); nothing unsafe; rv32 untouched.

## Why no prologue at all (rather than a shorter one)

Measured: removing the 64 steps from `Text.visible` saves 1.41 ms per 82-byte line while
adding 64 bytes of matcher work, so a step costs ~22 µs more than a matcher byte (~20 µs): a
step ≈ two bytes, not "hundreds". The only text a prologue helps is runs of one or two code
points (hostile text with a mark or wide character every few bytes), which the matchers now
cost 4–5 % more: 3,800/3,300 B/s instead of 4,100/3,460. The page says so. A prologue of a few
steps would win that back at the price of two code paths to keep equal; not worth it.

## Numbers (guest µs; rv64 / rv32; AFTER is the mean of three runs, which agree within 1 %)

shell-text-profile:

| text | before rv64 | after rv64 | before rv32 | after rv32 |
| --- | ---: | ---: | ---: | ---: |
| page 64 KiB whole (B/s) | 9.06 s (7,234) | 6.76 s (9,690) | 10.21 s (6,419) | 7.78 s (8,425) |
| ASCII page 64 KiB whole (B/s) | 6.30 s (10,403) | 3.54 s (18,500) | 6.97 s (9,401) | 4.10 s (16,000) |
| page4k visible | 184.7 ms | 115.8 ms | 190.0 ms | 116.7 ms |
| ASCII page4k visible | 184.5 ms | 115.7 ms | 193.2 ms | 116.9 ms |
| page4k whole | 511.5 ms | 393.6 ms | 583.4 ms | 435.8 ms |
| ASCII page4k whole | 358.5 ms | 209.2 ms | 387.1 ms | 224.7 ms |
| ascii 256 KiB whole (B/s) | 8.46 s (30,977) | 8.43 s (31,100) | 9.47 s (27,684) | 9.26 s (28,300) |
| ascii 256 KiB visible | 5.15 s | 5.17 s | 5.45 s | 5.47 s |
| mixed4k visible | 103.6 ms | 74.0 ms | 107.3 ms | 80.4 ms |
| mixed4k whole | 1.005 s | 1.034 s | 1.185 s | 1.188 s |
| mixed 64 KiB whole (B/s) | 16.32 s (4,015) | 17.14 s (3,823) | 18.95 s (3,458) | 19.80 s (3,310) |
| 1 MiB line (B/s) | 35,740 | 36,930 | 32,128 | 33,060 |

Grapheme and width walks (`String.graphemes`, `Width.columns`) unchanged, as expected.

shell-output-rate (the case's own verdict lines):

| | before rv64 | after rv64 (×3) | before rv32 | after rv32 (×3) |
| --- | ---: | ---: | ---: | ---: |
| ascii B/s | 33,271 | 34,276–34,735 | 30,089 | 31,185–31,602 |
| utf8 (Cyrillic) B/s | 37,717 | 43,727–43,749 | 35,112 | 41,010–41,036 |
| redraw µs (least of three) | 91,321 | 108,457–110,324 | 131,799 | 110,719–123,907 |
| pages mixed / ascii B/s (new) | — | 9,114 / 19,023 | — | 7,828 / 16,485 |

The pages draw slower inside shell-output-rate than in the profile (7,828 vs 8,425 on rv32:
the case's state, `d`'s own reduce), so the page floor is 6,500 (margin 1.2; the old encoder
would draw about 5,960 here and fail it) and the ASCII page's 12,000 (1.37). The redraw figure
(107–124 ms) is what the machine gives today on both the before and after trees; the page said
57/65 ms from B46's run and now says about 110.

Per line of the ASCII page after: 4.4 ms, against 2.6 ms for 82 bytes of a long line; the
1.8 ms left is the piece's: a call of each matcher in `Text.visible` and again in `Width.run`,
the newline's caret step and the piece cut, the row's bookkeeping. The page vs the ASCII page:
4.0 ms a line for the five lone graphemes and two run restarts (B60 estimated 3.5).

Logs: `$REDOUBT_TMP/B64/logs/before-*.log`, `after{1,2,3}-*.log`, `final-*.log`,
`final2-*.log` (console logs); `before-cases.log`, `after-cases-*.log`, `final*-cases.log`
(jobs.mk summaries).

## Gates

- `./test-shell test/redoubt/term/text_test.exs test/redoubt/term/width_test.exs` (q, 4
  cores) at c309276a8 (the code as committed): every stage ok, 20 tests, 0 failures on beamlet
  and BEAM (`focused-tests.log`). Again at the final bytes (text.ex's comment reworded after):
  formatting ok, native ok, 20/0 both VMs, entry_point, terminal, on_fake_kernel ok
  (`focused-tests-final.log`).
- `./test-shell` whole (q, 8 cores) at c309276a8: every stage ok; beamlet 362 tests, 0
  failures, 1 skipped; rc 0 (`test-shell.log`). The commit was amended after only in a
  comment, the tests' prose, the toml and the page.
- `make -f scripts/jobs.mk prebuilt`: rc 0 three times (before; after; final).
- `make -j4 -f scripts/jobs.mk set CASES="shell-text-profile shell-output-rate"`: before rc 0;
  after ×3 rc 0 (all PASS); `set CASES=shell-output-rate` at 4ead7f1f0 (floor 7,000) rc 0,
  PASS both widths (`final-cases.log`).
- `make -f scripts/jobs.mk docs` at the final tree: PASS, rc 0 (`docs-final.log`).
- shell-output-rate at 27ae944a8 (floor 6,500), after its own prebuilt (rc 0): PASS rv64
  11.1 s, PASS rv32 10.2 s, rc 0 (`final2-cases.log`); rv64 `rates: ascii 34187, utf8 43727`,
  `pages: mixed 8981, ascii 18997`, redraw 95,560 µs; rv32 `rates: ascii 31564, utf8 41006`,
  `pages: mixed 7749, ascii 16483`, redraw 108,286 µs: all at the floors and within the bound.
- `userland/otp/tools/difftest` (q, 8 cores) at 27ae944a8: 527/527 passed, 0 failed, 21
  skipped by design, rc 0 (`difftest.log`); a first attempt did not run (rc 127, a wrong
  relative path to q).
- Not run: the whole bench, the rv32/rv64 builds beyond prebuilt (no Rust changed), the
  unsafe count (unchanged by construction).

## Review round (steward-red-2: OK with notes), folded by amend into 87ba3378e

- P3-1: shell.md's profile bullet no longer quotes the old rates or the prologue; it says what
  is.
- Sweeps: `text_test.exs` takes a surrogate (ED A0 80), two overlongs (C0 80, E0 80 80) and
  ZWJ among its specials (so each also stands at the first byte), and a multibyte sequence cut
  short at the text's very end; `width_test.exs` takes ZWJ. All pass unchanged on both VMs.
- Found on the way, unreachable here, for whoever next touches `Width.run`: the three invalid
  encodings at the first byte of `Width.run`'s input raise `ArgumentError` from `:re.run`
  (`width.ex:38`) on the **BEAM**, which refuses invalid UTF-8 under a unicode-compiled
  pattern, and pass on **beamlet**, whose `:re` does not raise. `Width.run` takes text made
  visible (no such byte is left in it) and `term.ex:419` asks it only for a lead byte in
  0x20..0x7E or 0xC2..0xD4, so neither VM meets the case; the width sweep states the contract
  in a comment and holds no invalid byte. Before this package the same bytes at offset 0 fell
  through the Erlang clauses to `{0, 0}` and would have raised only past byte 64.
- Gates at 87ba3378e (q): `./test-shell test/redoubt/term/text_test.exs
  test/redoubt/term/width_test.exs`: formatting ok, native ok, 20 tests / 0 failures on beamlet
  and BEAM, entry_point, terminal, on_fake_kernel ok, rc 0 (`focused-tests-r2.log`; `-r1.log`
  is the run with the three width cases, BEAM failing as described); `make -f scripts/jobs.mk
  docs` PASS rc 0 (`docs-r2.log`). No code or case changed in the round, so no machine case
  was rerun (the orchestrator's word).

## Affected summaries checked

- `docs/userland/shell.md` "Printed text in runs", "The rate", "The profile": updated (above).
  Its status line already names `bench:shell-output-rate` and `bench:shell-text-profile`.
- `docs/userland/shell.md` elsewhere, `docs/userland/beamlet.md`, `docs/plan/m2-usable-shell.md`,
  `docs/README.md`, `README.md`, `GETTING-STARTED.md`: grep for "64 code points", "Erlang
  prefix", "prologue", "first 64", "one at a time in Erlang", and the moved numbers (7,200,
  6,400, 10,400, 9,300, 4,100, 31,900, 29,600): only shell.md's three bullets carried them; no
  change elsewhere. (`util.ex:265` "its first 64" is hexdump's help, unrelated.)
- `tests/shell-text-profile.toml`: its description and comments describe the stages, not the
  prologue; unchanged.
- `tests/size-budget.toml`: no native crate changed; not applicable.

## Scratch

`$REDOUBT_TMP/B64/` (env.sh, logs); remove when the package closes.
