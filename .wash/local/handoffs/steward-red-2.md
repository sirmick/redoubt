# steward-red-2 handoff (2026-10-15, checkpoint at 70 % context)

## Branch state
I am a reviewer: no branch, no worktree of my own, nothing uncommitted anywhere. I work read-only in the project root and in other members' worktrees (`.worktrees/<PKG>`). Every result went out as a Wash assignment result; I wrote no file. The review queue is empty: AGT2 renewed OK at 565b9cd37 and accepted.

## Traps (read before any review)
- **Reviewers never write.** No Write/Edit, no `>`/`>>`/`tee`, no git that changes anything, nothing under `.wash/local`. Results go only through `member_update assignment_results` (<= 2000 bytes; the server refuses more: trim, do not split into a file). Verdict is the first line: `Merge verdict: OK | OK with notes | BLOCK`. Always `cc` the implementer named in the assignment.
- **Never read `.wash/qa/*.md` whole or tail it** (base64 blob at the end); `grep -n '^## '` then `sed -n` the one event. Never `cat` logs; grep them. Read files by region.
- **Environment switches cwd under you** (the "Environment update" notes). Use absolute paths or `cd /home/mcloonan/redoubt/.worktrees/<PKG> &&` in every command.
- **A rebase/amend needs a renewal**: the orchestrator asks; diff old head..new head (`git diff <old> <new>`), and when a package rebases, check the VM/lib code is byte-identical to where its gates ran (`git diff <gate-head> <new-head> -- <paths>` empty) before accepting "gates stand".
- **Pid-in-body authentication hole (AGT2)**: any Elixir protocol in the shell that authenticates a message by a pid carried in the message body is forgeable by evaluated code (a block learns the shell's pid from `Process.info(self(), :monitored_by)`, the driver from `Redoubt.Shell.Driver.of_group/0`, and any process can `Process.info(pid, :messages)`). The fix pattern: a private reference minted by the receiver for the first asker before any line runs, carried in every message, kept only in loop state (never `limits`, never the process dictionary). Check every new `{:redoubt_*, ..., pid}` message for this.
- **B47's Life rule** (userland/otp/redoubt/src/system.rs): a handle that came through a VM-carved budget carries that budget's `Life` as `returns`; destroying the budget marks it gone and nothing closes its index (the kernel freed it; a reused index must never be closed by a stale term). Any new handle path must say which Life it is under; "kept, never closed" is the leak-not-close trade and must be stated on beamlet.md.
- **Tag/badge spaces**: badges on a served endpoint are SELF (u32::MAX) and LAUNCH_BADGE 0x8000_0000.. (B50); 0x200+/0x300+ are the VM's wake endpoint's; SERVED_BADGE 1 a child server's own endpoint. Only a receive right mints (objects.md:299). Check any new badge range against these.
- **Startup block VERSION** is `redoubt_wire::STARTUP_VERSION` (3, since B72's budget_pages); rt and the stub import it; a layout change moves all consumers together (28 root crates + beamlet; `tests/net` is redoubt-net-tests).
- **Floors in guest time**: icount cases (shell-output-rate, shell-text-profile) are load-independent within ~7 %; a margin of 1.2-1.3 has held through trains. A host-clock case's failure beside load is no verdict (testbench.md "On a shared host"); quiet class is the right place for one whose failure is load-bound.
- **Pages say what is, not what was.** History belongs in commits. The shell.md rate bullet keeps "before" figures by the owner's design; flag when it grows past one baseline.
- **Mutation evidence**: ask for the assertion line a reverted fix fails at, run on the final head, not an earlier variant.
- **Text.visible / Width.run (escaping path, Tier A strict)**: the matchers' classes must equal `control?/1` and the Width ranges exactly; `Width.run` requires valid UTF-8 (`:re` unicode raises on the BEAM, not on beamlet), satisfied only because its sole caller is `advance_text` over text already made visible with a 0x20..0x7E or C2..D4 first byte. The forbidden table is `userland/native/cells/forbidden.txt` (B69), held by three decoders; the joiner exception is narrower than GB11 (safe side).

## House rules I apply
- Record base, head, paths read; findings P1/P2/P3 with file:line, the concrete sequence, what breaks, and keep/trim/delete.
- "The page says what the code now does": check the owning page AND affected summaries (README, GETTING-STARTED, m2 plan, beamlet.md differences passage :361-397, SECURITY.md residuals). A removed residual must not leave an overclaim (e.g. look-alike letters after B69).
- Budget arithmetic read in full (steward vm_pages = session_pages - RELAY_PAGES; beamlet limits = budget/16; the footprint cap rule: peak <= cap/2, bench scan line `heap NAME PEAK of CAP pages`, peak from `target/jobs/rv64-<case>.log`).
- Tier A questions first: who writes the input, can the launchee/evaluated code forge or reach it, what dies with what budget, is the bound absolute or relative, does the order (kernel first, then bookkeeping) hold.
- Difftest coverage is often thinner than claimed: `userland/otp/tests/atomvm/*` and `cryptotests/garbage.erl` (catches everything, prints a count) prove little; say what they cover.
- OK with notes is the usual verdict for a sound change with page or test gaps; BLOCK only for a reachable wrong behaviour or an unverified Tier A claim. Renew in one round when the fold is exactly the notes.

## Done this session (all accepted or renewed OK)
B59 (home=), B53 (save.sh --local-only; P2 ROOT-from-cwd), B48 r1+r2 (Known entries under Life; hub disconnect), B54 (doccheck C11), B52 (event waits), B61 (hand-over waits), B51 (+ renewal on the files.rs merge hunk), B62 (FILE_WAIT_US), B60, B65 (identity shortcut in compare), B64 (+renewal), B68 (+renewal: absolute modpow bounds), B50 (launch badges), B70 (held clock), B69 (invisible format chars), B71 (compiled :binary.match), B72 (+renewal: budget_pages from the Cap), AGT2 (+2 renewals: pull protocol, driver-minted key).

## Coming next (reports live at `.wash/local/<PKG>-report.md`, briefs at `.wash/local/<PKG>-implementer.md` when the owner wrote one; node bodies: `grep -n 'id = "<PKG>"' -A5 .wash/plan.toml`)
- **AGT1** (Tier A: the agent principal: manifest, keys, launch). Check: init's manifest checks (servers/init/src/check.rs) for the new principal, key separation R35 (login vs approval keys), the steward's own-lines parsing (servers/steward/src/own.rs) and session_args, no new authority beyond a session's; the bench's `tests/data/steward/stage` and `image/manifest.json`; sessions.md "What a session is told"; steward.md.
- **AGT4** (Tier A: the defmodule store page fault found by AGT2: beamlet's 18-page scheduler stacks; heap-held compressor, a compile-footprint case, stack re-declared). Check: the manifest's stack declaration vs the measured peak (docs/kernel/budgets.md, MEM1's paint measurement), that the compressor moves to the heap and is counted toward the heap limit (BEAM16/17 rule: sized resources count), beamlet-footprint still under cap/2, and that shell-machine's case can send a real `defmodule` again (AGT2's residual on shell.md must then go).
- **B74** (Tier A libs/rt: the fake kernel's quiescent tick). Check against B70's held clock (`Fake::hold_clock/tick`, waits end only by change or tick, STUCK 60 s guard), that non-opted-in binaries are untouched, and beamlet-redoubt's `fake` suite (`cargo test -p beamlet-redoubt --features fake --test system` in userland/otp) is run, the fake's largest user.
- Later **AGT3**.

## Open notes I left for the orchestrator (not blocking)
B53 P2 (save.sh ROOT from cwd; owner's script); B59 P3 Session.identity public; B62 P3 lib.rs:444/:519 console waits ignore files.retry_in; B71 P3 no difftest for compile_pattern/split options; B68 difftest covers no bound case; B69 P2 look-alike overclaim on shell.md:536 (check it was folded); B72 P3 init.md row for a child in the VM's own budget.

## What consumed my context
Full diffs of 20 packages (each 200-900 lines), the reports, and repeated 2000-byte-limit resends of results (write tight the first time: ~1800 bytes).
