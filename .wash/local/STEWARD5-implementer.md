# STEWARD5: the steward's loop never waits on a shared server

Node STEWARD5 (parent AGENTS). Tier A (the steward's loop). Size M. Reviewer: steward-red-4.
Starts after STEWARD6 is on main (same file). Ruling: `.wash/local/wave-2026-10-18-rulings.md`
"F1"; the rules it builds on: `STEWARD3W-ruling.md`, `STEWARD3U-report.md`, `STEWARD4-ruling.md`
Q15–Q16; evidence `STEWARD3V-report.md` part 2 and scheduling.md "With the real steward".

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; the fmt gate is
  `make -f scripts/jobs.mk set CASES=formatting`, the docs gate `set CASES=docs`. No edit while
  prebuilt builds or cases run. Launch a `steward-latency` run longer than 1.5 h detached
  (`setsid nohup`), never in a background shell that dies at 2 h.
- Messages at most 2000 bytes; detail in a file. Read reports by section; never a QA file's tail.
- Pages carry no dates or ids; a number on a page shows its arithmetic; a declaration is twice
  the measured peak. A miss is a finding, not a tuning; no target moves.
- The core (`libs/steward`) is not touched. `cleanup.rs`'s rule (3W) is not changed: it moves.

## The symptom

3Y runs the cleanup steps at the loop's top within the steward's send bound (100 ms); 3W parks a
step after four. The loop is measured against a timer-wake target of 15 ms p50 / 50 ms p99.
One attempt at a busy `erofsd:system` landing in a sample's window is a 100 ms wait before the
receive: the p99 is missed by construction, one sample per round at N = 3 and 4 on both widths
at one hart (69–93 ms) and at two on rv64 (148 ms); the same retry shows in `reply_wait`'s p99
(scheduling.md's table, the paragraph under it; `.tmp/STEWARD3V/latency4-rv32-console.log`
716–720, 730–743 if still present, else the merge history).

## The design

1. **A cleanup thread.** The steward starts one thread with itself (as its crossing watchers
   and relay hands are started: the `work.receive` threads in `bin/steward.rs`) that owns
   `cleanup::Queue`: due steps, retries at `RETRY_US`, parking after `TRIES`, `wake`, `forget`,
   `run_parked`, all as 3W wrote them. Each step runs there with the client library's own call
   bound (`RELEASE_TIMEOUT`, 1 s, `libs/client/src/grants.rs`): the loop pays nothing, so a step
   may wait what a call waits. `CLEANUP_US` goes.
2. **The loop only hands over and takes back.** At an end's release, `rebind`'s replacement and
   `reconnected`, the loop sends the thread the step (`disconnect(id)` at its server, `ended` on
   `sshd`'s channel console), `wake(server)` (a mint there answered: taken or refused, not
   `Timeout`) or `forget(server)`. The thread reports each outcome (done; parked, with the
   attempt count; failed `Dead` or refused) as a message on the loop's own endpoint, and the
   loop says the console line through 3U's queue, so every line still goes through the one fid
   in order. The loop's top is then: the idle timer's ends, the console lines queued, the
   receive. It makes no call to a shared server.
3. **A mint at a server with parked steps** (3W: the mint needs the record a step frees). The
   loop sends `wake(server)` and mints; if the mint is refused and that server had parked steps,
   the loop waits for the thread's report of them, within one mint bound (STEWARD6's
   `SERVER_US`), and retries the mint once. A mint still pays at most one bound, a batch's rule;
   3W's test (a mint succeeds where it was refused) is kept in this shape.
4. **Two threads on one endpoint.** The thread's `disconnect` and the loop's `new_connection`
   may be in flight at once on one kept connection. Each is a kernel call with its own lent page
   and its own reply, so show it: the client's `Connection` keeps no per-call state a second
   caller could disturb on an unattached connection (read `libs/rt/src/client.rs`; say what you
   found). The server admits both in the steward's share (`in_flight`, 6 in the image: if the
   derivation on walfsd.md counts the steward's calls in flight, add one and show it).
5. **Cost.** One thread (its stack pages twice the measured peak, as sshd's stacks are stated),
   one `Lend` page, no new connection or State at any server. The steward's manifest entry grows
   by that arithmetic in the image and every memory-checked manifest; the memory table on
   testbench.md follows.

What stays: the console queue and its pre-reply drain (3U) and bound; `RETRY_US`, `TRIES`, the
parked rule and its console line (3W); the order of a step's own line after its outcome.

## Tier A invariants the reviewer checks

- The loop's turn (its top, between a reply and the next receive) makes no call to a shared
  server, under any outcome of the thread's; a server that never answers costs the loop nothing.
- The cutoff is still the destroy (R41): no step frees a record before `DestroyBudget` has run;
  the thread only ever disconnects what the loop released.
- The queue's argued bound (a connection enters once, leaves done or parked) holds with the
  hand-over: no step is sent twice, none is lost between the two threads.
- A mint pays at most one bound, and a refused mint with parked steps at its server is retried
  once, never looped.
- Every outcome has its line, in the order the outcomes arrived; no line vanishes silently.

## Cases

- Host (`servers/steward/tests/steward.rs`): the loop's turn completes with no call out while a
  server never answers (the fake kernel's record of calls by thread); 3W's five tests kept
  (parked after four, runs at the next answered call, a mint runs them first and succeeds where
  it was refused, `reconnected` forgets, a never-answering server costs no bound); the outcome
  lines said in the order the steps ended; a step is handed over once (3Y's "holds each
  released connection once").
- Bench, the acceptance: `steward-latency` (seed 3, by name, `--keep-failed`) at one hart on
  both widths: N = 3 and 4 meet the timer wake's p99 and `reply_wait`'s p99 carries no 100 ms
  retry; at two harts on rv64 the same at N = 3; rv32 two-hart N = 3 stays not reached (B88's
  cause; STEWARD8's). The steward set, `steward-server-restart`, `steward-blame-lockout`,
  `steward-session-ends`, `init-boot`, both widths, unchanged in verdict.

## Pages

- steward.md "Failure and restart": the cleanup bullet rewritten: the thread, the hand-over,
  the outcomes as events, the mint rule, what a turn's work is now; the status line; the loop's
  order sentence (idle ends, console lines, receive).
- scheduling.md "With the real steward": the N = 3 and 4 cells re-measured from one run each
  at seed 3 on both widths; the paragraph attributing the misses to the cleanup turn replaced by
  the measured figures; the two-hart rv64 N = 3 cell the same.
- testbench.md's memory table if the steward's entry moves; SECURITY.md R41's row if its tests
  change; serving.md's residual unchanged. Summaries checked: say which.

## Owned paths

`servers/steward/src/bin/steward.rs`, `src/cleanup.rs` (moves, rule unchanged), `src/console.rs`
only if the line path needs it, `src/lib.rs`, `src/drive.rs` (the hand-over at release), its
tests; `image/manifest.json` and the memory-checked manifests' steward entries; `tests/
size-budget.toml` (servers/steward's ceiling with its reason line); the pages above. Not
`libs/steward`, not `libs/rt`'s client beyond reading it.

## Gates

Host tests; release builds rv64 and rv32 with and without `latency-probe`; the bench cases
above both widths; `set CASES=formatting`, `set CASES=docs`, `size-budget`, `unsafe-budget`;
the memory-checked set (`init-boot userland-boot steward-session-ends beamlet-footprint`) if the
entry moved. Report each with its exit code and the latency table's new cells with log paths.

## Checkpoint

After the host tests of points 1–3 are green and before the latency runs, one progress note:
the hand-over's shape, what you found on point 4, the thread's measured stack.
