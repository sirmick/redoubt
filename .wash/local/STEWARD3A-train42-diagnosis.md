# STEWARD3A, train 42: diagnosis (steward3a-fixer, 2026-10-16)

`model/src/gen.rs` is unchanged by 2d831801b: no draw moved. The two mutations are not caught
within the caps at all (rerun alone, `.tmp/STEWARD3A-fix/mut-*.log`), not caught late.

## userland-boot

A sponsored principal prints nothing at boot: `Manifest::carves()` (libs/steward/src/manifest.rs)
skips it, and the steward's only boot line per principal is the carve. Fixed as agent-login does:
the expect line is gone, `steward: carved users/agent` is forbidden, the description says so.

## PolicyNoContextCap: masked, not unreached

The reach table at head (`.tmp/STEWARD3A-fix/reach-head.log`) reaches `rule under_cap refuses`
and `P19 a login refused at the cap` at seed 3. `carves()` gives each label set
`top.processes / n` = 12/3 = 4 processes, which hold 2 sessions (`sizes.session` is 2
processes), equal to the cap of 2. With `under_cap` passed, the kernel refuses the third login for
processes instead, so P19 never sees a third live session nor a `cap` refusal with fewer live.
Only carol's one-set domain (12 processes, 6 sessions) could exceed the cap, and her sponsorship
(her sessions carved in alice's {} sub-budget) took that away.

Fix: the model manifest's top gets 18 processes (6 a set, 3 sessions; alice's and bob's tops,
36, fit `users`' 48 in model/src/steward.rs); the reach table re-recorded.

## PolicySubAgentOutlivesAgent: uncatchable since the P2 fold, not since carol

`machines.rs:399` is the only place a lease gets a parent (a lease starting an agent) and the
fold's guard `caller_session` refuses exactly that, so the sub-agent arm of `carve_lease`
(effects.rs:84-91) is dead in the shipped core, and the mutated `carve_lease` (mutation.rs:922)
computes what the real one does. No generator or trace change can reach it.

Options:
- (A, recommended) drop the mutation (177 → 176) and the dead arm, so `carve_lease` reads what
  ships; keep `Lease.parent` (always None; in the audit record the bench cases match) and
  `sizes.sub_agent` (the manifest format) as the later package's. The model's unreachable
  sub-agent checks go too (policy.rs 654-667, 1157-1159, the "P1 a sub-agent" item). Pages:
  steward.md rows 236, 779, 1104, 1106; model.md 314 (the count) and the R39, sessions and
  leases rows; agents.md 113 and 148-150 say a sub-agent dies with its agent by the kernel's R10,
  what steward-sub-agent-dies-with-agent shows.
- (B) undo the fold: a lease may start a sub-agent (the reviewer ruled against).
- (C) leave it: 176 of 177 caught, the bench red.
