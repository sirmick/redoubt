# B46 report (try2-implementer, 2026-10-09)

Branch wp-B46 in /home/mcloonan/redoubt/.worktrees/B46: one commit, 3fd9684e4, on main 6b46c8c86 (BEAM19 merged).
It began as c0bc09f36 (ASCII runs only), was rebased three times as main moved, and the rest was
folded in. Not pushed.

## 1. The starting commit against the node body

c0bc09f36 made printable ASCII a fast path in `Text.scan` and `Term.advance_text`, measured on host
beamlet only (256 KiB: 920 → 48 ms). Missing: a profile by stage, UTF-8 (only ASCII was fast), any
measurement on the machine, a target and a case.

## Profile (host beamlet, 256 KiB, main's encoder)

| stage | time |
| --- | --- |
| making text visible | 24–96 ms |
| walking graphemes | 160–260 ms |
| measuring each grapheme (`Width.columns(Text.visible(g))`), walk included | 470–1050 ms |

Measuring dominated, about two thirds, and was redundant: the text was already visible. On the
machine (icount, guest time) the cost per Erlang loop step is ~87 µs, against ~0.13–0.15 s for a
BIF over all 256 KiB, so per-byte Erlang loops dominate there. That finding is BEAM19's; see
`.wash/local/BEAM19-finding.md`.

## What the commit does (userland/shell, the encoder)

- **Visible text** (`text.ex`): a printable UTF-8 run is kept as one slice. Its first 64 code
  points go through Erlang; past that, windows of 128 to 512 bytes go through `:binary.match`
  (controls, pattern compiled once in `:persistent_term`) and `:unicode.characters_to_list`
  (validity and code-point count, with no copy). Runs of ASCII control characters and DEL are
  drawn as carets into one growing binary. Tab, C0 and DEL are matched in `scan/3`'s own clause
  heads.
- **Measuring** (`width.ex`): `Width.run/1` covers the run set: ASCII, U+00A0–00AC, U+00AE–02FF,
  U+0370–0482 and U+048A–052F. It uses the same Erlang prefix, then a `:re` class on bounded
  windows. `Width.grapheme` treats a single code point below U+1100 as narrow.
- **Term** (`term.ex`): visible text is measured once per grapheme. Runs are taken whole, holding
  back the last code point when more follows. A run starts only where the first byte is 0x20–0x7E
  or 0xC2–0xD4.
- **The line redraw**: one pass draws the line and finds the cursor. A printable ASCII grapheme
  that fits its row costs one step, in both the draw pass and the cursor pass.
- **The kept cursor** (orchestrator's question): at 6bc2e8a80 `cursor/1` still walked the line
  from its start on every insert. Each step was cheaper, but a typed line was still O(n²). Now a
  key typed at the line's end sets `at: {:typed, pos}`. The public `request/2` makes that
  `{:kept, pos}` and drops any `at` a request leaves untouched; `resize` drops it too.
  `cursor/1` is asked only of the model a request was given. An earlier version keyed the kept
  cursor on the `before` list; beamlet compares lists per element, which cost 160 ms per redraw,
  so that version was replaced.
- **Bounded windows** keep the heap of the BEAM driver test (`include_shared_binaries`) below its
  limit. 4 KiB windows grew the heap to 168K words and killed the 1 MiB test.

## Measured

Machine, `icount` `shift=3`, alice's console, guest time (`shell-output-rate`):

| | main | branch, rv64 | branch, rv32 |
| --- | --- | --- | --- |
| 256 KiB ASCII | 678 / 599 B/s | 28,300 | 25,700 |
| 256 KiB Cyrillic | 1,379 / 1,207 B/s | 30,800 | 28,600 |
| 64 KiB mixed (tabs, controls, wide), rv64 | 998 | 2,394 | |
| redraw, a key mid-line in 400 characters (6 rows of 80), least of 3 | 1.64 / 1.84 s | 62 ms | 61–71 ms |

- **Floors:** 12,000 B/s for each text, 2.1× headroom on rv32 ASCII; redraw 250 ms, 3.5×.
  Run-to-run spread under 1%. `shell.md` notes the floors are re-set when beamlet's per-step cost
  falls (BEAM19).
- **`shell-long-output`** (SSH, host clock): 7.4 s rv64, 6.5 s rv32, against 167 and 186 s at B45.
- **Host beamlet**, 256 KiB: ASCII 855 → 43 ms; Cyrillic 696 → 40 ms; CJK 549 → 287 ms; mixed
  1013 → 283 ms. Every text draws the same bytes as main's encoder.
- **Typing cost** (`term_test`): 20 keys 180 characters in cost at most twice 20 keys at the start,
  in reductions. Without the kept cursor: 4.4× on one VM and 3.7× on the other, so the test
  fails.
- **BEAM driver test** (1 MiB lines, limit 352,256 words): peaks 173K and 187K, against main's 185K
  and 269K.
- **`beamlet-footprint`**: rv64 5,490 pages (main 5,481), so twice the peak is 10,980 of the 10,989
  cap, 9 pages of slack where main had 27; rv32 5,308. `beamlet.md`'s table and `budgets.md`'s
  peak sentence are refreshed (both were already stale on main).

## Gates at c4ffc0d3b (env from launch-env-2026-10-09.md)

- `q run --cores 8 -- ./test-shell`: rc 0, every stage. beamlet 347 passed; BEAM 287 passed, 60
  skipped; formatting, native, entry_point, terminal and on_fake_kernel ok.
- `make -f scripts/jobs.mk prebuilt`: rc 0.
- `make -k -f scripts/jobs.mk set CASES=…`, all PASS on both widths:
  - `shell-output-rate`
  - `shell-long-output`
  - `beamlet-footprint`
  - `userland-boot`, `userland-read-only`, `init-boot`
  - `steward-session-ends`, `steward-ssh-two-principals`
  - `size-budget` (rv64)
- `make docs`: rc 0.
- **Not run:** the whole bench, difftest, beamlet's Rust host tests (no beamlet code changed).

## Pages

- **`docs/userland/shell.md`, "The terminal library":** status line (machine-tested printed text,
  3 cases), "Printed text in runs", "The line being edited", "The rate" (numbers, floors, margins,
  re-set note). "Hostile text": the encoder draws visibly a run or one control at a time. The
  guarantees are unchanged: the same rule, the same bytes. The exhaustive tests hold
  `visible == itself` exactly when the text is not `control?`, short and long runs alike.
- **`docs/userland/beamlet.md`:** the "What the VM holds at its prompt" table refreshed.
- **`docs/kernel/budgets.md`:** the peak sentence (5,490 / 5,307; the rule's bound is 5,494).
- **`tests/shell-long-output.toml`:** its stale "8 KB/s" comment.
- **Checked, no change:** `Redoubt.Term`'s and `Redoubt.Term.Text`'s moduledocs are still true.
  `docs/testbench.md`'s guest-time counts ("145 of the 226") were already stale on main (251 boot
  cases); they are not B46's and the docs checker does not hold them.

## Fix round (steward-red BLOCK on c4ffc0d3b)

**P1 (the matcher windows untested).** Fixed:

- **What the old tests missed:** every "long" run was 40 code points, short of the 64 after which
  the matchers take over.
- **Window edges:** `text_test` and `width_test` sweep each edge, the prefix's end and 128, 640 and
  1152 bytes past it, five bytes each side, after ASCII and after "é". The specials:
  - controls: `\e`, NUL, DEL, U+0085 and U+009B (C2 8x), U+202A and U+2069 (E2 80 AA ...);
  - characters cut by an edge: é, €, 🙂;
  - a combining mark (U+0301) and a tab;
  - invalid input: FF, E2 82, F0 9F 99.

  For the run set, the specials are the controls, marks, U+00AD, U+0483, wide characters and
  U+0530, plus run members. `visible/2`, with its column, and `Width.run/1` must equal the Erlang
  path, the same text taken a unit at a time; a unit alone never reaches the matchers.
- **Long runs:** 65, 80, 100, 600 and 1,500 code points of x, é, я, xé and 世 pass whole.
- **Existing tests lengthened:** the "long" runs are now 100 code points; `term_test`'s cursor test
  adds runs of 70, 130 and 600.
- **Mutation check:** C1 controls dropped from the matcher's patterns fails `text_test` on both VMs
  (11/13); Cyrillic dropped from the run class fails `width_test` (5/7).
- **Commit message:** rewritten to say exactly this.

**P3 (footprint after a heavy draw).** Measured with `beamlet-footprint`'s setup (init starts
beamlet with `report_memory`, `memory = true`), its typed command replaced by
`shell-output-rate`'s two 256 KiB draws:

| encoder | rv64 peak | rv32 peak |
| --- | --- | --- |
| this branch | 6,073 pages | 5,851 pages |
| main's | 6,536 pages | |

The post-draw peak is the workload's, two 256 KiB strings in a session, and this branch lowers it.
B32's rule takes the prompt peak, 5,490 on rv64 against its bound of 5,494, so no cap moves.
Still, a session that draws this much exceeds half its heap cap, main included; that is worth a
note for whoever owns the session budget.

**Session numbers on this head:** 11,008 pages; heap cap 10,989; prompt peak 5,490 (rv64) and
5,308 (rv32). CTX2's 128 relay pages: whichever lands second redoes the arithmetic.

**Gates at 057643aa1**, rebased onto 22195522e (TRY2 merged), all rc 0:

- `./test-shell`: every stage (beamlet 349, BEAM 289 passed, 60 skipped).
- prebuilt.
- `shell-output-rate`: rv64 11.1 s, rv32 11.2 s.
- `shell-long-output`: rv64 8.8 s, rv32 8.7 s.
- `beamlet-footprint`: rv64 and rv32.
- `formatting`.
- docs.

## Rebase onto BEAM19's main (6b46c8c86)

The rebase was clean. Re-measured in `shell-output-rate`, guest time, two runs per width (spread
under 1.3%):

| | rv64 | rv32 |
| --- | --- | --- |
| ASCII, 256 KiB | 28,381 / 28,702 B/s | 25,727 / 25,831 B/s |
| Cyrillic, 256 KiB | 31,892 B/s | 29,627 B/s |
| redraw, least of three | 56.6 ms | 65.4 ms |

Probe, rv64, 64 KiB texts on the new VM (B/s):

| | ASCII | Cyrillic | mixed |
| --- | --- | --- | --- |
| this branch | 27,098 | 31,461 | 4,241 |
| main's encoder | 620 | 1,241 | 1,150 |

- **Floors kept:** 12,000 B/s for each text (2.1x on rv32 ASCII) and 250 ms for the redraw (3.8x).
  BEAM19 moved these paths little, since the encoder's time is now mostly native matchers.
- **`shell.md`:** the floors re-set sentence is replaced by a statement of what was measured. The
  per-step cost now cites `beamlet.md`'s measurement (~3,600 guest instructions on rv64) instead of
  my 87 us.
- **The case's description** carries the new numbers.

**Gates at 3fd9684e4**, all rc 0:

- `./test-shell`: every stage (beamlet 349, BEAM 289 passed, 60 skipped).
- prebuilt.
- `shell-output-rate`: rv64 10.6 s, rv32 9.8 s.
- `shell-long-output`: rv64 7.8 s, rv32 6.9 s.
- `beamlet-footprint`: rv64 and rv32.
- `formatting`.
- docs.

## Residuals

- `beamlet-footprint` rv64 slack is 9 pages. The next growth in the shell's prompt code moves the
  session size up a 128-page step (`budgets.md`'s rule).
- The case times the encoder alone. NSCR1's 3.3 s keystroke also included `group`'s and `edlin`'s
  work, and the driver's writes, which are not measured here.
- Text outside the run set (CJK, marks, emoji) is still measured a grapheme at a time: 2–4× main on
  the host.
- The BEAM `driver_test` crash-report case is order-sensitive: it failed only after the 1 MiB test
  had killed a driver, which left the logger's handler replaced. It passes now that the 1 MiB test
  does.

## Rebase onto CTX2's main (050357e8a), by b46-implementer-2

Head **1d3177086** on 050357e8a, one commit, message from `.tmp/B46/msg`.

**Conflict, docs/kernel/budgets.md:** I kept CTX2's paragraph (session 11,136 = VM share 11,008 +
relay 128; VM heap cap 10,989) and put in B46's peaks. Old: "from a peak of 5,459 pages on rv64
and 5,277 on rv32". New: "5,491 … and 5,308". The relay's 128 pages are outside the VM's
share, so B32's rule applies to the VM's 11,008 as before, and nothing moves.

**Other copies of the peak:**
- beamlet.md:235 said 5,459; it now says 5,491.
- testbench.md (CTX2's paragraph on the session VM) said "71 pages over twice … 5,459"; it now
  says "7 pages over twice … 5,491". The old B46 head carried both stale mentions too.
- image/manifest.json (`sessions.pages` 11136) and beamlet-footprint.toml (`budget_pages=11008`)
  are unchanged and agree.

**beamlet-footprint on this head** (report_memory, guest log):

| | rv64 | rv32 |
| --- | ---: | ---: |
| scan peak (`heap beamlet`) | 5,491 of 10,989 | 5,308 of 10,989 |
| accounted | 3,923 | 3,857 |
| runtime held, peak | 4,145, 4,563 | 3,975, 4,394 |
| not accounted | 222 | 118 |

beamlet.md's table rows above are updated. I left "heaps, collected" (57) and the per-category
rows alone, because this run does not give them in the table's rounding. Slack: rv64 is 3
pages of peak under the 5,494 bound, and twice the peak is 7 pages under the cap. **The cap
does not move and there is no Size budget line.**

**Rates (one run each):**
- rv64: ASCII 28,570 B/s, Cyrillic 31,888 B/s, redraw 56.7 ms.
- rv32: ASCII 25,604 B/s, Cyrillic 29,632 B/s, redraw 65.7 ms.

The commit message's figures still hold, except the footprint sentence: it now says 5,491 and 7
pages, drops the "9 pages more" delta and names testbench.md.

**Gates, all rc 0:**
- `./test-shell` (beamlet 349; BEAM 289 passed, 60 skipped), at b0589db53. That head is the same
  code as 1d3177086, which differs only in the three docs files.
- `make prebuilt`, then `make set CASES="beamlet-footprint shell-output-rate shell-long-output"`
  on both widths:
  - footprint: rv64 5.4 s, rv32 6.6 s;
  - output-rate: rv64 10.0 s, rv32 10.7 s;
  - long-output: rv64 8.3 s, rv32 10.7 s.
- `make docs` and the formatting case, at 1d3177086.

**Summaries checked:**
- budgets.md, beamlet.md and testbench.md: updated as above.
- shell.md: its rates still match.
- image/manifest.json: unchanged, and it agrees.
