# SHELL6c report: the file manager

Branch wp-SHELL6: f2e94d6c7 on top of 6b (dcf8b8c88), on origin/shell 027d6e392.

## Delivered
- `Redoubt.Editor.Manager` (lib/redoubt/editor/manager.ex): `fm(dir \\ ".")`, a screen program
  (ctrl_c: :key, so the editor in front copies with Ctrl+C) of two panes, each a List over
  `Files.list/1`: `/..` first except at `/`, `/name` for a directory, ` name` a file, `?name`
  other, `!name` refused. Keys: Tab pane; Enter: `..` up through Path.dirname of the pane's own
  dir (selection kept on the dir left), a listed directory by Path.join(dir, name), a file into
  the editor; F3 view, F4 edit; F5 copy, F6 move (to the other pane's dir), F8 remove: a confirm
  whose id carries {action, from_dir, name, to_dir} as listed when asked, and deaf_until now +
  300 ms; F7 a prompt for a name, Files.mkdir (which refuses a bad name); F10, Ctrl+Q leave. A
  failure is a dialog ("something of that name is there already", "a directory cannot go into
  itself", ...); both panes are listed again after every action and after the editor closes.
- The editor runs in front: Editor.init(doc), resized to the screen, and every event goes to it
  until it halts. Editor.open/2 takes view: true (read only, and a missing file refused); the
  editor's readonly is now false | :not_utf8 | :viewing, with its own message.

## Tests (test/redoubt/editor/manager_test.exs; both VMs, the screen case on beamlet)
- navigation in and up through the pane's own path, Tab;
- F5 copy (dialog id names a, one.txt, b), again refused as eexist in a dialog, F6 move of a
  directory, F7 mkdir, F8 remove, Esc does nothing: all judged on the file system;
- F8 then two Enters inside the deaf window: nothing removed, the question still open;
- crafted refused names (../b, x/y, ..) injected into a pane as a listing could give them: F3,
  F4, F5, F6, F8, Enter each refused with the message, no dialog, no editor; both directories
  unchanged;
- F4 edit and Ctrl+S save, Ctrl+Q back; F3 view refuses typing ("read only: viewing");
- through the shell's driver: fm draws a name with ESC and BEL as ^[ and ^G, F4 (ESC[14~) opens
  the editor, Ctrl+Q back, F10 (ESC[21~) leaves with :ok.

## Page
docs/userland/shell.md, "The editor": status names the file manager and manager_test.exs; the
intro says the two work as one (fm holds the editor in front, rather than "one program with two
views"); the panes' keys; a paragraph on fm; "What is not built" removed. "The editor's files":
status no longer says the file manager is not built. docs/plan/m2-usable-shell.md progress: the
editor with highlighting and the file manager, host-tested (that line was stale since 6b; see
the message to the orchestrator). Other summaries (README.md, docs/README.md,
docs/userland/README.md) name the editor and file manager as goals: no change.

## Tier
Tier B on the shell track as ruled (the pane operations themselves are Files, on main, Tier A).

## Gates (2026-10-08)
Rebased with 6b's fold onto origin/shell 624ab4b08: 6c is 0bf9eb848 on 6b 01841b74e.
Full ./test-shell rc=0; docs rc=0; format clean; prebuilt rc=0 (rv64 237, rv32 223);
beamlet-footprint PASS rv64 heap 5,459 / stack 35,880 B, rv32 heap 5,277 / stack 28,920 B.
Against 6b (5,456 / 5,275): +3 / +2 pages; against the base 624ab4b08 (5,455 / 5,274): +4 / +3.
Cause not verified (the log lists no modules); likely the command index the prompt imports, which
gains fm and its import, since the manager's module itself loads only when fm is called.

## Fold of b22-red's BLOCK on 0bf9eb848, and two commits on top (2026-10-08)
Branch rebased onto origin/shell 3f71a8077 (6b's merge). Now three commits:
- 33fb5f7a8 the file manager (6c, amended): a key dropped while a question is deaf, with keys
  still queued behind it, re-arms deaf_until to now + 300 ms, so the question stays deaf until
  300 ms after the burst's last key. Test: F8, 5,000 keys each with the rest queued (sleeping
  4 ms every 50, so over 300 ms pass), then Enter: one.txt still there, the question open. Page
  and moduledoc sentences fixed.
- 566bca36c View.runs walks graphemes only to the window's right edge (roles by byte offset as
  it goes; the end-of-line cursor or selection cell only when the end was reached); View.column
  reads only the graphemes before the column; the editor counts a selected row's end by bytes.
  Test: an 80-column window of a 1 MiB highlighted line with cursor and selection, exact runs,
  under 100 ms; View.column(line, 10) under 100 ms.
- 061a7d43c the editor's guard question gets the same re-arm, its own commit (6a is merged).
  Test: Ctrl+S in a burst, 5,000 queued keys over 300 ms, Enter: the file unchanged, the
  question open.
Gates on 061a7d43c (base origin/shell 3f71a8077): full ./test-shell rc=0; docs rc=0; format
clean; prebuilt rc=0; beamlet-footprint PASS rv64 heap 5,459 / stack 35,880 B, rv32 heap 5,277 /
stack 28,920 B, the same as 6c before the fold (6b at 5,456 / 5,275).
