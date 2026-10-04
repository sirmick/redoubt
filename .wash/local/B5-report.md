# B5 report

Branch wp-b5, base main ca5a6437b, tip 90e789f66 (one commit).

## Moved (SHARE + sched_oracle, kernel_features = ["sched-trace"])

Shares in thousandths. Figures are from the focused runs; the whole-bench run gives the same numbers to within 3.

| case / share | rv64 net (gross, audits µs) | rv32 net (gross, audits µs) |
| --- | --- | --- |
| sched-exit-churn threads-exit | 492 (492, 0) | 494 (494, 0) |
| sched-exit-churn processes-exit | 495 (459, 143538) | 493 (459, 135124) |
| sched-exit-churn processes-fault | 497 (464, 129902) | 498 (468, 119464) |
| sched-timer-flood sleepers | 469 (469, 0) | 468 (468, 0) |
| sched-timer-flood sleepers-and-deadlines | 472 (464, 30632) | 471 (468, 14870) |

Bounds unchanged (450..1000). Load unchanged. The trace ring dropped no records, and everything fits in RAM.

## Left in the program, and why

- deadline-flood-billed: a release build (no debug_assertions), so it runs no audits (sched::audit is cfg(debug_assertions)). Its share is already net. A comment in its toml says so.
- sched-destroy-billing: it judges no share. It checks a latency bound, 2*cost + 4 slices, and `cost` is measured in the same checked build, so it includes the same destruction audit.
- sched-carve-return: its one destruction (CarveSpin) falls inside CARVED, before U's judged window starts.
- sched-share, sched-large-weight, sched-carve-inflation, sched-sleep-gaming, sched-server-busy, sched-idle-gap: their roles are Spin, Gamer, Server, Flood, SpinFrom and SpinGap. None creates or destroys a process or a budget inside the window. Audits run only at a destruction (AUDIT_DESTRUCTION) and when the process index changes (AUDIT_PROCESS_INDEX), so none falls inside these windows. I checked this by reading the code, not by tracing a run.

## Pages

- scheduling.md "Responsiveness": the two cases added (tested 9), the net and gross figures recorded, and the reason the other shares stay in their programs stated.
- scheduling.md R23: the two cases added to the list of cases that turn the trace on.
- scheduling.md residual risks: "Some shares are judged gross of audits" removed.
- testbench.md "Checked builds": the two cases added (tested 9), and the sentence on shares extended.
- docs/todo/shares-judged-gross.md deleted, with its SUMMARY line.

## Gates (all through in-dev)

- `cargo testbench sched-exit-churn`: exit 0
- `cargo testbench sched-timer-flood`: exit 0
- `cargo +nightly fmt --all --check`: exit 0
- `cargo run -q -p redoubt-doccheck`: exit 0
- `cargo testbench --allow-skip`: exit 0, 279 PASS, exactly 1 SKIP (bench-ssh-loopback-openssh). Log: .wash/local/B5-bench.log

## Outside the owned paths

- tests/deadline-flood-billed.toml gets one comment line. The todo's "Done when" asks for the exception to be stated in the case itself.
- No kernel change, no change to the sched_oracle code, no new host test.
- unsafe count unchanged (no Rust change outside the test programs).

## Fix round 1 (in progress)

idle-gap, run once traced (B: [start+alone+gap, end], C: [c_from, end]); rv64 / rv32:
- B: net 335 = gross 335, audits 0 µs / net 334 = gross 334, audits 0 µs
- C: net 328 = gross 328, audits 0 µs / net 330 = gross 330, audits 0 µs
No audit falls in either window, so idle-gap stays as it was (the probe was reverted).

carve-return moved as asked (SHARE u-after-return over [start+CARVED, end], sched-trace on, bounds 450..1000). Result: FAIL on both widths, U net 0 = gross 0 (V 988). Untraced, the same program gives U 494. The diff is in .wash/local/B5-carve-return-attempt.diff, and the files are reverted.

Cause, from the trace (rv64): U is budget 32. U is picked partway into a slice and carves, and its weight goes 1000 -> 1 (pass 31,020,455 -> 8,034,402,286, by the rule). Its 9 ms spin then crosses the slice end (R at 103,968,620,526), and at weight 1 its pass leaves it behind V for seconds. The destruction (Y) runs at 2,269,988 µs, after the window's end at 2,256,875. Its audit runs 2,269,991..2,284,830 µs, about 15 ms. The case assumes the carve, the spin and the destroy all fit in one slice of U's, which is a matter of phase. The trace build's phase breaks it. This is not a kernel divergence: the oracle finds every pick in rank order.

The untraced destruction audit is also about 15 ms long, so it most likely runs past start+CARVED (20 ms) into U's judged window. The red finding stands either way.

## Fix round 1: done (tip 597d4ae73, amended)

The orchestrator chose option A. CarveSpin now sleeps 1 ms first: a 1 µs receive does not block, which the trace showed. So the carve begins a fresh slice. U then counts until 9 ms after its wake, not 9 ms after the create. The carve's create was measured at 1014 µs on rv64 and 1098 µs on rv32; added to a 9 ms spin, that overran the 10 ms slice. U reports the time_now of the carve's return, and U's SHARE window is [return, end].
- Traced: rv64 496 net = 496 gross, audits 0 µs; rv32 497 = 497, audits 0 µs. The destruction's audit runs before the return.
- Untraced, the same program: 496 and 499 (the old program gave 494). The measurement is unchanged.
- idle-gap: traced once, no audit in B's or C's window (see above). It is unchanged.
- Pages: carve-return is in both status lists (tested 10) and in R23's list of traced cases, the lead paragraph has the new figures, and the per-case sentence now covers it. exit-churn processes-exit on rv32 is now 492 (the program image changed).
- Gates: nightly fmt --check exit 0, doccheck exit 0, exit-churn, timer-flood and carve-return each exit 0 on both widths, whole `cargo testbench --allow-skip` exit 0 with 279 PASS and 1 SKIP (bench-ssh-loopback-openssh).
