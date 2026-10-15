# B60 report (b48-implementer, then b60-implementer, 2026-10-15)

Worktree /home/mcloonan/redoubt/.worktrees/B60, branch wp-B60 from main 146db8897. Tier A.

## Plan

**The paths today** (userland/shell/lib/redoubt/term.ex, term/text.ex, term/width.ex). A print is
cut into pieces at newlines and `@piece` bytes; each piece goes through three stages:
1. *visible*: `Text.visible/2` keeps printable UTF-8 (anything >= U+00A0 that is not a control,
   CJK, marks and emoji included) as runs, 64 code points in Erlang then the VM's matchers a
   window at a time; so B46's fast path already covers *visibility* for every text here.
2. *advance* (wrap and the cursor): `advance_text/3` takes a run of one-column code points
   (ASCII, Latin, Greek, Cyrillic below U+0540) at once through `Width.run/1`; any other lead
   byte, and a run's tail, goes to `advance_grapheme/3`: `String.next_grapheme/1` and
   `Width.grapheme/1` per grapheme, the latter `:unicode_util.is_wide/1` for anything >= U+1100
   and for every multi-code-point grapheme. This is the per-grapheme path B46 left: CJK, emoji,
   and any run broken by combining marks (U+0300.. have lead bytes 0xCC/0xCD, inside the run's
   lead range, so `Width.run` is asked and answers "none").
3. *encode*: the row and column bookkeeping and the iodata `[out, visible]` the request returns.

**Measurement**, on the machine, both widths, `icount shift=3`, guest µs by
`System.monotonic_time`, at alice's console prompt as `shell-output-rate` does
(`tests/shell-text-profile.toml`). For each text T: whole = `Term.request` over `Term.slices` of
a `put_chars`; visible = `Text.visible(T)` alone; graphemes = a grapheme walk alone; width = a
walk measuring each grapheme (graphemes + `grapheme/1`), so width's own cost is width −
graphemes; encode = whole − visible − width. Texts: (a) ASCII 256 KiB (B46's baseline, `z`);
(b) hostile mixed: `世界` + `🙂` + `é` as e + U+0301, repeated (13 bytes, 4 graphemes a copy);
(c) a realistic page: 82-byte lines of English with one CJK word, one emoji and one accented name
each; (d) the same page in ASCII alone (the control for (c)); and B45's line, `z` × 1 MiB through
the whole path, in B/s.

## Phase 1: the profile

Three runs. **Beside train 27's bench** (run 2, `.tmp/B60/logs/run-162656-*-rv64-smp1.log`,
`run-247621-*-rv32-smp1.log`): the walks written at the prompt over the full texts; three stages
printed, then the 2,400 s wall deadline in the fourth. **Alone** (run 3,
`.tmp/B60/logs/alone-run-466490-*` rv64, `alone-run-466491-*` rv32; `job3-*.log`: PASS 70.2 s
rv64, 66.4 s rv32 of wall time): the same walks over 4 and 16 KiB, every whole-path text whole.
**Alone, compiled walks** (run 4, below): the walks as `String.graphemes` and `Width.columns`,
and the ASCII page control.

### Beside the bench and alone, side by side (guest time)

| ASCII `z` × 256 KiB | rv64 beside | rv64 alone | rv32 beside | rv32 alone |
| --- | ---: | ---: | ---: | ---: |
| whole draw | 7.81 s (33.6 KB/s) | 8.32 s (31.5 KB/s) | 8.65 s (30.3 KB/s) | 9.21 s (28.5 KB/s) |
| `Text.visible` alone | 5.35 s | 5.23 s | 5.71 s | 5.55 s |
| prompt-written walk, per step, 256 KiB | 50.2 ms | — | 51.9 ms | — |
| prompt-written walk, per step, 16 KiB | — | 6.54 ms | — | 7.22 ms |
| prompt-written walk, per step, 4 KiB | — | 4.32 ms | — | 4.94 ms |

Beside and alone agree within 7 % on the whole draw and 3 % on visible: an icount figure is
guest time, and the bench beside it stretches wall time only.

### The prompt-written walk measured the evaluator, not the encoder

The walk's step costs 4.3 ms at 4 KiB, 6.5 ms at 16 KiB and 50 ms at 256 KiB (rv64): a constant
of about 3.6 ms plus 0.18 µs per byte of the binary still to walk (the fit predicts 51 ms at
256 KiB). Neither part is the encoder's. A `fn` typed at the prompt runs in the evaluator
(`userland/shell/lib/redoubt/shell/evaluator.ex:117`, `Code.eval_quoted_with_env`: `erl_eval`'s
interpreted funs), and the whole draw of the hostile text, which walks every grapheme through the
compiled `advance_grapheme`, costs 0.72 ms a grapheme *with* visible and encode (below). The
compiled step under beamlet on the host (`.tmp/B60/host/b60.erl`, `b60w.erl`, the pcre2 release
build): `unicode_util:gc/1` 0.37 µs a step over ASCII, 0.70 µs over the mixed text, and
`is_wide/1` adds 0.13 µs; linear in the text (1, 4, 16, 64 KiB: 0.36, 0.33, 0.33 µs a step
after the load); the BEAM 0.011–0.025 µs. So the handoff's headline ("~50 ms a grapheme,
unusable at size") was the measurement's artefact: the second run's 13,172 s was 262,144
interpreted calls over a shrinking 256 KiB binary. Not pursued (the encoder is not on that
path): where the evaluator's per-call cost grows with a binary among the bindings.

### Alone, every stage (run 3; guest ms; walks prompt-written, so graphemes/width stand only as upper bounds)

| text | bytes | graphemes | rv64 whole | rv64 visible | rv32 whole | rv32 visible |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| ascii4k | 4,096 | 4,096 | 129.7 (31.6 KB/s) | 84.9 | 144.8 (28.3 KB/s) | 90.2 |
| mixed4k | 4,836 | 1,488 | 1,199 (4.0 KB/s) | 122.7 | 1,386 (3.5 KB/s) | 127.4 |
| page4k (49 lines) | 4,018 | 3,626 | 534 (7.5 KB/s) | 183.7 | 565 (7.1 KB/s) | 214.2 |
| mixed 64 KiB | 77,441 | 23,828 | 18,860 (4.1 KB/s) | — | 21,801 (3.6 KB/s) | — |
| page 64 KiB (799 lines) | 65,518 | — | 9,183 (7.1 KB/s) | — | 10,358 (6.3 KB/s) | — |
| `z` × 1 MiB, one line | 1,048,576 | — | 29,771 (35.2 KB/s) | — | 33,052 (31.7 KB/s) | — |

The whole path is linear in the text: mixed 64 KiB is 15.7 × mixed4k for 16.0 × the bytes, page
64 KiB 17.2 × page4k for 16.3 ×; the 1 MiB line draws at the 256 KiB rate (it is drawn 64 KiB a
slice, 4 KiB a piece). B45's line: 29.8 s of guest time on rv64, 33.1 s on rv32, through the
encoder alone.

### Alone, compiled walks and the ASCII page control (run 4; `.tmp/B60/logs/run4-run-516292-*` rv64, `run4-run-516293-*` rv32; `job4-*.log`: PASS 26.4 s rv64, 25.7 s rv32 of wall time, a train-20 bench beside)

Guest ms. graphemes = `String.graphemes(s)` (the `:unicode_util.gc/1` walk, compiled); width =
`Width.columns(s)` (the walk and `Width.grapheme/1` on each).

| text | bytes | graphemes | width | rv64 whole | visible | graphemes | width | rv32 whole | visible | graphemes | width |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| ascii, 256 KiB | 262,144 | | | 8,514 (30.8 KB/s) | 5,177 | — | — | 9,415 (27.8 KB/s) | 5,472 | — | — |
| ascii4k | 4,096 | 4,096 | | 119.7 (34.2 KB/s) | 79.9 | 575.8 | 816.1 | 130.5 | 83.3 | 631.7 | 863.7 |
| mixed4k | 4,095 | 1,260 | | 1,004 (4.1 KB/s) | 103.4 | 531.6 | 806.4 | 1,159 (3.5 KB/s) | 107.5 | 650.0 | 919.4 |
| page4k, 49 lines | 4,018 | 3,626 | | 533.7 (7.5 KB/s) | 182.4 | 512.6 | 762.8 | 585.4 (6.9 KB/s) | 191.9 | 573.1 | 824.9 |
| apage4k, 49 lines, ASCII | 4,018 | 4,018 | | 360.4 (11.1 KB/s) | 184.5 | 555.9 | 765.4 | 390.4 (10.3 KB/s) | 193.2 | 614.9 | 812.0 |
| mixed, 64 KiB | 65,533 | 20,164 | | 15,849 (4.1 KB/s) | | | | 18,360 (3.6 KB/s) | | | |
| page, 64 KiB, 799 lines | 65,518 | | | 9,097 (7.2 KB/s) | | | | 10,244 (6.4 KB/s) | | | |
| apage, 64 KiB, 799 lines | 65,518 | | | 6,296 (10.4 KB/s) | | | | 7,008 (9.3 KB/s) | | | |
| `z` × 1 MiB, one line | 1,048,576 | | | 29,875 (35.1 KB/s) | | | | 33,219 (31.6 KB/s) | | | |

Per step, rv64 (rv32): a compiled grapheme walk costs 141 (154) µs a grapheme over ASCII and 422
(516) over the mixed text; `Width.grapheme/1` adds 59 (57) µs for a one-byte grapheme (the
`<<c::utf8>> when c < 0x1100` clause, plus `Enum.map`/`sum`) and 218 (214) µs for a CJK, emoji
or two-code-point one (`:unicode.characters_to_list` and `:unicode_util.is_wide/1`). Runs 3 and
4 agree on every whole-path figure within 8 % (ascii 8.32 vs 8.51 s; page 64 KiB 9.18 vs
9.10 s; mixed 64 KiB 18.86 s over 23,828 graphemes vs 15.85 s over 20,164: 0.79 ms a grapheme in
both, run 3's text being `div(65_536, 11)` copies of a 13-byte unit, 77,441 bytes).

### What dominates each text (rv64; rv32 the same shape, 8–15 % slower)

- **A long ASCII line** (and Cyrillic: the run path): `Text.visible` is 61 % of the draw (5.18 of
  8.51 s), the run path's wrap and encode 39 %; 30 µs a byte in all. The 1 MiB line draws at the
  256 KiB rate (35.1 KB/s, 29.9 s; rv32 31.6 KB/s, 33.2 s): it is cut 64 KiB a slice and 4 KiB a
  piece, and nothing in the path is superlinear.
- **Hostile mixed text** (CJK, emoji, a combining mark; every grapheme off the run path): 0.80 ms
  a grapheme (0.92 on rv32), of which the walk is 0.42, the width 0.22 (is_wide and the list it
  needs), visible 0.08 and the step and encode 0.08. The per-grapheme path is 80 % of the draw;
  4.1 KB/s, a third of `shell-output-rate`'s 12,000 B/s floor (which holds for the run path's
  texts only). Bounded: 64 KiB of it draws in 15.8 s (18.4 on rv32).
- **A realistic page** (82-byte lines, one CJK word, one emoji, one accented name each): 7.2 KB/s
  (6.4), 11.4 ms a line, against 7.9 ms for the same page in ASCII alone (10.4 KB/s; 9.3). The
  difference, 3.5 ms a line, is the line's five graphemes taken alone (the space before 世, 世,
  界, the space before 🙂, 🙂: three of them through `is_wide`, about 2.3 ms by the per-grapheme
  figures) and the two extra starts of `Width.run` the splits cost. The other 7.9 ms is the
  piece's: every line is a piece, and `Text.visible` and `Width.run` each take a piece's first
  64 code points one at a time in Erlang before the matchers (shell.md, "Printed text in runs").
  Measured, not inferred: the ASCII page's `Text.visible` costs 3.8 ms a line (184.5 ms / 49)
  where the long line's costs 1.6 ms for 82 bytes (79.9 ms / 4096 × 82); the page's visible is
  the same whether or not the line holds CJK (182.4 vs 184.5 ms), so none of the page's 3.5 ms
  difference is visible's.

### The answer to the node's question

The wide/combining path does not dominate a realistic text: it is under a third of such a page's
draw, and a run path for CJK and emoji (a matcher class for the wide ranges, marks taken with the
code point before them) would gain the page a fifth at most (3.5 of 11.4 ms, if it removed the
whole difference, which it would not: the spaces before 世 and 🙂 would still end their runs).
It dominates hostile text, which is bounded and slow (4.1 KB/s), as the page already says for
"text of short runs". **Recommendation: record the numbers and close with the floor as it is**
(done in the commit); the lever for ordinary text is the per-piece prologue, paid twice a line,
which is not B60's path and is left as an Open item on the page for the owner's polish list
(a page of ordinary lines at a third of a long line's rate; a 64 KiB page in 9.1 s of guest
time). Suggested node: shorten the prologue when the piece is short (the matchers from the first
byte for a piece under, say, 256 bytes), or carry a line's runs past its newline; gate it on
`shell-text-profile`'s page figures with a floor for the ASCII page in `shell-output-rate`.

Observation, not pursued (not the encoder's path): a fun written at the prompt costs about
3.6 ms a call plus 0.18 µs per byte of a binary among its arguments (rv64), so a loop typed at
the prompt over a large binary is quadratic; where in the evaluator (`evaluator.ex:117`,
`Code.eval_quoted_with_env`, erl_eval under beamlet) the size-dependent part is, I did not find.

### The case's place

`tests/shell-text-profile.toml` stays, with `whole_run = false` (as `worst-walk` and
`sched-cluster-old-control`): run by name, about 70 s a width alone and 26 s when the pieces are
prebuilt, never in a merge's run; its figures are read from the console log and nothing is judged
(the `expect` lines only require every stage to print, the `forbid` lines a clean VM). It is
named in the terminal library's status line as `bench:shell-text-profile`, as scheduling.md names
`worst-walk`, so the docs checker holds the page to a case that exists. A quiet-class bench case
was the other option; this case asserts nothing, so a place in every bench would cost 2 × 26 s a
train for no verdict.

## Reading

- `.wash/SWARM.md`: The implementer, Two tiers, The pages move with the code, Staging, commits
  and handoffs; `.wash/local/launch-env-2026-10-12.md`; `CONTRIBUTING.md` "Commits".
- `userland/shell/lib/redoubt/term.ex` (advance_text/advance_grapheme, 400–470),
  `term/width.ex` (whole), `term/text.ex` (as B46 left it); `shell/evaluator.ex:117`.
- beamlet: `vm/src/interp.rs` (bs_start_match, bs_get_tail, bs_match), `vm/src/term/mod.rs`
  (Bits, slice, bits, push_offheap, match_state), `term/gc.rs`, `process.rs` (maybe_collect):
  a sub-binary is an offset and a length over shared bytes, O(1); nothing in the VM's binary
  path is O(n) per step. OTP 28.5 `unicode_util.erl` gc/1 (the ASCII fast path returns
  `[CP|Rest]` with `Rest` the sub-binary); Elixir 1.20.4 `String.next_grapheme`,
  `String.graphemes` (both `:unicode_util.gc/1`).
- `docs/userland/shell.md` "The terminal library"; `docs/userland/beamlet.md` 212–222, 668–682;
  `docs/testbench.md` "The case file" (`whole_run`), the docs checker; `tests/worst-walk.toml`,
  `tests/shell-output-rate.toml`, `tests/boot-profile.toml`.
- Host tools (not committed, under `.tmp/B60/host/`): `b60.erl`, `b60w.erl`, run on the BEAM and
  on `userland/otp/target/pcre2/release/beamlet` (`tools/build-beamlet`).

## Gates

Branch wp-B60, one commit **d0f65700b** on main **1dd0acb27** (rebased from 146db8897; no
conflicts): `tests/shell-text-profile.toml` (new, `whole_run = false`) and
`docs/userland/shell.md` (the status line names the case; "The rate" updated; a "The profile"
bullet). No code changed; nothing unsafe; rv32 untouched.

On the rebased tree (f2cfcec33, the same commit before a page-wording fix: the docs checker's C1
refuses `**Open:**` in a built section, so the open question is prose in the profile bullet;
d0f65700b differs from f2cfcec33 in shell.md's prose only):
- `make -f scripts/jobs.mk prebuilt`: rc 0 (298 rv64 / 284 rv32 cases built, 170 s / 177 s).
- `make -k -f scripts/jobs.mk rv64/shell-text-profile rv32/shell-text-profile
  rv64/shell-output-rate rv32/shell-output-rate`: rc 0; PASS shell-output-rate rv64 8.7 s, rv32
  8.8 s (at the floors, within the bound); PASS shell-text-profile rv64 28.9 s, rv32 26.8 s (a
  train-20 bench beside). Logs `.tmp/B60/logs/gate-*.log`, `.tmp/B60/gate-cases.log`. The
  post-rebase figures match run 4 within 3 % (rv64: ascii 8.75 s, mixed4k 1.006 s, mixed 64 KiB
  15.92 s, page 9.10 s, apage 6.31 s, 1 MiB 29.9 s; rv32: 9.55, 1.160, 18.36, 10.25, 7.01, 33.3).
- `make -f scripts/jobs.mk docs` on f2cfcec33: rc 1 (C1, above).
- `q run --cores 2 -- cargo test -q -p redoubt-doccheck --test docs` on d0f65700b: rc 0, 2 passed
  (`.tmp/B60/doccheck2.log`).
Not run: the whole bench, host tests, builds (no Rust or Elixir source changed); the unsafe count
is unchanged by construction.

Scratch to remove when the package closes: `.tmp/B60/` (logs, host tools, the second-attempt
case drafts); the worktrees `.worktrees/B52`, `.worktrees/B61` (merged, nothing owed) and the
placeholder `.worktrees/B46` noted in the handoff.

## Affected summaries checked

- `docs/userland/shell.md` "The terminal library", "The rate": the numbers; updated (below).
- `docs/userland/shell.md` "Printed text in runs", "Width": the paths as described; unchanged,
  still what the code does.
- `docs/plan/m2-usable-shell.md`, `docs/README.md`, `README.md`, `GETTING-STARTED.md`,
  `docs/userland/beamlet.md`, `docs/todo/shell-invisible-format.md`: grep for `28,500`,
  `25,800`, `shell-output-rate`, `bytes a second`, `encoder`, `grapheme`, `CJK`, `emoji`,
  `1 MiB line`: no claim on the encoder's rate or the grapheme path outside shell.md's section;
  no change.

## Review amends (steward-red-2 OK with notes)

Head **9421660cc** (amended from d0f65700b; one commit): P2-1 the evaluator finding stated in
shell.md "The loop" step 3 (as prose, the node unnamed per C4); P3-1 the case's comment says the
prompt-written step grows with the bound binary (4 KiB 4.3 ms, 256 KiB 50 ms, 350× the compiled
walk). `cargo test -p redoubt-doccheck --test docs`: rc 0 (`.tmp/B60/doccheck3.log`). Frozen for
train 29.

Rebased onto main **3d5464c41** (train 28): head **94b26298b**, clean; doccheck rc 0 (`.tmp/B60/doccheck4.log`). Frozen for train 29.
