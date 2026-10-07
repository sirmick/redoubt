# STEWARD2 handoff (steward2-implementer-4), 2026-10-07, later

## Branch state FIRST
- wp-STEWARD2 (worktree /home/mcloonan/redoubt/.worktrees/STEWARD2) HEAD fc9944a35: 15 commits on main 226245507 (BEAM9 merged), panel P1/P2 + editor notes folded, B9's sizes taken (session 11,008, alice 44,040, heap cap 10,989). Not pushed, not yet announced.
- A gate batch was started on fc9944a35 in the STEWARD2 worktree (background; /tmp/s2batch.sh output; logs in target/jobs and /tmp/s2runs). Do NOT edit the STEWARD2 tree while it runs.
- Simplifier notes in progress in the scratch worktree /home/mcloonan/redoubt/.worktrees/s2-main-probe on branch s2-simplify (= fc9944a35 + 4 fixup commits): (1) rt's say used; (6) after() helper; (3) one spec(l, labels, account, deadline); (5) open_session in libs/steward machines.rs; (8) done() in sshd + libs/wire/src/labels.rs encode/decode used by steward protocol.rs and sshd; (9) init label_ids; (7) steward.md sentence on the console reopen. Builds both widths, host tests pass (steward, steward-server, sshd, wire, init).
- Remaining: (2) one pure fn binding(own, principal, labels, slot) in libs/steward, used by the steward's connect() and launch(), host-tested (the doc table is its test); (4) export SLOTS from libs/steward (= init's STEWARD_SLOTS), MAX_RULES from wire's ipd (= init IPD_RULES = ipd scope.rs:16), and move own.rs:101-125's ipd scope encoder into wire. (10) history stands.
- Then: bring s2-simplify's fixups onto wp-STEWARD2 (`git -C STEWARD2 cherry-pick <fixups>` after the gate finishes), autosquash on 226245507 (expect a steward.md conflict like before: keep the section's status as at each commit), size-budget ceilings (run `q run --cores 2 -- cargo testbench --exact size-budget`, set, fixup to the image commit), prebuilt, rerun: rv32 userland-boot, userland-read-only, boot-profile, boot-profile-unverified (+ rv32 boot-profile figures into docs/userland/beamlet.md's table row and paragraph, which say rv32 not measured; recheck targets by the rule), rv64 profiles, steward-* both widths, steward/sshd/init host tests, docs, formatting, size, unsafe. Then announce the tip as a `question` to the orchestrator, and to K23 (k23-implementer) and the cases implementer (f5bdce52), with results; report file section + rebase log (.wash/local/STEWARD2-rebase.md, sixth rebase written).

## Open
- rv32 steward-sub-budget-flood intermittent (bob's VM ends itself): K27, the beamlet implementer's; keep as open item.

## TRAPS
- /tmp/s2env.sh cd's into STEWARD2; for the probe worktree cd after sourcing.
- Never pkill -f patterns matching your own shell; use /tmp/s2kill.sh <cwd>.
- Python conflict resolvers: if an assert fails, `git add` + continue still commits markers: check `grep -c '^<<<<<<<'` before continuing.
- No package IDs in pages/messages; stage by path; no push.
