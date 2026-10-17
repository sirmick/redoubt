# B88 report: the rv32 two-hart round with two spinner VMs

Branch `wp-B88`, head `6995a0a8a`, on origin/main `ee1fa75a4` (train 61, STEWARD3V in).
Worktree `.worktrees/B88`; evidence under `.worktrees/B88/.tmp/B88/` (findings.md is the
investigation's log; `keep-*` the kept run logs). Reviewer kernel-red-3. Three commits:

1. `c879ec8bb` kernel: a test-only `block-trace` feature records each thread's block (`@`), set
   by no case; +12 kernel lines (Size budget line in the commit); listed on kernel/README.md and
   scheduling.md's diagnostic features. The instrument that named the cause.
2. `ae3868229` verityd: the data cache is sized from the manifest's heap (`Caps::from_heap`:
   half the cap less a 48-page reserve, floor 4). Host test
   `the_data_cache_is_sized_from_the_manifests_heap`. Box unchanged at cap 100 (peak 50, init-boot
   both widths PASS at this commit alone). +27 lines.
3. `6995a0a8a` verityd: read-ahead of a request's run of cold blocks in one `blkd` call (at most
   `MAX_SECTORS / SECTORS_PER_BLOCK` = 8 blocks and the cache's size; no new constant), every
   block hash-checked through the tree before it is kept; a failing block fails the request and
   keeps nothing of the run. Host test
   `two_readers_share_the_hot_set_and_read_ahead_their_runs_with_the_manifest_cache`. The
   read-ahead buffer (8 pages) moves verity:system's peak to 58 (59 on userland-boot's
   flipped-block path), so `heap_pages` 100 -> 118 in image/manifest.json and the ten case
   manifests the memory cases scan; budget 256 unchanged; the testbench memory table updated;
   scheduling.md's "With the real steward" names the cause; steward-latency.toml's comment too.
   +31 lines (reserve becomes 56 with the read-ahead buffer).

## Cause, with the trace

The round is not reached because a lease VM's launch runs past the shell's 5 s `start_agent`
bound beside two spinner VMs, and the launch is compute-bound there, not bound by the verified
system volume's reads.

Block-trace run 3 (seed 3, rv32, 2 harts, `.tmp/B88/keep-blk`; `@` = a thread's block; budget
ids: steward 86 on endpoint 852, erofsd:system 72 / ep 851, verity:system 69 / ep 850,
blkd:system 66, shell VM 110, spinner lease VMs weight 50):
- steward (86): reply waits (its calls out) n = 3416, max 1.43 s; send waits to any server incl.
  erofsd 851: max 0.11 s; recv on its own ep 852: n = 313, max 70.4 s. It is never stuck in a
  mint; it waits for a call that never comes.
- the wedge (round 2 under block-trace's phase, t = 460-511 guest s): spinner VM 254 loads its
  code t = 460.1-478.5 (86 erofsd reads), verityd (69) the busiest server (315 reply waits); the
  shell (110) blocks send->erofsd 851, gets 11 reads in before its start_agent bound expires,
  `timeout`, the round's loop dies; from t ~478 erofsd/verityd/blkd are idle (nobody asks) while
  the spinner spins on both harts until its deadline (511).
- so: not a kernel lost wake (the servers run whenever asked), not the steward (its calls return).

The launch time (gap between consecutive `AgentStarted` audit lines less the session's 1.5 s
sleep; 7 s for the 5 s leases):
- rv32 2 harts, gate run with the fix (`.tmp/B88/keep-gate`): 3.9-4.0 s alone (N = 1), 3.5 s
  beside one spinner (N = 2), 5.3-5.7 s beside two (N = 3) -> past the 5 s bound, `timeout`.
- rv32 2 harts before the fix (STEWARD3W's stall2 log): 3.3-3.5 s alone / one spinner, > 5 s
  beside two. The diagnostic pair on one base (keep-meas vs keep-d4): 3.8 s alone either way.
- rv64 2 harts (3W's kept run): 2.2 s alone, 3.3 s beside two = x1.5, a third equal-weight
  runnable on two harts, under the bound.
- rv32 1 hart (3W's kept run): 2.5 s alone, 3.2-5.1 s beside two, under the bound.
Pinning number: beside two spinners the launching VM is one of three runnables of equal weight
(`sizes.agent`, 50) on two harts, its share two thirds of a hart; 3.9 s x 1.5 = 5.9 s > 5 s on
rv32, 2.2 x 1.5 = 3.3 s < 5 s on rv64. Cutting the read path's I/O did not move it (below).

## Measured (steward-stall-measure diagnostic, seed 3, rv32 2 harts; AgentStarted count /
data hit rate / blkd reads at the 1,024th request served; 36 = rounds 2 and 3 wedge, 68-71 =
rounds 1 and 2 complete, 3 wedges)

    no fix:              36 / 18.7 % / 2704
    cache 27 only:       36 / 24.4 % / 2515   (cold-read dominated: the cache alone fails)
    cache 27 + RA:       68 / 24.4 % /  784
    cache 92 + RA:       68 / 41 %   /  624   (I/O cut further, round 3 unchanged -> compute)
    cache  4 + RA (d4):  71 / 20 %   /  924   <- shipped: the floor, smallest that gives the win

Cache size decision: the floor (4) + read-ahead gives the same outcome as 27 and 92, so the
image's entry stays at the floor (cap 118 -> data 59 - 56 = 3 -> floor 4); a cap of 166 would
afford 27. Two-reader host test: data 4: req 320 held 0 checked 320 blkd 229; data 27 + RA:
held 180 checked 140 blkd 33.

## Gates (exit codes; every command under scripts/q or jobs.mk)

- steward-latency rv32 2 harts, seed 3, `--keep-failed`: rounds N = 1 and N = 2 complete
  (LATENCY-COUNT N=1/N=2 lines), round 3 dies on `start_agent` `timeout` beside the two
  spinners (session log `** (MatchError) ... {:error, :timeout}`), as expected; three `parked
  after 4 attempts` lines at erofsd:system. The run was killed at 7200 s by my launcher's
  background limit, before the case's own 10800 s timeout, so no exit code: the verdict would
  be FAIL at the timeout. Logs kept in `.tmp/B88/keep-gate/`. NOT rerun to the verdict (3 h on
  4 cores, and it blocks the worktree); say if you want it.
- Round 4 (3V's) is not reached on rv32 at two harts either, since round 3 is not.
- Host: `cargo test -p redoubt-verityd`: 17 passed (exit 0); `cargo test -p testbench
  every_manifest_entry_holds_its_buckets_at_their_caps`: 1 passed (exit 0).
- Commit 2 alone: init-boot both widths PASS at cap 100 (peak 50).
- Final tree at 6995a0a8a, both widths (`make -f scripts/jobs.mk set CASES="formatting docs
  size-budget unsafe-budget init-boot userland-boot userland-read-only steward-session-ends
  steward-ssh-two-principals beamlet-footprint beamlet-agent-footprint beamlet-compile-footprint
  verity-flipped-tree verity-wrong-root verity-signed verity-bad-signature verity-rollback
  userland-bad-start boot-profile boot-profile-unverified"`, exit 0): 36 PASS, 0 FAIL;
  verity:system heap 58 of 118 (x14) and 59 of 118 (x2); size-budget kernel 10820 of 10820,
  servers/verityd 580 of 580; unsafe-budget unchanged (verityd forbids unsafe).
- Earlier on the WIP tree: the same memory set passed at 118 (14x "58 of 118", 2x "59 of 118");
  at 116 userland-boot failed both widths ("heap needs 118 for twice its 59-page peak"), which
  set the cap. beamlet-agent-footprint failed once in a loaded set (`beamlet: Deadlock` after
  `agent: result written`, both widths) and passed on rerun alone: a flake of that case under
  load, not this change's path.
- fmt (`set CASES=formatting`), docs (`set CASES=docs`), unsafe-budget, size-budget: in the
  final set above.

## Documentation check (summaries searched for the changed claims)

- docs/servers/verityd.md: "Memory and cost" rewritten (cache sized from the heap, read-ahead,
  cap 118 / peak 58-59; the "Read-ahead is not done" residual resolved); "A cache of checked
  data blocks" gets the sizing arithmetic, read-ahead's rule and the measured figures; status
  lines carry the two new host tests and bench:init-boot.
- docs/testbench.md: memory budget table, verity:system 59 / 118. The prose above it ("twice
  the largest peak") holds.
- docs/kernel/scheduling.md: "With the real steward" names the cause and the launch figures,
  says round 4 is not reached there either; the rv32 two-hart column stays "not reached" (no
  complete run); the diagnostic features list gains `block-trace`.
- docs/kernel/README.md: test-build features list gains `block-trace`.
- docs/servers/erofsd.md:144 ("the blocks read again ... which verityd's cache holds"): still
  true; unchanged.
- tests/steward-latency.toml: the header comment names the cause instead of "takes nobody's
  call".
- README.md, GETTING-STARTED.md, docs/servers/README.md, docs/userland/beamlet.md: no claim
  about verityd's cache or read-ahead; unchanged (grep for verityd, cache, read-ahead).

## Residuals and follow-ups

- The rv32 two-hart rows at N = 3 and N = 4: MISSED, cause named: compute (the launching VM's
  two-thirds share beside two equal-weight spinners x the 3.9 s rv32 launch). The 5 s target
  does not move.
- What would reach the row: a session's launch against its leases' weight (session-vs-lease
  scheduler weight) -> the Architect, not this node.
- The steward's mints at shared servers wait within FOREVER (its disconnects are bounded, its
  mints are not; max observed 1.43 s here): its own node.
- The read path remains the heaviest server work under two loaders (verityd's hashing on rv32);
  a bigger entry (cap 166 -> 27 blocks) cuts reads a further 15 % but buys no row.
- `block-trace` is a kernel feature no case sets (+12 lines, ceiling raised with its line).
- Local, uncommitted: tests/steward-stall-diag.toml and tests/steward-stall-measure.toml (the
  diagnostics; the latter waits for a SCHED-TRACE-END it cannot print, so every run ends at its
  2400 s timeout: fix before reuse).
