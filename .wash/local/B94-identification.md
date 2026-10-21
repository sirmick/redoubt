# B94 identification: lease-writes' session closed under a whole bench's load

Base 3e6f29443 (train 88). No change on `wp-B94` yet. Evidence under
`/home/mcloonan/redoubt/.tmp/B94/`.

## The failure (train 86, before IPD2)

rv64 `lease-writes` failed at 198.9 s: `{:started, 16}` matched 15.3 s after ssh started, then the
idle prompt, then `Connection to 127.0.0.1 closed by remote host`, status 255. No guest line after
the sixteenth `served start_agent`: no ipd, sshd, init or steward line. The case is on the host's
clock (`forward = [22]`). Logs: `.tmp/train-86/fail/`.

## Mechanism (read from the code, confirmed in a capture)

- `servers/ipd/src/stack.rs`: every established socket gets `set_timeout(60 s)` and
  `set_keep_alive(30 s)`.
- smoltcp 0.14 (`vendor/smoltcp/src/socket/tcp.rs`): `timed_out` is `now >= remote_last_ts +
  timeout`, where `remote_last_ts` is the last segment received; the keep-alive timer is rewound on
  every segment *sent* (`rewind_keep_alive`). The timeout check runs in `dispatch` before the
  keep-alive is sent, and ends the socket with a reset.
- So an idle session lives on one exchange per 30 s: ipd's keep-alive at T+30, the peer's ACK. If
  that one ACK does not reach smoltcp, the next dispatch at T+60 finds 60 s since the last frame in
  and resets the socket before it sends the second probe. The same happens if ipd does not get a
  turn between T+30 and T+60 (no probe is sent at all). **Tolerance: zero lost answers, or an
  ipd stall of 30 s.**
- One way the answer is lost in the guest: `netd` drops any received frame that `ipd` has not
  taken within `SEND_TIMEOUT_US` (50 ms; servers/netd.md). On one hart with sixteen writing VMs,
  ipd missing a 50 ms window once is plausible; nothing retransmits a keep-alive's answer.
- What I have not excluded: the close was a FIN from sshd rather than ipd's reset (no guest line
  either way); slirp/QEMU's main loop on the host stalling. The capture distinguishes both.

## Probe (scratch only, never merged)

Worktree `/home/mcloonan/redoubt/.tmp/B94/probe` (detached, 34f7e8438 on 3e6f29443), patch
`.tmp/B94/probe.patch`:
- ipd says `ipd-probe: socket … timed out at T: send/recv queue; frames in (last at), out (last
  at), dropped in; receive late now / at most` for an established socket a poll closes, and any
  timed wake 1 s or more past its deadline;
- netd says `netd-probe: dropped frame N … offered at T, gave up at T'` for every frame ipd did
  not take;
- testbench: `B94_CAPTURE=<dir>` captures every boot's card (QEMU `filter-dump`, before slirp);
- host watchdog `.tmp/B94/bin/watch.sh`: `ss -tnoi` on the session's forwarded port every second.
- Artefact: rv64 `beamlet-footprint` fails `ipd: stack needs 5 pages … declared 4` (its measuring
  manifest only; lease-writes gives ipd 7 pages).

## Runs

| run | load | result | probe lines | session capture |
| --- | --- | --- | --- | --- |
| run1 | none | PASS 595.9 s | none | 16 idle gaps, each closed by ipd's keep-alive and slirp's ACK within 1 ms, 30.0 s apart |
| load1/01 | B93's 8-case set, both widths, repeating | PASS 552.4 s | none | 16 gaps, max 30.05 s |
| load1/02 | same | PASS 555.9 s | none | 15 gaps, max 30.05 s |

| class1 | exclusive `cases-rv64` (the whole rv64 class, 401 cases, 390 PASS) | PASS 603.5 s | one `netd-probe` drop, at 3.7 s (boot, before any session; the send held 60 ms); no ipd wake ≥ 1 s late; no timeout | 16 gaps; every probe answered within 1 ms but one: the probe at 114.420 s answered at 118.569 s (4.15 s between the card and slirp, i.e. in QEMU/slirp on the host), the next probe 31.76 s later |

class1's other failures are the probe tree's own (ipd stack +32 bytes: the three footprint
cases; netd 16 lines over its size ceiling; the probe's unformatted lines and package-id
comments: formatting, docs; `B94_CAPTURE` set for every case: testbench's
`peers_get_the_wider_network_and_a_capture`) except two write-path timeouts not on ipd's path:
`launch-keep-disk` (`File.Error … connection timed out` writing /home/alice/boots) and
`steward-home-quota` (timed out waiting for `{:wrote, 5120}`); their logs are kept in
`.tmp/B94/class1/other/`, not rerun.

The side load ran on other cores and did not contend lease-writes' leased core (both loaded runs
were faster than the solo one). Not reproduced. Rate in a whole bench: 1 failure in 1 (train 86);
WRITE1's runs 4-5 saw the same signature under the virtual clock, where a clock skip past 60 s
alone produces it.

## Next

An exclusive `cases-rv64` slot from the probe worktree (`.tmp/B94/bin/class.sh`), granted and
queued. A failure there names the component: a `netd-probe` drop of the ACK (guest RX path), an
`ipd-probe` late wake ≥ 30 s (ipd starved), the ACK missing from the capture (slirp/host), or a FIN
from the guest (sshd closed, not ipd).
