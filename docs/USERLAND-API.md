# Userland API (draft)

The Elixir surface of the Redoubt operating system. `USERLAND.md` owns the
principles (the boundary, natives, pipes, launching); this note owns the
concrete module and function inventory. `OS-API.md` owns the Rust side.

Status: **draft** — not frozen. Milestone 1 needs only the console/files/launch
slice; the full inventory is a milestone 3 target (PLAN.md).

## The rule of one layer

Every operation that does not need its own address space is **pure Elixir.** No
`cp` binary, no `mv` binary, no separate `ls` process. The only native binaries
are system servers (TCB), drivers, and user programs that *must* run in their
own budget for containment or isolation. Everything else is a function call in
the IEx VM.

| What | Form |
|------|------|
| `cp a b` | `File.cp!("a", "b")` — pure Elixir |
| `grep x f` | `Enum.filter(lines, &String.contains?(&1, "x"))` — pure Elixir, or native pipe if untrusted |
| `cat a \| grep x \| wc -l` | `cat("a") \|> grep("x") \|> count()` — pure Elixir; `pipe(~w(...))` when a stage must be native |
| `ed file.txt` | `Redoubt.Ed.open("file.txt")` — pure Elixir TUI in IEx |
| Agent harness | `Redoubt.Agent.start(...)` orchestrates a separate beamlet VM |

## Three syntax layers

| Context | Syntax | Use |
|---------|--------|-----|
| **Programmatic** (`.ex`, scripts) | Full Elixir, verbose, explicit paths and handles | Code that outlives a shell session |
| **IEx helpers** (`Redoubt.Shell`) | Short names, default context (namespace, budget, console) | Interactive exploration |
| **IEx command mode** | Bare words auto-quoted, `\|>` chains stages, `\|` maps to native pipe | Quick one-liners, Unix-short |

The last two live only inside an IEx session. They are syntactic sugar, not a
different language.

---

## Module reference

### `Redoubt.Shell` — interactive context

A shell session holds a process-local context: the default namespace, the
default budget (a sub-budget carved from the principal's), and the console
connection.

```elixir
import Redoubt.Shell  # available by default in a Redoubt IEx session

# Set context (rarely needed; inherited from login session)
ns(work_conn)
budget(my_budget)
console(uart_conn)

# Show state
ns()         # show namespace prefix table
budget()     # show current budget usage
env()        # argv + namespace + handles map
```

All helper functions below (`cat`, `ls`, `cp`, etc.) resolve relative paths
against the session namespace and charge work to the session budget.

| Function | Returns | Notes |
|----------|---------|-------|
| `cat(path \| [path])` | `%Lines{}` | Lazy lines; prints (paged) when it is the value at the prompt |
| `cp(src, dst)` | `:ok` | Copy; within one volume is server-side, across volumes is client loop |
| `mv(src, dst)` | `:ok` | Rename; across volumes is copy+remove, not atomic |
| `rm(path)` | `:ok` | Remove; open files succeed (no "in use" channel) |
| `rm_rf(path)` | `:ok` | Recursive remove |
| `mkdir(path)` | `:ok` | Create directory |
| `mkdir_p(path)` | `:ok` | Recursive create |
| `ls(path \\ ".")` | `[String]` | List directory entries |
| `ls_r(path)` | `Stream.t(String)` | Recursive list |
| `find(path, pattern)` | `Stream.t(String)` | Glob + pattern match |
| `touch(path)` | `:ok` | Create empty or bump mtime |
| `stat(path)` | `File.Stat.t` | Name, length, mtime, qid; no mode/owner |
| `top()` | `String` | Budgets, weights, usage, processes |
| `ps()` | `String` | Process list |
| `df()` | `String` | Page and process usage per budget |
| `free()` | `String` | Available pages in current budget |
| `uptime()` | `String` | Time since boot |
| `whoami()` | `String` | Principal name and label set |
| `labels()` | `[atom]` | Current label set |
| `cd(prefix)` | `:ok` | Rebind local default prefix (does not change kernel working dir; there is none) |
| `pwd()` | `String` | Show current effective prefix |
| `clear()` | `:ok` | ANSI clear-screen to console |

### `Redoubt.File` — thin wrappers around OTP `File`

Once beamlet's `Platform` trait wires `prim_file` over 9P, OTP `File` works
unchanged. These wrappers exist only where Redoubt adds semantics (9P typed
operations, cross-volume copy, label handling).

```elixir
File.read!("data.txt")                # works via prim_file -> 9P -> fsd
File.write!("data.txt", "hello")      # same
File.cp!("a", "b")                    # cross-volume ok
```

| Function | Returns | Notes |
|----------|---------|-------|
| `copy_file(src, dst)` | `:ok \| {:error, :refused}` | Server-side copy within one volume; fails across volumes |
| `rename(old_dir_fid, old_name, new_dir_fid, new_name)` | `:ok \| {:error, atom}` | Typed `fsd` rename |
| `set_attr(fid, attr, value)` | `:ok \| {:error, atom}` | Custom per-file metadata |
| `get_attr(fid, attr)` | `{:ok, binary} \| {:error, atom}` | Read custom attribute |

All other file operations (`mkdir`, `rm`, `stat`, `ls`, `stream!`, `exists?`,
`dir?`) use OTP `File` directly.

### `Redoubt.Path` — path manipulation

No `Path.cwd/0` — there is no working directory. Relative paths resolve against
the namespace table's longest prefix match.

```elixir
Path.join("a", "b")          # works
Path.expand("../x")          # lexical clean only; never leaves root
Path.relative?("a/b")        # true if no leading /
```

### `Redoubt.Cmd` — native process pipelines

Native binaries are launched in their own budgets, connected by pipe files the
shell VM serves. `Cmd` wraps the single launch primitive; the
`Redoubt.Process.start/3` shape sketched in USERLAND.md is the same operation
at a lower level (budget, exit endpoint, startup block), and this note owns the
Elixir surface for it. The `Cmd` API is struct-based; IEx command mode provides sugar.

```elixir
import Redoubt.Cmd

# Explicit pipeline
cmd = Cmd.new()
|> source("log.txt")
|> pipe({"grep", ["error"]})
|> pipe({"wc", ["-l"]})

{:ok, [job]} = Cmd.run(cmd)
result = Job.await(job)
IO.puts(result.stdout)

# Short form: a pipe is lines of its stdout
pipe(~w(grep error log.txt | wc -l)) |> out()
```

| Function | Returns | Notes |
|----------|---------|-------|
| `Cmd.new(opts \\ [])` | `%Cmd{}` | `namespace:`, `budget:`, `stdout:`, `stderr:` |
| `Cmd.source(cmd, path)` | `%Cmd{}` | File path or `{:stream, Stream.t}` |
| `Cmd.pipe(cmd, {bin, args})` | `%Cmd{}` | Append a native stage |
| `Cmd.into(cmd, path)` | `%Cmd{}` | Sink to file |
| `Cmd.run(cmd)` | `{:ok, [Job.t]} \| {:error, term}` | Launch all stages simultaneously |
| `Job.await(job, timeout: :infinity)` | `%Result{exit: code, stdout: str}` | |
| `Job.kill(job)` | `:ok` | Destroy the stage's budget |
| `Job.status(job)` | `:running \| :exited \| :faulted \| :killed` | |

`Cmd.run/1` creates one sub-budget per stage and one 9P pipe file between each
pair. The shell VM serves the pipes; when a stage exits, its stdout pipe is
disconnected, the next stage reads EOF, and so on.

### The console and the `Platform` contract

This note owns the **Redoubt side** of beamlet's `Platform` trait: what each console method must
answer here, and which `Redoubt.*` module wraps it. `userland/otp/DESIGN.md` owns the trait itself.

- **`console_write`** writes output bytes to the VM's `/dev/cons` connection (a 9P write). No
  ANSI interpretation here: the user's terminal emulator does that.
- **`console_read`** is non-blocking and returns `ConsoleInput` (`Nothing`, `Data`, `Eof`); a read of
  the `/dev/cons` connection with nothing to read **parks** (NAMESPACES.md, Holding a call), so the
  VM is never blocked and `Eof` means the connection ended, not "no input yet".
- **`console_size`** returns `Some((cols, rows))` when the platform knows a size and `None` when it
  does not; **the trait default is `None`**, so a platform that says nothing is honest rather than
  silently claiming 80×24 (answer 162). On Redoubt it asks the `/dev/cons` connection for the
  `consol` `size` call (opcode 16, NAMESPACES.md, The console). A server that
  does not serve `consol` refuses the opcode as `Malformed`, and the platform answers `None`.
  `Redoubt.Console.size/0` reports that as `{:error, :unknown}`.
  **The size is asked afresh on every `size/0` call, not cached across one.** This is the accepted
  Redoubt contract; a trait/source comment suggesting a cache does not override it. The only console whose
  size changes is an SSH channel, and the only thing that tells a client it changed is the `resize`
  call (below, and not buildable until question 163) — so a cache no rule invalidates would answer a
  redraw with the size before the change. A caller that needs the current size calls `size/0`; a
  caller that wants to be told subscribes with `await_resize` once it exists.
- **There is a resize channel in milestone 1, and it is a parked call** (answer 160). A server
  pushes an unprompted event by holding a call the client made and answering it when the event
  happens (NAMESPACES.md, Holding a call): the client calls `consol`'s opcode 17 `resize`, the server
  parks it, and answers with the new `cols, rows` when the window changes. There is no callback and
  no endpoint — the client asks, and the server replies when it has news. `Redoubt.Console` exposes
  it as a **message**, `await_resize/1` (below), because this is a message-passing VM: the caller is
  re-called and the change arrives as a message, not as a function invoked inside the VM's I/O path.
  **It depends on question 163** (a parked *typed* call needs the typed dispatch to hand a request
  back, a `libs/rt` extension), so WP-B2a builds `size` first and `resize` when 163 lands. On a UART
  nothing resizes, so a parked `resize` waits for ever (NAMESPACES.md, The console).
- **`libvterm/`** (an untracked C tree at the repository root) is **reference only**: its terminal
  state machine and key tables are read for the Elixir decoders below, never built and never linked
  (TENETS.md 3 — no C in the build). It is a reading source, like the littlefs C reference for
  `libs/littlefs`.

### `Redoubt.Console` — ANSI terminal client

Pure Elixir. Emits ANSI escape sequences to `/dev/cons` and interprets key sequences from it.
No cell grid is maintained here; the user's terminal emulator does that.

| Function | Returns | Notes |
|----------|---------|-------|
| `size()` | `{cols, rows} | {:error, :unknown}` | Asks `/dev/cons` for the `consol` `size` call (opcode 16) on every call, so a redraw after a resize is not answered from a stale cache; `{:error, :unknown}` when the server does not serve it |
| `clear()` | `:ok` | Full clear + home cursor |
| `move_to(col, row)` | `:ok` | 0-based |
| `write(data)` | `:ok` | Raw bytes to console; `IO.write` equivalent |
| `await_resize(pid)` | `{:ok, ref}` | Calls `consol`'s `resize` (opcode 17) with a call that parks, and returns; the new size is delivered to `pid` as `{:console_resize, cols, rows}` when the server answers. It re-calls `resize` after each reply. **In milestone 1 by answer 160, not implemented yet**: question 163/WP-R1d supplies typed parking. `consoled` must park it indefinitely on UART; `sshd` answers when that channel's window changes. |

Not in milestone 1: alt-screen, cursor-visibility and colour/SGR helpers. They are one `write/1` of an escape sequence away and nothing in the design consumes them; add them when a program needs them. (The `consol` `size` call and `clear`/`move_to`/`write` are what `Redoubt.Ed` needs.)

### `Redoubt.Console.Key` — keyboard decoder

State machine fed by raw bytes read from `/dev/cons`. Returns structured key events.
Matches VT100/xterm/Linux function-key sequences.

| Function | Returns | Notes |
|----------|---------|-------|
| `new()` | `%KeyDecoder{}` | Empty decoder state |
| `feed(decoder, byte)` | `{decoder, [event]}` | Events: `:up`, `:down`, `:left`, `:right`, `:home`, `:end`, `:page_up`, `:page_down`, `{:f, n}`, `{:ctrl, char}`, `{:alt, char}`, plain `char` |

### `Redoubt.Ed` — in-VM text editor

A TUI editor written in Elixir, running as a GenServer inside IEx. Uses ANSI
escape sequences on `/dev/cons`.

```elixir
Redoubt.Ed.open("config.txt")
# Interactive: hjkl movement, i=insert, :w=save, :q=quit
# No separate process; the editor holds buffer state in memory.
```

| Function | Returns | Notes |
|----------|---------|-------|
| `Ed.open(path)` | `:ok` | Opens file in current IEx session (blocks) |
| `Ed.new()` | `:ok` | New untitled buffer |
| `Ed.save()` | `:ok` | Write to last path |
| `Ed.quit()` | `:ok` | Close editor, return to IEx |
| `Ed.goto(line)` | `:ok` | Jump to line |
| `Ed.find(text)` | `:ok` | Search forward |
| `Ed.replace(old, new)` | `integer` | Replace count |

Because it is pure Elixir, `Ed` can be scripted:

```elixir
Ed.open("log.txt")
Ed.goto(42)
Ed.replace("foo", "bar")
Ed.save()
Ed.quit()
```

### `Redoubt.Agent` — agent harness

Orchestrates a beamlet VM (the agent) in its own process, budget, and label set.
The harness itself runs in the parent shell VM; the agent code runs in the child.

```elixir
{:ok, agent} = Redoubt.Agent.start(
  model: {:via, gatewayd_conn},   # or direct endpoint
  lease: [pages: 1_000_000, time: :timer.hours(2)],
  labels: [:alice_work],
  workspace: [{"/work", work_conn}],
  tools: [filesystem, http, code_exec]
)

Agent.prompt(agent, "Audit the /work/src directory for security issues")
Agent.pause(agent)    # freeze the lease budget
Agent.resume(agent)
Agent.kill(agent)     # destroy lease budget, ends everything
```

| Function | Returns | Notes |
|----------|---------|-------|
| `Agent.start(opts)` | `{:ok, Agent.t} \| {:error, term}` | `model`, `lease`, `labels`, `workspace`, `tools` |
| `Agent.prompt(agent, text)` | `{:ok, response} \| {:error, :lease_expired}` | Submit to model, await reply |
| `Agent.status(agent)` | `%{cpu: weight, pages: usage, lease_ms: remaining}` | |
| `Agent.pause(agent)` | `:ok` | Freeze budget (if supported; else no-op) |
| `Agent.resume(agent)` | `:ok` | Unfreeze |
| `Agent.kill(agent)` | `:ok` | Destroy lease budget |

Tools are capabilities given to the agent: a filesystem connection (read-only or
read-write), an HTTP gateway connection scoped to specific hosts, or a code
execution sandbox (another sub-budget the agent may launch processes in).

### `Redoubt.Net` — network

OTP `gen_tcp` and `gen_udp` work unchanged once the `Platform` trait is wired.
These helpers exist for Redoubt-specific semantics.

```elixir
{:ok, sock} = :gen_tcp.connect('10.0.0.1', 443, [])
:gen_tcp.send(sock, data)
```

| Function | Returns | Notes |
|----------|---------|-------|
| `Net.allowed_hosts()` | `[String]` | Read from current network scope |
| `Net.scope()` | `String` | IP prefixes and ports this session may reach |
| `Net.http_get(url)` | `{:ok, body} \| {:error, term}` | Simple client; DNS is client-side |

### `Redoubt.Sys` — system introspection

```elixir
Sys.budget_usage()        # %Usage{pages: {limit, used}, ...}
Sys.time_now()            # monotonic microseconds
Sys.random(bytes)         # fills from kernel CSPRNG
Sys.sleep(ms)             # blocking sleep
```

### `Redoubt.Label` — information flow

```elixir
Label.self()              # current label set (read-only)
Label.can_read?(labels)   # self_labels ⊇ labels ?
Label.can_write?(labels)  # self_labels == labels ?
```

### `Redoubt.Budget` — resource containers

```elixir
Budget.self()             # handle to current budget
Budget.create_child(spec) # carve sub-budget
Budget.destroy(budget)    # kills everything inside
Budget.usage(budget)      # %Usage{}
```

### `Redoubt.Steward` — identity and approval

```elixir
Steward.login(principal, key_handle)
Steward.approve(request_id)   # out-of-band approval session
Steward.vault(label)          # start a vault session
```

### `Redoubt.Keyd` — key operations

```elixir
Keyd.sign(data, purpose, key_handle)
Keyd.holds(public_key)        # does keyd have this key?
```

## Utilities (`Redoubt.Util`)

Pure Elixir, imported in every session and `run` script (`Util.grep/2` when
not). Files come in through `cat` and go out through `w`; everything between
takes lines first and chains with `|>`.

`cat` returns `%Lines{}`: enumerable and lazy, read in 9P-sized blocks when
consumed, and closed when a consumer stops early (`head`). It checks the file
exists when called, so a missing file fails on its own line. Its `Inspect`
prints the lines, paged, so bare `cat("f")` at the prompt shows the file.
`grep`, `sub`, `head` and the rest return `%Lines{}` too. A `pipe` is lines of
its stdout.

```elixir
cat(path | [path])          # lazy lines; a list concatenates
follow(path)                # tail -f; Ctrl+G stops
w(src, path)                # temp file + rename: cat(f) |> ... |> w(f) is safe
append(src, path)
grep(src, pat) / grep_v(src, pat)   # pat: string or ~r//
sub(src, pat, rep)          # sed s/pat/rep/g
cut(src, sep, n)            # field n, 1-based
sort(src) / uniq(src) / uniq_c(src) # uniq_c: {count, line}, most first
head(src, n) / tail(src, n)
count(src)                  # wc -l
out(src)                    # print unpaged
glob(pat)                   # ["logs/a.log", ...]
now()                       # wall time string
checksum(path) / hexdump(binary)
```

## IEx command mode expansion

A preprocessor ahead of the Elixir parser. Bare words are quoted; `|>` is an
Elixir stage, `|` a native one, `>` is `w`, `>>` is `append`.

| Typed | Expanded |
|-------|----------|
| `cat a` | `cat("a")` |
| `cp a b` | `cp("a", "b")` |
| `grep x f` | `cat("f") \|> grep("x")` |
| `cat a \|> grep x \|> count` | `cat("a") \|> grep("x") \|> count()` |
| `cat a \|> sub x y > a` | `cat("a") \|> sub("x", "y") \|> w("a")` |
| `zcat a.gz \| sort > b` | `pipe(~w(zcat a.gz \| sort)) \|> w("b")` |
| `cat a \| parse --json \|> grep w` | `cat("a") \|> pipe(~w(parse --json)) \|> grep("w")` |
| `ls` / `top` / `cd d` / `ed f` | `ls(".")` / `top()` / `cd("d")` / `ed("f")` |

A command-mode `cat` is always the Elixir one; no native `cat` is launched.

## The interactive shell: terminal, line editing, completion, help

Proposed, not milestone 1 (WP-B2c). The starting fact: **nothing between the
keyboard and IEx edits a line.** `consoled` and `sshd` serve `/dev/cons` as a
raw byte pipe with no echo, and `beamlet_io` buffers input to a newline and
reports `echo: false`. On a host the terminal's cooked mode hides this; on
Redoubt nobody echoes a keystroke. The shell owns line editing, and completion
comes with it.

Four layers, bottom up.

### `Redoubt.Term` — terminal library

Grows `Redoubt.Console` and `Redoubt.Console.Key` into the one terminal library
every TUI uses (`Ed`, `top`, the pager, the line editor). Pure Elixir.

- **Output:** cursor movement and save/restore, erase line and screen, scroll
  regions, alt screen, cursor visibility, SGR colour and attributes (on top of
  Elixir's `IO.ANSI`), bracketed paste, synchronized update (mode 2026) so a
  redraw does not flicker.
- **Input:** the `Key` decoder extended with CSI modifier forms
  (`ESC [1;5C` is Ctrl+Right), SS3, UTF-8 assembly, bracketed-paste framing
  (a paste is one event, never a run of keys that could trigger completion) and
  optionally SGR mouse.
- **Width:** grapheme and East Asian width, so cursor arithmetic holds for CJK
  and emoji.
- **One target, no terminfo:** VT102 plus the common xterm extensions every
  current emulator speaks. `libvterm/` stays reference only.
- **Widgets:** a pager (`less`-shaped: space, `b`, `/search`, `q`), column
  layout for completion lists, later a picker. Owl (Apache-2.0) is the borrow
  target.
- Layout uses `Console.size/0`; on `{:error, :unknown}` it assumes 80 columns
  and says nothing.

### The line editor: OTP's `edlin` inside `beamlet_io`

On BEAM, `group` and `edlin` do line editing and are plain Erlang; only
`prim_tty` underneath needs OS tty support. IEx hands its completer to the I/O
server with `:io.setopts(expand_fun: ...)`. So `beamlet_io` gains `edlin` and a
small tty backend that writes through `Redoubt.Term` to `/dev/cons`, and
honours `expand_fun`. That gives:

- Emacs keys, kill ring, multi-line input, history and Ctrl+R search.
- **IEx's own Elixir completion (`IEx.Autocomplete`) unchanged:** modules,
  functions, variables, map keys.
- The job-control gap USERLAND.md (The shell) names: `edlin`'s Ctrl+G
  interrupts a runaway expression without killing the session.
- History persists per principal in the principal's home volume, capped in
  lines. A vault session keeps history in memory only: its labels forbid
  writing down to the unlabelled home volume, so there is nowhere to save it.

Echo is the editor's job, so a password prompt is a call that reads with echo
off (`Redoubt.Term.read_secret/1`), not a terminal mode.

### Completion

`Redoubt.Shell` installs an `expand_fun` that inspects the line before
deferring:

| Line so far | Completes from |
|-------------|----------------|
| `cp no⇥` (command mode, first word done) | the argument type declared for that command: path, package, principal, budget, label |
| `c⇥` (command mode, first word) | the command registry |
| anything else | `IEx.Autocomplete` |

- **Paths** resolve through the session namespace and read the directory
  over 9P, one read per Tab. Labels already filter directory reads (`fsd`
  lists only readable entries), so completion cannot reveal a name the session
  could not `ls`.
- **Behaviour:** first Tab inserts the longest common prefix; a second Tab
  lists the candidates in columns through the pager when they exceed a
  screen. Directories complete with a trailing `/`.
- A completer never launches a process and never writes; a slow server
  bounds it by a short timeout, after which Tab does nothing.

### Help: one registry drives everything

Every command-mode command is declared once:

```elixir
defcommand :cp,
  args: [src: :path, dst: :path],
  summary: "Copy a file",
  doc: """
  Copies SRC to DST. Within one volume the copy is server-side;
  across volumes it is a client loop.
  """,
  examples: ["cp notes.txt backup.txt", "cp /work/a.txt /home/b.txt"]
```

From that one declaration come the command-mode expansion, completion's
argument types, and help:

```elixir
help            # commands grouped by area, one line each
help cp         # full page in the pager: usage, doc, examples
help namespaces # a concept topic: namespaces, budgets, labels, pipes, sessions
h File.cp/2     # IEx's own module docs (needs Docs chunks, see Open decisions)
```

A test fails the build for any command without a summary, a doc or an
example. Concept topics are short, shell-oriented summaries of the
`docs/*.md` specifications, bundled as `.md` and rendered with SGR bold and
indentation.

### Open decisions

To be numbered in QUESTIONS.md once in-flight branches have settled their IDs.

1. **Docs chunks in the boot bundle.** `h/1` needs them in each `.beam`; they
   add substantially to the stdlib's size (to be measured). *Rec:* strip from the boot bundle,
   ship them as an optional docs package that `h/1` reads when present.
2. **Terminal discovery.** Assume the VT102-plus-xterm target, or send DA1
   (`ESC [c`) at session start and adapt? *Rec:* assume; a query on a UART
   that never answers costs a timeout on every login.
3. **Editor placement.** `edlin` in `beamlet_io` (above) or a fresh editor in
   Elixir. *Rec:* `edlin`; it is what IEx is tested against, and it keeps IEx
   completion for free.
4. **Running a script.** Command mode `run tool.exs a b` → `Code.require_file`
   with `System.argv/0` set, in the session VM. Running one in its own budget
   and labels is a child VM (as `Redoubt.Agent`). *Rec:* both, `run` for the
   first and `run --isolated` for the second.

## Scripting: bash equivalents

```elixir
# cat app.log
cat("app.log")
# grep -c error app.log
cat("app.log") |> grep("error") |> count()
# grep -Ei 'timeout|refused' app.log | tail -20
cat("app.log") |> grep(~r/timeout|refused/i) |> tail(20)
# head -5 big.log                      (reads one block)
cat("big.log") |> head(5)
# sed -i s/staging/prod/g config.txt
cat("config.txt") |> sub("staging", "prod") |> w("config.txt")
# grep -v '^#' app.conf > clean.conf
cat("app.conf") |> grep_v(~r/^#/) |> w("clean.conf")
# cut -d' ' -f1 access.log | sort | uniq -c | sort -rn | head
cat("access.log") |> cut(" ", 1) |> uniq_c() |> head(10)
# cat a.log b.log | grep error
cat(["a.log", "b.log"]) |> grep("error")
# grep error logs/*.log
glob("logs/*.log") |> cat() |> grep("error")
# for f in logs/*.log; do echo "$f $(grep -c error $f)"; done
for f <- glob("logs/*.log"), do: {f, cat(f) |> grep("error") |> count()}
# for n in alice bob; do echo "hi $n" > hi-$n.txt; done
for n <- ~w(alice bob), do: w(["hi #{n}"], "hi-#{n}.txt")
# echo "done $(date)" >> run.log
append(["done #{now()}"], "run.log")
# [ $(grep -c error app.log) -gt 0 ] && echo bad
if cat("app.log") |> grep("error") |> count() > 0, do: IO.puts("bad")
# untrusted parser as a native stage, in its own budget
cat("dump.bin") |> pipe(~w(parse --json)) |> grep("warn")
# all native
pipe(~w(zcat big.gz | sort | uniq)) |> w("uniq.txt")
# tail -f app.log | grep error
follow("app.log") |> grep("error") |> out()
# ./long-task & ... wait $!; echo $?
job = pipe(~w(long-task)) |> run();  Job.await(job).exit
```

Command mode:

```
cat app.log |> grep error |> count
cat config.txt |> sub staging prod > config.txt
zcat big.gz | sort | uniq > uniq.txt
```

A script is plain Elixir; its inputs are `System.argv/0` and its namespace.

```elixir
# count.exs — run count.exs error a.log b.log
[pat | files] = System.argv()
for f <- files, do: IO.puts("#{f} #{cat(f) |> grep(pat) |> count()}")
```

## Native binary inventory

Binary | Role | Isolated?
-------|------|----------
`init` | Boot orchestrator | Yes — system class
`steward` | Identity, sessions, powerbox | Yes — system class
`keyd` | Key storage | Yes — system class
`sshd` | SSH front door | Yes — system class
`consoled` | UART console driver | Yes — system class
`bootfsd` | Read-only boot bundle server | Yes — system class
`blkd` | Virtio block driver | Yes — system class
`fsd` | Littlefs filesystem server | Yes — one per volume
`netd` | Virtio net driver | Yes — system class
`ipd` | IP stack | Yes — one per network
`loader-stub` | ELF parser in every native process | No — runs in caller's budget
`/bin/ed` | Optional native line editor | Yes — if needed
`/bin/grep` | Optional, if processing untrusted data | Yes — own budget
`/bin/agent` | Agent runtime VM (beamlet) | Yes — lease budget

Everything else is **Elixir code** loaded from `.beam` files or typed into IEx.

## Reading order

| Before this | After this |
|-------------|------------|
| `USERLAND.md` | `OS-API.md` (Rust side) |
| `NAMESPACES.md` | Implementation work packages |
| `CAPABILITIES.md` | |
| `INIT.md` (startup block) | |
| `PACKAGES.md` (launching) | |
