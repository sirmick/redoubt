# IPC3 rebase onto main 7760bb18d (ipc3-implementer-6, 2026-10-05)

## Branch
wp-ipc3: a0fbbcab1 on e9c36bc96 -> **4cc35d84b on 7760bb18d** (`git rebase --signoff`), four commits,
messages unchanged:
8b0e82153 ipclist · af5d65a4f delivery · d02f0a707 expiry · 4cc35d84b worst-walk.
**SCHED1 rebases onto 4cc35d84b.** Not pushed.

## 577964049
Not a conflict: 577964049 ('the worst walk, measured at every PID with every thread') is already
an ancestor of e9c36bc96 (`git merge-base --is-ancestor` true), so a0fbbcab1 was written on top of it
and already reconciles both (rv64-only R10 measurement -> full occupancy both widths, the two
walk todos removed, destruction-walks-every-process added). Main's 23 commits since e9c36bc96 touch
no kernel/, libs/ipclist, tests/worst-walk* or docs/kernel/ walk text (kernel-side diff: boot.md,
beamlet-lookup tomls, ssh-reference only).

## Conflicts
One, in commit 4 (worst-walk): docs/SUMMARY.md, the todo list. Main (c4f0e3aa7 line) removed
`todo/beamlet-refused-module-falls-through.md`; IPC3 removed delivery-walks-every-thread and
expiry-walks-once-per-wait and added destruction-walks-every-process. Resolution: all three
removals hold, IPC3's one added line kept. Auto-merged clean: docs/SECURITY.md,
docs/plan/m1-separation.md, docs/testbench.md (separate hunks; docs gate PASS).

## Range-diff
Each commit: `+Signed-off-by: Michael`. Commit 4 also: the SUMMARY context line above, and two
testbench.md hunk headers moved by main's 7760bb18d. No code change.

## Host gates at 4cc35d84b (via jobserver share), exit codes
- cargo testbench formatting 0 PASS; unsafe-budget 0 PASS; size-budget 0 PASS (no kernel source
  change on main, ceilings untouched); docs 0 PASS; no-cruft 0 PASS.
- cargo test -p redoubt-ipclist 0 (10 passed); -p paging 0 (9 passed); -p testbench sched_oracle 0
  (16 passed).
- ./build --arch rv32 --programs 0; rv64 0. Only warning: untouched kernel-half-attack.rs:64.
- Note: bare `cargo fmt --all --check` exits 1 on stable rustfmt (nightly-only options ignored)
  in files IPC3 never touches (intc_plic.rs, irq.rs, mem.rs); the repo's gate is the formatting case.

## Cases (jobs.mk, both widths, shared)
40 names = the 12 focused filters + expiry-deadline-then-timeout, expanded to exact case names
(bench-virtio-devices rv64 only): 79 targets, every rc=0; 96 PASS lines, 0 FAIL/SKIP.
endpoint-destroy-full: R10 15,746 us rv64, 16,603 us rv32 (was 15,726 / 16,484).
worst-walk not run: no page line its numbers feed changed in the rebase.
Logs: target/jobs/rv{64,32}-<case>.log in the worktree; /tmp/ipc3r/.

## Pool defects found (for the orchestrator, not IPC3)
1. jobs.mk has rules only for whole case names; its header says a target is a substring filter.
   `rv64/budget-` etc. -> "No rule to make target".
2. .wash/local/jobserver was rewritten at 16:51:03 while my first run used it; running jobs
   then died at line 122 "atever: command not found", rc 127, after their cases printed PASS.
   That run was discarded and every target rerun.

# Second rebase onto main fb1f3a58f (2026-10-06)

wp-ipc3: 4cc35d84b on 7760bb18d -> **410b19d75 on fb1f3a58f** (`git rebase --signoff`), four commits,
messages unchanged: e6a0917cb ipclist · 2f78da108 delivery · 8b3235148 expiry · 410b19d75 worst-walk.
**SCHED1 rebases onto 410b19d75.** Not pushed.

## Conflicting hunks (each: both sides kept)
Commit 1 (ipclist):
1. tests/size-budget.toml, the [[crate]] after libs/stride: main's libs/verity (130) and IPC3's
   libs/ipclist (499) both, verity first.
2. tests/unsafe-budget.toml, the [[budget]] after stride: main's redoubt-verity and IPC3's
   redoubt-ipclist both (each max_unsafe 0, max_undocumented 0), verity first.
Commit 4 (worst-walk):
3. docs/SECURITY.md, the R12 row: IPC3's row (adds bench:worst-walk; main left R12 unchanged),
   then main's new R78 (fair kernel entry) row.
4. docs/SUMMARY.md, the todo list: main's new qmp-socket-private-dir line kept; IPC3's removal of
   delivery-walks-every-thread and expiry-walks-once-per-wait, and its destruction-walks-every-process line.
5. docs/testbench.md, the whole_run paragraph: IPC3's cut (the worst-walk sentence and its link to the
   deleted todo leave) with main's word "train" for "merge".
Auto-merged: commits 2 and 3 (kernel/ none; commit 3's tests/programs/Cargo.toml bin beside main's
heap-cap), docs/kernel/budgets.md, scheduling.md, plan/m1-separation.md. No reference to the two
deleted todos remains in docs/ tests/ kernel/ tools/.
Range-diff: the hunks above, sign-offs unchanged; commit 2 identical.

## Short gate at 410b19d75 (pool), every exit 0
- jobs.mk: build-rv64, build-rv32 (only warning: untouched kernel-half-attack.rs:64); docs,
  formatting, size-budget, unsafe-budget, no-cruft (rv64 target; width-free); model-host-tests (bounded).
- `jobserver bounded cargo test`: -p redoubt-ipclist -p paging (10 + 9 passed); -p testbench
  sched_oracle (16); -p redoubt-rt -p redoubt-sys -p stub (all passed; rt-host-tests is
  alone-class, so this ran shared under `bounded`, a pass).
- Cases, both widths, shared/net: redoubt-ipc, redoubt-ipc-attack, ipc-outcomes,
  ipc-fair-label-sets, timeouts, destroy-keeps-notices, destroy-keeps-notices-creator,
  endpoint-destroy-full, endpoint-destroy-open-calls, expiry-deadline-then-timeout,
  irq-first-receive; smoke userland-boot, init-boot, bench-net-peer: 28 targets rc=0, 46 PASS, 0 FAIL/SKIP.
