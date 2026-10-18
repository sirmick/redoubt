# steward-red-4 handoff (2026-10-18, ~70% context, no review open)

## Branch state
Reviewer: no branch, no worktree, nothing uncommitted anywhere. Work only read-only in other members' worktrees (`.worktrees/<PKG>`). Every result went out as an assignment-completion body + a message to the orchestrator (both ≤2000 B). No review open. STEWARD8's review (re-measure the N=16 / two-hart latency rows on STEWARD5's thread; the launch bound) comes in a few hours.

## Traps (read before any review)
- **Reviewers never write.** No Write/Edit/redirects/stage/commit. Result = assignment completion body (≤2000 B, first line `Merge verdict: OK | OK with notes | NOT OK`) AND a `message_send` to orchestrator (≤2000 B). For a **task assignment** complete it with `member_update assignment_results`; for a bare instruction (renewals) reply by `message_send type:answer`. `reply_to` an **assignment_id** fails ("unknown reply_to") — send without reply_to, or reply_to a message id.
- **cc a retired member fails** ("not an active workspace member"); drop the cc and resend.
- **Worktree cwd switches under you** (Environment update notes); use absolute paths or `cd .worktrees/<PKG> &&`. The orchestrator's `.wash/local/<PKG>-report.md` and rulings live in the MAIN checkout (`/home/mcloonan/redoubt/.wash/local/`), not the worktree.
- **Never cat console logs / QA base64 tails**; grep + region reads. `plan_get` = 115 KB, never.
- **First move on any review:** `git diff --stat <base> <head>` then confirm `libs/steward model libs/wire` are untouched (`git diff --stat <base> <head> -- libs/steward model libs/wire`). The STEWARD* server packages are nearly all server-embedding; the core is reviewed and frozen (STEWARD0's rule). An empty core diff shrinks the Tier A surface enormously.
- **Renewals:** `git range-diff <oldbase>..<oldhead> <newbase>..<newhead>`; `1: = 1:` means identical patch (clean rebase). Content lines `^    [-+][-+]` are the real changes.
- **Measurement bounds house rule:** twice the worst measured, rounded up to a second (SSH3 LOGIN_US 2×2.41→5s; STEWARD6 SERVER_US 2×1.66→4s; 3V SERVER_TIMEOUT). A bound far above the measurement proves less — flag it.

## The steward's rules I check, and where they live (all servers/steward/src)
- **3U console queue** `console.rs` + bin `Out`/`Console`: one kept /dev/cons fid, lines drained at the loop turn within CONSOLE_US, before a call's reply (audit before the caller's line) and at the loop top; retry/park/drop+count like 3W.
- **3W parking** `cleanup.rs`: a step the server didn't take parks after TRIES(4)/RETRY_US(1s), no timer, woken by an answered call; `forget` on restart.
- **3Y cleanup** `drive.rs give_back`: end batch answers after DestroyBudget (cutoff), connections given back as deferred steps; `release` closes the steward's own copy now.
- **STEWARD5 thread** `bin cleanup_thread` + `cleanup.rs Queue{steps,parked,running}`: cleanup runs on a thread; loop top makes NO shared-server call (only consoled). Queue under `redoubt_rt::sync::Mutex`, never held across a call (take→unlock→call→lock→settle). Thread idles in a GO call the loop answers by reply; fates come back as sends; no lost wakeup (cleanup_idle rings if due, cleanup_tick re-checks every top); retired handles closed only between the thread's calls; "a thread can't die alone" (process dies). Mint: wake async, mint once, fail if refused, no await.
- **STEWARD6 SERVER_US (4s)** `bin`: every shared-server call *inside a batch* is `within(SERVER_US)` (attach_within / Nine::within carry the timeout to later calls); `called` maps Sys(Timeout)→Timeout→fails the step. Cleanup disconnect uses STEP_US=RELEASE_TIMEOUT(1s) on the thread (loop pays nothing).
- **Admission derivations** (3Q/3V/3Z/STEWARD7): a server's usable budget = entry pages − heap cap − 128 stack (`libs/rt/src/server/admit.rs budget_of`); a minted connection is admitted in the *holder's* account bucket from its first request (3Z `ninep.rs arrived`); lease term = min over R7 dims + home_quota/agent.quota; in_flight = min(share/min(session,agent).pages, proc) + 1; R4a headroom: Σ in_flight ≤ MAX_OPEN_CALLS−OPEN_CALL_HEADROOM(192). **The two bench tests in `tools/testbench/src/case.rs` are the ground truth** — `the_servers_admit_what_a_manifest_lets_one_account_hold`, `every_manifest_entry_holds_its_buckets_at_their_caps`, `every_servers_buckets_leave_the_open_call_headroom`. They assert every manifest in the tree; if they pass the caps/entries are right by construction — verify the test LOGIC matches the ruling, then trust the pass over hand-arithmetic.

## Open notes I carry
- **STEWARD7 Q5 processes-min edge case**: in_flight carve term adds a processes min over the ruling's pages-only (only tightens; a hypothetical tiny-VM manifest would blow the headroom on pages alone, kept safe by the min). Awaits the Architect's word; no real manifest moves. Not a blocker.
- **N=16 latency rows**: measured pre-STEWARD5 (tip 5923f7bc8); rv64 one-hart timer wake missed (the 30th launch refused at a starved erofsd:system under 16 VMs with parked disconnects holding slots — the Architect's "F2 finding", stated, no node). STEWARD8 re-measures N=16 and the two-hart rows on STEWARD5's thread. Expect the one-hart N=1..4 to stay met (no 100 ms retry since STEWARD5).

## Review habits that caught real gaps
- **STEWARD6 first-time carve (NOT OK → one-line fold)**: they changed the *cached* carve return to `Nine::within(SERVER_US)` but left the *first-time* `Ok(Nine::new(endpoint))` = FOREVER, so a principal's first login's home mint (fresh_carved→first carve) was unbounded — the exact DoS the package closes. The host tests mock at the `Kernel::connect` boundary (Recorder), so they *can't* catch the real Machine impl. **Habit: trace the Machine's actual impl for a bound/leak, don't trust host tests that mock the Kernel trait.** Grep for the inconsistent twin (cached vs first-time, bounded vs not).
- **ATK11 bound shape (OK with notes → house-rule fold)**: cases bounded at 10s wall-clock (~5× worst), proving non-starvation not "served in her turn"; the fix was to assert the probe's own ms under the house-rule 4s via the expect digits and demote `within` to a flake guard. **Habit: ask what a PASS *demonstrates* vs what the row *claims*; a loose `within` with an unasserted value proves less than the claim.**
- General: for any loop/concurrency change, trace the lost-wakeup path and "can the loop ever block on a helper"; for any admission change, trust the tree-wide bench test once its logic matches the ruling.

## Files a successor reads first
1. `.wash/SWARM.md` Reviewers section; steward-red-3's handoff (`.wash/local/handoffs/steward-red-3.md`) for older traps (badge spaces, R41, the pid-in-body hole).
2. `.wash/local/wave-2026-10-18-rulings.md` (F1/F2/F3 and the agent-suite; the current wave's design).
3. `servers/steward/src/{cleanup.rs,drive.rs,bin/steward.rs,console.rs}` and `tools/testbench/src/case.rs` (the two/three bench derivation tests).
4. Per review: `.wash/local/<PKG>-report.md`, `.wash/local/<PKG>-implementer*.md`, the node body in `.wash/plan.toml` (`grep 'id = "<PKG>"'`).
