# KILL1 identification (phase 1): the steward's arms excluded; the kill is a kernel event after the neighbour's end

Worktree `.worktrees/KILL1`, branch `wp-KILL1` on origin/main `c01146946`; WIP `04235700e` (the
reproduction, `servers/steward/tests/steward.rs`). Logs: `.tmp/KILL1/host-tests-1.log` (68
passed, the new test included), `host-tests-mutation.log` (the test's teeth), `host-tests-2.log`
(68 passed after the mutation was reverted). Evidence read: `.tmp/READ1-evidence/gate10/*.log`
(the steward, `Terminating`, `killed`, `AgentStarted` lines only).

## (a) The fake-kernel reproduction

`sixteen_leases_ending_by_exit_each_destroy_their_own_budget_and_no_other`. The Recorder gets a
`recycles` mode modelling the machine's handle table as the kernel keeps it: every handle the
steward's copy of is closed frees its index for the next handle made (the connections handed to
the child at `launch`, bin/steward.rs:1135; a kept right at `release`/`clean`; the exit endpoint
the loop closes at the watcher's report, bin/steward.rs:2387, as `exit_taken`), the lowest free
index taken next (`make`), and a destroy revoking the budget's own handle and its children's
(`takes`). Sixteen leases are started back to back (alice's session first, as the gate), then
ended by their own exit in the three orders the gate's boots ended them (1–10, 14 → 11 killed;
1–13, 16, 14 → 15; 1–13, 16, 15 → 14). The verdict is the fake kernel's: `destroy` asserts the
index destroyed is the budget made for the lease whose step runs (`expect_destroy`), and after
every end each lease still running has its budget live and its exit endpoint naming it in `pids`.

**It passes on main** (every order). With the model mutated to close the budget's own copy at the
launch (the arm-(i) shape), it fails at once: all sixteen budget handles collapse onto index 6
(`host-tests-mutation.log`). So the test detects an index freed early; main frees none.

## (b) The arms, in order

**(i) a budget handle index closed or revoked before its end and reused** — excluded.
- The model above: with every close the machine makes modelled, no destroy hits another budget.
- The binary: every `close(` in bin/steward.rs (44 sites listed in my notes) closes a handle
  read from its own record or from the delivery that brought it; the EXITS handler closes the
  index the watcher reported, which is the one `watch_exit` gave it; `release` closes only a
  held copy with `via: Some`; `cleanup_done` only the record's own; nothing closes a budget index.
- The kernel's revocation of a steward handle happens only at the budget's own destroy or
  deadline (`close_dependents`: the Object chain of the dying budget, the Stamp chain of handles
  stamped with it; the steward's budget handles are stamped with the steward's budget).
- The gate's victims: in two boots of three the victim (11; 14) was created **before** the killer
  (14; 15), so no record of the killer's could name a later budget; the ruling's hypothesis
  (agent-11 created after agent-14's handle was lost) does not fit the launch order
  (`AgentStarted` lines 69–115: serial, 3.4–7.6 s apart, numbers in start order).

**(ii) the exit watcher's `pids` map stale across a reused index** — excluded. `launch` answers
the exit endpoint's index as the pid (bin/steward.rs:1149); the index is closed only in the
EXITS handler right before `steward.exited(key)`, on the same thread, and nothing installs between
the two; the model asserts the map names the right lease at every report. The gate's lines agree:
each `exited 0` names the lease whose `LeaseEnded` audit follows, and the victim's `was killed`
line names the victim correctly.

**(iii) the kernel: a process placed in or charged to another lease's budget, or killed by a
destroy that did not name it** — the arm that stands, by elimination and by the log's order:
- `killed` is produced only by `destroy_subtree`'s kill walk (kernel/src/budget.rs:1383–1420, the
  processes counted in a dying budget, and the caller) and by nothing else (`process_exit` is
  `Exited`, or `Faulted` with open calls; `free_charged` sends no notice; the notice decode
  `_ => Killed` at process.rs:832 reads a word only `record` writes). So PID 60 (rv64/2; 126 and
  419 on rv32) was on the counted chain of a budget being destroyed, or was the destroy's caller.
- **The order of the lines.** In every normal exit the kernel's synchronous `Terminating` line
  immediately precedes the lease's `exited 0`, then the steward's `audit` and `holds` lines
  (written after the whole event, destroy included) and the shell's `[lease … ended]` follow
  before the next `Terminating`. For the kill, `Terminating process with PID 60` comes **after**
  agent-14's `audit`, `holds 49031 pages, 30 processes` and the shell's `[lease alice/agent-14
  ended]` (rv64/2 lines 170–176; rv32 the same shape). Agent-14's `budget_destroy` ran inside its
  event, before those three lines were written; a kill inside it would print before them. The
  `holds` delta is one lease's carve exactly (−5249 pages, −2 processes), so that destroy took
  one lease budget. The kill is therefore a kernel event after agent-14's end completed, with no
  steward call between (the steward's next calls are the cleanup thread's disconnects, which
  destroy nothing).
- The victim is, in all three boots, the next lease in start order still running (11 after
  1–10; 15 after 1–13 and 14; 14 after 1–13 and 15): the one whose own load was next to finish.
  Deadlines are all normal (1.87–2.0 × 10⁹ µs; the kills at 6.0–7.5 × 10⁸), so not the deadline
  path. No process other than the steward holds a handle to an ancestor of a lease's budget; the
  lease's VM holds its own (`launch.handle("budget")`), and the shell lib destroys only budgets it
  carved (`Redoubt.Pipes`, `Pipeline`), neither of which the `load` agent runs.
- What I could not settle by reading: how `destroy_subtree` reaches that process. The checked
  build audits every list after a destroy and at every exit that changed one (`check_all`,
  `audit_destruction`), and `process()` asserts a frame's magic, so list or frame corruption is
  unlikely to pass silently. Sites to read (kernel-red-3): the kill walk and `subtree_next`
  (budget.rs:1058, 1383); `migrate_held_pids`/`recount` (process.rs:260, message.rs:1179), the
  only mover of a counted object; the exit → `free_object` at the watcher's receive
  (message.rs:1264) → the loop's `budget_destroy` sequence; `budget_handle`'s resolution of the
  steward's index (handle.rs:284).

## STEWARD10's "display artifact"

The record is one line (`orchestrator-handoff-2026-10-15.md:586`: `'agent-19 was killed 0'
printed twice for two ids`) and the STEWARD10 brief; STEWARD9's report does not mention it and no
log with the two lines survives under `.wash/local` or `.tmp` (the K18 outs hold no `was
killed`). Two distinct ids printing one name cannot come from `lease_name` (numbers are assigned
once at `route`, monotonic per domain) nor from a stale `pids` entry (excluded above). It is
consistent with this bug's shape — a lease killed by a kernel event the steward did not ask for,
its line printed beside a deadline kill of the same round — but without the log I cannot show it.
Not cosmetic until shown otherwise.

## Proposed next evidence (for the ruling)

A boot of `lease-code-load` (wp-READ1, once published or from its worktree) with the kernel
recording, at each `destroy_subtree`, the top budget's id and parent, and each PID it kills with
the budget frame its object was counted in and the budget it ran in; and the steward's exit line
carrying the index it destroys. The kernel is a hotspot and the probe a kernel change: the
Architect's call. Without it, the kernel arm is named by elimination, not by mechanism.

## Premise for STEWARD13

Nothing found false: on main a record's handle names its budget until the deadline (the model
and the binary sweep). The kill is not a lost handle.
