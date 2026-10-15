# B69 report (b69-implementer, 2026-10-15)

Node B69: the invisible format characters draw as `<U+XXXX>` in both decoders; U+200D kept only
inside an emoji ZWJ sequence; one shared table. Worktree `/home/mcloonan/redoubt/.worktrees/B69`,
branch `wp-B69`. Tier A (the terminal guard; reviewer steward-red-2, on main).

**Base** main 3b5e5e975 (rebased after review from c8422f745; no conflict). **Head** wp-B69 =
**efdeb40ed** (one commit, tree clean; measured as a081c1d0 on c8422f745, from which it differs by
docs, `forbidden.txt`'s comment, the message and the base). Review folds (steward-red-2, OK with
notes): P2-1 shell.md narrowed to hiding a character in a name, look-alike letters a one-sentence
residual on SECURITY.md; P3-1 forbidden.txt names OTP 28.5; P3-2 shell.md: a tag sequence draws
as its flag followed by its tags. Gates after the rebase: cells 14, beamlet-screen 25+10, fmt
(cargo +nightly, mix) clean, doccheck 0, ./test-shell on text/cells/term/draw/read on both VMs
and the fake kernel: every stage passed (`.tmp/B69/fold.log`).

## The table

`userland/native/cells/forbidden.txt`, beside the vectors, stated once; every decoder states it in
its code and a test holds it to the file.

| range | what |
| --- | --- |
| U+0000–U+001F | C0 controls |
| U+007F–U+009F | DEL, C1 controls |
| U+00AD | soft hyphen |
| U+200B–U+200D | zero-width space, non-joiner, joiner |
| U+2028–U+2029 | line and paragraph separators |
| U+202A–U+202E | bidi embeddings and overrides |
| U+2060–U+2064 | word joiner, invisible operators |
| U+2066–U+2069 | bidi isolates |
| U+FEFF | byte order mark |
| U+E0000–U+E007F | the tags |

214 code points. The joiner's exception, as the Architect's rule: kept only between an element (a
pictograph, alone or followed by U+FE0F or a skin tone modifier U+1F3FB–U+1F3FF: UTS #51's
`emoji_zwj_element`) and a pictograph; `Extended_Pictographic` is the 78 ranges of Unicode 16.0,
taken from the pinned OTP 28.5's `unicode_util.erl` (its `gc_ext_pict` clauses) and listed in the
same file under `pictographic`. Drawn `<U+200D>` anywhere else, Indic conjuncts (क्‍ष) included.
A joiner is judged by the original text before it, not by what was drawn, so the matchers'
windows cannot change the answer; a grapheme cut by a printed piece's 4 KiB edge is drawn as
parts, as it is measured (said on the page).

## Paths

- `userland/native/cells/forbidden.txt` (new): the table.
- `userland/native/cells/src/lib.rs`: `forbidden` gains the ranges; `JOINER`, `joined`, `allowed`
  (each char with whether a symbol may hold it there), `holds_forbidden`, `pictographic` and its
  78-range table; `symbol()` uses `holds_forbidden`. `src/tests.rs`: 17 new bad vectors (one per
  character class and the joiner's wrong places), `no_control_character_is_ever_a_symbol` from the
  file (U+2028 no longer good), `a_joiner_is_a_symbols_only_inside_an_emoji_zwj_sequence`,
  `the_forbidden_characters_and_the_pictographs_are_the_shared_tables` (every code point).
  `vectors.json` regenerated: 34 → 51 cases.
- `userland/otp/screen/src/buffer.rs` (third home, agreed with the orchestrator): `put`/`fill`
  refuse by `cells::holds_forbidden` instead of per-character `forbidden`, so `put("👩‍💻")` still
  works; `src/draw.rs` `visible` draws by `cells::allowed` (a forced-in character not allowed
  where it stands is `<U+XXXX>`, nine columns for a tag); `src/buffer/tests.rs`: new characters
  refused, the emoji sequence put as one symbol, vector count 51. `draw.json` regenerated (the
  shell's draw vectors now force in every character of the table).
- `userland/shell/lib/redoubt/term/text.ex`: `@controls`/`controls/0`, `control?/1` generated from
  them, `pictographic?/1`, `pictographs/0`, `joined?/2`; `scan` carries the piece and keeps a
  joined joiner; `unprintable` draws any non-ASCII control as `<U+XXXX>` (≥4 hex digits). The
  matcher pattern that ends a printable run is now 38 entries (the 33 C0/DEL bytes and the five
  UTF-8 prefixes `C2`, `E2 80`, `E2 81`, `EF BB`, `F3 A0`), a hit settled by `control?/1` in
  `stop/2` and a false one (NBSP, ©, a dash, a quote) stepped over, instead of one entry per
  control (70 before, 88 with the new set): beamlet's `:binary.match`
  (`userland/otp/vm/src/bif/binary.rs`, `find`) tries every pattern at every byte and re-collects
  the pattern list per call, so its cost is linear in the pattern count, and with 88 entries
  shell-output-rate fell 6–10 % (first gates run: ascii 32,121/28,783 B/s, utf8 39,206/37,093,
  pages 8,750/17,824 and 7,841/15,592; floors held). On the BEAM the two patterns cost the same.
- `userland/shell/lib/redoubt/term/cells.ex`: `forbidden?/1` symbol-level with the joiner rule.
- `userland/shell/test/support/terminal.exs`: the model refuses by `Cells.forbidden?`.
- Tests: `term/text_test.exs` (drawn cases per class; joiner kept/drawn cases; table test against
  the file by exact ranges and edges; pictographs against OTP's `unicode_util:gc` at every edge;
  sweep ranges extended to U+FEFF and the tags; window-edge specials gain the new classes and two
  emoji sequences), `term/cells_test.exs`, `term_test.exs` (the two `rm` lines draw apart, typed
  and printed; `👩‍💻` one symbol of two columns), `term/draw_vectors_test.exs`,
  `screen/draw_test.exs` (points filtered by the table, edges too), `screen/read_test.exs` (51).
- Docs: `docs/userland/shell.md` (the guard's set, the joiner rule, the table, the piece-edge
  note; residual gone; two new test names on the cell protocol's status line),
  `docs/SECURITY.md` (residual gone), `docs/userland/beamlet.md` (the natives' set),
  `docs/SUMMARY.md` and `docs/todo/shell-invisible-format.md` (deleted).

No `unsafe`; rv32 untouched (host crates only; the natives build for the machine in prebuilt);
`tests/size-budget.toml` has no entry for `cells` or `beamlet-screen` (checked: not applicable).

## Gates

All through `scripts/q`; logs under `/home/mcloonan/redoubt/.tmp/B69/` (`test-shell4.log`,
`prebuilt4.log`, `cases3.log`–`cases5.log`, `cases3b.log`, `run3/`–`run5/` the testbench logs).

- `./test-shell` (formatting, native: cells 14 + beamlet-screen 25 + 10, gen-width check; on
  beamlet 366 tests 0 failures 1 skipped; on BEAM ok; entry point; fake kernel): **every stage
  passed**, exit 0, on the measured head (a081c1d0) and on the two before it.
- `cargo test -p cells` 14 passed (vectors written with `CELLS_VECTORS=write`, then current);
  `cargo test -p beamlet-screen` 25 + 10 passed against the regenerated `draw.json`.
- `cargo run -q -p redoubt-doccheck`: exit 0 (rerun after the last docs edit: see below).
- `make -j4 -f scripts/jobs.mk set CASES="nscr-hostile-text nscr-beyond-cells" WIDTHS=rv64,rv32`:
  4 PASS (the orchestrator's addition: draw.rs's output and the Elixir decoder's refusals reach
  them).
- Rates, `make -j1 -f scripts/jobs.mk set CASES="shell-output-rate shell-text-profile"
  WIDTHS=rv64,rv32`, three runs alone (run 3 with nothing else on the scheduler; runs 4–5 with
  B71's one-core cargo tests), PASS ×12. shell-output-rate verdict lines (rv64 / rv32):

  | | run 3 | run 4 | run 5 | B64 (its report) |
  | --- | --- | --- | --- | --- |
  | ascii B/s | 44,354 / 37,968 | 44,138 / 38,214 | 43,903 / 38,012 | 34,276–34,735 / 31,185–31,602 |
  | utf8 B/s | 63,915 / 56,954 | 63,922 / 56,879 | 63,873 / 56,879 | 43,727–43,749 / 41,010–41,036 |
  | pages mixed B/s | 10,002 / 8,230 | 9,619 / 8,274 | 9,787 / 8,250 | 9,114 / 7,828 |
  | pages ascii B/s | 22,397 / 18,394 | 22,395 / 18,413 | 22,405 / 18,413 | 19,023 / 16,485 |
  | redraw µs | 104,191 / 114,309 | 106,563 / 100,856 | 104,973 / 123,727 | 108–110 k / 111–124 k |

  shell-text-profile, whole draw in guest µs (rv64 / rv32), B64's after-figures in brackets:
  ascii 256 KiB 6.63–6.78 M / 7.80–7.89 M [8.43 M / 9.26 M]; page 64 KiB 6.20–6.21 M /
  7.32–7.33 M [6.76 M / 7.78 M]; ASCII page 2.93–2.96 M / 3.59–3.61 M [3.54 M / 4.10 M]; mixed
  64 KiB 16.58–16.72 M / 19.33–19.42 M [17.14 M / 19.80 M]; the 1 MiB line 49,709–49,860 /
  42,299–42,461 B/s [36,930 / 33,060]. Every figure at or above B64's;
  nothing below; no floor moved. The gain is the smaller matcher pattern (38 entries against
  70), not this package's purpose; the first gates run with 88 entries is in the paths section.
- Measured head a081c1d0; the final head differs from it by docs (`shell.md`'s rate paragraphs)
  and the commit message only.

## Summaries checked

`README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/plan/m2-usable-shell.md` (line 53: the
natives refuse a control character: still true, broader now), `docs/plan/m3-agents.md:34` (the
steward's own approval-screen rule, not this guard), `docs/userland/shell.md` lines 109, 198, 375,
950, 1033 ("every control character", which the guard section now defines to include the set:
no change), `userland/shell/README.md`, `userland/native/README.md`: no claim about the
invisible characters besides the two residuals removed. Docs checker clean.

## Open

- The screen natives were outside the body's Files list; included by the orchestrator's answer.
- main moved to 3b5e5e975 (train 31) during the work; the branch is on c8422f745 and was not
  rebased (the gates ran on that base). Nothing of B66/B67 touches these paths as far as
  `git log --stat c8422f745..main -- userland/shell/lib/redoubt/term userland/native/cells
  userland/otp/screen docs/userland/shell.md` shows: not checked by me, for the merge.
- Diff review notes for the reviewer: `Cells.forbidden?` rebuilds `before` by concatenation per
  code point (a symbol is ≤ 32 bytes); `Text.visible`'s moduledoc shows the emoji sequence with
  its joiner as a `‍` escape in a heredoc, so the rendered doc holds the sequence itself.

## Follow-ons (plan candidates)

- **beamlet's `:binary.match` costs the pattern count per byte.** `find` in
  `userland/otp/vm/src/bif/binary.rs` is `(from..until).find_map(|i| pats.iter().filter(..))`:
  at every byte of the haystack every pattern is compared, and `patterns()` re-collects the
  pattern binaries into fresh `Vec`s on each call, `compile_pattern/1` being the identity. So a
  multi-pattern match is O(len × patterns): the shell's control pattern at 70 entries (B64) cost
  the printed-text path what it did; at 88 entries shell-output-rate fell 6–10 % (ascii
  34,500 → 32,121 B/s rv64, 31,400 → 28,783 rv32; utf8 43,700 → 39,206 and 41,000 → 37,093; the
  pages 9,114/19,023 → 8,750/17,824 rv64), the profile's 256 KiB ASCII whole 8.43 → 8.96 s rv64
  and 9.26 → 10.28 s rv32, while the grapheme-bound mixed text did not move. On the BEAM the two
  patterns cost the same (0.6–0.9 µs a call on 128–512 bytes). A first-byte table in `find` (a
  256-bit set of the bytes any pattern starts with, skipping every byte not in it; or a compiled
  Aho–Corasick kept in the `{bm, _}` tuple) would make the common case O(len) whatever the
  pattern count, and lift the whole printed-text path, B64's figures included. This package
  works around it by cutting the pattern to 38 entries with prefixes settled in Erlang.
- `Cells.forbidden?` rebuilds `before` by concatenation per code point (a symbol is ≤ 32 bytes).
