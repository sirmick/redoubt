# CTX3 design checkpoint: the cap, idle expiry, contexts()/detach()/end_context(), restart

Base: main 050357e8a (CTX2 merged), worktree .worktrees/CTX3, branch wp-CTX3. Brief rules 6 and
8 and "In the shell" (.wash/local/CTX1-brief.md); CTX1-design sections 2, 5, 6 and 7, which this
follows except where marked **changed**.

## (a) The cap per (principal, label set)

- **Where it lives.** Per principal in the manifest's `principals` entry, optional:
  `"contexts": { "max": N, "idle_secs": S }`. init checks it and writes it into the principal's
  manifest line (`contexts=N idle=S`), which the core parses as today's lines.
- **The number.** **Changed** from CTX1's flat default of 4, which the image cannot hold. The
  default is what the label set's sub-budget holds:
  `floor(min(sub.pages / session.pages, sub.processes / session.processes))`, less one in the
  console principal's unlabelled set, which the console session occupies. The steward computes it
  at start from the sub-budgets it carves and `sizes.session`; the steward-boot line says it per
  set. A manifest `max` may lower it. init refuses `max` 0, any `max` above 16, and any `max` the
  sub-budget cannot hold, so the cap, not a failed carve, is what refuses. The image gives alice
  {} 1, alice {alice-secrets} 2 and bob {} 2. Question C1 below.
- **Counted.** Live contexts of the domain, attached or detached (`Starting`, `Running`,
  `Detached`); the console session, leases and crossings are not contexts and are not counted.
  Only a login that would create a context is capped. A login that reaches a live context
  (reattach or takeover) never is.
- **Order (rule 6, red note 1).** A new guard, `under_cap`, runs after `login_key`,
  `owns_labels`, `not_locked` and the name check, and before any carve. It refuses `cap`. Nothing
  is carved and nothing evicted; the oldest stays.
- **Rule 3.** It comes after the key is checked, so it reveals nothing to an unauthenticated
  client. At the client every steward refusal is the same failed authentication: sshd maps them
  all to one `Refused` ("Permission denied"). The `cap` code reaches only sshd's console line.
  P17 is kept: a wrong key, a non-name or a foreign set is still refused `bad_key`, before the
  cap.

## (b) Idle expiry

- **The bound.** `idle_secs` per principal: default 86,400 (one day), init's range 60 to 604,800.
  It is measured from the context's last detach; an attach clears it. An attached context never
  expires.
- **How.** A kernel budget deadline cannot be cleared on reattach, so the steward runs the timer:
  - The core gains `EventKind::Idle`. With the event's `now`, it ends every `Detached` context
    past its bound through the usual `Ending` path, so its budget is destroyed and its relay goes
    with it. Everything else is untouched.
  - A pure `store::next_idle(&Store) -> Option<u64>` gives the embedder its next wake; the
    steward's receive loop uses the earlier of it and FOREVER, as the restart probe does.
  - The model sends `Idle` on its existing `Tick` op, so no new RNG draw.
  - Audit `Record::IdleEnded { session }`.

## (c) contexts(), detach(), end_context(name)

- **Authority.** The session's own `steward` connection, the minted badge every session already
  holds (`Redoubt.Namespace.handle("steward")`). Its kernel-stamped (account, label set) is the
  domain; the steward never takes a claim. No new handle, and the session's end revokes it
  (CTX1-design's question E, which nobody answered: I keep the existing connection as the
  handle).
- **Wire** (steward table, the session badge class only, malformed elsewhere):
  - 14 `contexts` (no fields): reply `list: string`, one line per context of the caller's
    domain: `name<TAB>attached|detached<TAB>idle_secs`. The default context's name is empty;
    names are the grammar's `[a-z0-9_-]`. At most 16 lines.
  - 15 `detach` (no fields): the caller's own context is detached, exactly as `channel_closed`
    does it. The steward's `ended` closes the channel with status 0. On the console session, or
    on a context with no channel, it is `unknown`.
  - 16 `end_context` `name: string`: ends that context of the caller's domain, its own included
    (as `end_session`). Any other name, another label set's or another principal's, is `unknown`,
    exactly as a name that does not exist.
  - Each is a session-badge call, so a labelled session may use all three, as it may
    `end_session`. Starting nothing, P8 is unchanged.
- **Shell.** `contexts()` returns `[%{name:, attached:, idle_secs:}]` (a table from the prompt).
  `detach()` returns `:ok`, and the terminal closes. `end_context(name)` returns `:ok` or
  `{:error, :unknown}`. All three go in shell.md's session commands. On the host, where the VM is
  no session, each returns `{:error, :no_steward}`.
- **Vault invisibility (R37).** The reply is computed from the caller's domain alone. The model's
  noninterference family gains `Contexts` from unlabelled sessions, so P10 compares what an
  unlabelled session lists with and without the vault's work.

## (d) Restart: contexts end with the steward

- **Unchanged (CTX1-design section 6).** K23's reap destroys `users` and every context with it.
  sshd's `watch` ends every attached channel, and the new steward starts on an empty `users` and
  an empty table. Session and attachment ids are random per instance (R36) and live in its memory
  only, so a name never reattaches to anything after a restart: the next login makes a new
  context.
- **Why not re-adopt:** it needs a kernel lookup of child budgets by a steward-set id, a table
  that outlives the steward (no store before M6), and rebuilt namespaces, since the dead
  steward's fresh connections are disconnected (K23). That is kernel, init and storage work for a
  crash that is a bug. **No generation word is added:** nothing survives for it to tell apart.
  The case attacks the property instead.

## Rule 3's timing (node body, from CTX1's red)

Measure at the client first: a bench case times 30 refused logins each of unknown principal,
known principal with a wrong key, and unknown label, by the host's `ssh` wall clock. It reports
the medians and spread in its log, with a verdict bound: the medians within the spread's
interquartile range. If they differ above jitter, equalize by running the blame and session
machines on a sentinel domain for the unknown ones. I expect SSH's exchange to dominate a
steward call's microseconds; the case decides.

## Model

- New ops `Contexts`, `Detach` and `EndContext`, chosen within existing draws by splitting an
  existing op range (no new draw). `Tick` drives `Idle`.
- Properties: P17 keeps; P19 cap (live contexts per domain ≤ max; a capped login changes
  nothing; the oldest stays); P20 idle (exactly the detached contexts past bound end on a tick;
  attached never); P10 covers listing.
- Mutations: `PolicyEvictOldest`, `PolicyCapBeforeKey`, `PolicyCapAcrossSets`,
  `PolicyListOtherSet`, `PolicyEndOtherSet`, `PolicyIdleAttached`. Each that the random search
  reaches slowly gets a directed scenario, as for `PolicyEndLeaseAdmitted`. Every job stays
  within 25 s; counts and the catch floor are measured again.
- The Elixir reference and the traces cover every new row.

## Cases (both widths)

- `steward-context-cap`: a test manifest with bob `max 2`. bob.a and bob.b live (one detached),
  bob.c refused (sshd line `refused (Cap)`, client "Permission denied"), bob.a reattaches with its
  state.
- `steward-context-idle`: bob `idle_secs 60`. bob.x detaches and alice.y detaches. After about
  70 s, bob.x's budget is given back (audit `IdleEnded`), alice.y still reattaches, and bob's
  console stays.
- `shell-contexts`: alice.work and alice.home. From alice.home's prompt, `contexts()` lists both
  and not alice+alice-secrets.v (also live). `end_context("work")` ends it.
  `end_context("v")` is `unknown`. `detach()` closes the terminal with status 0, and the next
  alice.home login reattaches.
- `steward-restart-context`: alice.work binds a value, the steward restarts (restart-probe), and
  alice.work logs in again: no `reattached` note, the value is unbound, and a new session id.
- `steward-login-timing` (rule 3), above.

## Questions

- **C1.** With the image's budgets alice gets one SSH context in her unlabelled set beside the
  console. Keep that (the cap tells the truth), or raise alice's budget in the image (B32's
  sizing, another package)? I propose keeping it, with the cap line in steward-boot making it
  visible.
- **C2.** `contexts()` shows the idle seconds of detached contexts only (attached shows
  `attached`). It shows no client addresses: an address belongs to the attach audit, not to a
  listing a VM can read. Confirm.

## Size and tier

M: core (guard, event, three ops, store fn), Elixir reference and traces, the model (3 ops, 2
properties, 6 mutations), wire, steward embedder timer, init manifest check, shell 3 commands,
5 cases, pages (sessions.md, steward.md, shell.md, sshd.md, M2). Tier A, steward-red.

## Rulings (orchestrator, on the checkpoint)

- Go as designed. C1: alice {} keeps 1 SSH context beside the console (GETTING-STARTED's
  walk-through); the cap case uses bob's 2. steward.md states the default's derivation.
- C2: contexts() lists name, state (attached/detached), and age since the last attach or detach;
  no client addresses.
- (a) under_cap after login_key, owns_labels, not_locked and the name check; same refusal text,
  no new timing class. The rule-3 measurement at the client goes in the report with numbers;
  equalize only above jitter; steward-login-timing's bound stated with margin.
- (b) The page says a production image sets hours (60 s floor is for the case). Idle ends a
  detached context only, never an attached one even if its last detach is old: attach clears the
  clock; a mutation of that.
- (c) A name outside the caller's domain is `unknown`, same text and time as a missing one;
  end_context on the caller's own attached context is allowed (it is exit).
- (d) Ends-with-the-steward is the rule for M2; M6 re-adoption stated as planned, not open.
- Model: split op range, no extra draw; mutations within deadline.
- Report at the first green cap case on both widths, then at the end.

## Implementation notes (implementer)

- The cap counts the domain's live sessions, the console's included (it holds a session's
  budget): max = what one sub-budget holds, uniform per principal; alice {} gets 1 SSH context
  beside the console, alice {secrets} 2, bob 2, as designed, with no per-set numbers.
- `Idle` reaches every context; the rows decide (Starting/Running/Ending: nothing; Detached:
  `idle_due`). Clock: set at a detach that finds it clear, cleared by an attach.
- Measured on the new generator: reach LAST policy (199, 142), noninterference (439, 145); never
  reached adds `rule is_context refuses` (the model has no console session). Catch floor policy
  1,418 (PolicyDeclassifyUnfit, scenario), noninterference 1,166 (PolicyDeclassifyLive); counts
  stay 2,000 and 3,000. New mutations caught: NoContextCap 3, CapAcrossSets 7, IdleNoBound 22,
  ContextsAcrossSets 26, IdleClockKept 35 (policy family).
- Rules (orchestrator): R82 "a principal holds at most its cap of live contexts per label set; a
  login past it is refused after the key and before anything is carved, and no context is evicted
  for it"; R83 "a detached context ends at its principal's idle bound; an attached one never does;
  an attach clears the clock". PolicyContextsAcrossSets -> R37; NoContextCap, CapAcrossSets ->
  R82; IdleNoBound, IdleClockKept -> R83. SECURITY.md gets both rows with their cases.
- Gate list adds `host-tests` (the testbench crate's own tests): CTX2 missed it.
