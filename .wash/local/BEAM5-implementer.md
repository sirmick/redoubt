# BEAM5: gen_tcp over /net

Node BEAM5 (parent AGENTS, needs BEAM3: done). Tier B unless a native is added (then re-tiered
to A: say so at once); size M; one reviewer, the red team, since an attack case lands with it.
Ruling: `.wash/local/wave-2026-10-18-rulings.md` "BEAM5". Pages it builds: beamlet.md
"beamlet on Redoubt" (the `/net` sentence after the platform table), m3-agents.md's loopback
login row; it reads ipd.md "The `/net` tree", "Scopes and grants", "The box's own addresses",
"Labelled callers", files.md "Files over 9P" (the hub's waits), agents.md "Leases".

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting` (nightly fmt runs in `userland/otp` too), docs gate `set
  CASES=docs`. No edit while prebuilt builds or cases run.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- Pages carry no dates or ids; a built section's status line names its tests; a security
  property's case gets its verdict from the system, never the attacker (rule F).
- One VM is one trust domain: nothing here adds authority to the VM; a socket is a file the
  namespace already holds. If you find you need a new native, stop and ask on a QA thread.

## What exists

`userland/otp/vm/lib/beamlet_tcp.erl` (318 lines): sockets are processes reached through OTP's
module-socket mechanism (`{'$inet', beamlet_tcp, Pid}`); a loopback network inside one VM
(`listen` registers a port, `connect` to a loopback address finds it); binary and list modes,
packet modes raw, line and 1/2/4-byte headers, active true/false/once/N, `recv` with length and
timeout, shutdown and close. `gen_tcp.erl` is a thin front. Both are embedded in the VM
(`userland/otp/vm/src/vm.rs`, `EMBEDDED`; `tools/build-lib` regenerates the `.beam`). `ipd`
serves `/net` over 9P (ipd.md): `/tcp/clone` (read at offset 0: a new socket `n: u32`),
`/tcp/N/ctl` (write one `net_ctl` operation in the wire encoding: `connect(addr, port)`,
`listen(port, backlog)`, `close`, `abort`; read: `state: u32, n: u32`, waits up to 60 s then
`timeout`), `/tcp/N/data` (read waits, 0 bytes is the peer's end; write waits while the 8 KiB
send buffer is full; 30 s then `timeout`, the client asks again), `/tcp/N/remote`. The session's
namespace binds its `ipd` scope at `/net` (sessions.md `ns()`); a lease's has none (agents.md);
a vault session's call is refused (R60); the box's own addresses are refused (R59). The VM's
files go through its hub (`FILE_WAIT_US` 10 s per request, `etimedout` and `busy` named,
`MAX_WAITERS`, nothing waits on the VM's thread: files.md).

## The design

1. **The transport is `/net` when the namespace has one.** `beamlet_tcp`'s socket process, on
   `connect`, reads `/net/tcp/clone` for its number, writes `connect(addr, port)` to its `ctl`
   (the wire encoding from `libs/wire`'s tables: the Elixir side has the generated codec; use
   it, never a hand-rolled frame), reads `ctl` until the state is established or an error
   (each read parked at `ipd` up to its 60 s, `timeout` asked again while the Erlang timeout
   allows), then moves bytes through `data`: a reader process per socket for active modes and
   `recv`, writes from the sender; `remote` for `peername`. `close` is `ctl close`; a socket
   process that dies aborts. Framing, packet modes and the active-mode mailbox stay as they are
   in Erlang; the hub's `etimedout` on a `data` read is asked again (the peer is quiet), its 0
   bytes is `tcp_closed`.
2. **Every socket is `ipd`'s then, loopback included.** With a `/net` in the namespace the in-VM
   loopback is not consulted: a connect to `127.0.0.1` or the box's own address goes to `ipd`,
   which refuses it (R59), and the backend answers `{error, econnrefused}` or the name `ipd`
   gave (`eacces` for a labelled caller, R60). Without a `/net` (a lease, a host test, the VM on
   the fake kernel) the in-VM loopback stand-in stays, and a connect to any other address is
   `{error, enetunreach}`: a lease has no network and the page says so.
3. **Names.** `connect` takes an IPv4 tuple; a hostname is `{error, nxdomain}` until the resolver
   (ipd.md "Name-scoped connections", planned). `listen` over `/net` follows `ipd`'s listen rule
   as the scope allows it (ipd.md "Scopes and grants"); if the rule gives a session no listen,
   `listen` is `{error, eacces}` and the page says so; do not widen a scope.
4. **No new native, no server change.** The backend uses the files the namespace holds through
   the existing file natives and the hub. `ipd` is untouched.

## Cases

- Bench: a session's shell connects to the bench's peer (`bench-net-peer`'s pattern: the host
  peer is the trusted party), sends and receives an echo in each packet mode the case exercises,
  closes: the verdict is the peer's report, never the VM's output alone. The attack row of
  m3-agents.md "Loopback login": a session's `:gen_tcp.connect` to the box's own `sshd` (its
  loopback and its listed address, port 22) is refused: the verdict is `ipd`'s refusal line
  (as `net-attacks` judges), the row updated to the case; a lease's `connect` to the peer fails
  as having no network (verdict: the steward's lease exit line and the peer seeing no
  connection). Both widths.
- Host (`beamlet-redoubt`'s host tests over the fake kernel's file server): the backend's
  `clone`/`ctl`/`data` sequence against a scripted `/net`; `etimedout` asked again; 0 bytes is
  the end; the in-VM loopback still serves when no `/net` is bound.

## Pages

- beamlet.md "beamlet on Redoubt": the `/net` paragraph becomes a built bullet with its status
  line (the cases above), the rule of point 2, names, listen.
- agents.md "Leases": the error a lease's connect gets. sessions.md "Vault sessions": `ipd`
  refuses it, so `gen_tcp` is `eacces` (one clause). m3-agents.md: the loopback login row.
- SECURITY.md R59's row gains the case. Summaries checked: README.md, GETTING-STARTED.md,
  docs/userland/README.md, docs/servers/README.md (the network path figure): say which.

## Owned paths

`userland/otp/vm/lib/beamlet_tcp.erl`, `gen_tcp.erl`, the `.beam`s `tools/build-lib`
regenerates, `userland/otp/vm/src/vm.rs` only for the embedded list; `userland/shell` only if a
helper is idiomatic (`Redoubt.Net`?: say why); `tests/` (the two cases and their manifests);
the pages above. Not `servers/ipd`, not `libs/rt`, not a native.

## Gates

`set CASES=formatting`, `set CASES=docs`; the two cases and `net-tcp`, `net-attacks`,
`steward-ssh-two-principals`, `beamlet-footprint` (the embedded code's size: the footprint
case's budget holds or the arithmetic says why), both widths; the beamlet host tests. Report
each with its exit code.

## Checkpoint

After the host tests of point 1 are green: one progress note with the backend's shape and
whether anything wanted a native (it should not).
