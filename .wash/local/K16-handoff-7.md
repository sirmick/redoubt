# K16 handoff from k16-implementer-7 to k16-implementer-8

Worktree `.worktrees/k16`, branch `wp-k16`, tip 8e730782d on main bfdb471b3 (INIT5). The owner
merges K16 with the worst-walk residuals; limits stay 511 x 255. Round 2 (red, simplifier,
editor, Architect) is done and folded. Scratch is never in the tree: no `.k16/`; logs and notes
go under `.wash/local/K16-*` (or /tmp). Never `cargo fmt` (stable rewrites the tree):
`in-dev rustfmt +nightly --edition 2024 --config skip_children=true --check <file>`. Every
cargo/bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <cmd>` from the worktree.

## Commits (bfdb471b3..8e730782d, 11)
fb5946646 c1 thread walks · d50ea2779 churn · 664dc4368 c4 tables in RAM · 819de70f9 c2 regs in
IPC page · 807bd9fe7 c3 16-bit PID · 87f7b2546 stride · fec7b5123 c5 511 x 255 (+ fsd/init test
fixes, model size line) · 36a46a36a c6 process-fill · 22e2f1066 c7 thread-limit · dd35ae6d0
testbench `whole_run` key (Case::matches/chosen, misspelt-key test) · 8e730782d c8 worst walk
(walk-trace, oracle walks, worst-walk case by name + must_fail, three todo pages, residuals).

## Running now (detached, started 10:15:53 by me; survives my end)
`in-dev cargo testbench --allow-skip --arch rv64 kernel-containment` then `in-dev cargo
testbench --allow-skip` (whole, both widths; worst-walk excluded by whole_run = false).
- Gate log `.wash/local/K16-gate-8e730782d.log`: DONE, exit 0, PASS 218.2 s; numbers already in
  K16-report-tip.md (share 829, R10 p50/p99 20,740/25,253 µs, lease end 32,961 µs).
- Bench log `.wash/local/K16-bench-8e730782d.log`: running. Done when its last line is
  `exit N`. Read it with `grep -aE '^(FAIL|SKIP)' <log> | cut -c1-200`,
  `grep -ac '^PASS' <log>`, `tail -3 <log>`; never cat it. Console logs: the run dir named in
  its last `Error:` line, under target/testbench/run-*. Check `pgrep -fa 'testbench'` before
  starting anything: one bench at a time on this host.

## Known failure in the bench so far (fix it)
`bundle-mapped [rv64]`: "[bundle-mapped] FAIL: system 15 processes, users 47, and root keeps one
for this one". tests/programs/src/bin/bundle-mapped.rs:105-113 (from main, 9a81fedea) pins the
boot split at 64 PIDs. At 511 the boot table (budgets.md:125-127, kernel/src/budget.rs:707-710)
is system 127 (processes / 4), users 382 (the rest less init's 1). Fix in c5 (fec7b5123): the
comment and the check, ideally derived (MAX_PROCESS_COUNT - 1 = 510; / 4; rest - 1), plus one
line in c5's "The cases follow the values" list. Fold with `git commit --fixup=<c5>` then
`git rebase -q --autosquash bfdb471b3` (no -i), message via an `amend! <subject>` commit
(-F file, --allow-empty). Rerun bundle-mapped (both widths) and any other bench failure's case,
by name, after the bench ends. Expect other cases from main pinning 64-era values the same way
(fsd's 9 labels and init's watcher test were two; both fixed in c5).

## What remains, in order
1. Bench result: wait for `exit` in the bench log; fix every K16 consequence as above (in the
   owning commit), rerun those cases by name; quote the bench (exit, PASS count, FAIL/SKIP
   lines, what was fixed and rerun) in `.wash/local/K16-report-tip.md`.
2. Final rebase, only then: `git rebase --onto 75245a114 bfdb471b3 wp-k16` (FSD3 merged; it
   touches servers/init, fsd, blkd, testbench). A docs/plan-only diff after the bench is
   accepted; anything in code or tests means rerunning what it touches (host tests of those
   crates, their bench cases by name; check `git diff --stat bfdb471b3 75245a114`).
3. Report the final tip to the orchestrator (message_send answer, <= 1900 B), set waiting.

## Folded in round 2 (do not redo)
Editor: 510 phrasing (PID 1 is the kernel's, processes take 2..511), objects.md:244 reflow,
m1 kernel bullet as three sub-bullets; kernel-attack-gaps.md's two long bullets left (page is
one line per bullet). Simplifier: Case::matches; residuals in timer/scheduling/budgets one line +
link, numbers once on the todo pages; P2-4 line in the report. Red: misspelt key pinned by test;
left() line in the report; gate rerun done (above).

## Traps
- Never `git stash`, never `git add -A`, never git in /home/mcloonan/redoubt; stage by path.
- Substring filters: `cargo testbench budget` runs every case containing "budget".
- `git cherry-pick` takes no -q.
- objects.md's "| thread IPC page |" row name is read by servers/init/tests/bound.rs.
- The expiry estimate is ~8M thread visits (not 16M).

## Whole bench at 8e730782d: FINISHED (after the handoff was registered)
`cargo testbench --allow-skip`, exit 1: 359 PASS, 1 SKIP (bench-ssh-loopback-openssh: podman
not installed, a host lack), 2 FAIL, both bundle-mapped (rv64 and rv32), the 64-PID boot split
pin above. Run dir target/testbench/run-1-1791047978421737188. Nothing else failed: after the
bundle-mapped fix in c5, rerun `cargo testbench --allow-skip bundle-mapped` (both widths) and
quote it; the rest of the bench stands.
