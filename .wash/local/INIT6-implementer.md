# INIT6: a restarted entry never meets its predecessor's mux session

Node INIT6 (parent AGENTS). Tier A (the serving library), size S, reviewer kernel-red-4 (or the
orchestrator's init/rt red). Found by B95 (`.wash/local/B95-identification.md` step 4, corrected
on thread B95-cause); ruling `.wash/local/wave-2026-10-18-rulings.md` "B95-cause" item 3 and
"corrected premise". Runs after B95 is on main.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs` (C11). No edit and no git
  operation while prebuilt builds or cases run. Copy a failing job's log aside before a rerun.
- A `libs/rt` change is built against every bin linking `redoubt_rt` (every server,
  `tests/programs`, `tests/net`), with their host tests, before any boot.
- Messages at most 2000 bytes; pages carry no dates or ids; a number shows its arithmetic.

## The gap

init re-mints the same badge number for a restarted entry, and a shared server keys its state by
it. The fids go at the successor's attach: the plain path's `Tversion` drops every fid of the key
("a new session on this connection: every fid on it goes", ninep.rs `answer`), and the hub's
contract has the caller attach before `Hub::connect`. The **mux session** does not go: it holds
its InFlight and its pending requests' Requests and Pages to the session bound, and every hold-0
completion call the successor's connect makes is parked on it, answered empty and resets its
deadline, so a successor retrying connect faster than the bound never gets a session (B95's
injected run: 8 failed connects in 250 ms, then the VM raised). Two more things cross with
it (servers-red on B95, P3-2): the successor's hold-0 call collects the predecessor's pending
answers, R-messages for tags the successor never sent; and the predecessor's InFlight, Requests
and Pages stay charged to the key until the session ends. The mux path already ends a
session at a `Tversion` (a protocol error); the plain path does not.

## The rule

**A `Tversion` on a connection key ends everything the key holds at the server: its fids, and
its mux session with its pending requests and admission.** One rule for both paths; `answer`'s
`Tversion` arm calls the mux's end for the key (`end_session`'s path, admission released, a parked
completion call answered status 4 as at any end). Nothing changes in `init`; init.md's Restart
bullet is reworded to what is true: a dead instance's state at a server it was handed a root
badge for goes at its successor's `Tversion`; the console connection and the steward's fresh
connections go at `init`'s disconnect.

## Cases

- Host (`redoubt-rt`, mux tests on the fake kernel's held clock): a key with a parked completion
  call and pending requests gets a plain-path `Tversion`: the session is gone (its call answered
  status 4, its requests dropped, InFlight/Requests/Pages released), and the next hold-0 call is
  answered OPENED; a live client's `Tversion` with no session is unchanged; the mux path's
  `Tversion` unchanged.
- Boot, both widths, extended from `init-restart` or new: a server entry holding a mux session at
  `walfsd:data` exits 0 and is restarted inside the session bound; the successor's first
  completion call is answered OPENED and its reads and writes succeed; verdict the server's (a
  count line the case's build prints, as `steward-restart`'s counts), not the client's prints.
  `beamlet-files`' replay is this shape too: it stays and must be clean.
- Unchanged: `init-restart`, `init-handed-revoked`, `steward-restart`, `steward-server-restart`,
  `aio-many-reads`, `aio-many-reads-two`, `beamlet-files`, the short gate.

## Pages

serving.md "Multiplexed connections", "The end" (the `Tversion` clause covers both paths) and
"A client dies"; init.md "Restarts and reboots", the Restart bullet; B95's residual sentence goes.
SECURITY.md rows whose tests change. Summaries checked: say which.

## Owned paths

`libs/rt/src/server/ninep.rs` (`answer`'s `Tversion` arm), `ninep_mux.rs` (an `end_for_key` if
none exists), `libs/rt/tests/mux.rs`, the case toml, the pages. Not `init`, not the hub.

## Report

Each gate with its exit code; what was deleted (B95's residual, init.md's false clause).
