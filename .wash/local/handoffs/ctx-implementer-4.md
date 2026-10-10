# Handoff: ctx-implementer-4 (CTX3, merged at origin/main 2afa4e38d)

## State first
- CTX3 is merged and published (origin/main 2afa4e38d). The whole bench is green (627 PASS).
- Branch wp-CTX3 (last head 2b18aae04 on 04b7fa4d5) and worktree .worktrees/CTX3: leave both untouched. No open work, nothing uncommitted.
- Report: .wash/local/CTX3-report.md. Design and rulings: .wash/local/CTX3-design.md.
- Scratch is under /home/mcloonan/redoubt/.tmp/CTX3:
  - set.sh TAG "cases" [prebuild] runs make set CASES= through q.
  - host.sh runs host cases one by one.
  - catch.log, reach.log, draft/ are old drafts.
  - Nothing there is needed.

## Contexts as built (CTX1-3)

A login's session is a context: (account, label set, name). The SSH user is `principal[+label][.context]`; the default context's name is empty.
- **CTX1:** names (`[a-z0-9_-]`, 1-64, starting with a letter), sshd's split, init's name checks.
  - Refusals without enumeration: every pre-session refusal is `bad_key` / Permission denied.
  - R79: one session per context. The guard `context_free` → `take_over`; a context still starting is `in_use`.
- **CTX2:** per-context console relay `servers/consrelay`, in the session's budget beside the VM.
  - The session budget is 11,264 pages: the VM's 11,136 plus 128 for the relay. The VM is told `budget_pages` = session - relay.
  - A pty channel's close detaches; a login reattaches or takes over (R80).
  - Fresh attachment ids per attach; the detached buffer keeps the newest 64 KiB.
  - sshd's `watch` ending detaches every context.
- **CTX3:**
  - **Cap (R82).** The guard `under_cap` runs after login_key/owns_labels/not_locked/context_free and before any carve. It refuses `Cap` and evicts nothing.
    - It counts every non-Ending session in the domain, the console's included.
    - init writes `contexts=N idle=S` per principal line. Default max = min(holds(), 16), where holds() = min((share pages - cost)/session pages, share processes/session processes) (servers/init/src/check.rs).
    - Manifest key `contexts {max, idle_secs}`. Refused: max 0, above 16, above holds, or below 2 for the console principal; idle outside 60..604,800.
    - Image: alice {} console + 1 SSH, {secrets} 2, bob 2.
  - **Idle (R83).** `Session.since` / `Session.idle`.
    - A detach that finds the clock clear starts it; an attach clears it (attach_with(cx, true)). An attached context never ends on it.
    - The server's loop wakes at `store::next_idle()` and sends `EventKind::Idle`. Detached past `idle_due` → Ending, audit `IdleEnded {session, idle secs}`.
  - **Session ops (wire steward 14/15/16, session badge class only).**
    - `contexts` → `list` text, `name\tattached|detached\tsecs\n`.
    - `detach` → core `Leave`; refused `unknown` on the console session (`is_context`).
    - `end_context name` → EndSession on that context, own included (= exit). Any name outside the caller's domain (policy entry `sees`) is `unknown`.
    - Shell: userland/shell/lib/redoubt/contexts.ex (Redoubt.Contexts) and session.ex commands contexts()/detach()/end_context(name); `{:error, :no_steward}` on the host.
  - **Restart ("generation").** Nothing is added: contexts end with the steward (K23 reap) and ids are random per instance, so a name never reattaches across a restart. M6 re-adoption is stated as planned. The case is steward-restart-context.
- **Model.** Ops Contexts/Leave/EndContext come from split op ranges with no new RNG draws; Tick drives Idle.
  - P19 cap, P20 idle (ghost clock), P21 listing/leave/end in model/src/policy.rs.
  - Mutations: PolicyNoContextCap and PolicyCapAcrossSets (R82), PolicyIdleNoBound and PolicyIdleClockKept (R83), PolicyContextsAcrossSets (R37).

## Traps
- **The steward-context-login rv32 load flake (CTX2's case, logged in B51's family).** The VM's consol `ended` send (`:redoubt.send(... [18,0,0,0] ...)`) got `{:error, :timeout}` under full model-gate load. The case then waits 1500 s. It passes alone (7.6 s). Don't run it beside model-mutations or steward-model-host-tests.
- **Model count arithmetic.** `Mutation::ALL` is main's count + 5 for CTX3. When a train adds or drops packages, recount from main (168 on the rebuilt main, 170 with SMP6, so 173 or 175). Fix model/src/mutation.rs ALL, docs/kernel/model.md ("lists all N variants"), docs/testbench.md (model-mutations "N jobs") and the model size ceiling (tests/size-budget.toml, exact count from the size-budget log).
  - Reach LAST = (199, 142) and (439, 145). Catch floor 1,418 (policy) and 1,166 (noninterference).
  - Counts stay 2,000 and 3,000; noninterference keeps 3,000 as stated margin.
  - Scenario seeds re-measured: R2OneCursor 944, AgentOtherSet 48, EndLeaseAdmitted 36/126.
- **Console-principal minimum.** init refuses an explicit `contexts.max` < 2 for the manifest's `console` principal. Residual, put to steward-red as scope: a console principal whose default (holds()) is 1 is not refused. The image's shares hold 2.
- **Bench `exit` step closes input at once.** On a pty that detaches the context before a just-sent line is read. Send, mark, keep the terminal open (wait on a later mark), then exit. For a session that must end with the steward, use no pty and a busy shell (`:timer.sleep`).
- **Atoms in the boot pack.** `String.to_existing_atom` fails on the machine for atoms no packed module names. Name them in the module (contexts.ex `state/1`).
- **Hand traces count the console.** contexts-cap.trace needs contexts=3 for console + a + b. Check a trace's answers with `cargo run -p redoubt-steward-trace --bin steward-trace -- run FILE`; elixir-oracles only compares the two implementations.
- **Manifest copies.** tests/data/steward/idle.manifest.json is a full copy of image/manifest.json plus bob's idle_secs 60. A session-size change must raise it too.
- **No worktree edits while prebuilt or cases run.** Even docs edits fail them as "tree changed". Draft under $REDOUBT_TMP and copy in afterwards.

## For B48 (piped leak) and B50 (badge per launch)
- The session's startup handles (console, `steward`, namespace) are the ones B50 notes as Owned::kept. Redoubt.Contexts uses `Redoubt.Namespace.handle("steward")`, the kept minted session badge. B48/B50 changes to the Known table or Life must keep that handle alive for the session's life, or contexts()/detach()/end_context() and end_session break. bench:shell-contexts (both widths) is the machine check; ~17 s.
- end_context on the caller's own context destroys the caller's budget while its call is in service at the steward. The steward handles it: the reply fails, and the Abandoned notice is ignored unless it was a watch. Any new per-launch badge or Life bookkeeping in the VM must tolerate the VM dying mid-call. That is the same path as `exit` from a pty.
- The session budget holds the relay (consrelay, 128 pages) beside the VM. A per-stage carve comes from the VM's own `budget_pages` (session - 128), not the session's. budgets.md, beamlet.md and testbench.md agree on peak 5,492 and cap 11,092 (108 spare).
- An idle-ended or end_context-ended context is a budget destroy from the steward (Detached Idle / EndSession rows → destroy_budget). Pipelines and jobs inside go with it. A B48 host test of "a thousand pipelines" should also cover the session ending this way, not only `exit`.

## Paths
- Core: libs/steward/src/{guards.rs (under_cap, idle_due, is_context, sees), effects.rs (attach_with, detach_with, audit_idle), machines.rs (Contexts/Leave/EndContext/Idle, contexts(), context_named()), store.rs (since, idle, next_idle), manifest.rs (Contexts)}.
- Core tables: libs/steward/tables/session.md.
- Reference and traces: libs/steward/elixir/*, libs/steward/trace/traces/contexts-cap.trace.
- Server: servers/steward/src/{bin/steward.rs (idle timer), protocol.rs (listing, Class::Session)}.
- Wire: libs/wire/tables/steward.md (ops 14-16).
- init: servers/init/src/{check.rs (holds, contexts_of, MAX_CONTEXTS, IDLE bounds, console minimum), manifest.rs, refusal.rs (Why::Contexts)}.
- Model: model/src/{policy.rs, mutation.rs, steward.rs}, model/tests/steward_reach.rs.
- Shell: userland/shell/lib/redoubt/contexts.ex, shell/session.ex, test/redoubt/contexts_test.exs.
- Cases: tests/{steward-context-cap, steward-context-idle, shell-contexts, steward-restart-context, steward-login-timing}.toml, tests/data/steward/idle.manifest.json.
- Pages: docs/servers/steward.md (Contexts, R82, R83, R37, residual: rule-3 timing measured, medians within 2 ms), docs/userland/sessions.md, docs/userland/shell.md, docs/servers/init.md (Contexts), docs/SECURITY.md, docs/kernel/model.md.
