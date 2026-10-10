# B8 report

Branch wp-B8, head f14be42d8, one commit on main bdb38430e:
`tests: aio-reader re-enters its hub while reads are queued, not after a 20 s receive`.

## Diagnosis

- **Reproduced** on wp-B8 (then on ef2ab5a94): FAIL on both widths, "aio-reader TEST FAILED:
  receive: Timeout" at about 21 s. aio-many-reads (one connection) passes.
- **The diagnostic** in the failure text reads `32 of 64 answered, 32 not yet taken`.
  littlefsd never took one connection's page of 32 reads within `SUBMIT_TIMEOUT_US` (1 ms),
  while it was serving the other connection's page. The hub kept it queued, as documented.
- **Why it stayed queued:** the hub sends its queue only at its next entry. In `two` mode the
  burst then idled in `own.receive(SECOND_WAIT = 20 s)`. Waiters wake it only for completions,
  and the queued reads were never sent, so nothing re-entered the hub and the receive timed out.
- **The clock:** the case has a `[disk]`, so it runs on the host's clock, not icount
  (testbench.md, "Which cases run in guest time"). It failed in trains 5 to 7 and in every
  reproduction here on both widths. Why the 1 ms window is missed so reliably now, where it was
  once a rare rv32 flake, is not established. The fix does not depend on it.
- **The hub drops and misdelivers nothing.** beamlet's io.rs (`Io::wait`) already polls and
  bounds its idle to `RETRY_US` while queued. Only the test reader broke the rule. Tier B: no
  hub code changes.

## Fix

- **tests/littlefsd-programs/src/bin/aio-reader.rs:** both of the burst's idles (waiting for
  the second program, and collecting) go through `idle()`. It calls `hub.poll()`, and while
  `hub.queued() > 0` it receives for at most `RETRY_US`; a timeout then means "try the queue
  again". The retries are capped (`RETRIES = SECOND_WAIT / RETRY_US`, reset by any delivery), so
  a server that never takes the queue still fails the burst itself, with its counts, not the
  bench's 90 s deadline. With nothing queued it waits `SECOND_WAIT` as before. The failure text
  keeps "N of 64 answered, M not yet taken".
- **The rule, in one form,** in libs/client/src/aio.rs's module doc (comment only) and in
  docs/userland/native.md ("Many requests at once", "Submitting is inline"): a caller that
  queues work and then idles re-enters the hub within `RETRY_US` while anything is queued (a
  poll, or a wait bounded by `RETRY_US`); a receive that outlives that is the caller's bug,
  not the hub's.

## Gates

- **On f14be42d8** (rebased onto bdb38430e, with the folds), all rc=0: prebuilt;
  aio-many-reads-two rv64 and rv32; aio-many-reads rv64; client-host-tests; formatting;
  size-budget.
- **Earlier, on 2cfbd8429**, all rc=0: both aio cases on both widths, client-host-tests, docs,
  formatting, size-budget, unsafe-budget, no-cruft.

## Docs checked

- docs/userland/native.md, "Many requests at once": the rule is added. Its status line already
  names both aio cases. No status change: the cases were built and listed before and pass now.
- docs/servers/serving.md, "Multiplexed connections": describes the server side; no caller
  rule there. No change.
