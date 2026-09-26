# Tests that behaved differently under load

## What

Three runs misbehaved under heavy load and were not all explained:
- a full run of the fuzz and host tests looked load-sensitive, in a change that touched none of
  the code those tests cover;
- `libs/rt/tests/parked.rs` stopped making progress at 0% CPU under six parallel heavy test
  binaries and had to be killed. It was a race in the test (an unbounded poll after the waiter's
  deadline expired) and the test was fixed by bounding it, but it shows how a test's timing
  assumptions break under load;
- a combined bench run killed at its timeout, right after a run with its own `timeout` wrapper,
  seemed to leave an orphaned test process. It did not happen again in four clean runs.

## Why it matters

Tenet 6 says a flaky test is a bug, in the test or the system, and is fixed rather than retried
([the tenets](../TENETS.md#6-tested-to-hell-and-back)). A test that passes only on a quiet machine
fails on a loaded one, and a bench that leaves processes behind skews the next run.

Belongs to no follow-up package: test and tooling work.

## Where

- [`libs/rt/tests/parked.rs`](../../libs/rt/tests/parked.rs) and the other host tests that wait on
  time.
- [`tools/testbench/src/qemu.rs`](../../tools/testbench/src/qemu.rs): how a timed-out boot's
  processes are ended.

## Done when

- Every host test that waits on time bounds its waits by its own deadline, not the machine's
  speed.
- A bench run killed at its timeout leaves no QEMU or helper process behind, checked by a
  self-check.
- The full bench and the host tests pass five times running under a stated parallel load.
