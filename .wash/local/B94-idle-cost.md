# B94: what the 10 s keep-alive costs an idle box (launch-idle)

Branch `wp-B94` 6af374608 against base 3e6f29443, `launch-idle` (4 harts, alice's SSH session
idle 60 s, ceiling 3.8 interrupts per hart per second, 12.2 user ecalls a second), one job at a
time, alternating. Raw lines: `.tmp/B94/idle/2/windows.txt`; `q ls` every 15 s:
`.tmp/B94/idle/2/qls.log`. "Beside" = other members' jobs running in that round's interval
(prebuilds, host tests), from `q ls`.

| round | width | beside | branch busiest hart | base busiest hart | branch / base supervisor ecall | branch / base user ecall |
| --- | --- | --- | --- | --- | --- | --- |
| 1 | rv64 | wt, B99 prebuilds | 3.1 | 2.1 | 13.0 / 7.4 | 9.3 / 8.3 |
| 1 | rv32 | B95 miri, prebuilt; SCHED5 | 3.3 | 1.9 | 13.4 / 7.0 | 9.3 / 8.3 |
| 2 | rv64 | B95 prebuilt | 3.1 | 2.0 | 12.4 / 7.1 | 9.3 / 8.3 |
| 2 | rv32 | B95 prebuilt; SCHED5 | 3.4 | 2.3 | 12.4 / 8.0 | 9.3 / 8.3 |
| 3 | rv64 | B95 host tests | **4.0 FAIL** | 2.8 | n/a / 9.4 | n/a / 8.1 |
| 3 | rv32 | quiet | **3.9 FAIL** | 2.6 | n/a / 8.7 | n/a / 8.1 |
| (idle/1) | rv64 | base's whole build | **4.3 FAIL** | (rv64 PASS) | 16.1 / – | 9.3 / – |

Per hart, the rise is all four harts' `m_software` (the kernel lock's hand-off and wake
interrupts): base sums to about 3.1/s over the four harts, the branch about 7.5/s (round 1 rv64).
So four more probes a minute (one socket: six a minute against two) cost about +4.4 `m_software`/s,
+5.6 supervisor ecalls/s and +1.0 user ecall/s: about 66 interrupts and 84 SBI calls per probe
across the machine.

`ipd` itself wakes once per probe: on the fake clock a silent socket over 120 s is 12 wakes, 12
frames out, and no poll delay under 1 s (a throwaway host test, not kept). So the per-probe cost is
the path's: `ipd`'s timer wake, its `transmit` call to `netd`, the card's answer interrupt, `netd`'s
receive thread and its send to `ipd`, `ipd`'s wake, each a cross-hart wake on an idle 4-hart box.

Reading: the ruling's condition "launch-idle and the idle ceilings unchanged" does not hold.
Branch rounds exceed 3.8 in 2 of 6 alternating rounds (and in the first loaded run), base in none,
and the one fully quiet round is a branch failure (3.916 against base 2.6).
