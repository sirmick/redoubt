# SSH3 report: sshd's connections bounded in time before login and sized by the manifest

Branch `wp-SSH3` in `.worktrees/SSH3`, head `e8e883b63`, on `origin/main` 9a6600ede (train 63).
Two commits, clean tree: `b4051190e` (testbench: `[[net.dial]]` `after` and `until_closed`) and
`e8e883b63` (sshd: the bound, the slots from the manifest, cases, pages). Reviewer: steward-red-4.

## Delivered

- **The bound.** `servers/sshd/src/slot.rs`: `LOGIN_US = 5_000_000` with the arithmetic; `Gate`
  (started at the connection's start, closed by `logged_in`, `deadline`/`due`). `src/bin/sshd.rs`:
  the driver's receive wakes at the earlier of the console's deadline and the gate's; a connection
  due and not logged in is closed with no byte to the client and the console line
  `sshd: connection N: no login within 5 s, closed`; `login` and `approve` close the gate on
  success. Measured at main f28960f34, one run each width, ssh's start to the prompt:
  launch-system 2.01 s (rv64) / 1.70 s (rv32); steward-ssh-two-principals alice 2.00 / 1.78 s,
  bob beside her session 2.41 / 2.06 s. Twice 2.41 s = 4.82 s, rounded up to a second: 5 s.
- **The slots.** `sshd`'s entry carries `buckets=N` (image 4). `slot::slots(buckets, budget)`:
  slots = N, refused (`Unsized {slots, need, have}`) unless `52 N + 4` pages (driver 48 + reader 4
  a slot, watcher 4) fit `redoubt_rt::server::budget` (pages − heap cap − 128). The image's four
  take 212 of 512; nine fit, ten do not. `serve` checks it before any handle is used and exits
  `BAD_LIMITS` (5) with the arithmetic. `BUSY` is sized by the library's `MAX_BUCKETS` (32); the
  first N are used. No compiled slot count remains. The ipd bound (N + 1 ≤ in-flight cap and ≤
  sockets of ipd's bucket for sshd's badge, default or `limits=`) is held by the host test
  `every_manifests_slots_fit_ipds_parked_calls` over image/manifest.json and every manifest under
  tests/data, parsing ipd's args with `redoubt_ipd::args::parse` (dev-dep already present;
  serde_json `=1.0.143` added as a dev-dep for the manifests).
- **Manifests.** `buckets=4` on sshd's entry in image/manifest.json and the eight tests/data
  manifests with 3–4 domains; six-principals declares 8 domains, so `buckets=8` there and ipd's
  entry gains `limits=6:9:4:10` (8 parked reads + the accept; 8 connections + the listener + one
  refused). init's rule (N ≥ domains + callers) is what sizes N; sshd has no callers.
- **Bench.** `[[net.dial]]` gains `after` (dial once a console line matches; the console reader
  raises the dial's flag as it sends the poke) and `until_closed` (after the echo, read until the
  far side closes; the hold's length is noted under the case's result; still open at the deadline
  fails). Absent, a dial behaves as before. `finish_dials` takes the case's `out` for the notes.
- **Cases.** `sshd-login-bound` (both widths): after the console shell's line, four held dials to
  22 send a client version line and expect `SSH-2.0-`; expect lists four `connection N on slot
  [0-3]` lines, then four closes; sessions then start and alice's login's session line and prompt
  are the verdict; the dials write nothing on the console. Holds measured 5.08–5.14 s.
  `sshd-refuses-slots` (both widths): the image's entry with a 700-page heap cap leaves 196 for
  212; sshd's line, init's `exited, code 5`, `restarted`, `rebooting: sshd was restarted 5 times
  within 60 seconds`, the loader's line. A 256-page entry could not hold sshd's image (init
  "could not start" on rv32, the kernel terminating it on rv64): found by the first run.
- **Host tests** (`servers/sshd/tests/slot.rs`): `the_bound_is_twice_the_slowest_login_rounded_up`,
  `a_connection_not_logged_in_is_closed_at_the_bound_and_not_before`,
  `a_connection_logged_in_is_never_closed_by_the_bound`, `the_image_entry_holds_its_slots`,
  `every_manifests_slots_fit_ipds_parked_calls`; the two slot-return tests now derive the count
  from the image entry. Bench: `a_dial_with_a_line_waits_for_it`,
  `a_held_dial_ends_when_the_far_side_closes`. init: `the_image_manifest_passes_...` expects
  `(12, 4)` for sshd (servers/init/tests/manifest.rs, one line, outside the owned paths: the test
  pins the image manifest's bucket plan).
- **Size budget.** servers/sshd 1419 → 1483 (`Size budget:` line in e8e883b63).

## Pages

- sshd.md "Sessions over SSH": the box's platform bullet rewritten with two sub-bullets (the slots'
  derivation and fit with the numbers; the bound with the four measurements and the R82 argument);
  status 23 → 30 with the two cases and five host tests; "Residual risks" first-come item narrowed
  (bounded in time before login and by R82 after; what remains is the order among principals).
- init.md "Sizing": sshd 4 in the per-server counts, its slots.
- testbench.md: the `[net]` example's two dial fields; a "A dial" bullet; the section's status
  16 with the two host tests and the case; the memory-budget sentence ties sshd's four slots to
  its `buckets=4`.
- Summaries checked, no change: README.md, GETTING-STARTED.md, docs/plan/m3-agents.md (no slot
  or first-come claim), docs/SECURITY.md (rows cite R67/R68/R80; no slot claim), ipd.md "Sizing"
  (names overrides a manifest may add; sshd's bucket not named), serving.md. `libs/rt/src/server/admit.rs`'s
  `Override` doc says ipd gives sshd "two calls per session": a stale phrase, not mine to edit.

## Gates (exit codes; `make -f scripts/jobs.mk set CASES=...`, logs .tmp/SSH3/gates*.log, target/jobs)

PASS, rc 0: formatting, docs, size-budget, unsafe-budget, sshd-host-tests, init-host-tests,
host-tests, sshd-login-bound (rv64, rv32), sshd-refuses-slots (rv64, rv32), sshd-loopback-logins,
-r67, -interrupt, -independent, -window-change, -window-change-zero, -env-refused,
bench-ssh-loopback, bench-ssh-loopback-host-key, steward-ssh-two-principals (both),
steward-login-refused (both), steward-restart-ssh (both), sshd-restart-detaches (both),
launch-system (both), steward-six-principals (both), boot-profile-unverified (both), pipe-carries
(both). `cargo test -p redoubt-sshd`: 30 passed. Release builds of `redoubt-sshd` for
riscv64imac and riscv32imac: rc 0. Not run: the whole bench; the other steward boots whose
manifests gained `buckets=4` (many-sessions, latency, three-contexts, confined, idle,
vault-launch): a one-argument change init's own check accepts, left to the train.

## Reading

Read in full: slot.rs, sshd.rs, tests/slot.rs, Cargo.toml, peer.rs, the cases, refused.json,
every manifest hunk. qemu.rs, case.rs, main.rs: the module heads, the Console struct and `next`,
`run`'s head and the boot's call site, the `Dial`/`Net` structs; not the whole files.

## Design notes and risks

- Reading of the ruling: N = slots, bounded by ipd's parked calls (5 − 1 = 4 in the image), not
  (parked − 1) × N; the orchestrator confirmed. A fifth domain in the image needs `buckets=5` by
  init's rule and ipd's `limits=6:6:4:7`: the host test says so, before a boot.
- The bound runs from the connection's start to the steward's answer, so a login the steward
  holds past 5 s (its 30 s wait) is closed by the gate first: the client sees a closed connection
  in either case; no page claims the 30 s reaches a client.
- The gate's check sits at the driver's loop top, so a close is at most one receive wake late
  (the receive's timeout is the gate's deadline).
