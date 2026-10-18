# BEAM5: gen_tcp over `/net` — implementer's report

Branch `wp-BEAM5` in `.worktrees/BEAM5`, rebased onto `origin/main` 9a6600ede (train 63, IPD1).
Head: 3bab87a57 (two commits: the backend; the fixture, host tests, cases and pages).
Tier B held: no native, no server change; the one design fault found is `ipd`'s and is node
IPD1 (the Architect's ruling "BEAM5 Q1").

## Delivered

- `userland/otp/vm/lib/beamlet_tcp.erl` (+ the regenerated `.beam`): the transport is the
  session's `/net` when the namespace has one. A socket process reads `/net/tcp/clone` for its
  number, starts its reader, opens its `ctl`, writes `connect(addr, port)` framed by the
  generated codec (`'Elixir.Redoubt.Wire.Proto.NetCtl':encode_file/1`), reads `ctl` until the
  state is established (the hub's `etimedout` asked again; the closed state is `econnrefused`;
  a refusal keeps `ipd`'s name, `eacces` for the box's own addresses, a labelled caller or the
  scope), then moves bytes through `data`: writes from the socket process (`etimedout` asked
  again), reads by a reader process of its own `data` fid (0 bytes is the peer's end). A socket
  process that dies has its socket aborted by its reader; a socket that fails to connect is
  aborted and its ctl closed at once. `listen` is `ctl listen(port, backlog<=8)`; `accept` is
  a read of the listener's ctl, the new socket's `remote` its peer name; a listener's `close`
  writes `ctl close` before answering. Framing, packet modes and the active-mode mailbox are
  unchanged. Addresses: IPv4 tuples; a name is `nxdomain`, an 8-tuple `eafnosupport`. Without
  a `/net` (the open of `clone` is `enoent`) the in-VM loopback stand-in serves; any
  non-loopback address is `enetunreach`. `shutdown(write)` then `close` write `ctl close` once.
  `sockname` of a socket of `ipd`'s is `enotsup` (nothing invented); a listener's is
  `{0,0,0,0}` and its port.
- `userland/otp/redoubt/src/fixture.rs`: `fixture::net(reach, own, echo)`, a scripted `/net`
  served over the 9P skeleton on the fake kernel: clone/ctl/data/remote as `ipd` serves them
  (states 2/4/5, `not_permitted` for `own`, closed for unreached, echo, held reads answered
  `timeout` at `NET_WAIT_US` = 8 s by the skeleton's own request deadline), with a script the
  test feeds, ends, accepts into and reads back (`seen`, `holds`).
- `userland/otp/redoubt/tests/tcp.rs` (6 host tests, `--features fake`), running the Erlang
  fixture `userland/otp/vm/tests/src/tcp_backend.erl` (+ `fixtures/tcp_backend.beam`) in a VM
  whose module dirs are the pinned toolchain's `erts`, `kernel`, `stdlib` and Elixir ebin dirs
  and the wire codec compiled from `libs/wire/elixir` through `scripts/q run` (fails naming
  the toolchain if absent):
  - `a_connect_is_clone_ctl_and_data_and_the_peers_end_is_closed`
  - `active_mode_delivers_what_arrives_and_the_end`
  - `a_read_the_server_times_out_is_asked_again` (a hold recorded strictly after the first
    hold + `NET_WAIT_US` can only be a re-asked read; the fed bytes then arrive)
  - `refusals_are_named_and_a_failed_socket_is_given_back`
  - `a_listener_accepts_through_ctl`
  - `without_a_net_the_in_vm_loopback_still_serves`
- Cases: `tests/session-tcp-echo.toml` (alice echoes through the bench's peer in raw, line,
  2-byte and active 4-byte modes; verdict: the peer counts exactly 4 connections, the capture
  4 SYNs and none to the box's addresses) and `tests/session-tcp-loopback.toml` (the loopback
  login row: alice's connect to the box's own sshd at 127.0.0.1:22 and 10.0.2.15:22 refused
  by `ipd`, verdict the capture's `self_forbidden` and the peer's count of 1 from her control;
  the agent's lease connect to the peer `enetunreach`, verdict the peer's count and the
  steward's `lease agent/session-N of 1003/{} exited 0` line). Both widths.
- Pages: `docs/userland/beamlet.md` "beamlet on Redoubt" (the `/net` paragraph built, with
  its bullets and the IPD1 residual; the status line names the cases and host tests);
  `docs/plan/m3-agents.md` (the loopback login row, "TCP in a session" out of Remaining work
  and into Progress); `docs/userland/agents.md` "Leases" (a lease has no network; status
  gains the case); `docs/userland/sessions.md` "Vault sessions" (`eacces`); `docs/servers/ipd.md`
  R59 (the case, one sentence); `docs/SECURITY.md` R59 row (the case);
  `docs/servers/README.md` "`netd` and `ipd`" (the session's path, status, figure edge).

## Summaries checked

- `README.md`, `GETTING-STARTED.md`: no claim about TCP, `gen_tcp` or `/net`; no change.
- `docs/userland/README.md`: "OTP's `File`, `IO`, `Path` and `gen_tcp` work unchanged over
  the VM's natives" — now true for `gen_tcp` (over the `prim_file` natives); no change.
- `docs/servers/README.md`: the network-path figure drew no session edge; a solid one added,
  text and status updated (above).
- `docs/plan/m3-agents.md` progress and remaining work: updated (above).

## Gates (exact commands, from `.worktrees/BEAM5`; every build and boot through `scripts/q`)

| Gate | Command | Result |
| --- | --- | --- |
| formatting | `make -f scripts/jobs.mk set CASES=formatting` | PASS (final tree: see the last row) |
| docs | `make -f scripts/jobs.mk set CASES=docs` | PASS (final tree: last row) |
| beamlet host tests | `make -f scripts/jobs.mk set CASES=beamlet-lookup-host` | PASS, 70.3 s (final tree: last row) |
| tcp host tests alone | `scripts/q run -- cargo test -p beamlet-redoubt --features fake --test tcp` (userland/otp) | 6 passed, serial (`--test-threads=1`) and parallel |
| size-budget, unsafe-budget | `set CASES="size-budget unsafe-budget"` | PASS, PASS (final tree: last row) |
| beamlet-footprint rv64, rv32 | `set CASES=beamlet-footprint` | PASS 6.2 s, PASS 6.4 s (the embedded beam grew 17540 -> 17624 bytes; the case's budget holds) |
| the beamlet set (18 cases), the net set (9: net-tcp, net-attacks, net-pinned, net-host-tests, bench-net-peer x4, bench-net-self-unrefused), steward-ssh-two-principals, both widths | `set CASES="..."` (the 28 names) | 53 PASS, 0 FAIL, on 8c01299b1 (before the rebase onto IPD1; the diff since is case files, pages and test-side code, and IPD1's own train ran its sets) |
| session-tcp-echo rv64, rv32 | `set CASES="session-tcp-echo session-tcp-loopback"` | PASS 5.7 s, PASS 5.4 s, on f1e7c5907 (rebased tree; its content is the final tree's but for test-side doc and cleanup edits the boot cases do not build) |
| session-tcp-loopback rv64, rv32 | same run | PASS 7.5 s, PASS 7.0 s |
| final tree 3bab87a57: formatting, docs, beamlet-lookup-host, size-budget, unsafe-budget, beamlet-footprint both widths | `set CASES="formatting docs beamlet-lookup-host size-budget unsafe-budget beamlet-footprint"` | all PASS: unsafe-budget, size-budget, docs 1.6 s, beamlet-footprint rv64 6.2 s and rv32 6.3 s, formatting 21.9 s, beamlet-lookup-host 67.8 s |

Not run: the whole bench (Tier B: the package's cases and the docs checker are its gates; the
sets above were run as the brief asked); `./test-shell` (the shell is untouched); rv32 of the
host-only cases (none).

Each attack case's verdict is the system's (rule F): `session-tcp-loopback` is judged by the
bench's capture (`self_forbidden` 127.0.0.0/8 and 10.0.2.15/32: no SYN to the box's own
addresses, exactly one SYN to the peer) and the peer's own count of connections (1, alice's
control), and by the steward's `lease agent/session-N ... exited 0` line for the lease's
session; what the shells print (`eacces`, `enetunreach`) is information only and said so.
`session-tcp-echo` is judged by the peer's count (1) and the capture (1 SYN, none to the box).

## Design problems found

- `ipd` parked a `data` read 30 s (`ctl` 60 s) while the hub gives a request up at 10 s and
  drops its late answer: bytes answering a given-up read were lost to the stream. Ruled the
  Architect's "BEAM5 Q1" as a serving rule (a server parks strictly shorter than its clients
  wait), built as IPD1 and published (train 63); the branch is rebased onto it, so no row needs
  anything beyond main. The page states the rule as kept.
- A session's sockets are bounded by `ipd`'s share rule (R26): `State` units per share are
  `limit/(n+1)` of the account's bucket (12 by default), its `/net` connection takes one, each
  socket one, and a socket the session closes keeps its unit through TCP's TIME-WAIT (10 s). With
  alice's console session beside her SSH session, that is three sockets: the echo case's fourth
  connection within 10 s of three closes was `too_many` -> `emfile`, shown by a diagnostic boot
  (three connect/close, `emfile` at once, `ok` after a 12 s pause; `/net/tcp` lists none of the
  closed sockets). Not a defect of the backend or of `ipd`; the page says what a client sees and
  the echo case uses one connection, changing modes by `setopts`. Worth the steward's or ipd's
  owner knowing: a session that opens connections in quick succession (an HTTP client) will meet
  it.
- The shell evaluates each line in a fresh process, so a socket's owner (the connecting line's
  process) is gone by the next line: active-mode messages go nowhere until
  `controlling_process`, as with any gen_tcp socket. The case does that; not a defect.
- With `pty = true` a session's VM did not end on the bench's `exit`, so the steward's
  `lease ... exited` line never came (two 1500 s runs); without a pty, as
  `steward-ssh-two-principals` runs, it does. The cases run without a pty. Whether a hangup on
  the pty should end the shell is the shell's or sshd's question, not this package's.
- `prim_file:open` with a write mode sets `create`, so the hub tries `Tcreate` first on `ctl`
  and `data`, which `ipd` refuses, and then opens: two extra round trips per open, by design
  of the files layer (a served file a server does not create). Not changed; noted.

## Open risks

- The hub's own retirement of a fid whose request it gave up (files.md residual) still applies
  to a `data` read that `ipd` holds past 10 s until IPD1 is on main.
- The listener's accept loop asks `ctl` in a blocking read of up to the hub's bound; a `close`
  of the listener waits for that read to return (<= 10 s, 8 s after IPD1).

## Next step

Review by the red team (the loopback attack case), then the orchestrator's merge: the branch is
two clean commits on main's tip. Nothing of IPD1's is on the branch.

## For the owners of other pages (not acted on)

- `ipd`'s share rule gives a session about three sockets at the image's defaults, TIME-WAIT
  counted (above): a client that opens connections in quick succession meets `emfile`.
- With a pty, the bench's `exit` did not end the session's VM within 1500 s (no steward exit
  line); without one it does.

## Review notes folded by the orchestrator (kernel-red-3, OK with notes)

- The verdict account: the shells' `eacces` expects are the proof that ipd, not the in-VM
  stand-in, refused the loopback connect; the capture's "no SYN to either" is a bound (no
  negative shows an unrefused SYN reaching the wire). Said in the case's comment.
- No backpressure: the reader drains the socket continuously and a passive socket's unread
  bytes accumulate in the socket process, bounded by the VM's budget alone. Said on beamlet.md.
- A connect killed at its timeout between `clone()` and the reader's start leaks that socket's
  State unit at ipd until the session ends (a tiny window); a residual, not fixed here.
