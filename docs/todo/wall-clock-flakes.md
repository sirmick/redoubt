# Four cases that race the host's clock

## What

Four cases pass alone and failed under a loaded host. Three order their steps with a fixed wait
on the host's clock, not on the event they mean to wait for. The fourth assumes no timer tick
lands in a window. None runs under `icount`, so a loaded host stretches the guest's steps past
the wait, or puts a tick in the window.

- **`uaf-lent-page`** (rv64, release kernel). The grabber calls `SYNC`, waits 100 ms, maps and
  stamps 64 pages, then asks the holder to `CHECK`. Its comment says the holder answers `SYNC`
  only once it holds the lend. `uaf-holder` answers `SYNC` whenever it arrives, so nothing orders
  the victim's lend, or the victim's death, before the grab. Its window is "the victim lends and is
  killed within 100 ms of the grabber's `SYNC`." When the window is missed:
  - the grab can come first, and `expect` (in order) sees `[grabber] stamped` before
    `[holder] holding page`;
  - or `CHECK` finds no lend ("test setup");
  - or, worse, the grab runs while the victim lives, and the case passes without testing the
    reuse it is about.
- **`touch-beyond-ram`** (rv64 and rv32, release kernel). The survivor waits 300 ms and then
  reports. The expected order puts the attacker's `refused after` line before the survivor's.
  Its window is "the attacker faults through 32 MiB and is refused within 300 ms." When it is
  missed, the survivor's line and `DONE` come first, and the case powers off without the
  attacker's line, or with it out of order.
- **`rt-host-tests`**, `libs/rt/tests/parked.rs`. The runtime's fake kernel reads the host's clock.
  - A's parked call expires after `LONGEST` (2 s), and B must wake it before then.
  - C's call times out after 100 ms.
  - D's call must outlive the 2 s deadline.

  Under host load 66 the run took 64 s against 4 s alone. A 2 s window on a host that slow can
  expire A's call before B's wake, and the assertion "answered without B's wake-up" fails.
- **`process-review`** (rv64 seen, both widths declared). The `zero_entries` child creates two
  threads with entry 0 and then calls `process_exit(43)`; its comment says neither allocation
  yields. A timer tick in that window runs an entry-0 thread, and the child dies of
  `InstructionPageFault` at 0 instead of `Exited(43)`. It failed 3 of 12 runs under load and
  0 of 20 run serially.

## Why it matters

A flaky test is a bug ([tenets](../TENETS.md#6-tested-to-hell-and-back)). `uaf-lent-page` can
also pass without testing anything, which is worse than failing.

## Where

- `tests/programs/src/bin/uaf-grabber.rs`, `uaf-holder.rs` and `uaf-victim.rs`;
  `tests/uaf-lent-page.toml`.
- `tests/programs/src/bin/touch-beyond-ram-survivor.rs`; `tests/touch-beyond-ram.toml`.
- `libs/rt/tests/parked.rs`, and the runtime's fake kernel's `time_now` and `sleep`.
- `tests/programs/src/bin/proc-review.rs` (`zero_entries`); `tests/process-review.toml`.

## Done when

- Each case orders on events, not on time:
  - The holder answers `SYNC` only once it holds the lend.
  - The grabber waits for the victim's end on something the kernel reports: the victim's exit
    notice, or the holder's abandoned-call notice for the held call, relayed by the holder.
  - The survivor reports only after the attacker's refusal and exit. The log server or the
    survivor learns it from the attacker's exit notice, not from a delay.
  - `parked.rs`'s deadlines run on a clock the test drives, so A's expiry, C's timeout and D's
    deadline are steps the test takes, not host time. Or each wait is on the parked state, with
    no deadline racing it.
  - `zero_entries`'s threads get an entry that parks or exits, or `process-review` runs under
    `icount`, whichever the case's purpose (distinct IDs for zero-PC threads) allows.
- A wait that remains states what it waits for, and a case fails if that event did not happen,
  rather than going on.
- Each case passes 20 runs in a row while the bench runs beside it under load (for example,
  `stress` on every core), on the widths it declares.
- This page is deleted.
