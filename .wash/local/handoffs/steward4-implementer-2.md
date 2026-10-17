# STEWARD4 handoff (steward4-implementer-2, 2026-10-17 ~02:30Z)

## Branch state

`wp-STEWARD4` at 3497d27ca = ONE `steward:` commit on main 68c4b2904 (train 48: B79 + STEWARD3Y),
folded (message in .wash/local/steward4-commit.txt); tree clean; size-budget PASS. Amend, never add commits (subject `steward: ...`, the
full account, the `Size budget: <crate>: <why>, A to B` paragraphs, last line `Co-Authored-By:
Claude Fable 5.1 <noreply@anthropic.com>`). Report file: `.wash/local/STEWARD4-report.md` (every
figure and finding, the gates table, the deviations). Rulings Q1-Q17: `.wash/local/STEWARD4-ruling.md`
(Q10-Q16 appended by architect-2; Q17 pending). Kept logs: `.wash/local/steward4-logs/`.

## Traps (each cost time)

- Kill by pid from the run-dir name (`run-<pid>-...`) or `q ls`; a `case` on cmdline text inside
  a /proc loop killed my own shell (exit 144).
- `expect_after` is strictly ordered over the whole console; sessions start only after every
  `expect` matched (end-of-run lines belong in `expect_after`); the steward's `served` line
  prints BEFORE the `Submitted` audit; `Pushed` prints before the crossing's exit line; the probe's
  counts print alphabetically (deadline_notice before decision_wake).
- A `[[file]] name = "manifest"` entry needs `verity = "image/userland.toml"` or the system
  volume has no root and the login's slot-5 connect is refused.
- A case with a manifest of its own lists `programs` (no `recipe`); every steward case listing
  programs needs `redoubt-crossing` since the image's `public` names it.
- Never `sed` a budget line by its text: it matched consoled/erofsd/sshd too (one spoiled run).
  Edit the latency manifest's steward entry by name (python regex on the `"name": "steward"` block).
- Under icount the wall time is short (idle skipped): a "77 s" case can have run three rounds.
- The notice `[an approval is waiting]` races the reply in the submitting session: one `(?s)`
  pattern covers both orders; the screen's two item lines are in random (id) order.
- `.tmp` and run dirs are pruned: copy logs to `.wash/local/steward4-logs/`.

## What is done (green)

- steward-declassify, steward-push, steward-confined-push: PASS both widths. The steward set
  (33 cases both widths, latency aside): 64/64 (server-restart rv32 passed alone; beside nine
  concurrent cases bob's lockout login once succeeded: timing).
- Host: steward server 47, init 69 (+ `confined_does_not_count_an_exempt_server_handed_a_labelled_endpoint`),
  crossing 4, oracle 64 tests incl. the marker test; elixir-oracles PASS; shell ExUnit 314 passed;
  docs PASS; unsafe-budget rv64 PASS; fmt clean.
- Q9 (LATENCY-END marker; oracle judges nothing after it), Q10 (confine.rs skips an exempt
  holder; init.md's bounding sentence verbatim), Q12 (alice-only), Q13 (B82 resolved: deadline
  leases 5 s, `smp = [1, 2]`, `gate_harts=2`), Q14 (latency manifest's steward weight 1000),
  Q15 (probe armed at the last point before its receive; `reply_wait` sample, the oracle's
  sixth measure, no target), Q16 (3U named in the report only), heap_pages 128 for the steward
  in every manifest (push OOM), pages: steward.md (sections, residual's measured numbers, the
  probe), init.md (confinement built with case 3, R34 15 tests), SECURITY.md R34 row aligned,
  serving.md + consoled.md residual bullets (Q12), testbench.md marker sentence, m3-agents.md.
- size-budget ceilings set to measured: wire 4153, steward 3082 (re-measure after the probe
  edits: the steward grew a little), crossing 185, init 2785.

## Open: steward-latency (the only unfinished item)

- Round 3 (two spinners) dies at the pinned seed in 4 of 5 re-armed runs: 3W's leak
  (`disconnect 16095599158017907573 at erofsd:system dropped after 4 attempts`, then the next
  lease's slot-5 connect refused on the steward's share's eighth State). Q17 asked architect-2:
  (a) rounds N = 1, 2 now, N = 3 removed until 3W (recommended); (b) drop the console session;
  (c) 3W first. On (a): in tests/steward-latency.toml remove the `round.(2, 16)` step and its
  expects, the N=3 count lines in expect_after, the probe's `LATENCY-END` must print after the
  SECOND round's counts (bin/steward.rs `Probe::flush`: `if self.rounds == 3` -> 2, and the
  marker check `probe.rounds >= 3` -> 2; the doc comments say "third round"), description text
  "N = 1, 2 and 3" -> "N = 1 and 2", marker files latency-round-1/2.
- Then: one confirming rv64 run (both harts) -> net figures incl. two harts; rv32 run; the
  scheduling.md "Responsiveness" paragraph "With the real steward" + table (one hart and two,
  weight 1000, re-armed; cells from the oracle's `N=.. (16): net p50/p99` lines; the ruling's
  sentence: "N = 4 and 16 are not reachable under the real steward: the box admits at most eight
  steward-minted system-volume connections, the steward's own share at erofsd:system, so the
  stand-in's columns at 4 and 16 stand alone until that cap is the account's"; the load-shape
  sentence; "a lease's end is measured to its destruction and notice; the servers' records are
  freed after, off the measured path"; the deadline-notice/destroy columns with pages destroyed
  per lease (5,248) and the gross-audit note (300-500 ms checked audits after a VM budget's
  destruction); if the image's weight stays 100 (owner's decision via the Architect), one
  sentence beside the table); R12's status already lists bench:steward-latency.
- The sweep: once when the case lands, `--sweep 1..20` through q (find the flag in
  tools/testbench/src/main.rs), ~6 min per run at one hart on rv64; report length first.
- Figures so far (weight 1000, re-armed, one hart, net µs p50/p99 N=1/2/3): timer wake
  23371/24087, 25814/27307, 26648/28351 (p50 missed: the wake pays for the loop's console work
  on its pass; 3U); decision wake 4085/7260, 5853/6299, 6429/10583; deadline notice p99
  28908/28432/31564; destroy_call 5793/7173, 6016/9532, 6021/9566; reply_wait 55917/56132,
  34309/37377, 41054/146503; R10 p99 9679; lease end 20262. Weight 100 and old-arm figures:
  the report.
- Steward work (residual, on steward.md): submit <= 100 ms rv64 / 95 ms rv32; end_lease of a
  VM lease <= 589 ms (rv64, one hart).

## Then

After train 54 (STEWARD3Z) publishes: rebase onto its main (the orchestrator gives the hash);
expect the sum rule on tests/size-budget.toml (rt 4048, consoled 423, ipd 2926 on main after
3Z: our raises as deltas over them), and check the crossing program's console fits consoled's
new default bucket of 2 consoles / 6 fids (init's override otherwise); the latency manifest's
consoled bucket count is the image's. steward-latency is `whole_run = false` until 3U.


size-budget rerun and exact ceilings; docs; fmt; fold; `git status --short | wc -l` = 0; reply
to the orchestrator with head and gates; reviewer steward-red-4; tell the orchestrator when the
package lands so STEWARD3W/3U/3Z may start.

## What consumed my context

Reading and re-reading run logs (console tails, session tails) for seven distinct case hangs;
five rulings' worth of thread traffic; the latency case's eleven boots; the predecessor's
handoff pointed at a 10-line file (its "full text" was itself).
