# CTX1 part a: report

Branch wp-CTX1, worktree /home/mcloonan/redoubt/.worktrees/CTX1, head 922a7c7e5, on origin/main
415c86ad6. Two commits, clean tree, nothing pushed:
- d513291dd testbench: a session has a name of its own, so two may log in as one user
- 922a7c7e5 steward, sshd: a login names its context, one session at a time, and every refusal
  before a session reads as a wrong key (with `Size budget:` lines for libs/wire, libs/steward,
  model, servers/sshd, servers/steward and servers/init)

The predecessor's fix to servers/steward/tests/steward.rs (init::check::args's 4th argument)
was dropped in the rebase: main carries the same fix (37587ae61).

## Delivered
- sshd: `principal[+label][.context]`, one order; names `[a-z0-9_-]`, letter first, 64 bytes or
  fewer, no `:`; `approve+x` and `approve.x` do not parse. A name that does not parse goes to the
  steward as `Login::NOBODY` (principal ""), so every refusal after the signature takes one path.
- wire: `login` gains `context: string`; error 15 `in_use`.
- Steward core: `Session.context` (None for the console's session); an external refusal for an
  unknown principal, a label set the manifest does not give, or a context that is not a name is
  `BadKey`. `owns_labels` now answers a login `BadKey` too (it was `NotOwner`, which told a
  known-but-unowned label set from an unknown one). New guard `context_free` (R79): a live
  context's second login is `InUse`. The audit record carries the context.
- Elixir reference, trace crate, hand traces: same changes. Rows for a context login, `in_use`
  and a bad context name; repeat logins in blame/request traces now name contexts.
- init: principals and labels refuse `:` and `+` (`Why::AccountName`); a principal named
  `approve` is refused (`Why::Reserved`); `manifest::RESERVED` is in libs/steward.
- Model: P17 checks one live session per context name, and that every wrong login (key, label
  set, context) is refused `BadKey`. Mutation PolicyContextTwice (R79), 155 variants. Reach and
  catch tables re-measured: policy 118 items, last at seed 3470, count 14,000; noninterference
  120 items, last at 405, count 2,000; catch floor 575/737. steward-model-host-tests timeout is
  now 35 (longest job steward_policy, 26 s).
- Bench: an optional session `name`; users may hold `.`; the key defaults to the principal.
- New case steward-context-login (rv64 and rv32): bob.work live; a second bob.work refused
  `InUse`; `bob.work+x` logged as `login : refused (BadKey)`; `bob+alice-secrets.work` refused
  `BadKey`; bob's default context beside bob.work, markers kept apart; the audit records carry
  `context: Some("work")` and `Some("")`. steward-login-refused: unowned label now `BadKey`.
- Behaviour change: two `ssh alice@box` at once now give one session and an `in_use` refusal.

## Tests run on the rebased head (all through q/jobs.mk, exit 0 unless noted)
- Cases: size-budget, unsafe-budget, no-cruft, formatting, docs, steward-host-tests,
  sshd-host-tests, init-host-tests, wire-host-tests, host-tests (bench self-tests),
  elixir-oracles, bench-elixir-oracles-broken-guard, model-host-tests, steward-model-host-tests.
- Model mutations (Policy*, R2OneCursor), release: all caught, before the fold; model code
  unchanged since.
- run-traces 0; `run-traces --break context_free` diverges (rc 1), before the fold.
- prebuilt rv64 237/0, rv32 223/0.
- Beamlet set (`./scripts/shell-cases origin/main`, 31 cases) together with every steward-*,
  sshd-*, init-*, userland-boot and bench-ssh-* case: 71 cases on both widths, 123 PASS,
  2 FAIL: init-refuses-bound rv64/rv32. That failure is on main too: main's init prints 1061
  pages and the case expects 1047. Run alone on origin/main 415c86ad6 (and on 93f933c7a): same
  failure. Not this package's.
- Run of `scripts/shell-cases` from the main checkout picks no cases (it cds to its own repo);
  run it from the worktree.

## Pages and summaries checked
- Updated: docs/userland/sessions.md (how-to, new Contexts section, Logging in status),
  docs/servers/steward.md (Contexts section, R79, guards table, login op, trace encoding, status
  list), docs/servers/sshd.md (Login bullet, status list), docs/servers/init.md (Names, test
  list), docs/kernel/model.md (counts, seeds, 155, R79 row, wall time), docs/testbench.md
  (session name, key default, steward-model-host-tests row), docs/SECURITY.md (R79 row),
  docs/GLOSSARY.md (context), docs/plan/m2-usable-shell.md (remaining work and progress).
- No change needed: README.md and GETTING-STARTED.md (no login-name or context claims);
  sessions.md's handle-name rule (`_:+-`, the startup block's, unchanged); libs/wire table text.
  The claims that closing SSH ends a session stay true in part a, and the pages say the rest is
  planned.

## Design notes and risks
- `owns_labels` is the one keeper of the vault rule; its refusal now depends on the kind
  (session: BadKey, request: NotOwner). PolicyVaultWithoutOwnership is still caught.
- PolicyDeclassifyLive is now caught at seed 488 against the 500-seed steward cap, close to it.
- The reserved-name list is in two places, sshd's `Login::RESERVED` and libs/steward's
  `manifest::RESERVED`, because sshd does not depend on the steward crate.
- InUse after authentication tells the principal that its own context is live. This is by
  design: the principal is authenticated.

## Next
Steward red review (Tier A). CTX1b: relay, detach/reattach, takeover (it can use the bench's
session `name`).
