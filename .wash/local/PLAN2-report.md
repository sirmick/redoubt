# PLAN2 report

Branch wp-PLAN2, worktree /home/mcloonan/redoubt/.worktrees/PLAN2, base 56a77fcf3, head d9a39ca42 (one commit).

## What changed

- docs/plan/m1-separation.md: Goal is the SSH slice (SSH and console logins into Elixir sessions; fixed sub-budgets per label set; read-only system, writable /home and /vault; a dead steward restarts without a reboot; the scheduler's targets hold). Attack suite keeps the intro and the kernel cases. The hostile agent and user tables moved out. Four first citations became full links (R10, R12, R3, R2) because C5 now meets them first here. Remaining work keeps: follow-ups, client library, VM platform, the steward's first half plus restart, Files (walfsd /home and /vault), sshd sessions. Progress unchanged except the last line: "and the agent" dropped, because the agent is no longer M1's.
- docs/plan/m3-agents.md (new, after M2 in SUMMARY): goal; the attack suite text moved whole (intro, hostile agent and hostile user tables); remaining work, in order after M2: the steward's second half (leases, agents, powerbox at approve@box, crash blame, then declassification and push, plus the destruction cost, the decision-wake target and the latency rerun with the real steward), sshd's vault sessions and approve@box, gen_tcp over /net, the agent and the attack suite, the model on the real kernel; progress (the policy core is host-tested, the kernel's halves and the containment gate exist).
- Renames: git mv m3-files→m4-files, m4-self-hosted→m5-self-hosted, m5-persist→m6-persist. Every link updated. Titles are M4/M5/M6. M4 now runs after M3.
- docs/plan/m2-usable-shell.md: the host shell steps start ahead of M1, as several harts' first package did. The shell's Redoubt steps and the rest of several harts follow M1.
- Status lines moved M1→M3: steward.md (Leases, Powerbox, Declassification, Crash blame, R37–R42); sshd.md (approve@box, R67, R68); sessions.md (Vault sessions, approve@); userland/README.md (What an agent sees); agents.md (all five M1 sections); model.md (Replaying traces). SECURITY.md rows R37–R42, R67, R68 match. R36, steward auth/sessions, protocol, Authority and Failure and restart stay M1.
- Prose: every "M3/M4/M5 (...)" now reads M4/M5/M6, and "beyond M5" now reads "beyond M6" (title of beyond/README.md included). Prose that put agent or approval work in M1 now points to M3: TOUR (walls figure, road text, mermaid with six nodes), servers/README figure caption, kernel/README (two links to m3-agents.md), todo/kernel-attack-gaps (trace replay), sessions.md (approve@ residual). keyd and leases in init.md, steward.md and agents.md now say "until M6 (persist, install, share)", which matches keys in leases in M6. userland/README.md and shell.md say "what the milestone needs" in place of "what the attack suite needs".
- README.md: Today paragraph and Goals (M1's new goal; M2's host track ahead of M1; a new M3 bullet; M4–M6). docs/README.md milestone table gets an M3 row. GETTING-STARTED.md and CONTRIBUTING.md cite no milestone, so no change. There is no root SECURITY.md (docs/SECURITY.md is updated).
- **Outside docs, needed by the docs check:** tools/doccheck/src/lib.rs MILESTONES is now 6 entries (M3 is "agents, approvals and the attack suite") and the beyond exception is M6. tools/doccheck/tests/fixtures/good/README.md and the comment in tests/rules.rs follow.
- Code comments that cited a milestone by number: servers/keyd/src/server.rs and server_tests.rs (M5→M6), userland/otp/redoubt/src/lib.rs (M5→M6), model/src/check.rs (the endpoint-flooding case now cites plan/m3-agents.md).

## Checks

- `make -f scripts/jobs.mk -C .worktrees/PLAN2 prebuilt rv64/docs`: the docs case PASSES (rc=0) on the final tree. prebuilt itself returned rc=0. Its rv32 prebuild reported 19 cases failing with "mix: not found" (no Elixir toolchain on PATH in this shell). Those failures are environmental and not related to this change.
- rv64/no-cruft: PASS (rc=0).
- `q run -- cargo test -p redoubt-doccheck`: 2 + 18 passed, rc=0.
- `rustfmt +nightly --check` on the 5 touched .rs files: clean.

## Summaries checked

README.md (updated), GETTING-STARTED.md (no milestone references, no change), docs/README.md (updated), docs/TOUR.md (updated), docs/TENETS.md (renumbered only, and its M1 mentions stay true), docs/SECURITY.md (updated), docs/kernel/README.md (updated), docs/servers/README.md (updated), docs/userland/README.md (updated), docs/todo/README.md (its M1 follow-up sentence stays true, no change).

## Open points for the orchestrator

- The M1 node title in the plan is "sessions over SSH, kept apart". The book and the checker keep "M1 (separation and containment)" because the assignment did not ask for a rename. Changing it means changing the checker's table and every M1 citation.
- The plan's node bodies still point to m1-separation.md#attack-suite: agent-suite at .wash/plan.toml line 105, and the gate node's "#remaining-work" text. They should now point to plan/m3-agents.md#attack-suite. I did not touch them (Wash only).
- The worked configuration (init.md) keeps its M1 status even though it shows Alice's agent, because most of it is M1's. servers/README "Restarts and crash blame" stays M1 for the restarts.
- Reading: I read the two plan pages and the whole diff in full. For the ~45 files changed only by renumbering or a status line, I read the diff hunks, not each whole file.
