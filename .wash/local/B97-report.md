# B97 report: an empty listener is charged to its own bucket's entries

Branch `wp-B97`, base d66839252, head c0023d4be (one commit; 73eb47101 before the round-1 docs fold).

## What changed

- `servers/ipd/src/stack.rs`: `Stack::entries_fit(key, cap, more)` — the bucket's own entries
  (every table entry whose `owner.key` is the bucket, lingering included) plus `more` at most
  `2 * cap`, and the table at most `max_entries` (the backstop). `room()` uses it in place of the
  table-only check; `listen()` uses it for its `backlog` entries in place of
  `table.len() + backlog > max_entries`. Module doc ("What a socket costs its owner") states it.
- `servers/ipd/tests/stack.rs`: helper `empty_listener`; `a_full_table_refuses_a_listen_or_a_socket`
  reworked to the per-bucket bound (cap 1: one empty listener + one socket, its listen refused,
  released it still lingers and holds its entry, gone it gives it back);
  new `empty_listeners_fill_only_their_own_bucket` (bucket A makes empty listeners until refused,
  keeping its last socket; bucket B still gets a socket and a listener); new
  `the_table_backstops_buckets_past_max_sockets` (two buckets whose caps pass max_sockets: the
  table's own bound refuses b's listen).
- `docs/servers/ipd.md` Sizing: status 20 -> 22 tests; the table paragraph carries the
  arithmetic (sockets <= cap; a listener with a backlog holds a charged backlog socket, so own
  use never passes 2 x cap; live buckets at most `buckets=`, each counted in max_sockets at
  max(cap, default), so together <= 2 x max_sockets, the table's size; the backstop and its one
  window; only a listen scope, the image's sshd, never a session, so the host tests are its
  attack). Miri row: the three table-bound tests have not run under Miri.
- `tests/size-budget.toml`: servers/ipd 2984 -> 2988 (`Size budget:` line in the commit): the
  base sat exactly at its ceiling; `entries_fit` is 4 code lines.

## Red on the base

New tests with base `stack.rs` (src reverted in place, tests kept):
`a_full_table_refuses_a_listen_or_a_socket` FAILED (stack.rs:632: the base lets the second
listener's listen through), `empty_listeners_fill_only_their_own_bucket` FAILED (stack.rs:653, "another bucket's
socket refused too_many"), `the_table_backstops_buckets_past_max_sockets` ok (the backstop is the
base's check, kept). With the change: all three pass; `--test stack` 21/21.

## Design (B97-shape, Architect OK with conditions 1-4)

Rejected alternative (condition 4): charging an empty listener a `State` unit. The accept that
empties a listener has no reservation to pay from (the refill already failed for want of room),
and refusing that accept instead would stop sshd accepting at its cap. Residual-only rejected:
the fix is a few lines and removes the cross-bucket refusal.

No attack case: only a listen scope reaches the refusal (the image's sshd, badge 6); no session
or agent holds `l:` (grants never add listen, R61). The host tests are the attack.

## Gates

All on head 73eb47101, through q / scripts/jobs.mk (env per launch-env-2026-10-20.md):

| Command | Result |
| --- | --- |
| `q run --cores 4 -- cargo test -p redoubt-ipd --test stack` | 21 passed, rc 0 |
| `make -f scripts/jobs.mk set CASES="ipd-host-tests size-budget docs formatting"` | ipd-host-tests PASS 5.0s, docs PASS 2.1s, formatting PASS 28.3s (rc 0 each); size-budget FAIL first (servers/ipd 2988 > 2984), PASS after the ceiling raise (rc 0) |
| `make -f scripts/jobs.mk prebuilt` | rc 0 (rv64 366 cases, 0 failed, 277.3s; rv32 352 cases, 0 failed, 377.4s) |
| `make -j4 -f scripts/jobs.mk set CASES="session-tcp-churn net-tcp"` | session-tcp-churn PASS rv64 8.2s, rv32 6.2s; net-tcp PASS rv64 0.4s, rv32 0.4s; rc 0 each |

net-tcp's 0.4 s was checked against its console log: a real boot, the judge's echo through
10.0.9.100:7 and its listener on 8000 with a backlog of 2. Not run: the whole bench, rv32 build
apart from the prebuilt (which builds and passes rv32), mutations, Miri, fuzz.

## Affected summaries checked

- `docs/servers/ipd.md` Sizing: updated (above). "The `/net` tree" ("stays charged to its owner
  until it is gone"), Failure and restart, Residual risks: still true, no change (the finding was
  fixed, not stated as a residual).
- `docs/SECURITY.md` R26 row: names only ipd's sizing host tests; R26's own status line lives in
  serving.md and lists no stack tests; no change (the new tests sit under ipd Sizing).
- `docs/servers/serving.md` R26 (admission fairness): the rule is admission's; ipd's sockets
  already in it; no change.
- `docs/servers/sshd.md` Listening / residuals: sshd's one listener on 22 holds a backlog and at
  most its cap of sockets, well under 2 x cap entries; no claim changes.
- README.md, GETTING-STARTED.md, milestone progress: no ipd table claims (grep "socket table",
  "too_many", "empty listener"); no change.

## Open risks

- The backstop's one window: between a connection's disconnect and the next poll's sweep of its
  released empty listeners, another key may take its admission slot; the table bound then
  refuses that instant. Stated on the page.
- The new tests have not run under Miri (stated in the Miri table).

## Round 1 fold (servers-red, OK with notes)

- P3-1: ipd.md now says a bucket's cap is its `State` units, in which its sockets are paid.
- P3-2: ipd.md adds that the stack is polled only after a receive that returned no call, so a
  steady stream of calls lengthens the backstop's window.
- P3-3: rv64's prebuilt ran: 366 cases, 0 failed, 277.3s (target/jobs/prebuilt.log line 12).
- Head c0023d4be (docs only over 73eb47101): docs PASS 9.2s rc 0, formatting PASS 23.7s rc 0.
