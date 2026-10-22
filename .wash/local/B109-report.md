# B109: the companions' two-hart deadline notice judged where the harts run at once

Base main b6d28a6b6 (train 99: K35, B110's C14). Branch wp-B109, head 737fccffb: one commit, no WIP (review P3s folded: rv64's icount range 25 to 35 ms in the targets row, the companions' later 33.5/34.1 in their comments, n16-mttcg's re-derivation clause; docs and formatting rc=0 PASS on 737fccffb).

## Delivered (rulings 'B103: ...' and 'B109: the companions follow B103')

- `tests/steward-latency-n4-mttcg.toml` (new). steward-latency-n4's round (three spinners,
  D = 16), manifest, probe and session, with no icount, on the host's clock, in the quiet class,
  and `whole_run = false`. Boots rv64 at [1, 2] and rv32 at [1], as its companion does.
  - Judged: the deadline notice's p99 at two harts, 40 ms net.
  - Recorded: rv64:1 and rv32:1 at N=4.
  - Lease: 2 s.
- `tests/steward-latency-n16-mttcg.toml` (new). The same for N = 16 (fifteen spinners, D = 8),
  rv64 only at [1, 2]. Records rv64:1:N=16. Lease: 4 s.
- `tests/steward-latency-n4.toml` and `tests/steward-latency-n16.toml`: each adds
  `record=rv64:2:N=k:deadline_notice:p99` and a comment giving the turns reason. No target moves.
- `scripts/jobs.mk`: both twins are in the quiet list.
- `tools/testbench/src/case.rs`: the probe-builders test lists the twins, and its doc comment
  names them.
- `docs/kernel/scheduling.md`:
  - "With the real steward" has a paragraph with the twins' figures, lease derivation, N = 16 cost
    and sensitivity.
  - In the measured table, the rv64 two-hart cells at N = 4 and 16 are bold and marked recorded.
  - The targets row names the twins.
  - The QEMU-turns residual names the companions' cells and the twins.
  - Responsiveness is now tested (29), and R12 is now tested (72); K35 had made it 70 on main.
- `docs/SECURITY.md`: the R12 row lists the twins (C7).
- `docs/testbench.md`:
  - The host-clock list names the three steward-latency twins.
  - C14's counts follow the two new files: 359 boot, 171 host clock (twice), 144 disk and 75
    sockets.
- `docs/servers/steward.md`: the latency-probe builders. The page said "those five" and was
  already stale (it was missing steward-latency-mttcg and steward-launch-overlap). It now names
  nine, the list the host test holds.

## Leases (twice the worst LATENCY-LAUNCH over every run, rounded up to a second)

| Twin | Measuring runs (5 s lease) | Acceptance runs | Final runs | Worst | Lease |
| --- | --- | --- | --- | --- | --- |
| n4 | 0.69 s | 0.78 s (rv32 1h) | — | 0.78 s | 2 s |
| n16 | 1.48 s | 1.70 s (rv64 1h) | 1.53 s | 1.70 s | 4 s |

The n16 lease was first 3 s, from the measuring runs. The acceptance runs' 1.70 s put it at 4 s
(orchestrator agreed), so n16's three alone runs were redone on the final content.

## Runs (q run --quiet, each alone, all rc=0 PASS, every trace dropped 0)

Deadline notice, net p99 in ms:

| Boot | Measuring | Run 1 | Run 2 | Run 3 |
| --- | --- | --- | --- | --- |
| n4 rv64 2h (judged) | 8.6 | 7.0 | 6.7 | 7.6 |
| n4 rv64 1h (recorded) | 8.4 | 6.2 | 6.2 | 5.9 |
| n4 rv32 1h (recorded) | 8.9 | 7.9 | 6.0 | 6.3 |
| n16 rv64 2h (judged), 4 s lease | — | 9.5 | 10.1 | 8.2 |
| n16 rv64 1h (recorded), 4 s lease | — | 9.4 | 9.1 | 9.0 |
| n16 at 3 s (superseded) 2h | 9.6 | 10.1 | 9.7 | 10.6 |
| n16 at 3 s (superseded) 1h | 10.0 | 7.0 | 12.1 | 10.2 |

- The n4 acceptance runs (11:34-12:31Z) were on the pre-rebase tree. The n4 case file is the same
  as at head.
- The n16 final runs (13:20-13:46Z) were on the rebased content, whose only change since is the
  page figures.

## Icount companions with the new record cells

Two-hart rv64 runs on a48d8acf5, rc=0 PASS, both "met, recorded, not judged":
- steward-latency-n16: 34.1 ms
- steward-latency-n4: 33.5 ms

## N = 16 cost on the host's clock

The plan expected host time to be shorter than icount. It is longer here.

| N = 16 boot | Host's clock (twin) | Icount (companion) |
| --- | --- | --- |
| 2 harts | 292 s | 226 s (above), 246 s on 2026-10-20 |
| 1 hart | 229 s | 196 s on 2026-10-20 |

Icount with sleep=off skips the session's idle sleeps, which the host's clock waits out. The page
states this with the numbers.

## Negative

Inherited from B103, not redone: no cheap negative exists. The page states the twins'
sensitivity: it is steward-latency-mttcg's, the same notice path on the same clock, about
+30 ms of host time.

## Gates on 99fc225a4 (prebuilt rc=0)

- `make -f scripts/jobs.mk set CASES="docs formatting host-tests size-budget"`: rc=0, all PASS.
- `q run cargo test -p testbench only_steward_latency_builds_the_latency_probe`: rc=0, 1 passed.
- Not run: the kernel, rv32 builds and unsafe are untouched (only case files, tests and pages
  changed); no whole bench.

## Summaries checked

- Updated: scheduling.md, SECURITY.md R12, testbench.md, steward.md (all above).
- No change needed:
  - steward.md "Leases" ("the per-notice target, 40 ms, is met one lease at a time") is still true.
  - kernel/README.md:180-265, timer.md:356/402 and memory.md:181/743 name the stand-in's or the
    containment gate's notice, not this cell.
  - budgets.md:824 names the target.
  - README.md and GETTING-STARTED.md do not name the notice.

## Files read in full

I read in full the two new tomls and every page passage changed. For case.rs, jobs.mk and the
companions I read the changed regions.

Scratch files, consoles and run scripts are in /home/mcloonan/redoubt/.tmp/B109/.
