# BEAM4: the natives: namespace, calls, serving, budgets, labels and launching; the shell launches a native program

Tier A (the VM's system natives on Redoubt, the generated Elixir clients, the shell's launch),
size M+. Needs BEAM3 (in review at a8fb9d349 on wp-BEAM3: the hub, `Files`, the error-name
table): start from `main` once BEAM3 is on it. BEAM2, AIO1, BEAM7 are in. Run everything through
`q run` (`.wash/local/RESUME-q.md`).

## Context rules (read these first)

- **Don't read whole files** but the ones named below. `userland/otp/redoubt/src/io.rs` (BEAM3's
  hub owner: `connect`, `request`, `completed`, `dispatch`, `MAX_WAITERS`) and `platform.rs`'s
  `Files` trio (`asker`/`finished`/`abandon`, `FileError::Later`, `Ctx::await_io`) whole; the
  interpreter and `vm.rs` by function. `libs/client/src/{launch,ns,typed,grants}.rs` by their
  public signatures.
- **The pages are the spec:** beamlet.md "Natives" (the table of seven natives and the handle
  rule), native.md "Launching from a session", "Standard input and output, and pipes", "Killing
  a job", "Many requests at once", wire.md "Generated clients", sessions.md "Namespaces and
  binds", shell.md "The shell in a session" (what the M1 shell launches), files.md for the
  error mapping at the `File` boundary (BEAM3's table: the natives' errors are Redoubt names,
  never POSIX).
- **Don't open `.wash/qa/*.md` or other packages' reports** but BEAM3's "The surface BEAM4 and
  BEAM5 build on" and "Departures and residuals".
- **Reports under 1,900 bytes,** detail in `.wash/local/BEAM4-report.md`.

## Reading list (only these)

- `docs/userland/beamlet.md` "Natives" and "The `Platform` boundary"; `docs/userland/native.md`
  "Launching from a session", "Standard input and output, and pipes", "Killing a job", "The
  client library", "Many requests at once", "Dropped files, calls by path and generated calls",
  "An `Rerror` has a name"; `docs/servers/wire.md` "Generated clients" and the generator's
  description; `docs/userland/sessions.md`; `docs/userland/shell.md` "The shell in a session";
  `docs/kernel/budgets.md` "The calls" (what `budget_create/destroy/usage` carry); `docs/kernel/
  objects.md` "Mint" (handles, revocation scopes); `docs/servers/serving.md` "Multiplexed
  connections" and R77 (what `serve` must honour as a server).
- `.wash/local/BEAM3-report.md`, the two sections named.

## The design (the page rules; this is how to build it)

1. **The seven natives**, in `userland/otp/redoubt` behind the VM's `Platform` (a new
   `Platform::system() -> Option<&mut dyn System>` beside `files()` and `programs()`, with the
   host `fake-redoubt` implementing it so the host stays a strict subset), each as the page's
   table says and no more:
   - `ns_lookup/1`, `bind/2`, `ns/0`: the client library's `ns` (`lookup` gives the longest
     matching prefix's connection and the rest of the path; `bind` adds `PREFIX=HANDLE`; `ns`
     prints the table as sessions.md shows it). A bind of a path under an existing prefix
     shadows it for longer matches only, as the library does; a handle bound must be a
     connection handle (anything else: `not_a_connection`).
   - `call/3` and `send/2`: a call is submitted through the hub when the connection has a
     multiplexed session and the reply arrives as a message `{:reply, ref, result}` to the
     calling process; a typed call (no hub carries one) runs on the VM thread as BEAM3's `rename`
     does, and the page says which ones. `send` is one-way. Arguments are the wire's words and
     handles; the native encodes nothing of a protocol: that is the generated client's.
   - `serve/1`, `reply/2`: an endpoint the VM holds is served through the serving library's
     multiplexed loop as a hub-fed stream of requests `{:request, ref, badge, account, labels,
     body}`; `reply/2` answers one; R77's admission and the session bound are the library's,
     and a request the Erlang side never answers is abandoned by the library's rule, never held
     by the VM.
   - `budget_create/1`, `budget_destroy/1`, `budget_usage/1`: the kernel calls over the VM's
     own budget handle (the spec record's six fields as budgets.md's table; a deadline makes a
     lease); the handle returned is a resource term.
   - `labels/0`: the VM's label set, read once at start from its budget (`budget_usage` on its
     own handle), fixed.
   - `launch/1`: the client library's `Launch` (image bytes, budget, exit endpoint, namespace
     entries, named handles, args, stack and heap pages, grants) from an Elixir map; the launcher
     reads the program's bytes itself (native.md: no kernel path lookup; in M1 programs come from
     `/boot`); at most `MAX_START_HANDLES` entries; the job's exit notice is a hub-fed completion
     (BEAM3's `dispatch` has the (conn, tag) seam for it), delivered as `{:exit, job, cause,
     code}`; standard streams and pipes as native.md's "Standard input and output" states them,
     and no more than it states (what is Open there stays Open: say so on the page).
2. **Handles are resource terms:** unforgeable, collected (a dropped resource closes its handle
   at the next collection; a `budget` resource dropped does not destroy the budget: the page's
   rule is that destruction is a call), never serialisable (`term_to_binary` of a resource is a
   plain reference that grants nothing on decode: the attack test). A copy inside the VM is the
   same connection. Delegation is `new_connection`, a typed call, never a native.
3. **Generated Elixir clients** (wire.md): the generator writes beside each protocol's Elixir
   codec in `libs/wire/elixir` one module with one function per message over `call/3`,
   returning the decoded reply or the protocol's error by name (the `libs/wire` error table
   BEAM3 made), handles returned to the caller and none kept on an error (R13); checked in and
   held by `generated_files_are_current`. Then the thin hand-written layer: `Redoubt.Namespace`,
   `Redoubt.Budget`, `Redoubt.Process` over the natives, `Redoubt.Keys` over `keyd`'s generated
   calls; no authority in them.
4. **The shell launches a native program** (shell.md "The shell in a session", M1's scope): one
   commandlet runs a program from `/boot` in a budget carved from the session's, with the
   session's console as its standard streams as native.md states them today, and waits for its
   exit; `Redoubt.Cmd` and pipes are M2 (shell.md "Native programs and pipes": not here).
5. **Attacks** (every native): hostile arguments (wrong types, oversize lists, a path with NUL
   or `..`, a spec over the limits, a handle of the wrong kind, a decoded resource) each refused
   by name with no panic and no handle leaked; a native bounds its work per call (no unbounded
   loop over an Erlang list without a cap stated).

### The rules it keeps

R13 (one outcome per call) at the generated clients; R77 and R26 at `serve` through the
library; R1/R14 unchanged (the kernel stamps; the VM forges nothing); the page's handle rule;
native.md's launch rules (the launcher reads the program; at most `MAX_START_HANDLES`; no shared
text; no dynamic linking).

## The cases (both widths; system verdicts)

- Host (`beamlet-redoubt` on `fake-redoubt`, `beamlet-vm`): each native's round trip and each
  attack; a decoded resource grants nothing; a dropped resource closes its handle; `labels/0`
  fixed; a generated client's call and error by name; `generated_files_are_current`.
- Machine: `beamlet-natives` (a VM under `init`: `ns()` prints its table; `bind` then
  `File.read` through the bound prefix; `budget_create` a child with a deadline, `budget_usage`
  shows it, the deadline ends it; `call/3` on `keyd` through the generated client); `beamlet-serve`
  (the VM serves an endpoint a second program calls; requests arrive with badge, account and
  labels; an unanswered request is abandoned by the library's bound, the VM unaffected);
  `beamlet-launch` (the shell's commandlet launches a `/boot` program in a carved budget, its
  output reaches the console, its exit arrives as a message, its budget's usage returns);
  `beamlet-natives-attack` (the attack table on the machine for what the host cannot show: a
  handle of another VM's never decodes to authority). `userland-boot`, `beamlet-files` and
  `beamlet-footprint` unchanged in substance (report the footprint's change).

## Page lines (exact text in the report)

- `beamlet.md` "Natives" to built with its status list; the typed-call exceptions named; the
  resource rule's attack tests named.
- `native.md` "Launching from a session" to built; "Standard input and output, and pipes" and
  "Killing a job" only as far as this package builds them, their Open lines kept or closed
  honestly; "The client library" gains the VM as a caller of `launch` and `ns`.
- `wire.md` "Generated clients" to built, the generator's section naming the Elixir client's
  file; `shell.md` "The shell in a session" status line gains the launch case; `sessions.md`'s
  `ns()` example stays true. SECURITY.md rows only where a rule's tests change (R13's gains the
  generated-client test). No dates, package IDs or review history.

## Owned paths

`userland/otp/redoubt/**` (the `System` platform, the natives, launch and serve plumbing on the
hub), `userland/otp/vm/src/platform.rs` and `bif/**` for the `System` trait and the natives'
registration only, `libs/wire/elixir/**` and the generator's Elixir-client output (`libs/wire`'s
generator: the new output kind), `userland/shell` (the launch commandlet and the thin modules
`Redoubt.Namespace/Budget/Process/Keys`), `tests/beamlet-{natives,serve,launch,natives-attack}.toml`
and their programs, the pages named. **Not yours:** `libs/client` beyond a signature the natives
need (listed in the report), the serving library, the kernel, the steward, `/net` and `gen_tcp`
(BEAM5), BEAM8's five VM files (`module`, `interp`, `loader`, `memory`, `opcodes`: if BEAM8 is
still in flight, rebase; whichever is accepted first merges first).

## Gates

The short gate: both builds; host tests of `beamlet-vm`, `beamlet-redoubt`, `redoubt-client`,
`redoubt-wire` (the generator's), the shell's own; the docs checker, `cargo fmt --check`, the
size budget (beamlet's ceiling at the fold, delta reported), the `unsafe` ratchet unchanged, the
no-cruft gate; own cases on both widths: the four above, `beamlet-files`, `beamlet-console`,
`beamlet-boot`, `beamlet-footprint`, `aio-many-reads`; the smoke set. The whole bench is the
train's.

## Not here

`/net`, `gen_tcp`, `ipd`'s binding (BEAM5); `Redoubt.Cmd`, pipes and job control beyond one
launch-and-wait (M2, shell.md); the screen natives; the agent harness's grant kinds (agents.md,
M3); a native that parses a large untrusted format (the page forbids it); anything in the
steward.

## Checkpoints

1. After `ns_lookup/bind/ns`, `call/3` and one generated client work on the host, before `serve`
   and `launch`: one progress line with the `System` trait as written and the typed-call
   exceptions found.
2. After `beamlet-launch` is green on one width, before the pages: the launch map's fields, the
   streams as built, and what stays Open on native.md.
