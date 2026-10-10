# SHELL6 design checkpoint: the editor, then the file manager

Basis: wp-SHELL5 (9b76ff3f9), shell.md "The editor" (planned), "Full-screen programs", "Widgets,
focus and themes", "A native program's screen and the session's key" (planned, Open),
"Paste, scrolling..." (planned), shell-plan.md section 4 slice 4. No code yet.

## What gets built

`ed(path)` and `fm(dir)` are commandlets that start one screen program,
`Redoubt.Screen.Editor`, with two views: the editor and the file manager's two panes. F4 in the
panes opens the editor; closing the editor returns to the panes. Its modules load only when
`ed`/`fm` is first called, through SHELL7's index, so the prompt pays nothing for them.

The parts:
1. **The buffer** (pure Elixir, no I/O; `Redoubt.Editor.Buffer`). A file is held as lines in a
   gap form: lines before the cursor reversed, then the lines after. A cursor is
   {line, grapheme}. Edits are insert, delete, split and join. Undo and redo keep a stack of
   inverse edits, coalescing typed runs. There is one clipboard per editor. Several files are
   open at once, switched with a key. A rope comes only if a measured large file needs one.
2. **Find and replace:** Regex over a line, or literal text. On beamlet the engine is the
   linear-time Rust one; ExUnit also runs on the BEAM's PCRE, so the tests avoid patterns
   whose behaviour differs between them.
3. **The view:**
   - Lines are drawn through the widgets and `Text.visible`. A tab expands to the next stop; a
     control character is drawn as `^X` and moves the cursor by its width.
   - Horizontal scroll, no wrap; a status line; a menu bar (SHELL5) for discoverability.
   - Dialogs: find, replace, go to line, save changes? (SHELL5's dialog stack).
4. **Syntax highlighting:** a small tokenizer per language (Elixir, Erlang, Rust, Markdown,
   TOML, JSON), chosen by the file's extension. A token class maps to a theme role, so every
   style is still the code's: file text never sets a colour and stays visible-filtered. It is
   line-by-line with a per-line start state (for strings and comments spanning lines), redone
   from the first changed line. Each language's module loads only when a file of that language
   opens.
5. **The file manager:**
   - Two panes, each a list (SHELL5) of one directory.
   - F3 views (the editor, read only), F4 edits, F5 copies, F6 moves, F7 makes a directory, F8
     removes; each confirms in a dialog.
   - Tab switches panes; Enter enters a directory, or `..` goes up through the pane's own path,
     never through a listed name.

## The file rules (from the page)

- **Loading:**
  - A file is read whole, through `File`.
  - Proposed: a file that is not UTF-8 opens read only, its bytes drawn visibly, since editing
    it as text would change it.
  - Proposed: a size cap (a fraction of the screen's heap limit) refuses a larger file with a
    message.
  - `\r` stays part of its line (drawn `^M`), and a missing final newline is kept as it was, so
    a save changes only what was edited.
- **Saving:**
  - Only to the path it was opened with: write a temporary file in the same directory, then
    rename it over the original.
  - Proposed: if the file changed on disk since it was read (size or mtime), ask before
    overwriting.
- **The panes act only on what they list:** a listed name holding `/`, NUL, `.` or `..` is
  shown visibly and refused; every operation is made as Path.join(listed_dir, name) after that
  check.
- **No content becomes an action:** no modelines, nothing evaluated, no path taken from a
  file's text.

## Questions

1. **Track.** The rules put "anything acting with the session's authority: ... files" on main's
   strict path. The editor's load and save (temp file and rename) and the panes' copy, move,
   mkdir and remove act on files. Proposal:
   - On main, Tier A: a small file-actions module (`Redoubt.Editor.Files`: read for editing,
     save through temp and rename, list with the name check, and the five pane operations on a
     listed directory), with attack cases: a crafted listed name (`../x`, `a/b`, NUL, `..`)
     refused and nothing outside the listed directory touched; a save reaching only its path; a
     file whose content looks like a command or a modeline staying text.
   - On shell: the buffer, view, highlighting, keys and the panes' UI over that module.
   Or is the editor UI with its file actions Tier B, since it uses only `File` with the
   session's existing authority?
2. **Ctrl+C.** The driver ends any screen on Ctrl+C (driver.ex send_keys), so the editor
   cannot copy with it. The page's Open item: the session keeps another key (candidate Ctrl+\
   0x1C) and lets a screen take Ctrl+C. That is a driver change: the drawing and terminal path,
   so strict, for main. Options:
   - (a) settle the Open item (owner: the default key) and change the driver on main first;
   - (b) ship the editor with copy on another key (for example Alt+C or Ctrl+Insert) until
     then, and say so on the page.
   I recommend (a), with (b) as the stopgap if (a) waits.
3. **Paste.** There is no bracketed paste yet (planned in "Paste, scrolling"), so a paste
   arrives as typed keys: a pasted Ctrl+C or Esc would act, and a pasted Tab inserts a tab. Fine
   for the first slice? Bracketed paste is a decoder change (drawing path, main).
4. **Slicing**, each its own merge into shell with the footprint gate:
   - SHELL6a: the buffer, editing, undo, find and replace, save, `ed`;
   - SHELL6b: highlighting;
   - SHELL6c: the file manager, `fm`, F4 into the editor.
   The editor's view and keys are tested on beamlet's model terminal like pick, the buffer and
   highlighting on both VMs. Any Tier A split goes to main as its own package.
5. **Non-UTF-8 read only, the size cap value, and the changed-on-disk check:** OK as proposed?
