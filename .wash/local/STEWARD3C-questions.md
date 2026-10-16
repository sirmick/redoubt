# STEWARD3C questions (steward3c-implementer, 2026-10-16)

## Q1: case 7 needs three crashes of `walfsd:data` blamed on bob; a restarted walfsd strands the steward's kept connection (QA `STEWARD3C-shared-restart`)

Facts, from the code and the pages:

- The steward reaches `walfsd:data` through the fresh connection `init` made for it (init.md
  "Fresh connections per child"); every session's home is minted below it
  (`servers/steward/src/bin/steward.rs`, `fresh_carved`: `kept.new_connection(...)`).
- The bench's injection is walfsd's `cut-after-write` (`servers/walfsd/src/cut.rs`): a walk to
  `walfsd-cut-N` arms it, and the instance exits (`CUT_EXIT` 10) inside the operation its N-th
  block write completes in. An exit holding the caller's call is `faulted`, blamed on the
  caller's account and labels (`walfsd-power-loss` expects
  `init: walfsd (PID n) faulted, code 10, serving nobody, labels none; blamed on nobody: no steward`;
  under the steward, bob's write gives `serving account 1002, labels none`). One instance ends once.
- `init` restarts `walfsd:data` with empty tables (serving.md "The server restarts"): the
  steward's connection id there is dead, so every later login of anyone fails its home step
  (`Error::Refused` → a failed step → the login refused), and every live session's connection at
  walfsd is dead too (init-restart: "the new instance refuses an id the old one minted").
- So one instance yields at most one blame on bob, and three blames need three live connections
  of bob's at three instances, which the steward cannot mint.
- No page says what the steward does after a shared server's restart: steward.md "Failure and
  restart" covers the steward's own death; init.md's residual "A restarted `consoled` forgets
  every server's console ... `init` has no way to hand a running server a new one" is the
  analogue, unwritten for the file servers and `ipd`.

Options I see:

- (a) The steward fails closed on a mint refused at a dead kept connection: it exits, `init`
  reaps `users` and restarts it (steward.md line 111: "the server fails closed: it exits, and
  `init` restarts it"). Then alice's session ends with bob's crash, so case 7's "alice's session
  is untouched" cannot hold on the machine, and bob restarts the whole steward three times before
  the lockout; the steward's restart also forgets the blame counts (empty tables), so R40 never
  reaches three.
- (b) `init` hands the steward the servers' badges, not connections, and the steward mints each
  session's connections through the badge: survives restarts, loses "disconnect at the steward's
  exit frees them all" (init.md) unless `init` releases per session, or the steward's exit is
  followed by every file server's restart.
- (c) Case 7 proves the lockout on the machine another way: the three blames need not be crashes
  of one shared server, e.g. a bench-only server `init` restarts (like init-restart's
  `restartee`) that faults while serving a call from bob's session's VM; this needs a route
  from bob's shell to it (a slot in the binding table: a core change).
- (d) Case 7 stays on the host: the server test drives three `blame` calls from `INIT` and
  checks `DestroyBudget` of bob's sessions and lease, a login refused `LockedOut`, the window
  passing, with the audit lines; the restart gap is recorded as a residual on steward.md
  "Failure and restart" and init.md, with a follow-up node.

Asked: the restart gap's owner and the page words; what case 7 proves on the machine; whether
the blame timeout is init.md's `RELEASE_TIMEOUT` (1 s, `libs/client/src/grants.rs`), which I
take unless told otherwise.

Meanwhile I build the server binding of `blame`, `init`'s call with its timeout and console
line, the host tests and the pages.
