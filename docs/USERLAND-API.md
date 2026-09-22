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
| `cat a \| grep x \| wc -l` | `pipe([{"cat",["a"]}, {"grep",["x"]}, {"wc",["-l"]}])` — native processes wired by the shell VM |
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
| `cat(path)` | `String` | Read entire file, print to console |
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

# Short form (IEx only)
pipe(~w(cat log.txt | grep error | wc -l))
|> read
|> IO.puts
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
  **The size is asked afresh on every `size/0` call, not cached across one.** The only console whose
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
| `await_resize(pid)` | `{:ok, ref}` | Calls `consol`'s `resize` (opcode 17) with a call that parks, and returns; the new size is delivered to `pid` as `{:console_resize, cols, rows}` when the server answers. It re-calls `resize` after each reply, so a process that keeps handling the message keeps hearing about changes. **Not in milestone 1** (question 163: a parked *typed* call is not buildable yet; WP-R1d adds it), and it is `sshd`'s obligation, not `consoled`'s (nothing resizes over UART). |

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

Pure Elixir stream functions. No native processes needed.

```elixir
grep(pattern, stream)       # Stream.filter
wc(stream)                  # lines / words / bytes
count_lines(stream)
uniq(stream)
sort(stream)
head(n, stream)
tail(n, stream)
hexdump(binary)
checksum(path)              # SHA-256
```

## IEx command mode expansion

In a Redoubt IEx session, a preprocessor intercepts input before the Elixir
parser:

| Typed | Expanded |
|-------|----------|
| `cat a` | `cat("a")` |
| `cp a b` | `cp("a", "b")` |
| `grep x f` | `grep("x", "f")` |
| `cat a \|> grep x` | `pipe([{"cat",["a"]}, {"grep",["x"]}]) \|> read()` |
| `cat a \|> grep x \|> wc -l` | `pipe([{"cat",["a"]}, {"grep",["x"]}, {"wc",["-l"]}]) \|> read()` |
| `cat a \|> out` | `pipe([{"cat",["a"]}]) \|> into("out") \|> run()` |
| `ed f` | `ed("f")` |
| `ls` | `ls(".")` |
| `top` | `top()` |
| `cd d` | `cd("d")` |

The `\|>` operator is overloaded in command mode: between bare commands it
means "native pipeline." In programmatic mode it is ordinary Elixir pipe.

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
