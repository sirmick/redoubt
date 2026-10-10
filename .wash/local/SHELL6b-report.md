# SHELL6b report: the editor's syntax highlighting

Branch wp-SHELL6, one commit on origin/shell 027d6e392 (6a's merge): see the head in the message.
Rebase from 52aa9b31b had one conflict in docs/userland/shell.md (6a's burst note and 6b's
drawing sentence in one paragraph); resolved keeping both.

## Delivered
- `Redoubt.Editor.Syntax` (lib/redoubt/editor/syntax.ex): language by extension (.ex/.exs,
  .erl/.hrl, .rs, .md, .toml, .json; else plain), the behaviour (line/2: pieces + state), and
  one table-driven scanner: line and block comments (Rust's nest), strings by delimiter with or
  without escapes, single- or multi-line, each with a role (Erlang's quoted atoms are
  constants), keywords, constants, capitalised names, Elixir's :atoms and key:, character
  literals (? and $), TOML table lines as headings. Neighbouring pieces of one role merge.
- Language modules (lib/redoubt/editor/syntax/): Ex, Erl, Rust, Toml, Json as tables; Markdown
  by hand (headings, fenced blocks with state, code spans). Each loads when a file of it opens.
- Theme: six roles, keyword, string, comment, number, constant, heading, in all three themes
  (plain: bold, italic, dim, bold+underlined; no colours, as plain is the terminal's own).
- View.runs/7 takes the line's pieces; each grapheme takes the role of the piece it starts in;
  cursor > selection > highlighting. Visible-text drawing unchanged.
- Editor: per doc `lang`, `marks` (start state every 128 lines), `seen` {version, cursor row}.
  After every update (update/2 wraps the old clauses, now handle/2), the current doc's marks
  are dropped above min(cursor row before, after) when the buffer's version changed, and
  extended down to the window's top in one slice pass; replace_all resets them. view/3
  highlights the window from the mark at or above its top.

## Tests (both VMs)
- test/redoubt/editor/syntax_test.exs: extension choice; each language's roles and its
  multi-line states, rendered as <role:text>; for every language, five hostile lines (escapes,
  bidi, NUL, combining marks, wide characters, unclosed strings) from every state: the pieces
  rejoin to the line byte for byte and every role is one of the seven; an escape inside a
  string is drawn ^[ in the string's role, cursor and selection over highlighting; every theme
  has every role.
- editor_test.exs: a 400-line .rs file: marks kept down to the window; "/*" typed on line 1
  drops all marks below it and the re-extended mark at row 256 is {:comment, 1}; a .txt file has
  no language.

## Page (docs/userland/shell.md)
Status line: the editor and its highlighting built, syntax_test.exs named; a paragraph on what is
highlighted, that a file chooses only a role, the start states and their cost (the first jump to
the end of a large file scans it once); "What is not built" now only the file manager; the theme
bullet names the code roles; Why: search is Regex, highlighting reads each line once.
Summaries checked: README.md, docs/README.md, docs/userland/README.md, docs/plan/m2-usable-shell.md
name the editor and highlighting as M2 goals without status claims: no change.

## Tier
Tier B on the shell track as ruled for 6a: the highlighting parses a file's text only into roles
from a fixed set, inside the session's screen; nothing reaches the terminal but through the
screen buffer and Text.visible.

## Gates on dcf8b8c88
Full ./test-shell rc=0 (every stage); docs rc=0; format clean (test-shell's stage); prebuilt rc=0
(rv64 237, rv32 223); beamlet-footprint PASS: rv64 heap 5,456 / stack 35,880 B, rv32 heap 5,275 /
stack 28,920 B; base origin/shell 027d6e392 measured rv64 5,455 / rv32 5,274: +1 page each. Cause not
verified (the log lists no modules): Theme's six roles if Theme is loaded at the prompt, or the
larger module pack (seven new modules); the Syntax modules are called only from ed.

## Fold of b22-red's BLOCK on dcf8b8c88 (2026-10-08): head cb13f236a
- Quadratic merge: the scanner now records each piece as {length, role}; neighbours merge by
  adding lengths, and the line is cut once with binary_part. Measured, Syntax.Ex.line on a line
  of spaces: beamlet 88 / 353 / 1,600 ms for 16 / 64 / 256 KiB (about 6 us a byte, linear; a
  `a <> b` fold was still quadratic there, beamlet does not append in place); BEAM 4 / 15 / 68 ms.
- Cap: Syntax.line/3 reads only a line's first 4,096 bytes (Syntax.scanned/0); the rest is one
  plain piece, and the state is where the scan left off. after_lines/3 goes through it too, so a
  window's scan and the marks' extension are bounded per line.
- Redo's stale marks: undo and redo (and replace, as before) reset the doc's marks
  (unmarked/1); the next update re-extends them from the top.
- Tests: syntax_test, a 1 MiB line of spaces scanned whole by Syntax.Ex.line (one plain piece,
  under 20 s; ~7 s on beamlet) and as drawn by Syntax.line on a Rust comment line (a 4 KiB
  comment piece and a plain rest, under 1 s); editor_test, "/*\nopen\n" pasted at the top of a
  400-line .rs file, Ctrl+End (marks[2] comment), undo and Ctrl+End (code), redo with the
  cursor at row > 256: marks[2] comment again.
- Progress line (the miss): docs/plan/m2-usable-shell.md now says the editor has syntax
  highlighting, without the file manager yet; 6c makes it "and the file manager, host-tested".
- Page: the highlighting paragraph adds the drops on undo/redo/replace, the 4 KiB cap and the
  per-length cost.
6c rebased on it: 24afb30c1 (one conflict, that progress line).
Rebased onto origin/shell 624ab4b08 (no conflicts): 6b 01841b74e, 6c 0bf9eb848.
Gates on 01841b74e (SHELL7 worktree, detached there): full ./test-shell rc=0; docs rc=0; format
clean; prebuilt rc=0; beamlet-footprint PASS rv64 heap 5,456 / stack 35,880 B, rv32 heap 5,275 /
stack 28,920 B; base 624ab4b08 measured rv64 5,455 / rv32 5,274: +1 page each.
