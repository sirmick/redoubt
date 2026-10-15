# B71 — beamlet's `:binary.match` gets a compiled matcher

- **Base:** wp-B69 efdeb40ed (one commit on main 3b5e5e975). **Head:** wp-B71 1c2828ae9, one
  commit (rebased from 3b5e5e975 onto efdeb40ed as instructed; clean, no conflicts). Not pushed.
- **Review folds (steward-red-2, OK with notes), amended into the commit:**
  - P3-1: two differential test modules, in tests/erlang (Redoubt's own suite; the atomvm
    directory is AtomVM's vendored tests, every file under its licence header):
    `binary_compiled.erl` (a compiled pattern through match, matches and split, with scopes, the
    `{bm, Ref}`/`{ac, Ref}` shape, forged compiled patterns badarg, compile_pattern of a compiled
    pattern or an empty pattern badarg) and `binary_split_scope.erl` (split with a scope beside
    global, trim and trim_all, a two-pattern separator, a negative-length scope, a scope past the
    end badarg). `tools/difftest erlang atomvm`: 511/511, both new modules pass on BEAM and
    beamlet (build/difftest/erlang/binary_compiled.verdict, binary_split_scope.verdict: pass).
  - P3-2: shell.md's rate bullet now carries the current figures and one baseline, B64's (one
    matcher pattern per control, before the compiled matcher); B69's and the runs-built figures
    are gone from it. The profile bullet keeps its one generation of before (B69's).
  - Gates after the folds: cargo test -p beamlet-vm exit 0; tools/difftest erlang atomvm 511/511;
    cargo testbench docs PASS; rustfmt check clean. No rate rerun (tests and a page only).
- **Worktree:** /home/mcloonan/redoubt/.worktrees/B71
- **Paths:** userland/otp/vm/src/bif/binary.rs, userland/otp/vm/src/bif/mod.rs (one `pub use`),
  userland/otp/vm/tests/binary_match.rs (new), docs/userland/beamlet.md, docs/userland/shell.md
  (the rate and profile bullets), tests/shell-output-rate.toml (floors and description).

## The matcher

`Matcher { pats: Vec<Vec<u8>>, starts: [u32; 257] }` (binary.rs). `new` sorts the patterns by first
byte and, within a byte, longest first; `starts` is the prefix count, so the patterns beginning with
byte `b` are `pats[starts[b]..starts[b+1]]`. `find(hay, from, until)` truncates the haystack to
`until`, and at each position reads one byte, takes its group (empty for most bytes of real text)
and returns the first pattern of the group that is a prefix of the rest: the longest at the leftmost
position, so leftmost-longest is kept by construction; a pattern crossing `until` cannot match
because the slice ends there. No Aho-Corasick: the table is O(len) on the realistic page (0.44 ns a
byte, flat; see below).

`pattern(c, t)` returns `Pattern::Compiled(Held<Matcher>)` for `{bm | ac, Ref}` whose Ref is a
resource holding a `Matcher` (any other tuple is badarg, as on BEAM), or `Pattern::Fresh(Matcher)`
built for the call from a binary or a list of binaries (none empty, list non-empty, improper list
badarg: unchanged). `compile_pattern/1` builds the matcher once and returns `{bm, Ref}` for one
pattern, `{ac, Ref}` for several (BEAM's tags), the resource sized with `Matcher::bytes()` so it
counts toward holders' heap limits (the shell keeps it in `persistent_term`). `match`, `matches`
and `split` take `Bits::to_bytes()`, a borrow of the heap bytes when the binary is byte-aligned
(every binary is), so the haystack is no longer copied per call; `split`'s pieces still slice the
original `Bits`.

Semantics kept: badarg cases (empty pattern, `[]`, improper list, non-binary haystack), OTP 28's
empty-subject behaviour, `{scope, {S, L}}` with negative L, `matches` non-overlapping left to right,
`split` global/trim/trim_all. `binary:replace` is OTP's Erlang over these.

## Flatness measurement (host, `cargo test --release -p beamlet-vm --test binary_match`)

The page: 65,529 bytes of printable ASCII (what shell-output-rate draws; no newline, since `\n` is
a control pattern). Patterns: the guard's 74 control characters plus 14 format characters = 88;
the subsets are the first 1 and 8. Least of five runs.

| patterns | compiled matcher | built per call | before (every pattern at every byte) |
| --- | --- | --- | --- |
| 1 | 0.44 ns/byte | 0.44 | 1.68 |
| 8 | 0.44 | 0.44 | 11.26 |
| 88 | 0.44 | 0.47 | 129.93 |

Debug build for comparison: 9.9 / 9.8 / 9.9 compiled; before 26 / 85 / 746. The test asserts
88 patterns ≤ 3 × one pattern + 50 µs (release ratio 1.0). The "before" column is the old `find`,
kept in the test as the oracle the matcher is held to over 2,000 random haystacks × every scope.

## The machine's rates (after train 32's DONE, each case alone, from target/prebuilt)

Three runs per case and width, all PASS (12 + the 4 of a round whose transcripts the bench had
pruned before I copied them, also PASS; the make logs of every run are .worktrees/B71/.tmp/B71/
make-*.log, the twelve transcripts .tmp/B71/console/). rv64 / rv32; B69's figures from its report
in brackets.

| shell-output-rate | run a | run b | run c | [B69] |
| --- | --- | --- | --- | --- |
| ascii B/s | 64,973 / 51,210 | 65,989 / 50,796 | 62,542 / 49,510 | [44,100 / 38,100] |
| utf8 B/s | 122,883 / 98,601 | 122,998 / 98,645 | 123,097 / 98,760 | [63,900 / 56,900] |
| pages mixed B/s | 11,714 / 10,452 | 11,995 / 10,257 | 12,136 / 10,459 | [9,800 / 8,250] |
| pages ascii B/s | 37,270 / 31,283 | 37,273 / 31,322 | 37,251 / 31,259 | [22,400 / 18,400] |
| redraw µs | 98,410 / 109,462 | 108,835 / 113,043 | 105,280 / 133,625 | [104–107 k / 101–124 k] |

shell-text-profile, whole draw in guest µs (rv64 / rv32), ranges over the three runs:
ascii 256 KiB 4.89–5.02 M / 6.32–6.45 M [6.63–6.78 / 7.80–7.89], of which visible 1.49 M /
1.97 M (three tenths; it was three fifths); page 64 KiB 5.00–5.03 M / 5.80–5.82 M [6.20–6.21 /
7.32–7.33]; ASCII page 1.77 M / 2.10 M [2.93–2.96 / 3.59–3.61]; mixed 64 KiB 16.08–16.24 M /
18.70–18.85 M [16.58–16.72 / 19.33–19.42]; 1 MiB line 79,200–79,450 / 61,454–61,515 B/s in
13.2 / 17.1 s [49,709–49,860 / 42,299–42,461 in 21.1 / 24.8]. Every figure better than B69's;
the redraw (a keystroke, not matcher-bound) unmoved.

**Floors** (all three runs agree, margins stated): lines 12,000 → 24,000 B/s (rv32 ASCII 49,510
at the least: 2.1); pages 6,500 → 8,000 (rv32 mixed 10,257: 1.3) and 12,000 → 24,000 (rv32 ASCII
page 31,259: 1.3), the page margins kept close as before, since they trap the per-line prologue;
redraw 250 ms unchanged. Verified after the change with `cargo testbench shell-output-rate`
(both widths): PASS rv64 8.1 s, rv32 7.7 s; its lines rv64 61,633 / 122,871, pages 11,788 /
37,257; rv32 51,630 / 98,862, pages 10,401 / 31,317 (.tmp/B71/floors-run.log, console/floors-*).

## Gates (all through scripts/q)

Before the rebase (base 3b5e5e975, head a9777ad53), then again after it (efdeb40ed, the same
code):
- `cargo test -q -p beamlet-vm` (userland/otp): exit 0 both times, 47 unit tests (5 new in
  binary.rs) and every integration test incl. binary_match's two; release run of binary_match
  exit 0.
- `tools/difftest` (whole, before the rebase): run 1 526/527, the one failure atomvm/prime_smp, a
  scheduler-timing test that uses no binary function: the BEAM oracle's cached result, taken at
  08:55 while test-shell loaded the machine, was BEAM's own `failed` after 11 attempts. I deleted
  that one cached oracle entry and reran `tools/difftest atomvm` alone: 459/459, exit 0. The tag
  tightening (`{bm|ac, Ref}` only) was committed after run 1; the atomvm suite, which holds the
  binary functions (test_binary_match/split/replace), ran after it. Not rerun after the rebase
  (B69 touches no Erlang the difftest runs; the VM code is byte-identical).
- `./test-shell`: exit 0 both times, every stage passed (after the rebase: beamlet 366 passed /
  1 skipped; BEAM 297 / 70 skipped).
- `cargo testbench docs`: PASS before the rebase, after it, and after the shell.md/toml edits.
  The dead_code warning in its log is pre-existing (tools/testbench/src/idle.rs:72), not mine.
- rustfmt +nightly --check on the three Rust files: clean (after the rebase too).
- tests/size-budget.toml: beamlet (userland/otp) is not in the budget; no change.
- Not run: no other QEMU case; the two rate cases are the body's.

## Summaries checked

- README.md, GETTING-STARTED.md, docs/README.md, docs/plan/m2-usable-shell.md: no claim about the
  pattern search, compile_pattern or the output rates; no change.
- docs/userland/beamlet.md: added the bullet "A compiled pattern is a compiled matcher" among the
  runtime's features (after Compression) with the host figures, and the two host tests in the
  section's status line. Its "OTP's own natives parse … patterns" sentence still holds.
- docs/userland/shell.md: the rate bullet and the profile bullet rewritten with the machine's
  figures, B69's kept as the "before"; line 403's description of the matcher window unchanged
  and true. The per-grapheme figures (0.85 ms, 0.42, 0.22) are B69's: the profile prints those
  stages as lists my extraction did not parse, and the grapheme walk does not search.
- tests/shell-output-rate.toml: description and the comment on the pages' floors updated with it.
- userland/otp has no README naming the binary natives.

## Residuals / notes

- `{ac, Ref}` with a one-pattern matcher (or vice versa) is accepted; BEAM checks the tag against
  the type. Harmless; noted.
- The 14 "format characters" in the test's 88 are padding to the number in the body; the
  measurement is the pattern count, not the guard's exact list (B69's final guard has 38 entries).
- Scratch: .worktrees/B71/.tmp/B71/ (logs, transcripts, the two run scripts); nothing under /tmp.
