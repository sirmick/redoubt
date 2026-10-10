# B42 report

Branch `wp-B42` (worktree /home/mcloonan/redoubt/.worktrees/B42), off origin/main 5f66a4963.
Head e712b4ca6. One commit, because two of the three items were already fixed on main. Not
pushed.

## (1) K26: changed, e712b4ca6

On main the case already expected `sshd: connection N on slot [01]` for the third login, but that
did not judge the claim. sshd hands a connection the lowest free slot (sshd.rs, `(0..SLOTS).find`
on BUSY). With alice's slot leaked:
- bob would take slot 1 and give it back when he left;
- the third login would then take slot 1 again, and `[01]` would pass.

Fix: bob now stays logged in (mark `bob-in`, wait for `third-in`) until the third login is in.
The two connections hold slots at the same time, so they land on 0 and 1 only if alice's slot came
back; with hers leaked they would land on 1 and 2, and `[01]` fails. The description and the
comments are updated to match.

The run confirms it. rv64 log: alice `connection 1 on slot 0`; after the restart, bob
`connection 1 on slot 0`, then the third `connection 2 on slot 1` while bob was in.

Bob's own slot coming back on a normal close is not judged here. The host tests
`a_slot_returns_when_the_client_hangs_up_first` and `..._server_ends_the_connection_first`
cover it.

## (2) B34: already on main, no change

docs/userland/beamlet.md, "A fun's identity is its module's checksum", already lists it as the
third artifact: a fun decoded before its code is loaded stays apart from that code once it
loads with the fun's checksum ("BEAM's table then makes the two equal; here they compare
unequal").

## (3) B35: already on main, no change

All four tests/data/beamlet/{launch,natives,serve,natives-attack}.json give bootfsd
`heap_pages` 2176. That is twice the ~1,050-page rv32 peak, rounded up to 128, as B32's rule asks.
It came in with b95d0fda2, B35's own commit.

## Gates

- `make -f scripts/jobs.mk prebuilt`: rv64 and rv32, 0 failed.
- `make -k -j -f scripts/jobs.mk rv64/steward-restart-ssh rv32/steward-restart-ssh
  rv64/size-budget rv32/size-budget`: exit 0.
  - steward-restart-ssh: PASS on rv64 (21.4 s) and rv32 (21.2 s).
  - size-budget: PASS. It runs once, from rv64.
- `cargo run -q -p redoubt-doccheck`: exit 0.
- Formatting: no Rust or Elixir changed; the only file is a TOML case.
- beamlet's host tests and the beamlet-launch, natives, serve and natives-attack cases were not
  run, because nothing under items 2 and 3 changed.

## Summaries checked

- docs/servers/sshd.md (status lists bench:steward-restart-ssh; the slots paragraph at
  lines 209–218) and docs/servers/steward.md (Failure and restart status): still true, no change.
