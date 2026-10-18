# IPD1 report

Branch `wp-IPD1`, worktree `.worktrees/IPD1`, base `f28960f34` (origin/main), head `f6dd7b468`,
one commit, tree clean. Nothing pushed. Reviewer: kernel-red-3 (the orchestrator assigns).

## Delivered

Ruling BEAM5 Q1 (`.wash/local/wave-2026-10-18-rulings.md`), option (A) as a serving rule.

- `servers/ipd/src/server.rs`: `DATA_WAIT_US` 30 s → 8 s, `CTL_WAIT_US` 60 s → `DATA_WAIT_US`;
  doc comments state the rule (strictly below the hub's `FILE_WAIT_US`, 10 s, by a 2 s margin
  that covers the kernel's delivery of the reply); module doc adds the sentence. No other ipd code
  change.
- `servers/ipd/tests/data_deadline.rs` (new): host test
  `a_data_read_with_nothing_coming_ends_with_the_data_deadline_and_late_bytes_wait_for_the_next`
  on the rt fake kernel, mirroring `ctl_deadline.rs`, with `redoubt_ipd::fake::Peer` (a second
  smoltcp stack) so a connection is established: the data read is still parked at
  `DATA_WAIT_US - 1`, answered `timeout` at `DATA_WAIT_US`; the peer then sends, the frames are
  pumped with no read parked, and the client's next read is answered at once with the bytes
  (`parked()` stays 0). Clock is the test's.
- `docs/servers/ipd.md`: "The `/net` tree" bullets (8 s, the client asks again, late bytes wait
  for the next read); "Pinned and abandoned calls" gains the rule paragraph with the arithmetic
  and the host test in its status line (tested 4 → 5); the 30 s / 60 s sentence rewritten.
- `docs/userland/files.md` "Files over 9P": the `FILE_WAIT_US` sentence no longer derives 10 s
  from "the file server's own longest wait" (the derivation the ruling found broken); states the
  rule and ipd's numbers, linking ipd.md.
- Stale statements of the old numbers, not ipd code: `tests/net-pinned.toml` description,
  comments in `tests/net/src/bin/net-judge.rs` and `tests/net/client/src/main.rs`. Flagged here
  because the ruling's scope names ipd and the two pages; the orchestrator may drop these
  three hunks if it prefers them untouched (they change no behaviour).

## Gates (every one through `scripts/q run` / `make -f scripts/jobs.mk`), logs in
`/home/mcloonan/redoubt/.tmp/IPD1/` (`gates-host.log`, `gates-fmt2.log`, `gates-boot.log`)

| gate | result |
| --- | --- |
| `set CASES=formatting` | FAIL first (new test unformatted) → `cargo +nightly fmt -p redoubt-ipd` applied → PASS (rc 0) |
| `set CASES=docs` | PASS |
| `set CASES=size-budget` | PASS |
| `set CASES=unsafe-budget` | PASS |
| `set CASES=ipd-host-tests` | PASS (whole crate incl. the new test) |
| `set CASES="net-tcp net-pinned"` rv64 + rv32 | PASS ×4; judge lines `ok: 64 abandoned reads, a read and an accept ended by ipd's deadlines, the echo: outcome 0` and `ok: echo through 10.0.9.100:7` + `TEST PASSED` on both widths |
| `set CASES=sshd-loopback-{env-refused,independent,interrupt,logins,r67,window-change-zero,window-change}` | PASS ×7 (rv64; these are host loopback cases, rv32 has no target in jobs.mk) |
| `cargo test -p redoubt-ipd --test data_deadline --test ctl_deadline` | 2 passed |
| `cargo test -p redoubt-sshd --test listener` (uses `CTL_WAIT_US`) | 2 passed |

Not run: the full bench, rv32 build of the whole system beyond the net cases' boots (the net
cases built and booted rv32 images), Miri. Unsafe count unchanged (no unsafe touched).

## Documentation check

- `README.md`, `GETTING-STARTED.md`, `docs/README.md`, `docs/servers/README.md`,
  `docs/milestones/*`: no claim about ipd's deadlines or wait numbers; no change.
- `docs/userland/beamlet.md` (constants table, `FILE_WAIT_US` 10 s), `docs/userland/native.md`:
  still true; no change.
- `docs/servers/sshd.md:162` ("asked again when ipd answers timeout"): true and now more often;
  no number stated; no change.
- `docs/servers/serving.md` "Parked calls": generic; no number; no change.
- `userland/otp/redoubt/src/files.rs:136-142` doc comment still derives `FILE_WAIT_US` from
  the file server's `COLLECT_WAIT` (both 10 s): true as a statement of equality; left alone, out
  of scope (hub crate). Could be reworded to the rule in a later package.

## Design notes / risks

- `CTL_WAIT_US` equals `DATA_WAIT_US` (8 s) rather than a separate constant: the ruling names
  both coming "below FILE_WAIT_US (8 s against 10 s)". A connect that neither completes nor is
  refused within 8 s now answers `timeout` and is asked again (sshd's listener does so already;
  the net client's `Connect` role reports `TIMED_OUT`, which no case judges on).
- The margin is stated, not measured: the kernel's delivery of a reply is microseconds; 2 s is
  the ruling's figure.
- The hub side (`FILE_WAIT_US`) is untouched; BEAM5's echo case lands on this.
