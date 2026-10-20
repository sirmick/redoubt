# SHELLN1 report: the split moved nothing; the pages now state the measured cause

Branch `wp-SHELLN1`, head **8c5411655**, one commit on origin/main c01146946 (train 79, STEWARD12
in). Worktree `.worktrees/SHELLN1`. Ruling: Architect on QA thread STEWARD8-bound (recorded as
"SHELLN1 lands + WAKE1" in `.wash/local/wave-2026-10-18-rulings.md`): revert the split, land the
correction of the pages, the follow-up is a beamlet investigation node (the Architect's working
name WAKE1, plan `.wash/local/plan-2026-10-20-wake1.toml`).

## Delivered

- `docs/servers/steward.md`: Leases ("Each path tells the sponsor"), Failure and restart (the
  burst paragraph), Residual risks (the burst bullet): the cause is the session VM's take of each
  forwarded notice, not the shell's print; the listener experiment and the stamped boot are
  stated with their numbers; the follow-up named as beamlet's idle and wake path, its own package
  (no node id on the page); the second lost notice (one of sixteen, rv64 one hart) recorded.
- `docs/kernel/scheduling.md` "With the real steward": the same correction of the burst passage.
- Not delivered, by the ruling: the listener split (`Redoubt.Shell.Notices` two processes) and its
  two ExUnit tests — built, host-tested and gated, then reverted since it moved no measured
  quantity (code that moves nothing does not land); the burst tomls' bound and all-N assertion
  (the quantity did not change; the cases stay as STEWARD12 left them, recording with no bound).

## Evidence (kept, git-ignored)

`.worktrees/SHELLN1/.tmp/SHELLN1/burst-finding.md` — the before/after table, the mechanism
reading, and "The diagnostic boot" (the send-return / receive / print alignment at both hart
counts, the trace tallies across the two longest gaps). Logs: `after/` (the split's burst runs,
five boots), `diag/` (the stamped boots and `timeline-smp{1,2}.txt`), `timeline.py` (the
extractor). `gates-before-rebase.md` and `shell-set-9b887286f.log` (the split's shell set).

## What was measured

1. The split on the burst cases, same seed as STEWARD12, rebased on c01146946 (all rc=0;
   rv64 smp1/2 112/124 s, rv32 smp1 188 s, -16 rv64 smp1/2 115/130 s): last of six 1.23-1.72 s,
   last of sixteen 2.17-2.60 s gross after the deadline; gaps between send returns 60-380 ms;
   15 of 16 `[lease ended]` lines on rv64 at one hart (agent-8 lost). STEWARD12's shape exactly.
2. The stamped boot (scratch commit 0c631b195, dropped), rv64 both hart counts, rc=0: each notice
   reached the listener 43-193 ms (one hart) / 56-185 ms (two) after the steward's send returned;
   the next send returned about when the previous receive happened; the print followed its
   receive by 150-966 ms, backlogging. Across the gaps the hart's commonest runner record is none
   (H0 62 and 28 times in the two longest one-hart gaps) and the receiving VM's budget has no wake
   record until a timer. The session's budget id is inferred from the trace, not read (the
   steward prints none): WAKE1's first step.
3. Conclusion (the Architect's ruling agrees): the gate is the VM's take of the serve thread's
   forward (beamlet's idle/wake path, or the kernel's waking of a receiver with a deadline), not
   the listener, not the print, not the steward's send. The re-bind benefit of the split
   ("a notice no longer waits on a re-bind") is unmeasured: no server restart is in the case.

## Tests and gates, with exit codes

On the final head 8c5411655 (docs only): `make -k -f scripts/jobs.mk set CASES="docs formatting"`
→ exit 0 (PASS docs 1.7 s, PASS formatting 26.0 s; `.tmp/SHELLN1/gate-docs-formatting-final.log`).
No case or code changes on the head, so no boot gate applies; the shell set is not selected by
`scripts/shell-cases` for a docs-only change.

On the reverted split (for the record; 9b887286f on 4f381d216, then 0d80aac5e on c01146946):
`scripts/q run -- ./test-shell test/redoubt/shell/notices_test.exs` exit 0 (8 passed on beamlet and
on BEAM); `set CASES="docs formatting"` exit 0; the narrowed shell set
(shell-commands, shell-compile-module, shell-compile, shell-contexts, shell-long-output,
shell-machine, steward-lease-ends, steward-end-lease, steward-server-restart; no `agent-lease-*`
case exists) both widths, 18 runs all rc=0, 5.7-19.3 s each; the burst cases as in 1 above.
Not run: the steward-latency cases (forbidden by the brief); a whole bench (none applies).

## Documentation check (summaries named)

- `docs/servers/steward.md`, `docs/kernel/scheduling.md`: updated (above). The status lines
  name no new test; none was added.
- `docs/userland/shell.md` (machine-mode bullet, "printed by the shell's listener when the send
  arrives"): unchanged on main and true of the code on main (the split is not landed); no change.
- `docs/userland/sessions.md` 281-288, `docs/userland/agents.md` 86: say the shell serves the
  endpoint and prints the line, nothing of the timing; no change.
- `README.md`, `GETTING-STARTED.md`, `docs/SECURITY.md` R39 row: no claim about the burst or
  the listener; no change. Milestone progress under `docs/plan/`: no notice claim found.
- Case files left as STEWARD12 landed them, by the ruling, though their prose carries the
  disproven cause: `tests/steward-deadline-burst.toml`'s description ("a rendezvous the sponsor's
  shell takes only once it has printed the previous `[lease ended]`", "until the shell takes a
  notice faster than it prints one") and `tests/steward-latency.toml`'s comment at lines 150-153.
  **Flagged for the orchestrator / WAKE1**: a one-line correction of each when that node touches
  the cases, so a reader of the case is not sent to the shell.

## Design problems found

- The ruling SHELLN1 was built on (STEWARD12 burst finding → the shell's take-then-print)
  attributed the burst to the shell's print rate; the measurement refutes it and the Architect
  has recorded the correction. The real gate is in beamlet's wake/take path or the kernel's
  receive wake: WAKE1.
- The second lost notice (a sponsor never told of a lease's end) is a standing finding that
  lands with WAKE1, as the Architect ruled.

## Risks and state

- Head 8c5411655 is a clean single commit; no WIP, nothing uncommitted; the scratch stamp commit
  is dropped. Nothing is running; q holds no SHELLN1 job.
- The `.tmp/SHELLN1` evidence is in the worktree only (git-ignored): if the worktree is removed
  before WAKE1 starts, copy `burst-finding.md`, `diag/` and `after/` aside first.

## Next step

Review by steward-red-5 (docs-only diff, two pages); merge as a docs commit on the next train.
WAKE1 (Tier A, investigation first) per the Architect: make the session VM's budget id readable
in the trace, then exclude in order beamlet's idle receive / timer wait, the kernel's waking of a
receiver with a deadline, and the serve thread's runnability between receive and forward; measure
whether every completion to an idle VM waits the same; acceptance recv − send at the kernel's
delivery and no drop at NOTICE_US.
