# B94 report: lease-writes' idle session reset by a single lost keep-alive answer

Branch `wp-B94`, base 3e6f29443 (train 88), head **f43fbebb2**, one commit:
`ipd: a silent socket lives through two lost keep-alive answers`. Tier A (servers/ipd).
Identification: `.wash/local/B94-identification.md`; cost: `.wash/local/B94-idle-cost.md`;
rulings: `.wash/local/wave-2026-10-18-rulings.md` "B94-cause" and "B94: the keep-alive's idle cost".

## Cause

ipd gave every established socket smoltcp's 60 s timeout (counted from the last segment
**received**) and a 30 s keep-alive (counted from the last segment **sent**), and smoltcp checks
the timeout before sending the probe. So an idle session lived on one probe and its answer per
30 s: one lost answer, or no ipd turn between T+30 and T+60, reset it at T+60. The book declares
a loss path for exactly that frame: netd drops a frame ipd has not taken within 50 ms. The code
departed from the page's "60 s without progress".

Not reproduced (solo, 2 runs beside a side load, and an exclusive whole `cases-rv64`: 4 PASS of
lease-writes on the probe build). The margin measured in the exclusive run: one netd drop (at boot),
no ipd timed wake 1 s or more late, every keep-alive answered within 1 ms except one, which the
host took 4.15 s to answer. Train 86's loss source stays unobserved; the fix is ruled on the
mechanism.

## Delivered

- `servers/ipd/src/stack.rs`: `PROBES = 3`; `KEEP_ALIVE = IDLE_TIMEOUT / (PROBES + 1)` = 15 s;
  `IDLE_TIMEOUT` stays 60 s. A silent socket survives two lost answers in a row, or no ipd turn
  for under 45 s after a probe whose answer it lost.
- `servers/ipd/src/fake.rs`: `Wire::lost`, the peer's next frames dropped before ipd takes them
  (netd's drop).
- `servers/ipd/tests/stack.rs`: `a_silent_socket_survives_all_but_its_last_lost_keep_alive_answer`
  and `a_silent_socket_lives_through_a_stall_after_a_lost_answer_until_its_timeout`. **Both fail
  with the old 30 s keep-alive** (checked by putting 30 s back).
- `docs/servers/ipd.md`: "A socket lives" restated (60 s with nothing from the peer); new bullets
  "A silent socket is probed" (the tolerance, the loss model, the exclusive run's margin) and
  "What a probe costs" (the per-probe path, ~66 cross-hart interrupts and 84 SBI calls per probe,
  the launch-idle numbers, the per-packet finding named without an id: K35); the state diagram's
  edge; the `/net` tree status 13 → 15 tests.
- `docs/servers/netd.md`: the drop sentence points at the keep-alive tolerance.
- `tests/size-budget.toml`: servers/ipd 2978 → 2984, with its `Size budget:` line.

## Gates (on the branch, `make -f scripts/jobs.mk ...`; all exit 0)

| gate | result |
| --- | --- |
| `cargo test -p redoubt-ipd` (q, 4 cores) | 16 test binaries ok |
| `set CASES="size-budget formatting docs ipd-host-tests"` (f07c53cfe) | 4 PASS |
| `set CASES="ipd-host-tests size-budget unsafe-budget"` (unsafe-budget, before the size fix) | unsafe-budget PASS |
| `set CASES="lease-writes steward-ssh-idle"` (f07c53cfe) | lease-writes rv64 553.6 s, rv32 550.4 s; steward-ssh-idle rv64, rv32: 4 PASS |
| launch-idle, alternating with base, every round alone (`.tmp/B94/idle/4`) | 12 PASS; busiest hart branch 2.9–3.5/s vs base 2.4–2.8/s (ceiling 3.8); user ecalls 8.8 vs 8.3 (12.2); host cores ≤ 0.005 (0.01) |

rv32 builds: ipd and netd built and booted for rv32 in launch-idle, lease-writes and
steward-ssh-idle. Not run by me: the full bench (the train's).

## Measured cost history

10 s keep-alive (first ruling) failed launch-idle 2 of 6 alternating rounds and the one quiet
round (3.916 vs base 2.6); the Architect re-ruled 15 s (`.wash/local/B94-idle-cost.md`). The
per-probe cost (~66 IPIs, 84 SBI calls) is the box's per-packet cost at rest: node K35.

## Affected summaries checked

README.md, GETTING-STARTED.md, SECURITY.md, docs/servers/sshd.md, docs/servers/serving.md, the
milestone pages: no keep-alive, idle-timeout or "without progress" claim (grep); no change needed.
servers/ipd has no README. docs/servers/ipd.md and netd.md updated as above. The Miri table in
ipd.md is a recorded run (`stack` pass); the two new tests were not run under Miri.

## Beside the work, not B94's

The exclusive `cases-rv64` (probe tree) also failed `launch-keep-disk` (`File.Error … connection
timed out` writing /home/alice/boots) and `steward-home-quota` (timeout waiting for
`{:wrote, 5120}`): write-path timeouts, not on ipd's path; logs `.tmp/B94/class1/other/`, not
rerun. Its other failures were the probe build's own.

## Evidence

`.tmp/B94/`: `probe.patch` (the probe build, never merged), `run1/`, `load1/`, `class1/` (pcaps of
every net boot, lease-writes' console, ssh and watchdog logs), `idle/1`–`idle/4` (windows.txt,
qls.log), `bin/` (the scripts).

## Review round 1 (servers-red P2-1, P2-2; the Architect's ruling)

- P2-1: launch-idle's ceiling stays 3.8. docs/testbench.md "The machine at rest" now states that
  main measures 2.4–2.8 alone and 2.9–3.5 with the 15 s keep-alive, so the ceiling catches a rise of
  about a tenth, not a doubling, and names the rise since it was set and the per-packet cost as
  findings (no ids; B101 and K35 in the plan).
- P2-2: docs/servers/ipd.md residual "Probes scale with an account's sockets": alice's 32 silent
  sockets at 4 probes a minute are about 2.1 probes/s, some 140 interrupts and 180 SBI calls a
  second, bounded by the manifest, paid at the servers' weights (R12), the interrupt work billed
  to the device's owner (R5).
- Folded into the one commit (f43fbebb2), with P3-1 (test 2's probe loop bounded and asserted), P3-2 (the 66/84 per probe named as the 10 s measurement), P3-3 (the Miri table notes the two new tests have not run under Miri); docs and size-budget PASS after it. Code unchanged
  since f07c53cfe.

## State and next

Branch clean at f43fbebb2, one commit, never pushed. Next: review (servers-red), then the
train's full bench. B97 (servers/ipd) may start: my ipd changes are stack.rs's constants,
fake.rs's `Wire::lost`, and tests/stack.rs.
