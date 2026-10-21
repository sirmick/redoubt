# B95 identification: where beamlet-files' eio comes from

Base 3e6f29443, branch wp-B95. Diagnostic commit 0958edaf7 (WIP, never merged): walfsd's mux
says when a session opens, a fill takes ≥ 1 s, a session ends for want of a call, and a hold-0
call lands on an open session; beamlet counts failed hub connects. Logs in
/home/mcloonan/redoubt/.tmp/B95/ (alone-*, inject-*); B93's failures in .tmp/B93/side/.

## The component

**The 9P multiplexer in libs/rt (`libs/rt/src/server/ninep_mux.rs`), as walfsd runs it, plus the
hub's reading of a fresh session (`libs/client/src/aio.rs`).** Not walfsd's write path, not
blkd. No `walfsd: volume poisoned` line appears in either failing log, so the volume was never
poisoned. That is unlike IPD2's steward-three-contexts failure (.tmp/IPD2/failed/
rv64-steward-three-contexts.smp1.log:63, `walfsd: volume poisoned: create: blkd answered
Failed`), which is B51's device fault. These are two different paths.

The chain:
1. `NineServer::pump` (ninep_mux.rs:563-579) answers a parked completion call, then sets
   `deadline = now + bound`. Here `now` is the time the triggering event was received, read
   before `fill` serves every pending request synchronously (walfs → blkd). walfsd sets no
   `requests_wait`, so its bound is `COLLECT_WAIT` (10 s). A fill that takes ≥ 10 s leaves the
   session's deadline already past when its answer goes out.
2. The loop's next `expire` (ninep_mux.rs:525) finds no call parked and the deadline due, and
   ends the session. The waiter's next completion call opens a **new** session (`OPENED`).
3. The hub's waiter takes any reply with word 2 ≠ 0 as the connection's end (aio.rs `hand_over`,
   "a session the server opened afresh is as good as an end"). The volume's connection ends, its
   waiter returns (`1 waiters`), and every later file operation of that VM is `eio`. Beamlet
   never reconnects a connection it has (io.rs `conns`), so the first run's cleanup fails
   silently: `cleaned: []` in B93's logs, a mismatch with the expect that the bench reports
   only later, as the replay's forbidden EXCEPTION.
4. The session opened in step 2 has no call parked and outlives the VM by up to 10 s; nothing
   abandons it. init restarts beamlet with the **same handed badge** (manifest `item.badge`,
   init.rs:510), and the session key is the badge, so the replay's `Hub::connect` (a hold-0
   first call, aio.rs:158) lands on that open session. It is answered empty `[0,0,0,0]`, not
   `OPENED`, so the connect fails `Unexpected`. `Io::connect` returns early without recording it,
   so every operation retries and fails the same way, and files.rs maps the failure to `eio`:
   `files: wrote: {error,eio}`, `io: 1 requests`.

## Reproduction

- Alone (diagnostic build, `make -f scripts/jobs.mk set CASES=beamlet-files`, rc=0): both
  widths pass in about 1.7 s. One `session opened, badge 8`, `136 requests`, `2 waiters`
  (.tmp/B95/alone-beamlet-files-*.log).
- Injected (scratch worktree .tmp/B95/inject: walfsd's 90th answering fill spins 11 s; same
  command, rc=2): both widths fail exactly as B93's did, `forbidden output /EXCEPTION/ ...
  {badmatch,{error,eio}}`. Lines in .tmp/B95/inject-beamlet-files-rv64-smp1.log:
  - 64: `a fill took 11000 ms (61 bytes), from 1479 ms`
  - 65: `session ended, no call since its deadline 11479 ms, now 12482 ms, badge 8`
  - 66: `session opened, badge 8, at 12485 ms`
  - 68: `cleaned: []`
  - 69: `128 requests ... 1 waiters`
  - 77-85: the replay's hold-0 calls on that open session
  - 84: `wrote: {error,eio}`
  - 87: `hub connect failed 8 times, last Some("Unexpected")`

  rv32 is the same (inject-beamlet-files-rv32-smp1.log:62-88).
- Not yet shown: the **natural** load reproduction with the diagnostic. B93's load was 16 jobs,
  and its 15.2 s case time against 1.7 s alone is consistent with a starved guest. The natural
  run tells which way the gap arises: (a) one fill ≥ 10 s, the stale-deadline bug above, or
  (b) the waiter not calling again within 10 s of the answer. This needs the orchestrator's go
  for a load run.

## Question 2 (the case ending at its exit)

Unchanged by the above: the replay runs inside the bench's grace in every passing run as well.
The question goes to the Architect with the cause.
