# K17 progress

## Early checkpoint (2026-10-02)

Branch wp-k17, base main e48232cb4. Commit 4cd53ae4e: the case alone
(tests/destroy-keeps-notices.toml, tests/programs/src/bin/destroy-keeps-notices.rs, the
tests/programs/Cargo.toml bin entry). fmt --check clean (exit 0).

Command: `.wash/local/in-dev cargo testbench destroy-keeps-notices`, exit 1, both widths FAIL:

    FAIL  destroy-keeps-notices [rv64, smp=1]  ... FAIL: E holds a second notice, cause killed, for the other child (pid 0)
    FAIL  destroy-keeps-notices [rv32, smp=1]  ... FAIL: E holds a second notice, cause killed, for the other child (pid 0)

Console, rv64 (rv32 the same shape, PIDs 22 then 34):

    [destroy-keeps-notices] ok: P1 and P2 are both receiving on E
    [!] Terminating process with PID 4
    [!] Terminating process with PID 20
    [destroy-keeps-notices] ok: E holds a first notice, cause killed (pid 20)
    [destroy-keeps-notices] FAIL: ... for the other child (pid 0)

What the tester got: one notice, killed, for the higher PID (killed second); the second
receive (1 s timeout) returned no notice.

Which child took the other's notice: the kernel's kill lines show the lower PID (4 / 22) ended
first; its notice is the one missing, so the higher PID (20 / 34), still in `receive` on E,
took it and was killed holding it. Exactly the todo page's shape.

Wake rule: held on both widths. Each child sends READY to the judge's inbox (judge blocked in
`receive` there) and then calls `receive` on E; the judge's "both receiving" line passes and the
bug reproduces on both widths, ordered on events only (no wait_ms anywhere).

Doomed holder's path: nothing asserts the abandoned-call notice flag is clear.
`end_thread` -> `drop_open_call` / `finish_served` / `close_call` read no F_NOTICE. The only
assertion on F_NOTICE in the kernel is in `reply` (message.rs:1340), "an abandoned call owes a
notice on a destroyed endpoint", which a doomed holder never reaches (it ends, it does not
reply). `drop_dying_notices` clears flags, asserts only CALL_MAGIC. No other F_NOTICE use in
kernel/src (recursive grep).

Deviation from the brief, to note: the brief says the verdict uses "the PIDs `process_create`
returned", but `process_create` returns a handle and the creator is never told the PID
(processes.md, "The creator is not told the PID"; there is no getpid). The case checks two
notices, both `killed`, with two distinct PIDs, and prints each PID; the kernel's
`Terminating process with PID` lines name the same two.

Next: wait for the go-ahead, then the fix (one private receiver helper in `pump`, used by its
three `find_thread` closures) with the pages, in one commit.

## Fix and gates (tip a53058456, on main e48232cb4)

Commits: 4cd53ae4e (the case), a53058456 (kernel: `receiver_on` in message.rs, used by pump's
three receiver picks; the pages: ipc.md rule beside R4b + status, residual gone; processes.md
Delivered bullet + Exit notices status (one line beyond the brief's list: the case cited there
too); SECURITY R4b row; SUMMARY line; todo page deleted).

- `in-dev cargo testbench destroy-keeps-notices`: exit 0, PASS rv64 and rv32.
- The brief's cases, each `in-dev cargo testbench <name>`, exit 0, both widths: ending-pumps-once,
  redoubt-dead, process-lifecycle, budget-deadline, pid-pinning-attack, pid-reuse-authority,
  receive-bad-record, budget-destroy-kills, budget-destroy-attack, endpoint-destroy-full,
  endpoint-destroy-open-calls, budget-destroy-growth, sched-latency (+tcg). GATE1 not on main.
- `in-dev cargo testbench --allow-skip`: exit 0, 276 PASS, 1 SKIP (bench-ssh-loopback-openssh),
  both widths, rv32 included; size-budget PASS (kernel 7717 of 7752, no raise); unsafe kernel 19,
  0 undocumented (unchanged; no unsafe added); formatting, docs PASS.
- `in-dev cargo +nightly fmt --all --check` exit 0; `in-dev cargo run -q -p redoubt-doccheck` exit 0.

### R10 (virtual time, so deterministic; main e48232cb4 measured here as baseline)
| case (µs) | rv64 main -> fix | rv32 main -> fix |
|---|---|---|
| endpoint-destroy-full | 11069 -> 11081 | 11088 -> 11105 |
| budget-destroy-growth filled | 1214 -> 1224 | 1388 -> 1398 |
| sched-latency seed 3 p99 | 5562 -> 5580 | 5830 -> 5864 |
| gate H / D medians (9 each) | 20672/18784 -> 20762/18917 | 20825/18689 -> 20965/18860 |
| gate threads' ending H / D | 2897/2519 -> 2948/2566 | 2988/2595 -> 3067/2665 |

Gate: wp-gate1 tip 5fdfeb298, its 5 test files copied uncommitted into scratch worktrees of main
(base) and main + this message.rs (fix), plus K13's scratch-only deadline marker (bit 40 on the
X/Y id) so K13-phases.py splits H/D. Gate PASS both widths both trees. Base reproduces K13's
recorded numbers exactly. The rise is real (icount), not noise: under 1% (+0.09..0.17 ms at the
gate's full fill), all targets still met. budgets.md still says 20.8/18.7 rv32, 20.7/18.8 rv64:
not my path, left unchanged; if it should carry the new figures, that is the orchestrator's call.
Scratch worktrees removed.

Model: not edited. It settles each endpoint once after the operation (doomed receivers already
gone), so it agrees with the fixed kernel; no finding.

## Fix round 1 (tips 8d6297d69 case, 59965058c fix; on main aa74eee32)

- Rebased onto aa74eee32 (the doomed definition on budgets.md under R10 step 3).
- budget.rs: `runs_in_dying(pid)` (budget_of dying; destroy_subtree's kill loop uses it, as
  before) and `process_is_doomed(pid)` = runs_in_dying || the pid's process object's creator
  budget is dying (process::budgets_dying's free-and-kill test). receiver_on consults
  process_is_doomed. The kill loop deliberately keeps runs_in_dying: switching it to the union
  would kill C through `died`, which flags C's notice (F_NOTICE) before its teardown pumps. A
  pump then could deliver a notice R10 says is never made. budgets_dying ends C without a notice.
- New case destroy-keeps-notices-creator. P in dying B creates C in live L; C receives on E.
  On main and on round 0's tip both widths FAIL: "E holds P's notice ... (pid 0)". It PASSes
  after the fix. My first draft passed on round 0: C's READY could be queued before the judge
  blocked, and the judge took it without a wake. Now only C signals, while the judge is blocked.
- Pages: ipc.md rule in the Architect's words; processes.md Delivered trimmed to one line
  linking R4b; the case cited in the R4b, Exit notices and SECURITY rows; receiver_on's perf
  remark dropped; budgets.md gate medians 21.0/18.9 rv32, 20.8/19.0 rv64, and threads'
  teardown 3.1 ms (was 3.0). The baseline sentence is kept.
- Runs, exit 0:
  - both cases on both widths;
  - the brief's 13 related cases on both widths;
  - gate PASS on both widths: rv64 H/D 20785/18984 (threads 2971/2588), rv32 20983/18929
    (3090/2688);
  - endpoint-destroy-full 11081/11104; growth 1224/1398; sched-latency p99 5580/5863;
  - fmt --check, doccheck;
  - testbench --allow-skip: 278 PASS, 1 SKIP (bench-ssh-loopback-openssh); kernel 7722 of
    7752 lines; kernel unsafe 19, 0 undocumented.
