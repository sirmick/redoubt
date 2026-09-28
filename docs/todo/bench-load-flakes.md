# Tests that behaved differently under load

## What

Four runs misbehaved under heavy load and were not all explained:
- a full run of the fuzz and host tests looked load-sensitive, in a change that touched none of
  the code those tests cover;
- `libs/rt/tests/parked.rs` stopped making progress at 0% CPU under six parallel heavy test
  binaries and had to be killed. It was a race in the test (an unbounded poll after the waiter's
  deadline expired) and the test was fixed by bounding it, but it shows how a test's timing
  assumptions break under load. The runtime's fake kernel, which the runtime's and the servers'
  host tests run on, now fails a wait with no deadline that sees nothing change for 60 s, so a
  stuck test fails rather than hangs;
- a combined bench run killed at its timeout, right after a run with its own `timeout` wrapper,
  seemed to leave an orphaned test process. It did not happen again in four clean runs. QEMU now
  ends with the bench that started it, however the bench ends
  ([what a case passes on](../testbench.md#what-a-case-passes-on));
- a whole-bench run, while another package ran its own cases on the same machine, failed
  `dma-reset-quarantine` on rv64 with `the checker did not report`. Five reruns on a quiet
  machine passed.

## Why it matters

Tenet 6 says a flaky test is a bug, in the test or the system, and is fixed rather than retried
([the tenets](../TENETS.md#6-tested-to-hell-and-back)). A test that passes only on a quiet machine
fails on a loaded one.

The host tests that wait on time are fixed with the servers follow-up package, which owns
`libs/rt`; the loaded runs close this page.

## Where

- [`libs/rt/tests/parked.rs`](../../libs/rt/tests/parked.rs) and the other host tests that wait on
  time.

## Done when

- Every host test outside the runtime's fake kernel that waits on time bounds its waits by its
  own deadline, not the machine's speed.
- The full bench and the host tests pass five times running under a stated parallel load.
