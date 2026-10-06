# B13 report: net shares never past the whole

Branch wp-B13, commit 5de701643 on main fa08fe2c8 (one commit).

## Cause
`sched_oracle` judged a share as `cpu * 1000 / (window - audits inside)`. `cpu` is the program's
calibrated count (`count * 1000 / rate`), an estimate that can run slightly over the work done.
When the victim takes nearly the whole window (the deadline variant, ~98% gross), subtracting
every audit leaves a net window smaller than the counted CPU: 1,960,670 µs counted, 2,000,000 µs
window, 46,022 µs audits gives 1003. My rerun on main reproduced it (1004, 49,076 µs audits, the
same CPU figure; the train-3 console had been overwritten, so the test uses the rerun's CPU and
the recorded audit total, which together give 1003 / gross 980).

## Fix (the arithmetic, not the bound)
`credit = inside.min(window - cpu)`; net = `cpu * 1000 / (window - credit)`. An audit holds the
hart and the counted work stops for it, so the net window holds at least the share's CPU. That
is the "clamp at the window less the other budgets' net work" option, with the others' work
taken at its lower bound (0), since the SHARE line carries only the victim's CPU. A net share
is at most the whole unless the gross already is (cpu > window still fails, rightly). Shares
under 1000 net are unchanged: the clamp binds only where the old net exceeded 1000. The report
line says `(credited N, the window less the share's CPU)` when the cap binds.

## Test
`host:testbench::shares_are_judged_net_of_audits` gains the churn case: a 46,022 µs audit in a
2 s window with 1,960,670 µs counted reads `net 1000, gross 980 ... (credited 39330, ...)`, and
asserts the old formula gives 1003. 34 sched_oracle host tests pass:
`jobserver share cargo test -q -p testbench --bin testbench sched_oracle` (rc 0). I didn't run
the whole `cargo testbench host-tests` case (the alone class).

## Gate (exact stems via jobs.mk, no -j)
- rv64/sched-budget-churn PASS (deadline: net 1000, gross 980, audits 49077, credited 39343)
- rv32/sched-budget-churn PASS (deadline: net 1000, gross 970, audits 71097, credited 58089;
  rv32 was also over the whole before the cap)
- rv64/sched-large-weight, rv64/sched-server-busy, rv64/sched-carve-inflation, rv64/sched-share
  PASS
- before the fix: rv64/sched-budget-churn FAIL (net 1004), reproducing the defect
- `cargo testbench docs` PASS, `formatting` PASS, `size-budget` PASS, `no-cruft` PASS;
  `cargo +nightly fmt -p testbench --check` rc 0

## Docs and summaries checked
- docs/testbench.md "Checked builds": one sentence beside B5's net-of-audits sentence (updated).
- tools/testbench/src/sched_oracle.rs module doc and `run`'s doc: both say how shares are
  netted, so both now name the cap (updated).
- tests/sched-budget-churn.toml description and comment: say "net of the checked build's audits
  inside its window" without the arithmetic. Still true, so not changed.
- docs/kernel/scheduling.md "Responsiveness" (l. 461-464) quotes the spinning-parent and shell
  victims' net shares. Both are under 1000 and the cap doesn't move them, so not changed.
- tests/programs/src/sched.rs `judged_share` doc: describes the SHARE line, not the netting,
  so not changed.
- README.md, GETTING-STARTED.md, the plan pages: none describe share netting (grep "net of.*audit").

## Risks
None found. The cap can't make a share with max < 1000 pass. It binds only when the old net
was > 1000, and then the result, 1000, is still above that max.
