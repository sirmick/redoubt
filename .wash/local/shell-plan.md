# The Redoubt shell: the plan

The owner signed this off on 2026-09-29. It is a plan, not a page of the book, and the design it
settled is now booked (section 9): where the two disagree, the book holds. What stays here is the
order of the work, what is built on the host, and what is still to settle.

## 1. What the shell is

Booked in [the shell](../../docs/userland/shell.md): `Redoubt.Shell`, a loop of Redoubt's own over
Elixir, not IEx; plain Elixir at the prompt; commands as commandlets. The layers, bottom up:

```text
/dev/cons (Redoubt)  |  raw tty (host)
        | bytes, no echo
beamlet_io ............... byte pump embedded in the VM (a raw subscription)
        |
Redoubt.Shell.Driver ..... replaces user_drv and prim_tty; holds /dev/cons and the encoder
        | {data}   ^ {requests}
group + edlin ............ OTP, unchanged: editing, history, Ctrl+R, the expand_fun hook
        | the I/O protocol
Redoubt.Shell ............ the loop; replaces IEx.Server
```

Today the loop reads whole lines from the group leader with `IO.gets/1`; the driver, and
`group` and `edlin` under it, come with the terminal work of section 6.

## 2. The source tree

```text
shell                      the dev entry point: Redoubt.Shell on beamlet on this machine
toolchains/                the pinned OTP 28.5.0.6 and Elixir 1.20.4 (not tracked)
userland/
  otp/                     beamlet; gains a raw tty and its size and resize in the CLI, and a
                           raw subscription in beamlet_io
    screen/                * beamlet-screen: the screen buffer's natives (section 4)
  shell/                   a Mix project with no Hex dependencies
    lib/redoubt/shell.ex        the loop
    lib/redoubt/shell/          the driver, evaluator, printer, completer, helpers
    lib/redoubt/term/           * the encoder, the buffer's wrapper, key and paste decoding, width
    lib/redoubt/screen/         screen programs: the behaviour, layout, widgets, the editor
    lib/redoubt/util.ex         the text toolkit: Lines, grep, sub, head, tail, count, w
    lib/redoubt/commandlet/     commandlets: defcommand, typed parameters, the registry, help
    lib/redoubt/sys/            the platform seam: host.ex (Linux) and native.ex (Redoubt)
  native/                  a cargo workspace outside the root one, as userland/otp is
    cells/                 * the cell protocol
tools/beam-pack/           * later: the userland disk's image and system.index
```

The shell's Elixir reads the generated wire codecs from `libs/wire/elixir/` through
`elixirc_paths`, and never copies them.

A `*` marks code held to Tier A's review, because a hole in it reaches past one session: the
encoder and the cell protocol are the only path to a person's terminal, and the packer decides
what the userland disk holds. So are beamlet's platform boundary and its natives, the check of a
program's hash at launch, and the bench's runner for these tests. The rest is Tier B.

## 3. The host is a development aid

Until beamlet runs on Redoubt, the shell is developed on Linux, because it is faster there, and
nothing is built for the host alone. `Sys.Host` gives a strict subset of what `Sys.Native` will:
no `/bin/sh`, no environment variables, no signals, and files only under the VM's `--root`.

| Command | What it does |
| --- | --- |
| `./shell` | the shell on beamlet, on this terminal |
| `./shell --fake` | the shell on beamlet's Redoubt platform, over the client library, on the fake kernel: the console now, files and programs as they come |
| `./test-shell` | formatting, the tests on BEAM, the same tests on beamlet (a disagreement with BEAM is a beamlet bug), the shell's entry point, and the Redoubt platform on the fake kernel |
| `mix test`, in `userland/shell` | the tests on BEAM alone: the fastest loop |
| `cargo test`, in `userland/native` | the cell protocol |
| `cargo test -p beamlet-screen`, in `userland/otp` | the screen buffer's natives |

On the host: line editing, history, Ctrl+R, completion, the loop and the interrupt, the
commands over the sandbox's files, the pager, help, and the attack case that hostile text never
drives the terminal. Only on Redoubt: namespaces and `bind`,
labels, native pipes, jobs on budgets, resource use, the console's parked `resize` (the host uses
`SIGWINCH` until then), persistent history, and a vault session's rules.

## 4. Screens

The design is booked: [the shell](../../docs/userland/shell.md#full-screen-programs) (screen
programs, widgets, focus, the editor as an Erlang process) and
[beamlet](../../docs/userland/beamlet.md#screen-natives) (`beamlet-screen`'s natives and their
rules), with the reasons in each page's Why. In short: a native buffer and the diff between frames
in beamlet, primitives over cells and never widgets; everything above it Elixir; ratatui removed,
since a process per screen spends the scarcest thing on the box and its dependency tree is large
for the least demanding part of a TUI.

### The first slices

1. **The buffer.** `beamlet-screen` with `new`, `resize`, `put`, `fill`, `width` and `diff`,
   its Rust tests and vectors; `Redoubt.Term.Buffer`; tests that decode each frame into a model
   of the screen and judge the model. `table` draws in Elixir again, as text, with no screen.
2. **The encoder and one screen.** `Redoubt.Term.Encoder` (G2) with the hostile-text case;
   `Redoubt.Screen`, `Layout`, the box, the list and the status line; `pick(items)`, a
   `menuconfig`-style chooser, driven in tests by key events and judged on the model. On a live
   terminal it needs G1's raw tty.
3. **The widgets.** The menu bar, dialogs, the input, the completion pop-up, `plot` and the
   canvas, and the Esc timeout.
4. **The editor.**

## 5. On Redoubt

Three stores, split by whether they change:

| Store | Holds | Written | Kept whole by |
| --- | --- | --- | --- |
| the boot bundle | the kernel, `init`, the servers, beamlet with its embedded modules, `system.index` | never | its signature, checked at boot |
| the userland disk, a new read-only virtio disk | OTP, Elixir and Redoubt's own Elixir, one object per module named by its hash | never; the host enforces it | its consumer: each object is hashed and checked against `system.index` before use |
| the data disk | homes, `/system/pkgs/`, the steward's state, history | yes, littlefs through `fsd` | signatures on packages; capabilities on homes |

```text
/boot          bootfsd: programs and system.index, from the signed bundle
/system/lib    the userland disk: <sha256> objects, read-only
/system/pkgs   the data disk: packages
/home/<p>      the data disk: a principal's home
```

- **`load_module` on Redoubt** looks the module's hash up in `system.index`, reads
  `/system/lib/<hash>`, hashes what it read, and hands the loader only bytes that match. A
  mismatch fails loudly. A program is checked the same way before it is launched.
- **Objects, not archives.** Archives were only for `bootfsd`'s 64 entries and 8 MiB; one object
  per module needs no index inside the image, and lets two bundle slots share one disk.
- **Stripped and deterministic.** Measured, stripped: an idle prompt's 102 modules are 1.03 MiB;
  seven applications (577 modules) 3.75 MiB; with `crypto`, `public_key`, `asn1`, `ssl`, `ssh` and
  `syntax_tools`, 771 modules, 5.19 MiB (28.16 MiB unstripped). Deterministic compiles make a
  build on the box hash the same as the host's.
- **Served by `fsd`**, one instance on the read-only disk. The consumer checks every byte, so the
  file system in between is not trusted, and no new server is needed. (Superseded 2026-10-03 by
  VOL1: decision 5's amendment, section 7.)

This extends verified boot from privileged code to the system's userland: a new rule for the
book.

## 6. The phases

**1. On the host, now; none of it waits for M1.**
- **G0, the skeleton:** the Mix project, `./shell`, the tests on BEAM and on beamlet.
- **G1, the raw console:** the CLI puts the tty in raw mode and restores it on every exit, a
  panic included; `console_size` from `TIOCGWINSZ`; resize from `SIGWINCH`; the raw subscription
  in `beamlet_io`.
- **G2, the driver and `Redoubt.Term`:** `group`'s requests drawn through the encoder; tests
  judge a model of the screen; the hostile-text case passes.
- **G3, the loop:** reading, parsing, the evaluator, printing, and the interrupt, with what
  survives it written down.
- **G4, the helpers:** `Redoubt.Util` and the pager, over `File`.
- **G5, the commands:** commandlets, their registry and help; the first wave of commands;
  completion on `Code.Fragment`, of commands, and of paths inside a string.

Built so far: G0; the loop of G3 without the interrupt; and of G4 and G5 everything but the pager
and completion. The first wave of commands, 32 of them, and `table`:
- **Files:** `pwd`, `cd`, `ls`, `ls_r`, `find`, `glob`, `stat`, `cp`, `mv` (across volumes too),
  `rm`, `rm_rf`, `mkdir`, `mkdir_p`, `touch`.
- **Text:** `cat`, `grep`, `grep_v`, `head`, `tail`, `count`, `sort`, `uniq`, `uniq_c`, `sub`,
  `cut`, `w`, `append`, `hexdump`, `checksum`.
- **Shell:** `help`, with the topics `elixir` and `terminal`; `h`; `exit`.

Of the second wave, the cell protocol is built: `cells` in `userland/native`, and
`Redoubt.Term.Cells` in the shell, both held to one `vectors.json`. A first milestone drew with
ratatui in a `render` program behind a port; it was removed for the reasons of section 4, and
`table` draws in Elixir. beamlet keeps what it gained for it: host programs start from one
directory only (`--exec-only`).

Toward the third phase, beamlet has the first part of its Redoubt platform
(`userland/otp/redoubt`), over the client library: the console, the clock and randomness. It runs
on the host on the fake kernel (`./shell --fake`), against a console server that keeps
`consoled`'s protocol with this terminal for its device, and the shell runs on it. A console read
is made by a thread of its own, never the VM's, as every call that waits will be. Files and
programs come next, through the same library.

Left of the book's list, for the reasons given: `clear`, the pager and `out` need the terminal
(G2); `follow` needs the interrupt (G3); `now`, `today` and `ago` need wall-clock time, which
beamlet on Redoubt has from M5; `ns`, `bind`, `whoami`, `labels` and resource use need Redoubt.

**2. On the host, the second wave:** screens, in the slices of section 4: the buffer, the
encoder and one screen, the widgets, the editor; the pager and help drawn as screens; the first
self-hosting step, `mix compile` running on beamlet.

**3. On Redoubt**, once beamlet, `init` and `fsd` run there: `Sys.Native`, the packer, the
userland disk and `system.index`, the shell on `/dev/cons`, and the attack cases as bench cases.

**4. The rest of M2:** `bind` and namespaces, native pipes and jobs, resource use, the parked
`resize`, persistent history, the editor.

**Later:** compiling on the box (M4); rebuilding the system's beams on Redoubt to the same
hashes, which is also diverse double-compiling against BEAM; signing what the box built (M5).

## 7. Decided

The booked pages (section 9) hold the reasons; this is the list.


1. The shell is `Redoubt.Shell`, not IEx.
2. The source tree of section 2.
3. The host is a development aid, and `Sys.Host` a strict subset of Redoubt.
4. A control character in text is always drawn visibly; colour is a cell attribute that only
   trusted code sets.
5. A separate read-only userland disk, bound to the signed bundle by `system.index` and checked
   object by object by whoever uses it. **Amended by the owner, 2026-10-03 (VOL1):**
   - The disk becomes a verified volume: `verityd` checks every block against a hash tree whose
     root the signed manifest pins.
   - `system.index` and the per-object check go once BEAM2 has landed with them.
   - Readers trust the servers that verify their volume, so "the file system in between is not
     trusted" no longer holds.
   - Brief: `VOL1-implementer.md`.
6. Tier B for the shell and the native libraries, and Tier A for the parts marked `*` and those
   named with them in section 2.
7. The prompt is plain Elixir: no command mode, no other syntax. Each command is a commandlet,
   declared once with `defcommand`, typed, and with its help.
8. Commands are tested against real files: a seeded tree copied into each test's own directory,
   and no stand-in for `File`, which is itself the platform's seam. A fault that no real file
   system gives on the host (a full disk, an I/O error) is injected below `File`, in beamlet's
   `Files`, when it is needed.
9. This work enters the plan graph as its own package, without waiting for M1. The node is
   Wash's to write: `plan.toml` is never edited by hand.
10. `./iex` goes, now that `./shell` exists. IEx itself stays where the differential test needs
    it, `userland/otp/tests/elixir/iex_test.ex`, which `tools/difftest` runs; what went is the
    entry point at the top of the tree and its section in `GETTING-STARTED.md`.
11. Screens are drawn as section 4 says: a native buffer in beamlet (`beamlet-screen`), and
    widgets, layout and screen programs in Elixir, each screen an Erlang process; the editor is
    one. ratatui, `backend` and `render` are removed, and nothing third-party enters the VM.
    Natives are primitives over cells and never widgets.

## 8. Open

For the owner: whether beamlet, on a platform with no wall clock, counts system time from the
Unix epoch at boot, as it now does, so that OTP's logger and anything else that stamps a time
works, and a date says 1970 until M5. (Whether `userland/native` vendors a crate at first use or
at image time no longer arises: with ratatui gone it has no third-party crate.)

To settle in the work: bracketed paste (OTP has none, and `edlin` takes a pasted Tab as
completion); the escape sequences `group.erl` hardcodes in the Ctrl+R prompt; the key a
full-screen program leaves to the session.

## 9. The pages it changed

Booked on 2026-09-30, on the owner's word:
- [the shell](../../docs/userland/shell.md): `Redoubt.Shell`, not IEx; commandlets, and no command
  mode; the loop, the commands and help as built on the host; screens and the editor in Elixir;
  the Why.
- [beamlet](../../docs/userland/beamlet.md): the three kinds of native and the bar every native
  meets; the screen natives; one X25519 and Ed25519 for the box; compression; the split of I/O
  threads; why its dependencies are userland's.
- [native programs](../../docs/userland/native.md): the client library's dropped files, error
  names, calls by path, whole reads and writes, and generated calls; no TUI library.
- [wire](../../docs/servers/wire.md): error names, and generated clients.
- [M1](../../docs/plan/m1-separation.md), [M2](../../docs/plan/m2-usable-shell.md), the glossary,
  the userland pages' prompts and examples.

Still to book, when that work starts: `load_module`'s row and the userland disk
([packages](../../docs/userland/packages.md), [bootfsd](../../docs/servers/bootfsd.md), and a new
rule with its row in [the security register](../../docs/SECURITY.md)).
