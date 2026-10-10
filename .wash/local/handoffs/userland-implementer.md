# Handoff: userland-implementer -> successor (CTX1a in progress; SHELL8 waiting)

## Branch state (nothing pushed)
- **CTX1a**: worktree /home/mcloonan/redoubt/.worktrees/CTX1, branch wp-CTX1 off main 93f933c7a. ONE WIP commit ca5e062ae "WIP CTX1a: names, contexts in the core, refusals" (35 files; no Co-Authored-By, fold it before review). Tree clean. NO tests run yet on it: `cargo check --workspace --tests` passes (only vendor warnings). The last tool call (tests) was rejected by the user mid-run; the commit had already been made.
- **SHELL8**: wp-SHELL8 8306ef6ed (.worktrees/SHELL8), unchanged, waiting for the beamlet red review; rebase on SHELL4 when it merges (conflicts likely in Driver.run, the loop's receive clauses, shell.md "Hostile text", driver_test.exs).
- B34 (7cf1fdcb7, stack cut) and B37/B38 were sent/merged earlier; nothing open there.

## Traps
- Every build/test via /home/mcloonan/redoubt/scripts/q (`q run --cores 8 --tenant CTX1 -- ...`) or `make -f scripts/jobs.mk`. Exports: PATH=$HOME/.cargo/bin:$PATH BEAMLET_TOOLCHAINS=/home/mcloonan/redoubt/toolchains RUSTSBI_PROTOTYPER=/home/mcloonan/redoubt/bios/target/riscv64gc-unknown-none-elf/release/rustsbi-prototyper RUSTSBI_PROTOTYPER_RV32=/home/mcloonan/redoubt/bios/target/riscv32imac-unknown-none-elf/release/rustsbi-prototyper. Scratch in $REDOUBT_TMP/CTX1, never /tmp.
- Keep one command per tool call for commits; don't chain `git branch -f`/commits after steps that may fail.
- Generated code: `cargo run -p redoubt-wire-gen` (wire tables) and `cargo run -p redoubt-steward-gen` (steward tables: regenerates libs/steward/src/gen, elixir/gen, mermaid). Never edit gen output by hand. A new guard must be listed in libs/steward/gen/src/lib.rs GUARDS (done for context_free).
- **Pre-existing main bug fixed in the WIP**: servers/steward/tests/steward.rs did not compile on main (b95d0fda2 gave init::check::args a 4th `entries` arg). Now `args(&m, entry, &[0;32], &[])`. Report it.
- Behaviour change: a second login into a live context of the same name (incl. two `ssh alice@box`) is now refused `in_use` (CTX1b turns it into takeover). Checked: no machine case logs one name in twice at once. libs/steward/tests/core.rs has tests logging `carol` (and others) twice in one domain: they will now fail with InUse; give them distinct contexts (the core test helper `login(name, labels, key)` at line ~197 needs a context arg; policy_current.rs already uses a counter c0,c1,...).
- Elixir reference uses no regex (beamlet-re): `name?/1` is a binary loop.
- model.md says "Mutation::ALL lists all 153 variants" but code had 154 before this; now 155. Fix the page count.

## The design (approved)
Full text: /home/mcloonan/redoubt/.wash/local/CTX1-design.md. Orchestrator's answers: A yes, no ':' in principal/label/context names (handle names keep theirs). B contexts are free names made by first login, within the cap. C wall clock if any, else "up 2h13m" since boot; no invented date. D relay program in the context's own budget. E the session's own steward badge is the handle. F split into three packages merged in order:
- **CTX1a** (this): names, parsing, the init checks, the table, the refusal/enumeration rules.
- **CTX1b**: the relay (consrelay), detach/reattach, takeover (both terminals told, ipd /tcp/N/remote address), the 64 KiB buffer; sessions.md says plainly closing SSH no longer ends a session.
- **CTX1c**: the cap, idle expiry (`contexts.detached_secs`, default 86400), commandlets contexts()/detach()/end_context, generation on restart.
- Cap decision (my answer, accepted path): default `max` = what the label set's sub-budget holds, floor(share/(session+cost)) = 2 today; init refuses an explicit max that does not fit. Each package: steward red + its own machine cases.
- Restart rule: contexts END with the steward (K23 reap), plus a per-instance generation (CTX1c).

## Done in the WIP (ca5e062ae)
- sshd: `Login{principal,label,context}`, grammar principal[+label][.context], names [a-z0-9_-] letter-first ≤64, no ':'; `Login::NOBODY` (principal "") sent to the steward for an unparsable name (one path, no enumeration); `pub fn name`. Box bin sends `context` and logs `p+l.c`. Host tests rewritten (grammar, NOBODY, context passing).
- Wire: steward `login` gains `context: string` (after label); error 15 `in_use`. Regenerated.
- Core (libs/steward): `EventKind::Login{..,context}`; `Session.context: Option<String>` (None = console session); `Refusal::InUse`; `manifest::name`; external Login: unknown principal / label set / bad context name -> `BadKey` (was Unknown/NotOwner); guard `context_free` (R79): no other non-Ending session of the domain with that context -> InUse; table row `| - | Login | !context_free | - | refuse |` after not_locked; audit Record::Login gains `context`.
- Server protocol: key id first; unknown label name -> BadKey (was NotOwner); passes context; InUse mapping.
- Trace crate: input `context=` optional (default ""), writer writes it, output prints `context=` on Login records and session store lines (`none` for console).
- Elixir reference: same in steward.ex (with-chain -> BadKey), guards.ex context_free, effects.ex audit, trace.ex parse/print.
- Model: PolicyOp::Login{context}; CONTEXTS pool ["", "", "a","b","c","work","Work","a.b"]; P17 (one live session per context name per domain; a non-name context never logs in) with reach instances "P17 a named context", "P17 a login to a live context"; mutation PolicyContextTwice (rule "R79", p.context_free = pass), ALL 155; steward_reach guards list has context_free; contracts.rs/policy_current.rs updated.

## Left in CTX1a
1. Run and fix: `cargo test -p redoubt-steward` (core.rs: same-domain double logins -> add contexts; tests of unknown principal/NotOwner expect BadKey now; add host tests: context_free, uniform refusals, bad context name), `-p redoubt-sshd`, `-p redoubt-steward-server`, `-p redoubt-steward-trace`, `-p redoubt-steward-gen`, `-p redoubt-wire-gen`, `-p redoubt-init`.
2. Traces: add hand-trace rows for the new row (libs/steward/trace/traces/session.trace: a second Login same context -> refused InUse; a login with context; bad context -> BadKey); run `libs/steward/elixir/run-traces` (both sets) and `steward-trace check`; `bench-elixir-oracles-broken-guard`/`elixir-oracles` cases.
3. Model: steward_policy / steward_noninterference families (model-host-tests), `REDOUBT_MODEL_MUTATIONS=PolicyContextTwice` mutations run; re-measure the reach table (steward_reach `the_reach_table_is_reproduced`, LAST constants) and kernel/model.md's tables (mutation table row R79, family reach numbers, the variant count).
4. init: manifest check refuses principal and label names with ':' '+' '.' (labels already?) and reserved principal names (`approve`); docs init.md "Names"; servers/init tests.
5. Docs: R79 defined in docs/servers/steward.md Security properties (status line naming the guard tests/cases), a "Contexts" section (steward.md cites "Contexts" in comments), login op fields, guards table row (context_free, PolicyContextTwice), trace encoding (context=), sshd.md Login bullet (grammar, NOBODY path), sessions.md How to use it (`ssh alice.work@box`, a second login of a live context is refused until reattach arrives — phrase without package IDs), SECURITY register row for R79 if the register lists steward rules; docs checker.
6. Machine case(s) both widths, e.g. steward-context-login: alice and alice.work live at once; second alice.work refused; `alice.work+x` refused; alice+alice-secrets.work separate from alice.work (vault manifest data from steward-vault-session). Then steward-ssh-two-principals, steward-session-ends, steward-restart-ssh, sshd-loopback-* (malformed names now hit the host login as "" -> "login : refused" log line; check server_log patterns), userland-boot, init-boot.
7. Fold WIP into logical commits (e.g. sshd+wire; core+tables+reference+traces+model; init; docs+cases), Co-Authored-By line, then send head as `question` and update B-reports in .wash/local/CTX1-report.md.
