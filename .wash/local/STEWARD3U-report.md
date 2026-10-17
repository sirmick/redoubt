# STEWARD3U report (steward3u-implementer)

Branch `wp-STEWARD3U` on main 75a2c8b62, worktree `.worktrees/STEWARD3U`. Head: cbf6d2539 (one
commit, `git status --short | wc -l` = 0; 8 files, +588/-124), amended from fe9fc7eb4 by
steward3u-fixer: scheduling.md's table only (8 lines; the code, tests and toml are byte-identical
to fe9fc7eb4). Reviewer: steward-red-4.

## Gate 3: the final head (steward3u-fixer, 2026-10-17; logs .tmp/STEWARD3U/final/)

All on fe9fc7eb4 through `make -f scripts/jobs.mk` (env per the launch), the amend after:

| Gate | Command | Result |
| --- | --- | --- |
| steward-latency, both widths, seed 3 (the case's smp = [1, 2]) | `TESTBENCH_QEMU_SEED=3 make -f scripts/jobs.mk -k set CASES=steward-latency` | rv64 rc=0 (smp 1 and 2 PASS); rv32 rc=1: smp 1 PASS, smp 2 FAIL by design (timer wake N=2 p50 37249 µs missed; the oracle accepted this trace); make rc=2 (that leg). Logs latency.log, rv64-/rv32-steward-latency.log |
| the steward set (34 steward-* cases incl. steward-restart-reboot) + init-boot, both widths | `make -f scripts/jobs.mk -k set CASES="$(cat final/steward-set-cases.txt)"` | 35 cases, 68 PASS (the two host-tests cases run once), 0 FAIL, make rc=0. Logs steward-set.log, cases/ |
| docs | `make -f scripts/jobs.mk docs` | PASS rc=0 (docs.log); rerun on cbf6d2539: PASS rc=0 (docs-amended.log) |
| size-budget, unsafe-budget | `make -f scripts/jobs.mk -k set CASES="size-budget unsafe-budget"` | both PASS, rc=0; servers/steward 3280 of 3280 (budgets.log) |
| fmt | `scripts/q run --cores 2 -- cargo +nightly fmt -p redoubt-steward-server -- --check` | rc=0 (fmt.log) |

What moved against run 2 (the table that was on the page): every one-hart p50 within a
millisecond (worst rv32 N=1 timer wake 708 → 753); p99s, each the maximum of 16 samples, moved
more on both widths, worst rv32 N=1 decision wake p99 29770 → 38447 and rv32 N=2 destroy_call
p99 7667 → 13740 (both met, lease end 50111 → 50106), rv64 N=2 decision wake p99 20819 → 21974.
No case moved. Fix: the page's four columns are now this run's (one tree, one run, all figures
from the oracle's lines in the logs), the rv32 two-hart column on this run's accepted trace, and
its "accepted trace of the same tree" sentence reworded; nothing else cites the figures (grep over
docs/ and the toml). Run 3's figures are listed under Measurements.

## Bench and gates on the earlier trees (logs .tmp/steward3u/gate.log, gate2.log, run2/)

- Gate 1 (tree before the start-up turns and the ceiling's second raise): docs + the 34 other
  steward-* cases + agent-login, launch-system, shell-compile, shell-contexts,
  sshd-restart-detaches, init-boot, both widths: 40 PASS, 1 FAIL: steward-restart-reboot rv64
  timed out on `steward: users holds 0 pages` — the start's lines were queued and a steward
  dying in its console session's launch never reached a turn (exit 101, no line). Fix: a turn
  after the `users holds` line and after the carved lines (`machine.out.turn(0)`), commented.
- Gate 2 (final tree): docs, steward-restart-reboot, steward-restart, steward-boot, init-boot,
  both widths: all PASS. size-budget PASS against the commit (3113 → 3280 with its reason line);
  unsafe-budget PASS; fmt +nightly clean; host tests 51 passed; rv64/rv32 release builds clean.
- Not rerun on the final tree: steward-latency (run 2's tree differs only by two start-up turns
  at an idle console, scheduling.md, the toml's comment and the ceiling) and the 34 steward
  cases of gate 1 (the same difference). The 16 order-dependent cases passed in gate 1 with the
  drain before the reply (option b).
- The rv32 two-hart row stands on run 1's accepted trace; run 2's was rejected by the oracle's
  claim check (the two-hart trace finding's node is in the plan; not named on the pages, C11).

## Summaries checked

README.md, GETTING-STARTED.md, docs/README.md, docs/plan/m3-agents.md, docs/servers/init.md
(the consoled residual stays true and is now cited), docs/servers/consoled.md, docs/testbench.md
(line 792, LATENCY-END: unchanged): no claim about the steward's console cost or the latency
case's verdict outside scheduling.md and steward.md, both updated. SECURITY.md: no row names a
test that changed (the cleanup/console host tests are on steward.md's status line only).

## Open

- Thread STEWARD3U-console (architect-2, non-blocking): option (b) taken and stated on the page;
  if (a) is ruled instead, the pre-reply turn is one line to remove and 16 cases' expects move.
- Decision wake at rv32 one hart N=1 reads 23.8/29.8 ms (met; was 5.3 before) while N=2 reads
  0.9 ms: the arm is now after the sample lines' writes and before the receive, so this is a
  wake the kernel's trace could attribute (consoled draining the UART at equal weight?); not
  measured further, noted for the Architect.
- STEWARD3W: not rebased (no word yet); both queues run at the loop's turn, cleanup steps first,
  then lines, as the page says.

## Delivered

- `servers/steward/src/console.rs` (new, library, host-tested): the console queue. `Queue`
  holds the lines said, in order; `Sink` is the console's side (open the fid once, one write per
  line). A turn (`Queue::run`) opens the fid if not open, writes the lines due in order, stops at
  the first the console did not take within the bound (`Timeout`: the line waits `RETRY_US`,
  `TRIES` attempts, then dropped and counted); any other failure drops every line queued,
  counted, closes the fid; the next turn re-opens and says the count first. `Queue::flush` is the
  exit's last turn. Constants reuse `cleanup::{RETRY_US, TRIES}`.
- `servers/steward/src/bin/steward.rs`: `say` (a fresh Lend + attach/open/write/close per line)
  is gone; `Console` owns the one attached `/dev/cons` connection (the same one the UART
  session's console is minted through: no second `Tversion` can end the kept fid), a second
  `Nine` client on it bounded by `CONSOLE_US` (= `NOTICE_US`, 100 ms), one `Lend` page and the
  fid (taken from the connection's pool). Every line is `machine.out.say(..)` = a push. Drains:
  (1) before a call's reply (`after(..); machine.out.turn(now); finish(..)`), so a call's records
  precede the caller's own line (16 cases match that order: see the thread); (2) at the loop's
  top after the cleanup steps, `wait` folding in the queue's due; (3) after the probe's counts
  flush; (4) `serve` wraps `run` and flushes at exit. Module doc updated.
- `servers/steward/src/lib.rs`: `pub mod console`.
- Host tests (`servers/steward/tests/steward.rs`, `Paper` sink): one write per line on the kept
  fid (+ long lines in iounit pieces); a timeout costs one call, keeps order, retries at RETRY_US,
  drops after TRIES with the count said first; a refused/dead write drops and counts, re-opens
  later, count said first, an open refused costs one call a turn; the exit flush.
- Pages: steward.md "Failure and restart": new bullet beside the cleanup queue (mechanism, the
  two drains, order with the cleanup queue: steps first, then lines, so a step's own line goes in
  the same turn; the one-bound-per-queue rule; drops, counts, duplicates residual; consoled's
  restart per init.md's residual; the argued bound) + status line (4 host tests) + the probe
  paragraph's `reply_wait` description. scheduling.md table/toml: see Measurements.
- `tests/size-budget.toml`: servers/steward 3113 → 3269 (the queue module and the kept fid).

## Design question raised (thread STEWARD3U-console, architect-2)

The ruling puts every line at the loop's turn; the bench matches `expect` in order and 16 cases
expect the steward's audit record before sshd's `login …: session` line written on the reply.
Took option (b): a call's lines drained before its reply (one write each, one bound at most),
all other lines at the turn. Also: init never re-hands consoled to a running server, so the
re-open after consoled's restart is refused until the steward restarts; stated on the page.

## Gates

- rv64 and rv32 release builds of servers/steward (default and `latency-probe`): clean.
- Host tests `cargo test -p redoubt-steward-server`: 51 passed (steward.rs), all suites ok.
- `cargo +nightly fmt -p redoubt-steward-server -- --check`: clean.
- unsafe-budget: PASS. size-budget: FAIL until the raise is committed with its reason line.
- docs: PASS.
- Bench: (filled below).

## Measurements (seed 3; net µs p50/p99 at N = 1 / N = 2; logs .tmp/steward3u/run*/)

Run 1 (lines queued; the decision timer still armed at the timer wake, before the drain):
- rv64 1 hart PASS: timer wake 1962/3252, 766/1448 (was 23362/24089, 25887/27735); decision
  wake 11500/12465, 1163/3024; deadline p99 9193, 10505; destroy_call 6008/7100, 5020/6167; R10
  p99 9664; lease end 22129; reply_wait 6475/6498, 6556/6709 (was 55911/… , 34389/…).
- rv64 2 harts PASS: timer wake 7986/11350, 2568/23534 (was 40365/49236: p50 missed); decision
  wake 2327/3176, 4895/19933 (was 28806/42572 missed); deadline p99 12271, 11742 (was 49511,
  55108 missed); reply_wait 17907/42749, 17388/20826 (was ~116/94 ms p50).
- rv32 1 hart FAIL: timer wake 717/1324, 1180/1982 met (was 53310/57418 missed); decision wake
  N=1 38776/39422 MISSED p50 (every sample ~39 ms: the two sample lines drained between the arm
  and the receive, the same placement flaw Q15 fixed for the timer), N=2 1045/1413; deadline p99
  10473, 12033; reply_wait 6787/6822, 8200/8221 (was 34708, 36833 p50).
- rv32 2 harts FAIL: timer wake 3730/5799, N=2 38408/44605 MISSED p50 (was 58412/84010,
  61174/66142); decision wake 6786/17146, 3686/56101 met (was 55659/69864 missed); deadline p99
  13372, 23799 met (was 80148, 86247 missed); reply_wait 70866/71076, 29049/43783.
Fix after run 1: the decision timeout is armed at the last point before the receive, as the
timer's is (`Probe::decide`). Run 2 (the page's figures):
- rv64 1 hart PASS: timer wake 1961/3252, 766/1854; decision wake 1120/11416, 17270/20819;
  deadline p99 9835, 11622; destroy_call 6047/8701, 6077/6183; R10 p99 9664; reply_wait
  6473/6499, 6549/6583.
- rv64 2 harts PASS: timer wake 7997/13847, 2796/23494; decision wake 1883/7648, 1954/4538;
  deadline p99 12269, 12377; destroy_call 7999/8624, 8501/11329; R10 p99 9745; reply_wait
  17906/42748, 17396/29000.
- rv32 1 hart PASS: timer wake 708/1318, 1176/1313; decision wake 23751/29770, 865/40483 (met);
  deadline p99 11628, 13311; destroy_call 6704/7897, 5853/7667; R10 p99 9628; reply_wait
  6785/6818, 8198/8227.
- rv32 2 harts FAIL: the oracle rejects the trace (`record 1940487: a device interrupt returned
  without its claim`), the two-hart trace finding STEWARD4 saw at this seed under load; its row
  on the page is run 1's accepted trace (timer wake N=2 p50 38408 missed; the rest met).
Every target at one hart is met on both widths at N = 1 and 2 (the brief's condition for the
whole run), but the toml judges two harts too (gate_harts=2, as STEWARD4 shipped it, B82 done)
and rv32 at two harts misses the timer wake's p50 at N=2 on an accepted trace and is rejected
by the claim check on another, so whole_run stays false with that reason.
Run 3 (fe9fc7eb4, the page's figures now; logs .tmp/STEWARD3U/final/):
- rv64 1 hart PASS: timer wake 1962/3250, 734/1448; decision wake 1138/11072, 17291/21974;
  deadline p99 11103, 10386; destroy_call 6053/7210, 6075/8359; R10 p99 9664; reply_wait
  6470/6498, 6544/9354.
- rv64 2 harts PASS: timer wake 7985/8017, 3372/23534; decision wake 1946/18788, 2323/4594;
  deadline p99 11400, 11998; destroy_call 8000/8639, 8237/10750; R10 p99 9744; reply_wait
  17913/42748, 17409/21366.
- rv32 1 hart PASS: timer wake 753/1323, 1198/2735; decision wake 23739/38447, 1233/40484;
  deadline p99 10752, 11996; destroy_call 6702/7851, 6731/13740; R10 p99 9622; reply_wait
  6795/6827, 8194/8237.
- rv32 2 harts FAIL (by design, trace accepted): timer wake 3579/6510, 37249/42998 (N=2 p50
  missed); decision wake 2470/4637, 2679/6342; deadline p99 11433, 23443; destroy_call
  9201/9912, 10257/13914; R10 p99 9751; reply_wait 69380/70922, 28961/80391.
reply_wait is no longer the console's cost: a call's lines go before its reply (option b), so
the gap is the loop's own work and the reply's hand-over to the caller (~6.5 ms at one hart,
both widths; 17-18 ms p50 at two), with no console call in it.
