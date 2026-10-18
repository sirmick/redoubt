# STEWARD7 report (steward7-implementer)

Branch `wp-STEWARD7` on main f28960f34; worktree `/home/mcloonan/redoubt/.worktrees/STEWARD7`.
Logs under `/home/mcloonan/redoubt/.tmp/STEWARD7/`. Head: see the end of this file.

## Rulings built on

- F2 (the term): `.wash/local/wave-2026-10-18-rulings.md` "F2".
- Q5 (c), thread STEWARD3V-budgets: `in_flight` is what the share carves at once in VMs, and a
  spare; `state` keeps F2's sums; a bench assertion guards R4a's headroom across the tree.
- Q6 (accepted): the term takes weight beside pages and processes; the latency alice at weight
  2,000, pages 190,756 (the per-budget page in the arithmetic), processes 96.
- One note sent on Q5 (awaiting the Architect's word): the `in_flight` carve term takes
  processes and weight beside pages (the smallest), because the beamlet case manifests' `agent`
  size is 128 pages / 1 process with alice at 32,768 pages / 8 processes, so pages alone gives
  255 + 1 there (past the headroom at four buckets) and processes 8 + 1. No real manifest's
  number moves (image alice 7 / 16 / 10 → 8; latency alice 18 / 24 / 20 → 19).

## What was delivered

- **The bench's derivation** (`tools/testbench/src/case.rs`,
  `the_servers_admit_what_a_manifest_lets_one_account_hold`): the constant `LEASES = 1` is gone.
  Per principal (a sponsored one reading its sponsor), with `share` = budget.pages / label sets −
  cost, processes and weight likewise over the label sets:
  - `leases = min(share / agent.pages, processes / agent.processes, weight / agent.weight)`;
  - `sessions = console + cap + 1` (the cap as before: `contexts.max` or what the share holds);
  - `state` needs: erofsd `2 × sessions + leases`, home / `/boot` `sessions + leases`, each the
    largest over the tree, held `>=`;
  - `in_flight` (walfsd, littlefsd, held `==`): `min(share / min(session, agent).pages, processes
    / min(session, agent).processes, weight / min(session, agent).weight) + 1`, the largest over
    the tree.
- **New bench test** `every_servers_buckets_leave_the_open_call_headroom`: for every manifest
  entry of erofsd, bootfsd, walfsd, littlefsd, keyd, consoled (with `init`'s override) and ipd
  (with its arguments' overrides), the program's own `Admission` construction succeeds, so a
  manifest whose buckets × caps pass `MAX_OPEN_CALLS − OPEN_CALL_HEADROOM` (192) fails the bench
  and never its boot (R4a).
- **The servers' `limits`** (each with the derivation in its comment and its entry test moved):

| server | term | cap before → after | bucket bytes before → after | entry (image) |
| --- | --- | --- | --- | --- |
| erofsd `state` | 2 × (1 + 3 + 1) + 18 | 11 → 28 | 226,816 → 235,520 | 1,024 holds (512 by the rule; 15 buckets, not 16) |
| walfsd `in_flight` | 95,377 / 5,248 + 1 | 6 → 19 | 626,688 → 1,486,336 | `walfsd:data` 1,280 → 2,560 (6 buckets, not 7); `alice-secrets` 1,280 → 2,048 |
| walfsd `state` | (1 + 3 + 1) + 18 | 8 → 23 | (above) | |
| littlefsd `in_flight` / `state` | as walfsd | 6 → 19 / 8 → 23 | 626,688 → 1,486,336 | 1,024 → 1,792 (4 buckets, not 5) |
| bootfsd `state` | (1 + 3 + 1) + 18 | 8 → 23 | 165,888 → 169,728 | 4,096 holds (2,816 by the rule; 40 buckets, not 41) |

  The largest terms come from the latency manifest's alice (share 95,377 pages, 48 processes,
  weight 1,000: leases 18, carves 18 + 1); the image's alice gives 7 and 8.
- **Entries by the entry rule** (`every_manifest_entry_holds_its_buckets_at_their_caps` names
  each): walfsd 4 buckets 1,280 → 1,792 (1,452 + 64 + 128 = 1,644 → 1,792; with no heap cap
  1,580 → 1,792), 5 → 2,048 (1,815 + 192 = 2,007), 6 → 2,560 (2,178 + 192 = 2,370), 9 → 3,584
  (3,266 + 192), 10 → 3,840 (3,629 + 192); littlefsd 4 buckets 1,024 → 1,792 (1,452 + 128 =
  1,580). Manifests moved (31): `image/manifest.json` (+2,048 pages), `tests/data/steward/`
  confined, latency, many-sessions, three-contexts, vault-launch (+2,048 each), idle (+1,280),
  six-principals (+3,840: 1,792 → 3,840 and 3,584), `boot-profile/manifest-unverified` (+2,048),
  `pipe/manifest` (+1,280), `beamlet/` files, launch, natives, natives-attack, pipe, serve (+512
  or +768), `beamlet-footprint`, `beamlet-compile-footprint` (+512), `aio/boot`, `verity/signed`,
  `littlefsd/` boot, confined, label-check, large-directory, one-volume (×2), `walfsd/` boot,
  confined, label-check, own, one-volume (×2), two-disks (×2) (+512 or +768 each). erofsd and
  bootfsd entries are unchanged everywhere.
- **System budgets hold.** The 1 GiB cases (`system` ≈ 64,200 pages at the kernel's split):
  the image's manifest 19,469 pages of servers (+2,048: init-boot, the steward set,
  steward-many-sessions, three-contexts, vault-launch, confined), six-principals 22,285
  (+3,840), pipe-eight 18,701 (+1,280). The 512 MiB footprint cases (`system` ≈ 31,000):
  22,411 (+512). The 256 MiB cases (`system` ≈ 15,000): the beamlet manifests 10,248 (+768),
  the walfsd/littlefsd/aio/verity ones at most 6,151 (+1,536). The latency case's `users`:
  241,605 pages on rv32 (241,707 on rv64), alice 190,757 and bob 32,769.
- **Host fixtures** give the programs the new entries: erofsd's refusal test one page short of
  345, bootfsd's of 249 (and 249 serves), walfsd's 2,560 / 64, littlefsd's and
  `userland/otp/redoubt`'s 1,792.
- **The latency case:** alice 190,756 pages, 96 processes, weight 2,000 (her share: 95,377
  pages = 11,393 + 16 × 5,249 + 0 spare; 48 processes ≥ 10 + 32; weight 1,000 ≥ 100 + 800;
  the kernel carved eight leases at weight 1,000 and refused the ninth); `round.(15, 8)`, its
  two `LATENCY-COUNT N=16 … 8` lines, the description's N list and D; `LATENCY_ROUNDS` 4 → 5
  (one constant, `servers/steward/src/bin/steward.rs`, kept alone in its hunk; STEWARD6 shares
  the file). `memory_mib` stays 1536.

## What the N = 16 runs found (rv64, both hart counts, each deterministic)

1. **The home's quota is a fourth carve** (Q7, ruled in). The first run launched 7 leases of
   the fifth round and the eighth's `Connect` at its home was refused: an unlabelled lease's
   root is `project`, minted below the sponsor's home with `sizes.agent.quota` (1 MiB) carved
   from `home_quota` (8 MiB): seven fit beside her files. The term takes
   `floor(home_quota / sizes.agent.quota)` (image alice 8 ≥ 7, bob 8 ≥ 6: no cap or entry
   moves); the latency homes are alice 18 MiB, agent 4 MiB, bob 8 MiB, 30 MiB of the data
   volume's 33,537,024 bytes. Found by elimination: the mint's error is collapsed to `Refused`,
   and `Step::Connect`'s `Shared(1)` is the binding index (the home), not the token's slot 5.
2. **The case holds two alice sessions** (Q8, pending). The rerun launched 13 and the 14th's
   `CreateBudget` was `OutOfMemory`: the UART console session (the probe reads its markers from
   the console principal's home) and the SSH session that drives the rounds hold 22,786 pages
   before any lease, so sixteen need 106,770 of the 95,377 share; the steward's `holds` lines
   show it exactly. Sizing her for both (213,542 pages, 104 processes, weight 2,200; bob
   16,384 so `users` holds 229,928 of 241,605) makes her carve term 20 and `in_flight` 21, past
   the headroom at six-principals' ten walfsd buckets (210 > 192; its nine domains need nine).
   Ruled (a): alice so sized; six-principals' `walfsd:data` at its derived nine buckets (alice's
   two domains, bob, carol, dave, erin, mallory, the sponsored agent, the steward; the tenth
   was 3Q's spare for a case's extra client, which that case has none of), 9 × 21 = 189 ≤ 192
   under the R4a bench assertion; walfsd/littlefsd `in_flight` 19 → 21, the bucket 1,486,336 →
   1,617,408 bytes, and the entries by the rule: six buckets 2,560 → 2,816 (2,370 + 192 = 2,562),
   five 2,048 → 2,304 (1,975 + 192 = 2,167), nine 3,840 (3,554 + 192 = 3,746), four 1,792
   unchanged (1,580 + 192 = 1,772; littlefsd 1,580 + 128 = 1,708); `state` and erofsd/bootsfd
   unchanged (the lease term stays 18, the quota binding). Ten manifests moved again.

3. **At one hart the round's 30th launch is refused at `erofsd:system`: parked disconnects hold
   the account's `State`.** Each five-second probe lease's end under fifteen spinners leaves its
   `disconnect` at the system volume parked after four attempts (the server starved of the
   hart: sixteen VMs of weight 50 beside it), each a `State` of alice's bucket the server has
   not released; live are 2 × 2 (two sessions) + 15 spinners + 1 probe lease = 20 of the cap's
   28, and the ninth parked disconnect refuses the next lease's connect there. A finding of
   the parked-step rule under this load (the 3W residual at scale), not a tuning: the one-hart
   N = 16 row is stated unreached with this cause. At two harts the round completes (every one
   of its 31 launches; eight disconnects parked there too).
4. **The case numbered its fifth round 16.** `round.(15, 8)` wrote `latency-round-16` and
   printed `{:round, 16, :done}`, so the probe (waiting for `latency-round-5`) never flushed
   the N = 16 counts and the session waited out the 3 h bound after the round had finished.
   The closure takes the round's number explicitly now (`round.(15, 8, 5)`).

## The N = 16 column (seed 3, run from the tip 5923f7bc8; logs `latency-rv64-*.log`)

rv64, one hart (662 s wall; every round ran; `latency-rv64-smp1-console.log`, oracle in
`latency-rv64-job.log`): N = 16, D = 8, net µs p50/p99:

| measure | N = 16, rv64, 1 hart | verdict |
| --- | --- | --- |
| steward timer wake | 54,549 / 57,268 (gross 100,266 / 116,636) | p50 and p99 missed (15,000 / 50,000) |
| steward decision wake | 7,291 / 38,682 | met |
| deadline notice (p99) | 15,675 | met |
| `budget_destroy`, call to return | 4,738 / 7,138 | no target |
| R10 (p99; 165 destructions) | 9,699 | met |
| a lease's end (worst decision wake p99 + R10) | 65,872 (N = 4's) + 9,699 = 75,571 | met |
| reply to the next receive | 51,601 / 61,578 | no target |

This run's N = 1 … 4 cells differ from the page's (one run each at the seed; STEWARD5 re-measures
the one-hart columns when it lands after this); its timer-wake p99 misses at N = 3 and 4 are the
known cleanup-turn window (51 and 76 ms). The N = 16 timer wake is missed at p50 and p99 alike:
with fifteen spinner VMs of weight 50 runnable beside it, the steward's timer wake waits behind
whole slices of the queue (the stand-in's table saw the same shape at N = 16 on one hart before
the real steward existed: a wake never preempts), and the reply-to-receive gap rises to 52 ms
with it; a finding, not a tuning. rv64, two harts: not reached. The round's 29th launch was
refused its connection at `erofsd:system`: 20 live connections of the cap's 28 (two sessions'
four, fifteen spinners', the probe lease's) and eight ended probe leases' `disconnect` there
parked after four attempts, each a `State` the server has not released (23 disconnects parked
in that run, 31 in the one-hart run, which the server took back in time). Which hart count hits
it is a load race: the earlier rv64 run refused at one hart and completed at two.

## Rebases and work outside the owned paths

- Rebased onto 9a6600ede (train 63, IPD1) and 472c5840a (train 64, SSH3), both clean: SSH3's
  `buckets=4` on sshd's entries and my pages on the file servers' are different lines.
- `servers/init/tests/manifest.rs` `servers_that_do_not_fit_in_system_are_refused` sums the
  image's entries by hand: `walfsd:data` 1,280 → 2,560 (folded into the caps commit).
- At the orchestrator's request (SSH3's review): `libs/rt/src/server/admit.rs`'s `Override` doc
  said `ipd` gives `sshd` "two calls per session"; it now says one call per slot, the slots the
  entry's `buckets=N`, the bound N + 1 held by `sshd`'s host test over every manifest. Its own
  one-line commit, the last on the branch.

## Pages

steward.md "Leases" (the bound as arithmetic with the image's numbers), serving.md `admit`
(R4a's headroom held by the bench; the per-account caps derived; status +1), walfsd.md
(`in_flight`'s form, the entry arithmetic, status +1), littlefsd.md (the full derivation and the
`state` term), erofsd.md, bootfsd.md (the term and the arithmetic), image/README.md (alice's 7
and 8, the entries), kernel/scheduling.md "With the real steward" (the N list, the sizing; the
N = 16 column after the run). Summaries checked: README.md, GETTING-STARTED.md, SECURITY.md,
docs/plan pages, testbench.md (the memory table is heap caps, unchanged; `memory_mib` unmoved),
init.md, agents.md, keyd.md, ipd.md, consoled.md: none names the lease term or an entry's pages;
no change.

## Gates (every command through `scripts/q run` / `scripts/jobs.mk`)

| gate | result |
| --- | --- |
| host tests: testbench, erofsd, walfsd, littlefsd, bootfsd, keyd, ipd, rt, fileserver, client (`host1.log`, then `test2.log` for the bench's case tests after the two fixes) | green |
| `set CASES=formatting` (`fmt1.log`) | PASS |
| `set CASES=docs` (`docs1.log`, then `gates2.log`) | FAIL on four citation forms (C5's `R7 (carving)` / `R4a (open calls)`, walfsd.md's status count), fixed in the caps commit; rerun PASS |
| 46-case set both widths: every `steward-*` boot case, agent-login, aio-many-reads-two, walfsd-quota, init-boot, beamlet-launch, beamlet-lookup-host, beamlet-footprint, pipe-carries, pipe-eight, littlefsd-boot, walfsd-boot, littlefsd-one-volume, verity-signed (`gates1.log`, `gates1.cases`) | 90 of 91 runs PASS; `beamlet-lookup-host` FAIL (`a_vm_busy_past_the_session_bound_keeps_its_console`, a console-timing host test under 45 concurrent cases; log kept as `lookup-host-fail1.log`; rerun alone in `gates2.log`: PASS) |
| size-budget, unsafe-budget (`budgets1.log`) | PASS, PASS (no `Size budget:` line moved: the servers' constants and the bench's tests change no guest binary's size) |
| steward-latency | (pending: queued behind STEWARD6's run) |
