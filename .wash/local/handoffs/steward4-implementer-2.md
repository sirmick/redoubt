# STEWARD4 handoff (steward4-implementer-2, 2026-10-17 ~07:20Z, checkpoint at 70 %)

## Branch state

`wp-STEWARD4` = 79827b45a, ONE `steward:` commit on main 82a026733 (train 55: 3Z, B81-B86); the
rebase from 68c4b2904 onto 82a026733 is DONE (conflicts in docs/servers/walfsd.md and
tools/testbench/src/case.rs resolved; my serving.md/consoled.md residual bullets dropped, 3Z's words
stand; scheduling.md and the latency toml reworded for 3Z). Tree clean. The size budget merged
right: main's rt 4048, consoled 423, ipd 2926; mine wire 4153, steward 3113 (over main's 2527),
crossing 185, init 2785. The commit's FINAL message is prepared in
`.wash/local/steward4-commit.txt` (the pages list without serving.md/consoled.md, the N = 4/16
sentence in 3Z's wording): after the re-verification, `git commit --amend -F
.wash/local/steward4-commit.txt` (never a second commit; amend only when no case is running).

Two background jobs are running on the rebased tree, launched 07:05Z; let them finish, read:
- `target/jobs/reverify-a.out`: host tests (steward server, init, crossing, testbench), fmt,
  docs, size-budget, unsafe-budget, elixir-oracles, then `set CASES="steward-declassify
  steward-push steward-confined-push steward-latency"` both widths (latency fails on its own
  targets by design, see below: that FAIL is expected; the other three must PASS).
- `target/jobs/steward-set-2.out`: the rest of the steward set, both widths (expected 58/58; a
  timing fail of steward-server-restart beside many runs has happened once: rerun alone).

Report file: `.wash/local/STEWARD4-report.md` (every figure, finding and ruling; gates table to
finish from the two outputs). Rulings Q1-Q17: `.wash/local/STEWARD4-ruling.md` (Q10-Q16
architect-2's; Q17 the orchestrator's). Logs: `.wash/local/steward4-logs/` and
`$REDOUBT_TMP/steward4-logs/latency-rv64-seed3-smp2-trace.log` (the accepted two-hart trace).

## Traps

- Kill by pid from the run-dir name (`run-<pid>-...`) or `q ls`, never by cmdline text in a
  /proc loop (it killed my own shell).
- `expect_after` is strictly ordered over the whole console; sessions start only after every
  `expect` matched; `served` prints before `Submitted`; `Pushed` before the crossing's exit line;
  the probe's counts print alphabetically.
- A `[[file]] name = "manifest"` needs `verity = "image/userland.toml"`; a case with its own
  manifest lists `programs`; every steward case listing programs needs `redoubt-crossing`.
- Never `sed` a manifest budget line by its text (it matched consoled/erofsd/sshd); edit the
  steward's entry by name.
- `memory_mib` 2048 wraps rv32's address space (RAM at 0x8000_0000): silent hang; 1536 works.
- `--keep-failed` conflicts with `--sweep`; a sweep's `--jobs 2` needs `--ports 2` (B86, on main
  now). Under icount wall time is short; "77 s" can be three rounds.
- The 3W leak (`disconnect ... at erofsd:system dropped after 4 attempts`) is deterministic at
  seed 3 in round 3: that is why the case runs N = 1, 2 only.
- No edits or git moves while prebuilt/cases run; `.tmp` and run dirs are pruned: copy logs.

## Done (green before the rebase; re-verifying now)

Cases: steward-declassify, steward-push, steward-confined-push PASS both widths; the steward set
64/64. Host: steward server 47, init 69, crossing 4, bench 64 oracle tests + the probe's builder;
elixir-oracles; shell ExUnit 314; docs; unsafe rv64; fmt; size-budget. Rulings applied: Q9
LATENCY-END marker; Q10 confine.rs skips an exempt holder + init.md's sentence (verbatim from the
ruling); Q12 alice-only; Q13 (B82: 5 s deadline leases, smp [1, 2], gate_harts=2); Q14 steward
weight 1000 in the latency manifest; Q15 probe armed before its receive, `reply_wait` sample (the
oracle's sixth measure); Q16 3U named in the report only; Q17 rounds N = 1, 2, `whole_run =
false` on steward-latency with the reason in the toml and the sentence on scheduling.md. Heap 128
for the steward in every manifest. scheduling.md "With the real steward" paragraph + table
(seed 3, both widths, one and two harts): one hart meets everything but the timer wake's p50
(23-26 ms rv64, 53/26 rv32); two harts also miss the decision wake's p50 (rv64 N = 1; rv32 both)
and the deadline notice's p99 (both widths): the loop's console work (reply_wait 34-56 ms p50 one
hart, 94-164 two harts), STEWARD3U's. Steward work on steward.md's residual: submit <= 100 ms
rv64 / 95 rv32, a VM lease's end <= 589 ms.

Sweep so far (rv64 seeds 1..10 serial, `.wash/local/steward4-logs/sweep-rv64-a.log`): one hart
stable across seeds (timer p50 23.3-26.1 ms, decision p50 3.2-6.1, notice p99 26.5-31.3); two
harts: six of ten traces rejected by the oracle (`record N: a device interrupt returned without
its claim`, seeds 1-4, 8, 9: a kernel/bench trace finding, architect-2 has it; a sweep cannot
keep the failing log, so a replay `TESTBENCH_QEMU_SEED=1 cargo testbench steward-latency --arch
rv64 --keep-failed DIR` keeps one), the four accepted like seed 3's row. B86 facts sent to the
orchestrator (lease ports 23000-23001, --jobs 2, both boots bound 23000).

## Next

1. Read the two job outputs; rerun alone anything that failed beside others; fix nothing else.
2. `git commit --amend -F .wash/local/steward4-commit.txt`; `git status --short | wc -l` = 0.
3. Finish the report's gates table (commands and exit codes) and its state; report with
   member_update assignment_results (head, gates, reviewer steward-red-4); tell the orchestrator
   the package is in review so STEWARD3W/3U/3Z work may start ("say when").
4. During the review, on this tree: rv64 sweep 11..20, rv32 1..10 and 11..20 (`q run --cores 4
   --ports 2 -- cargo testbench steward-latency --arch W --sweep A..B --jobs 2`, ~100 min each),
   the seed-1 replay with --keep-failed; append to the report file (the page's table is the
   pinned seed's).
5. Open for the orchestrator: with 3Z on main the N = 4/16 rounds may boot (the six-principal
   manifest is in this report's history); not this package's run unless they say so.

## What consumed my context

Eleven-plus latency boots with their console and session tails read for seven distinct hangs
(expect order, verity, programs lists, memory on rv32, the leak, the arm point); five rulings'
worth of thread traffic with full figures each way; the predecessor's handoff was a 10-line
pointer to itself.
