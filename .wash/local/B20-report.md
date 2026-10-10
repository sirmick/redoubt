# B20 report

Branch wp-B20, head 5182dd06b, one commit on main b60c7cc5c:
`tests: the hub's tests send what they queue before they idle or time a wait`.
Test-only: libs/client/tests/aio.rs. No code in libs/client changed, so Tier B.

## Measurements (all through `q run --quiet`, Q_PRIO=5, one q job running the loop)

| What | Build, threads | Before | After |
| --- | --- | --- | --- |
| The margin test alone (`--exact`) | release, 1 core | 0 of 50 | not rerun |
| The aio test binary, as the case runs it | debug, RUST_TEST_THREADS=4, 4 quiet cores | 2 of 50 (both margin) | see below |
| Same, diagnostic run | same | 6 of 60 (3 margin, 3 busy-session) | |
| The whole redoubt-client package (every test binary, console.rs included) | debug, 4 threads, 4 quiet cores | not measured on main | **0 of 50** |

The first fix (a drain before the wait only) still failed 3 of 50: 2 margin, now as "never sent",
and 1 busy-session. That run led to the real cause below.

The console.rs flake (fake lib.rs:818, seen once by B8's red) did not occur in the 50 whole-package
runs. It is not reproduced here, and nothing in this commit touches it.

## Cause

- **Margin test.** `until_parked` returns as soon as the server holds any call. Sometimes that is
  the session's opening call (`hub.connect`'s collect of 0), in the moment before the server
  answers it. The main thread then paused the server before the read reached it.
- **The diagnostic shows it:** at "never sent", the server was `paused true`, with 0 open calls.
- **So the read stayed queued.** `Hub::wait` rightly held at most `RETRY_US` (10 ms), and the call
  timed out at 1.011 s against the test's floor of 50 ms + 1 s. That is exactly the "gave up at
  1.011s" B18 saw.
- **The hub and the margin are both right;** the fixture raced.
- **Busy-session test** (`a_caller_busy_past_the_session_bound_keeps_its_session`) times out its
  10 s receive: its read was not taken within the submit's 1 ms. The test then slept 12 s and
  received with no poll, so the read was never sent. That breaks the hub's rule (B8: a caller
  that queues work re-enters the hub within RETRY_US).

## Fix

- **The margin test orders on the event.** The client raises `taken` once its read is taken, and
  the main thread waits for that before `until_parked`, so the only call left to park is the
  wait's. It also drains its queue before timing the wait.
- **A helper `sent(hub)`** polls until nothing is queued, with a 20 s bound and "never sent". It is
  called after each submit that is followed by an idle or a timed wait:
  - the margin test;
  - the busy-session test (both reads);
  - a_tag_a_flush_names_is_not_reused_before_its_rflush (read, flush, gate read);
  - two_connections_have_a_waiter_each_and_the_caller_idles_in_receive (after its 16 reads,
    B8's shape).
- **The other tests collect through `Hub::wait`,** which polls and bounds its hold itself.

## Gates on 5182dd06b, all rc=0

- prebuilt
- client-host-tests
- docs
- formatting
- size-budget
- unsafe-budget
- no-cruft
- the 50-run whole-package count above

One earlier measurement read "50 of 50 failed": the test file did not compile (a variable shadowed
`sent()`). I had committed without building. It was fixed, built, amended and rerun; that run is
not counted.

## Docs checked

- docs/userland/native.md "Many requests at once": its status list names the aio host tests by
  name, and none was renamed or added. The rule these tests now follow was written by B8. No
  change.
