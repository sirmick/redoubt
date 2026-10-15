# B70: the parked tests on a clock the test holds

- Base: main c8422f745. Head: wp-B70 089a17c37 (one commit, tree clean). Not pushed.
- Paths: libs/rt/fake/src/lib.rs, libs/rt/tests/parked_write.rs, libs/rt/tests/parked.rs,
  docs/SUMMARY.md (line removed), docs/todo/parked-write-clock.md (deleted).

## Gates (all through scripts/q)

| gate | command | result |
|---|---|---|
| 20 runs loaded, final tree | `q run --cores 1 -- sh -c '6 × (loop: cargo build -p testbench, CARGO_BUILD_JOBS=4, own target dir) & loop.sh 20'` | 0 of 20 failed (load 24–27 on the lease's one core, 1.4–1.6 s a run); earlier identical loop on the fix before a cosmetic tidy: 0 of 20 |
| 20 runs alone | `q run --cores 1 -- sh .tmp/B70/loop.sh fixed-alone 20` | 0 of 20, 0.04 s a run (was 0.40 s: the 300 ms wait is a tick) |
| rt host tests | `q run --cores 4 -- cargo test -p redoubt-rt` | exit 0, 27 result lines all ok (two pre-existing dead-constant warnings in tests/connection.rs, untouched) |
| the fake's other users | `q run -- cargo test -p redoubt-fileserver -p redoubt-client -p redoubt-littlefsd -p redoubt-bootfsd -p redoubt-erofsd -p redoubt-keyd` | exit 0, all ok |
| rt fuzz crate | `q run -- cargo +nightly-2026-05-11 check --manifest-path libs/rt/fuzz/Cargo.toml` | exit 0 (untouched; it does not depend on the fake) |
| docs | `q run -- cargo testbench docs` | PASS docs 7.0 s |
| formatting | `cargo +nightly fmt --check` | exit 0 |
| unsafe | libs/rt/src untouched; the fake (dev-only, not in tests/unsafe-budget.toml) keeps its 21, none added | unchanged |
| beamlet-redoubt's fake suite (the fake's largest user; covers `wait_held` at system.rs:288, :554, :661) | in userland/otp: `q run --cores 4 -- cargo test -p beamlet-redoubt --features fake --test system` | exit 0: 26 passed, 0 failed, 33.1 s (run after review, on head 089a17c37; 18 pre-existing "ambiguity" warnings from the crate) |

Logs: /home/mcloonan/redoubt/.tmp/B70/{alone,loaded,heavy,niced,fixed-heavy,fixed-alone,final-heavy}.txt
and the per-run logs beside them.

## Step 1: the capture

Not reproduced: 0 failures in 91 runs of the old test under four loads, all through q on one
leased core: alone (20); sharing the core with one `cargo build` (20); sharing it with six
concurrent builds, load 11–21 (20); the same with the test loop at `nice 19` under six builds,
load 20–42, runs taking 30–120 s (11 of a 20-run loop, stopped). The host was otherwise quiet
(load 4) with no bench running; the todo page's 14/20 was under busy loops on every core plus
benches at load 100–170, which q's leases do not give (each job has cores of its own, and a
`nice`d test starves every one of its threads alike, so the server still takes the call before
the client's 50 ms is checked).

The assertion and window therefore by analysis of the fake, not a captured panic:

- **Window 2 (the likely one), assertion parked_write.rs:125** `assert_eq!(server_thread.join()
  .unwrap(), 11, "one abandoned and one expired write")`, failing `1 != 11`. The client's first
  write is queued with a 50 ms timeout; if the server thread is not scheduled to `receive` it
  within 50 ms the fake withdraws the call unseen (`message`, "timed out while queued"), the
  client gets its `Timeout` as asserted, `open_calls` is already 0, the second write parks and
  expires, and the server returns 0 abandoned × 10 + 1 expired.
- **Window 1 (the other), assertion parked_write.rs:107** `assert_eq!(c.write(..b"lost"),
  Err(Sys(Timeout)))`: the write parked, the client's thread not run for ~250 ms past its 50 ms,
  the server's 300 ms deadline answers it first with `Rerror(Timeout)`; the final count would
  then also fail (`1 != 11`).
- The deadline assertion at :122 cannot fail: the server's park time is after the client's
  `started`, on the same host clock.

## Step 2: the ordering

The fake kernel's clock (`Fake::now`): the host's until a test calls `hold_clock()`; from then
`time_now`, and every `call`/`send`/`receive` deadline (`Fake::deadline`, in fake µs, compared
in `wait`), stand still and only `tick(dt)` moves them (under the state lock, with a
`notify_all`, so no waiter misses it). A wait on a held clock is a wait for a change, with the
same 60 s STUCK guard as a FOREVER wait. Mirrors the model kernel's `tick`. Opt-in per test
binary, idempotent; a binary that never holds it is unchanged.

parked_write.rs: the server counts its parks in a shared `AtomicU32`; the client's attach and
open are done first (so the only open call is the write); the first writer is a thread with a
50 ms timeout; the test waits until the count is 1, ticks 50 ms, then waits until the call is
answered; the second writer records `time_now` before and after; the test waits for the count
to reach 2, ticks 300 ms; the writer asserts `Rerror(Timeout)` and `waited >= LONGEST` (now
exact: the reply can only follow the tick). Ends as before with 11.

parked.rs: the retry loop for C is gone: C calls once with 100 ms, the test waits for the open
call, ticks 100 ms; D's server (1 ms deadline, read before the receive that takes the call) is
ticked 1 ms once D's call is open. `serve` returns its own counter; the Arc is gone.

## Summaries checked

- docs/servers/serving.md R28 test list: the three host test names unchanged (verified); the
  "Parked calls" text makes no clock claim. No change.
- docs/userland/native.md fake-kernel bullets (:375, :547): describe the fake as the installed
  Transport; no claim about its clock. No change.
- docs/todo/README.md: never listed the page (the index was docs/SUMMARY.md:76, removed).
- docs/testbench.md "host-clock case" is about QEMU `icount`, not the fake. No change.
- libs/rt/fake module doc: a sentence added on the held clock.

## Open

- No panic captured: the window is named by analysis (above). The fix removes both windows
  regardless of which fired.
