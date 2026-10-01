# The parked write test races the host's clock

## What

`libs/rt/tests/parked_write.rs` failed 14 of 20 runs in a loop on a loaded host (a busy loop on
every core plus benches, load 100 to 170), and 0 of 15 when run directly under load. It has the
shape of `parked.rs`: the runtime's fake kernel reads the host's clock, and the test races two
waits on it.

- The server parks a waiting write for `LONGEST` (300 ms), then refuses it with `timeout`.
- The client's first write gives up after 50 ms, and the server must count it abandoned. The
  second waits until the server's deadline answers it, and must not be answered before 300 ms.

The failure is not yet characterised: the panic message was not captured, so which assertion
fails is unknown. Two windows fit the shape. A client stalled past 300 ms gets the server's
`Rerror` instead of its own `Timeout`. A client whose 50 ms passes before the server parks the
write abandons a call that was never parked, and the server counts no abandoned write.

## Why it matters

A flaky test is a bug ([tenets](../TENETS.md#6-tested-to-hell-and-back)). It also hides whether
the parked-write path (park, abandon, expire) is right under load.

## Where

- `libs/rt/tests/parked_write.rs`, `a_waiting_write_is_parked_abandoned_and_expired`.
- The runtime's fake kernel's `time_now` and receive timeouts, which `parked.rs` shares.

## Done when

- The panic under the loaded loop is captured, and this page names the assertion and the
  window.
- The test orders on events, not on time: the first write is abandoned only once the server
  has parked it, and the server's deadline is a step the test takes on a clock it drives, not
  host time. Or each wait is on the parked state, with no deadline racing it.
- It passes 20 runs in a row in the loaded loop.
- This page is deleted.
