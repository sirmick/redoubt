# CTX3 report (ctx-implementer-4): named contexts part c

Branch wp-CTX3, worktree .worktrees/CTX3. Base 98b866854 (train 17 tip), head 303f33e89. Clean
tree, not pushed. Four commits:

- a5d0e4f35 steward: cap, idle, a session's contexts (libs/steward incl. Elixir reference and
  traces; model). Size budget: libs/steward, model.
- de384f2f3 wire, steward, init: ops 14-16 on a session badge, the idle timer, the manifest's
  `contexts`. Size budget: libs/wire, servers/steward, servers/init.
- a1d90342b shell: contexts(), detach(), end_context(name).
- 303f33e89 tests, docs: five cases, the pages.

## What changed in this session (on top of ctx-implementer-3's WIPs)

1. **Shell `contexts()` crashed on the machine.** `String.to_existing_atom("attached")` raised:
   the boot pack held no `:attached`/`:detached` atom. Now `state/1` names both in
   `Redoubt.Contexts`.
2. **shell-contexts.** The bench's `exit` step closes the pty at once, which detached `home`
   before the shell had read `end_context("home")`, so it never ran. The case now sends it,
   marks, and keeps the terminal open. alice.after then logs in 2 s later (ProxyCommand sleep).
   The verdicts are from the system:
   - the cap lets alice.after in only once `home` has ended (alice {} holds one SSH context
     beside the console);
   - `users/alice/{} holds .., 10 processes` (the budget given back);
   - alice.after's listing shows only itself;
   - bob's listing after all this still answers.
   The steward was never hung: in the failed run it refused alice.after `Cap`.
3. **steward-restart-context.** Its sessions now run without a pty, with the shell busy
   (`:timer.sleep`), so closing input detaches nothing. The channel ends at the steward's exit
   with status 1, as the case requires.
4. **contexts-cap.trace.** With `contexts=2` and the console counted, `b` was refused `Cap`, so
   half of the trace's story never happened. Now `contexts=3`. The Rust core's run now shows the
   whole story: b admitted, c Cap, a wrong key BadKey, listings, unknowns, b IdleEnded at 300 s.
5. **Host coverage of `is_context`'s refusal.** It had none: the comment claimed it, but no test
   called it. `a_session_sees_and_ends_its_own_domain_s_contexts_only` now opens the console's
   session, checks its listing is empty and that its `Leave` is `unknown`.
6. Pages written (below), WIPs folded into four commits, rebased onto 98b866854 with no
   conflicts.

## Gates, as run (exit codes)

- **Final set on 303f33e89** (`make -k -f scripts/jobs.mk prebuilt`, then `set CASES=<55 cases>`):
  prebuilt rc=0, set exit 0, 84 PASS, 0 FAIL.
  - Host and check cases: formatting, docs, size-budget, unsafe-budget (no new `unsafe`),
    steward-host-tests, init-host-tests, sshd-host-tests, consrelay-host-tests, wire-host-tests,
    **host-tests** (the testbench crate's own tests; fixed by the rebase, which brings main's
    recipe test that expects consrelay), elixir-oracles, steward-model-host-tests.
  - Every `steward-*`, `sshd-*`, `consrelay*` and `bench-ssh-loopback*` case, on both widths
    where the case has them.
  - shell-contexts, shell-commands, init-boot, boot-profile, userland-boot, beamlet-footprint.
  - New cases, rv64/rv32: steward-context-cap 10.1/8.6 s, steward-context-idle 89.8/88.5 s,
    shell-contexts 16.3/16.0 s, steward-restart-context 29.3/29.5 s, steward-login-timing
    5.4/5.6 s.
- **Model gates** (`set CASES="model-mutations model-host-tests steward-model-host-tests
  elixir-oracles"`), all rc=0. They ran on af7294842, whose code is identical to the head's; only
  testbench.md's job count differed.
  - model-mutations: 173 jobs, every one caught; longest R2NoWaitCap 16.4 s; the new five ≤1.0 s.
  - model-host-tests: 169 s.
- `make build-rv64 build-rv32`: both rc=0.
- `./test-shell` (through q): rc=0, every stage passed.

## Rule 3: refusal timing at the client (steward-login-timing)

Thirty refused logins, interleaved. Time to `Permission denied`, in seconds:

| | unknown principal | known, wrong key | unknown label set | all 30 |
| --- | --- | --- | --- | --- |
| rv64, median | 0.104 | 0.105 | 0.104 | 0.104 |
| rv64, IQR | | | | 0.103-0.105 |
| rv32, median | 0.111 | 0.113 | 0.112 | 0.112 |
| rv32, IQR | | | | 0.111-0.113 |

- rv64 comes from the c3 run (the same steward code before the rebase); the final run's directory
  was already cleaned. rv32 comes from the final run.
- The medians differ by ≤2 ms, inside the interquartile range, so nothing is equalized.
- The case's verdict is sshd's `refused (BadKey)` for each refusal, plus each ssh's exit 255. The
  times are a measurement, recorded in steward.md's residual risks.

## Pages and summaries checked

Updated:
- steward.md:
  - Contexts: the cap and its default derivation, idle and "production sets hours", the three
    session calls, ends-with-the-steward with M6 re-adoption planned; the "planned" line removed.
  - New R82 and R83, each with its status line.
  - R37: the listing and P21.
  - The manifest-lines format (`contexts=N idle=S`).
  - The rule-3 residual, with the numbers.
- sessions.md Contexts: cap, idle, commands, restart; the "planned" line removed.
- shell.md: session commands and status line.
- init.md: the `principals` row and a new "Contexts" section.
- SECURITY.md: R82 and R83 rows; R37 row (sees, tests).
- kernel/model.md:
  - 173 variants; the R82 and R83 rows, and PolicyContextsAcrossSets under R37.
  - Three property rows.
  - Reach 142 at seed 199, and 145 at seed 439 (searched to 10,000); catch floor 1,418 and
    1,166.
  - Counts kept at 2,000 and 3,000 (the rule gives 2,000 each; noninterference keeps 3,000 as
    margin).
  - Scenario seeds re-measured (R2OneCursor 944, AgentOtherSet 48, EndLeaseAdmitted 36 policy /
    126 noninterference); the is_context residual.
- plan/m2-usable-shell.md: the "what is left" paragraph removed, and the progress paragraph
  extended.
- GETTING-STARTED.md: the cap, detach, contexts() and end_context.
- testbench.md: 173 mutation jobs.

Checked, no change needed:
- README.md: no claims about contexts.
- sshd.md: every refusal after the signature takes one path, and the `cap` code shows only on
  its console line.
- consrelay.md: the relay ends with the budget.
- No crate READMEs exist for these crates.

## Open risks, and notes for review

- Contexts' ages and the idle clock use the steward's `time_now`. Idle is tested at 60 s only.
- The core's P20 ghost clock in the model assumes no detached context dies another way during a
  Tick. That holds in every run so far.
- `under_cap` counts the console's session where it runs, so alice {} holds one SSH context.
  This is ruling C1.
- Next: review (Tier A, steward-red).
