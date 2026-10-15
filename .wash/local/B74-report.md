# B74: a tick of the fake's held clock is a quiescent step

- Base: main 3a80e5af0. Head: wp-B74 c7e51670e (one commit; amended after review with the six dependents run and tick's doc rule). Worktree /home/mcloonan/redoubt/.worktrees/B74.
- Files: libs/rt/fake/src/lib.rs (the fix and its docs), libs/rt/tests/held_clock.rs (new: the test that would have caught it), tests/rt-miri.toml ("held_clock" added to the case's files).

## The reading, verified natively

The Architect's reading holds, narrowed by one line. `until(parks == 2)` (parked_write.rs:157) observes the park; the hole is not the whole gap between the park and the receive but the gap between the server's reading of the time at :97 (which fixes the receive's `timeout` at :103) and the `receive` at :104, whose deadline is `now + timeout` at the call (fake lib.rs `deadline`). A tick in the first part of the gap, between the park and :97, is harmless: the next reading sees it and `parked.expired` answers the write. A tick between :97 and :104 leaves the receive a deadline one LONGEST past the tick that no later tick reaches, and the FOREVER client waits until the guard.

Shown natively, before the fix, with a 20 ms sleep inserted just before `ep.receive(timeout, 0)` (not committed): `q run -- cargo test -p redoubt-rt --test parked_write` → exit 101, the same guard panic ("a wait only a change can end saw none in the fake kernel for 60s") and the same `late.join().unwrap()` line, finished in 60.10 s (.tmp/B74/repro-before.log). The same sleep after the park (in the `Ok(())` arm) did not fail: that confirms the narrowed reading. With the fix in place the sleep-before-receive variant passes in 0.14 s (.tmp/B74/repro-after.log).

## The fix (libs/rt/fake/src/lib.rs)

- `State` counts the fake's threads: `alive` (those `run`, `as_process` and `thread_create` make, through a `Counted` guard and a thread-local flag, counted once however they nest) and `waiting` (those blocked in `wait`). A thread that exits (`thread_exit`, whose host thread parks for good) leaves the count (`uncount`), as does one whose `run` or `as_process` ends, by panic too.
- `tick` is a quiescent step: under the state lock it waits on a new `quiet` condvar until `waiting == alive - (1 if the ticker is itself a thread of the fake)`, then moves the clock and notifies. If that takes STUCK (60 s) it panics naming the held clock's time and "n of m" threads waiting.
- `wait` increments `waiting` before blocking and signals `quiet`; its STUCK panic now names the clock (held or the host's) and its time, and the waits outstanding.
- Docs: the module doc and `tick`'s doc state the quiescent tick and why (the reading-to-receive gap); `wait`'s doc the naming. No docs page describes the fake's clock (B70's report says the same); the docs checker passes.
- No host timeout lengthened: STUCK stays 60 s, rt-miri's `timeout_secs` untouched. libs/rt/src untouched: unsafe count 18, as before. The fake (dev-only) gained no unsafe.

## The test (libs/rt/tests/held_clock.rs)

`a_tick_waits_for_the_receive_whose_deadline_it_ends`: a server reads the time, flags it, sleeps 50 ms, then `receive(300_000)`; the test ticks 300 000 as soon as the flag is up. Old fake: FAILED at 60.05 s with the guard (.tmp/B74/held-clock-old-fake.log, run with HEAD's lib.rs swapped in). New fake: ok; the receive ends with `Timeout` after exactly one tick. Added to rt-miri.toml (14 files now).

## Gates

| gate | command | result |
|---|---|---|
| rt-miri alone ×20 | loop of `make -k -j1 -f scripts/jobs.mk set CASES=rt-miri WIDTHS=rv64` | 20 of 20 ok, 0 failed; 58–87 s a run (.tmp/B74/miri-alone.log) |
| rt-miri beside load ×20 | the same loop beside a `scripts/q run -- cargo build` loop of another crate and the native loaded loop | 20 of 20 ok, 0 failed; 61–96 s a run, beside three `cargo build -p testbench` rebuild loops (own target dirs, CARGO_BUILD_JOBS=4) and the native loops; machine load 5–33 over the hour (.tmp/B74/miri-loaded.log) |
| B70's 20-run loaded loop (parked, parked_write, held_clock natively) | B70's shape: `q run --cores 1 -- sh -c '6 × (loop: rm target; cargo build -p testbench, CARGO_BUILD_JOBS=4, own target dir) & loop.sh loaded3 20'` | 0 of 20 failed, 2.0–2.3 s a run at load 30–33 on the lease's one core (.tmp/B74/loaded3.log, loaded3-load.txt); two earlier 20-run passes on their own lease beside the machine's load 5: 0 of 40 failed, 0.11 s a run |
| held-clock tests under Miri once (before the loops) | `MIRIFLAGS=-Zmiri-disable-isolation q run --cores 4 -- cargo +nightly miri test -p redoubt-rt --test parked_write --test held_clock --test parked` | exit 0: 1 + 3 + 1 passed |
| rt host tests | `q run --cores 4 -- cargo test -p redoubt-rt` | exit 0, 28 result lines all ok (27 before + held_clock); the two pre-existing dead-constant warnings in tests/connection.rs only |
| the fake's other users | `q run --cores 4 -- cargo test -p redoubt-fileserver -p redoubt-client -p redoubt-littlefsd -p redoubt-bootfsd -p redoubt-erofsd -p redoubt-keyd` | exit 0, 31 result lines all ok |
| the fake's six remaining dependents (review fold P2; their raw std::thread sites wrap `fake().as_process`, so the debug_assert on an uncounted waiter is what could have fired) | `q run --cores 4 -- cargo test -p redoubt-<crate>` each | all exit 0, no FAILED lines: consoled 21 passed (6 result lines), consrelay 26 (5), ipd 51 (14), piped 18 (5), sshd 34 (7), walfsd 35 (4); .tmp/B74/user-<crate>.log |
| beamlet-redoubt's fake suite | in userland/otp: `q run --cores 4 -- cargo test -p beamlet-redoubt --features fake --test system` | exit 0: 27 passed, 0 failed, 37.8 s |
| docs | `q run -- cargo testbench docs` | PASS docs 2.0 s (a first run failed C11 on "B74" in the new test's doc comment; removed, amended) |
| fmt | `cargo +nightly fmt --check -p redoubt-fake-kernel -p redoubt-rt` | exit 0 |
| clippy | `q run -- cargo clippy -p redoubt-fake-kernel --all-targets` | no warnings from the fake (pre-existing ones in redoubt-rt and redoubt-wire only) |
| unsafe | `grep -rc unsafe libs/rt/src` summed | 18, unchanged (libs/rt/src untouched) |
| rv32 | not run: the fake is dev-only host code; nothing that compiles for the machine changed |

Logs under /home/mcloonan/redoubt/.worktrees/B74/.tmp/B74/.

## Summaries checked

- README.md, GETTING-STARTED.md: no claim about the fake kernel's clock; no change.
- docs/testbench.md (:386, :1502) and docs/userland/native.md (:547): name the fake kernel's existence, its device memory and its users, not its clock; no change.
- docs/servers/serving.md, docs/servers/README.md status lines: "attacked in host tests with the runtime's fake kernel", unchanged in truth; no change.
- libs/rt has no README; the fake's module doc is its page, updated.
- The milestone pages do not mention the fake; no change.

## Risks

- A test that holds the clock and keeps a thread of the fake busy outside any `wait` (a thread blocked on a host channel, say) would now stall its tick until the 60 s guard, with a panic that names it. No test today does: only parked.rs and parked_write.rs hold the clock.
- `hold_clock` is binary-wide and tests in one binary run concurrently: a tick in one test now also waits for the other tests' fake threads to be in a wait or gone (parked.rs has three tests). The loops cover that.
