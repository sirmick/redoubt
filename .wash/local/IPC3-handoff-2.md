# IPC3 handoff 2 (from ipc3-implementer-2, 2026-10-03)

Worktree /home/mcloonan/redoubt/.worktrees/ipc3, branch wp-ipc3. Every cargo/bench command through
`/home/mcloonan/redoubt/.wash/local/in-dev <cmd>` from the worktree. Detail report (keep it
current): /home/mcloonan/redoubt/.worktrees/ipc3/.wash/local/IPC3-report.md (its commit hashes are
the old ones; the tip below supersedes them). Read first: .wash/local/IPC3-layout.md and
IPC3-layout-ruling.md (sections 1-5 and **(3b)**), and the brief IPC3-implementer.md.

## Tip: fead0ac09 on 53bcd9704 (main is now f9135ce8a; see Rebase)

1. `f2b6de13c ipclist: the kernel's IPC lists, host-tested` — libs/ipclist (no_std, forbid unsafe,
   no deps): `Words` trait, `List`, R2's two group lists with node moves, merge sort, audits, the
   per-endpoint member count `waiting` (word E_WAITING, ENDPOINT_WORDS = 9), `first_group`. Root
   member, kernel dep, tests in tests/host-tests.toml, size-budget 462, unsafe-budget 0,
   model.md paragraph + status line (the orchestrator's conditions, all met).
2. `dcae23afe kernel: delivery follows the endpoint, not every thread` — message.rs lists and
   paths; R10 step 4 in the existing owner walk (`endpoint_dying`, skipped when `waiting` is 0),
   then dying budgets' queued chains, then taken chains; audit per ruling (3b): `check_lists`
   (from threads) at each exit that changed a list (end of `redoubt::handle`, `expire_due`,
   `device::irq_fired`); `check_all` (threads + objects, asserts each endpoint's count and that a
   0 count means all list words 0) inside the destruction audit (budget.rs), the process-object
   free audit (process.rs `index_process`, free only) and a new audit before kmain idles
   (main.rs). AUDIT_IPC_LISTS = 3 (sched.rs). Pages: ipc.md, devices.md, scheduling.md (R12
   sentence and the Architect's audit page line), budgets.md item 4. Kernel size 8413.
3. `fead0ac09 kernel: the expiry walks once, however many waits it ends` — `collect_due`, sort,
   shares billing, EXPIRY bracket around collect+sort. Pages timer.md, scheduling.md. Kernel 8450.

Uncommitted in the working tree: commit 4's prep (below).

## Results (QEMU; the host was mine, now released)

- Focused list both widths (redoubt-ipc*, ipc-*, timeouts*, budget-*, process-*,
  endpoint-destroy*, destroy-keeps-notices*, irq-*, device*, sched-timer-flood,
  deadline-flood-billed): 79/79 PASS with the (3b) audit, **before** the endpoint count was added.
  sched-timer-flood cancelled-waits: 69 (rv64) / 71 (rv32) finding another budget's wait ended
  early, audits in window 74/88 ms (was 0 found, 521 ms, under the per-exit object audit).
- At the tip (with the count): endpoint-destroy-full PASS, R10 15.7 ms rv64 / 16.5 ms rv32;
  kernel-containment (the gate) PASS both widths, R10 p50/p99 22.8/23.8 ms rv64 (220 s),
  23.7/24.6 ms rv32 (90 s), trace "dropped 0". Before the count it failed at 32.6/33.6 ms (the
  9-word idle check per endpoint, ~2 us each in the checked build).
- Owed at the tip: rerun the focused list once (the count changed teardown and audit after it
  passed); quick (~12 min both widths).
- Host: crate tests 8 pass; all host-tests cases PASS (model ~550 s) earlier; unsafe-budget,
  size-budget, docs PASS at each commit; fmt clean; kernel release+checked rv64/rv32 build.
- Do NOT run the plain `host-tests` case while someone times (its testbench tests spawn QEMU).

## worst-walk: not yet measured

The first rv64 run was killed (its build may have caught mid-edit sources). Owed: by name, rv64
then rv32, with the orchestrator's word (`cargo testbench worst-walk --arch rv64`, then rv32).
Prepared toml: arch both, no must_fail/whole_run, `memory_mib = 2032` (rv32's physmap ceiling;
the case has one memory_mib). If rv64 at 2032 does not hold 509 holders ("every PID in use: true"
on the console), fall back to a separate rv32 case file (ruled acceptable by the orchestrator).
Measure from the trace (walk-trace M/m spans; oracle summary): one delivery's PUMP at full
occupancy vs smallest fill, R10 (<= 30 ms), the EXPIRY walk (collect+sort only) for the shared
deadline and the PUMPs apart, both widths. The oracle prints "walks" detail; to get it on a
failing run, a throwaway `#[test]` in tools/testbench/src/sched_oracle.rs calling `run(log, "")`
on the saved log works (remove it after; never commit it).

## Commit 4 prepared (uncommitted)

tests/worst-walk.toml and tests/programs/src/bin/worst-walk.rs (fills what RAM holds; ring of the
last 250 reports; prints `every PID in use`); docs: both todo pages deleted, SUMMARY.md lines,
m1-separation.md items (reconcile's kept), testbench.md's worst-walk example dropped,
scheduling.md status line (bench:worst-walk listed, tested 41) and the reconcile residual (K22's
paragraph, only the delivery/expiry clause). Still to write with numbers: budgets.md's
full-occupancy sentence at ~:764 (it still links the deleted todo page, so the docs case fails
until rewritten) and timer.md's residual figure; also check budgets.md's measured list item "the
thread walks, 1.3 ms" (now the dying endpoints' counts and lists).

## Then

`git rebase --onto f9135ce8a 53bcd9704 wp-ipc3` (ABI1/ABI2; no kernel change), then report;
the Architect's check of the whole package; the whole bench both widths, alone, on the
orchestrator's schedule. Report items owed: Group::order removed (dues are distinct counter
values, no tie); no WAIT_CAP reason row exists in ipc.md (R2 text updated instead); the endpoint
frame gained one word (E_WAITING, word 12) beyond the layout's 4-11; remaining-walks list (in the
report); exact page lines.

## What consumed context

Reading message.rs whole (~1,800 lines) and the model's pump; writing the crate and the kernel
rewrite; two commit-splitting rebuilds (commits 2/3 share message.rs: an intermediate file is
built by removing the expiry parts: DUE static, Frames::deadline, Page::Kernel arms, end_thread's
due unlink, collect_due/first_due/pop_due -> old next_timeout, check_lists's due assert);
bench logs (read only through grep).
