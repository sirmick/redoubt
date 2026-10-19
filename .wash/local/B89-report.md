# B89 report: sshd-login-bound orders on the accepts it saw

Branch `wp-B89`, worktree `.worktrees/B89`, base `5bfbd2270` (origin/main), head `0ae1d1c9b`,
one commit, tree clean. Delivered: `tests/sshd-login-bound.toml` only. Never pushed.

## The cause, as found

Train 74 run 2, rv32 (`.tmp/B89/fail-rv32-sshd-login-bound*.log`, copied aside): the four
holders dialed at once on the shell's line; the bench peer held the fourth dial 11.60 s against
the others' 5.88–5.98 s. Console: `connection 1 on slot 0`, `2 on slot 1`, `3 on slot 2`, three
closes at the bound, then `connection 1 on slot 0` again (the fourth holder, a reused socket
number on the freed slot) and its close. Three concurrent at most, slot 3 never used, no fourth
distinct accept; the expect's fourth close never came, 900 s timeout. The expect assumed all four
accepted before the first closed: the host's clock, not an event. `buckets=4` is right.

## The change

- Dials 2–4 carry `after = 'sshd: connection [0-9]+ on slot {0,1,2}$'`: each waits for the
  previous holder's slot line (`peer::Dials::starters`, the console raises the flag on the first
  match, order-independent), so no SYN is in flight while a slot is free.
- The expect names `on slot 0`, `1`, `2`, `3` in order (sshd takes the lowest free slot,
  `servers/sshd/src/bin/sshd.rs:987`), so slot 3 is actually used and a fourth holder that could
  not get a distinct slot is a miss, never passed over.
- Comments on the expect and the dials say why; no package ids in the file.

## Escalation

Did not fire: with the dials serialized the fourth holder got slot 3 every time (80 of 80
boots). No sshd change, no page change.

## Gates (all through `make -f scripts/jobs.mk -C .worktrees/B89`, prebuilt first)

| gate | result |
| --- | --- |
| `prebuilt` | exit 0, 7m33s (`.tmp/B89/prebuilt.log`) |
| `set CASES=sshd-login-bound` alone | exit 0: rv64 9.5 s, rv32 9.1 s (`.tmp/B89/alone-1.log`) |
| loaded set `set CASES="sshd-login-bound sshd-refuses-slots sshd-loopback-logins sshd-loopback-independent sshd-restart-detaches"` | exit 0; login-bound rv32 9.3 / rv64 9.5; refuses-slots 0.4/0.5; loopback-logins 0.1; loopback-independent 0.3; restart-detaches 19.4/20.6 — the same verdicts and times as train 74's (`.tmp/B89/loaded.log`) |
| 20 × `set CASES=sshd-login-bound` | 20/20 exit 0, both widths each run; rv32 9.2–10.4 s, rv64 9.5–9.7 s; host loadavg 4.5–8.7 during the runs (other members' work); every holder of the 80 boots held 5.02–5.05 s (`.tmp/B89/gates.log`, `rep-<n>-rv{32,64}.log`) |
| `set CASES=formatting` | exit 0, PASS 21.3 s |
| `set CASES=docs` | exit 0, PASS 4.3 s |

Per-run verdicts and times: `.tmp/B89/gates.log` lines `run 1` … `run 20`.
`steward-ssh-two-principals` was not rerun (a quiet-class case, untouched by this diff; train 74
had it green both widths).

## A serialized run's console (rv32, `.tmp/B89/pass-sshd-login-bound-rv32-smp1.log`)

```
[con a4011145e248bf08] Redoubt shell, on Elixir 1.20.4. `exit` or Ctrl+D ends it.
[con 0b9b90a9750f3188] sshd: connection 1 on slot 0
[con 0b9b90a9750f3188] sshd: connection 2 on slot 1
[con 0b9b90a9750f3188] sshd: connection 3 on slot 2
[con 0b9b90a9750f3188] sshd: connection 4 on slot 3
[con 0b9b90a9750f3188] sshd: connection 1: no login within 5 s, closed
[con 0b9b90a9750f3188] sshd: connection 2: no login within 5 s, closed
[con 0b9b90a9750f3188] sshd: connection 3: no login within 5 s, closed
[con 0b9b90a9750f3188] sshd: connection 4: no login within 5 s, closed
[con 0b9b90a9750f3188] sshd: connection 1 on slot 0          <- alice's ssh
[con 0b9b90a9750f3188] sshd: login alice: session d8c44d540d54fb9b, labels []
```
(rv64 copy beside it, the same shape.)

## Summaries checked

- `docs/testbench.md` §Peers, dials and the capture (line ~1286): says the case holds every
  sshd slot with held dials and the verdict is the login the box serves — still exactly true; no
  change. Its status list names `bench:sshd-login-bound`, unchanged.
- `docs/servers/sshd.md` §Sessions over SSH status list names `bench:sshd-login-bound`; the
  slot behaviour described (lowest free, closed at the bound) is what the case now asserts more
  precisely; no change.
- `README.md`, `GETTING-STARTED.md`, `servers/sshd/README.md`: no mention of the case or of
  "four at once"; nothing to reconcile.
- The case's own `description`: "four clients connect … holding every slot of the image's four"
  remains accurate (they still all hold at once; only the dialing is sequenced).

## Open risk (not a finding, outside scope)

Why the fourth SYN of a simultaneous burst reached the guest ~5.7 s late is not established
here: a dropped SYN and a retransmit (3 s + …) would fit, as would a listen backlog of three in
the guest's stack. The serialized case no longer depends on it; `sshd-refuses-slots` (five at
once, one refused) is unchanged in verdict. Worth a look if another burst-dial case flakes.

## Next step

Review by kernel-red-3 (the orchestrator's pick); merge on OK.
