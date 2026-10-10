# SHELL4 design checkpoint: the pager, help drawn in it, completion (host)

Branch wp-SHELL4 on main 2db31544b, worktree .worktrees/SHELL4. Pages: docs/userland/shell.md
"Session commands and the pager", "Completion", "Help".

## 1. The pager: `Redoubt.Screen.Pager`, a `Redoubt.Screen`

**When it engages.** The printer's `value(%Lines{})` (the value of a line at the prompt) reads
the first `rows` lines through a suspended `Enumerable.reduce`. If the lines end within a
screen, they are printed inline as today (no alternate screen): short output, and every machine
case (none prints a long `%Lines{}`; the console's unknown size is 80x24), stay as they are. If
not, `Redoubt.Screen.run(Pager, {read_so_far, continuation, style})` takes over. Where there is
no shell terminal (the driver does not answer: `Screen.driver!` raises today), the printer falls
back to printing every line. `out(value)` prints without the pager. Any other value is inspected
and printed as now (a long inspect is not paged in this package).

**Lazy, and it closes what it opened.** The pager holds the continuation and pulls lines only
as the view or a search needs them, so `cat("big.log")` reads what is shown. Quitting halts the
continuation (`{:halt, acc}`), which closes a `cat`'s file. Lines read are kept in the pager's
process (to page back); the process has the evaluator's heap limit, so a file larger than that
ends the pager as any line past the limit ends ({:screen, :killed}): a stated residual.

**Keys.** Space, f, Page Down: a page on. b, Page Up: a page back. Down, j, Enter: a row on.
Up, k: a row back. g, Home: the top. G, End: the end (reads to it; Ctrl+C ends a stream that
never ends). / then text then Enter: search forward from the top row; n next, N previous; Esc
leaves the search line. q, Esc: quit. Matches drawn reversed. The status line: the row range, `
(END)` when the end is known and shown, or the search being typed.

**Layout.** A line longer than the width wraps onto further rows; the pager's position is a
(line, row-within-line) pair. Wrapping and searching are pure functions in their own module
(`Redoubt.Screen.Pager.Doc`), tested on the BEAM too; the screen draws with `Widgets.label`.

**Return value.** The pager ends with `nil`; the prompt then shows nothing more (the printer
prints nothing after a paged value).

## 2. Help drawn in it

`help` and `h` keep returning `%Lines{}`, so `help() |> grep("file")` still works. `Lines` gains
one field, `style` (nil, or a function from a line to `[{text, style}]` segments), which only the
pager reads; `grep`, `head` and friends build new `Lines` without it, so filtered help is plain.
Help's style: a command page's usage line and its "Examples"/"Parameters" headings bold; a
topic's Markdown headings (`# `, `## `) drawn bold without the hashes; a list item's wrapped rows
indented under its text (the pager's wrap takes a hanging indent from the style); `code` spans
left as they are. Inline (short) help prints plain as today.

## 3. Completion

**Where it runs.** `group` calls its `expand_fun` in `group`'s own process with the line before
the cursor. The shell, before each read, sets it with `:io.setopts(expand_fun: fun)` (group
supports this), a closure over pure data: the names bound at the prompt and the env's aliases.
So the completer never messages the shell, and the shell needs no new state.
`Redoubt.Shell.Completer.expand(before, context)` is pure but for the path listing.

**Source**, from `Code.Fragment.cursor_context/1` and `container_cursor_to_quoted/1`:
- a name being typed: commands (the registry), then the prompt's variables, then Kernel's
  imports; a command completes to `name(`.
- `Mod.fu`: the module's exported functions (`module_info(:exports)`, so beamlet needs nothing
  new); `Mo`: aliases, then the loaded and available modules (`:code.all_available/0`, which
  beamlet has).
- inside a string argument of a command: the parameter's declared type. `path`: the directory
  listing; `command`/`name`: commands and help topics. Principal, budget and label types do not
  exist yet (Redoubt-only); they stay on the page as planned.
- **Paths:** relative to `File.cwd!()`; one `File.ls` per Tab, and `File.dir?` only on the names
  that match, for the trailing `/` (on Redoubt a 9P read carries each entry's mode). The listing
  runs in a spawned process with a timeout (300 ms); past it, Tab does nothing. The completer
  never writes and never launches.

**The key and the list.** Tab, edlin's own; the first Tab inserts the longest common prefix.
The candidates are shown by `group`'s expand area below the line, which `Redoubt.Term` already
draws (SHELL2): up to seven rows, Tab again for all, Page Up/Down scrolling them. **Decision
asked:** the page says a long list goes "through the pager"; I propose the page say `group`'s
expand area instead (it already pages, needs no screen while a line is being edited, and keeps
the line on screen). Alternative: the driver intercepts the expand request and runs the pager.

## 4. Through Text.visible

Every row the pager draws (`Widgets.label` makes it visible, and the buffer refuses a control
character anyway), its status line, and the search text typed; help's segments; completion's
candidates (drawn by Term's expand path, already guarded) and what Tab inserts (drawn by Term).
A file name holding a control character completes to that character inside the string literal,
so it names the real file; it is drawn as `^[` and the like.

## 5. Tests

- BEAM and beamlet: `pager/doc_test.exs` (wrap, hanging indent, positions, search forward and
  back, end unknown); `completer_test.exs` (commands, variables, Mod.fun, modules, path inside a
  command's string, dirs with `/`, a non-path string gives nothing, a slow listing gives nothing,
  the longest common prefix); help style (headings bold, plain when grepped).
- beamlet only (screen buffer), on the terminal model through the driver: a short `%Lines{}`
  prints inline; a long one enters the pager; space, b, /search, n, G, q; the file of a `cat`
  is closed on q; a hostile line is drawn visibly in the pager; `help(:cp)` long enough shows
  its usage bold; `out(lines)` prints without the pager; Ctrl+C ends the pager.
- Driver: Tab after `he` completes `help(` and the line shows it; Tab Tab lists.
- The pseudo-terminal test gains a pager run (long `Enum.map(1..100, ...)` lines, space, q).

## 6. Pages

"Session commands and the pager" splits: the pager and `out` built; `ns`, `bind`, `whoami`,
`labels`, `clear`, `follow`, `now`… planned in a sibling. Completion built for what is built
(commands, variables, modules, paths on the host), principal/budget/label completion planned.
Help: "drawn in the pager with bold and indentation" built. The two figures' dashed completion
edge goes solid. M2 progress and GETTING-STARTED updated.

## 7. Gates

./test-shell, the crates, prebuilt, the five static checks, and userland-boot,
userland-read-only, steward-vault-session, steward-ssh-two-principals on rv64 and rv32.
