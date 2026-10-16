# STEWARD3Q report: walfsd's and littlefsd's per-account admission follows the manifest

Base: main 6713d6ffa (rebased from c1eb9aa3c, no conflicts). Head: 794449d21 (one commit, tree clean); both review rounds folded in (e7a5e9060 was the first renewal). Branch wp-STEWARD3Q, worktree .worktrees/STEWARD3Q.
Gate logs under .worktrees/STEWARD3Q/.tmp/STEWARD3Q/.

## The derivation

A bucket is keyed by (account, label set) (serving.md "Buckets", R26: unchanged). So a bucket's
`in_flight`, one completion call per multiplexed session, must hold what a manifest lets one
account have open at once in one label set:

    console session (1) + cap of live contexts per label set + leases per principal (1) + 1 spare
    (a context detaching while another attaches: its connection lives until the relay ends)

The contexts cap is the manifest's `contexts.max`, or what a label set's share holds as init
derives it (servers/init/src/check.rs `holds`: the equal share less a budget's own cost, over a
session's pages; and processes likewise; the smaller, at most 16).

- image/manifest.json: alice 47,624 / 2 sets - 1 = 23,811 pages / 11,392 = 2; processes 20 / 10 =
  2; bob and agent 2 as well (image/README.md "two sessions"; steward-context-cap's description).
  The console session counts inside alice's unlabelled set's cap. Sum: 1 + 2 + 1 + 1 = **5**.
  (The ruling counted the cap as 3 and the image's sum as 6; the brief's "6 for the image" does
  not hold: see the question sent to the orchestrator, message a6285d76.)
- tests/data/steward/three-contexts.manifest.json (new, the new case's): the image's manifest
  with alice's budget 68,360 pages, 60 processes, so one set's share holds three sessions
  (34,179 / 11,392 = 3; 30 / 10 = 3): her cap is 3, sum 1 + 3 + 1 + 1 = **6**.
- The constant is the largest sum over the tree's manifests: **6**, held by
  tools/testbench `case::tests::the_servers_admit_what_a_manifest_lets_one_account_hold`, which
  walks image/manifest.json and every tests/data/**/*.json with a steward and principals,
  replicates init's derivation, and asserts both servers' `IN_FLIGHT` equal the largest sum
  (exactly: more is a guess, fewer refuses a context its home). Leases per principal is named
  there as a constant 1 (no manifest field or steward constant exists yet; MAX_LEASE is a
  duration).

Open-call headroom: 6 buckets x 6 = 36 of 192: fine (Admission::new's `Unsized` check).

## The bucket cost

Per bucket at its caps, COST unchanged: 6 x 65,536 (completion calls) + 32 x 2,048 (fids) +
8 x 512 (connections) + 128 x 256 (requests) + 32 x 4,096 (pages) = **626,688 B**
(was 364,544 with 2 calls: +262,144 a bucket, as the finding computed).

- walfsd: the image's `walfsd:data` has 6 buckets: 3,760,128 B. BUDGET 3 MiB -> **4 MiB**
  (4,194,304): holds 6, not 7 (4,386,816). Host test `the_limits_fit_the_budget` (walfsd):
  limits(6) fits, limits(7) does not, and the bucket is 626,688.
  The entry: BUDGET is 1,024 pages, beside 64 of heap and 14 of stack, so the two walfsd entries'
  `budget.pages` 1,024 -> **1,280** in image/manifest.json and the manifests derived from it
  (tests/data/steward/idle, vault-launch, three-contexts; tests/data/boot-profile/
  manifest-unverified.json), and after the review (P2: one rule) in tests/data/pipe/manifest.json
  and tests/data/walfsd/{boot,confined,label-check,one-volume,own}.json too. The rule, stated once
  on walfsd.md "Serving" (Admission): an entry's pages are what the server may hold at its limits,
  BUDGET + heap cap + stack rounded up to 256 pages, 1,280 for every walfsd entry whatever its
  bucket count; a case's manifest sizes by the rule, never by use. docs/kernel/budgets.md: the
  image's servers need 17,421 of system's pages (was 16,909: +512).
- littlefsd: the manifests' most is 4 buckets: 2,506,752 B > 2 MiB. BUDGET 2 MiB -> **3 MiB**
  (3,145,728): holds 5 (3,133,440), not 6. Host test: limits(5) fits, limits(6) does not. Its
  entries' 1,024 pages hold the 768 beside heap and stack as walfsd's did; unchanged.
- Heap caps (`heap_pages`): unchanged. A completion call's cost is its caller's lend, charged to
  the server's budget (kernel/ipc.md R3), not its heap; the per-session records are within the
  unchanged `requests`/`state` caps. The memory-scan cases were not among this package's gates.

## The negative (the latent defect, shown once)

walfsd's IN_FLIGHT set to 2 (the old constant; littlefsd untouched), prebuilt, steward-three-
contexts rv64 once (2026-10-15, before the session-level forbid was added): FAIL at the deadline,
"session alice.a: step 7: timed out waiting for mark b-up". The third context's own log,
.tmp/STEWARD3Q/negative/steward-three-contexts-rv64-smp1-alice.b.ssh.log: line 3
`** no home to start in (:eio); this session starts in /`, line 8 `{:b, {:error, :eio}}`; the
console's session and alice.a (…-alice.a.ssh.log: `{:a, :ok}`, `{:ar, {:ok, "second"}}`) held
the bucket's two calls. Console log: …/negative/steward-three-contexts-rv64-smp1.log; the bench
line in …/negative/rv64-steward-three-contexts.log. The case now forbids `:eio` in alice.b's
session, so a regression fails at the banner, not at the deadline. Constant restored to 6 before
the gates below.

## Gates (all under .worktrees/STEWARD3Q/.tmp/STEWARD3Q/, 2026-10-16, after the final edit)

- walfsd + littlefsd host tests: `q run -- cargo test -p redoubt-walfsd -p redoubt-littlefsd`,
  exit 0: 54 + 8 (walfsd) and 32 + 4 (littlefsd) passed, 0 failed, the new
  `the_limits_fit_the_budget` in each: host-servers.log.
- The bench host test: `cargo test --manifest-path tools/testbench/Cargo.toml
  the_servers_admit_what_a_manifest_lets_one_account_hold`, exit 0, 1 passed: host-testbench.log.
- formatting: PASS (formatting.log). docs: PASS (docs.log).
- prebuilt both widths: rv64 and rv32, 313 cases, 0 failed (run2/prebuilt.log).
- `make -k -j4 -f scripts/jobs.mk set WIDTHS=rv64,rv32`, exit 0 (run2/set.log, per-case logs
  run2/<w>-<case>.log): size-budget PASS; and on both widths PASS: steward-three-contexts
  (10.2 / 9.3 s), steward-home-quota (189.5 / 171.9 s), walfsd-quota, walfsd-boot, image-disk,
  steward-context-cap, steward-context-idle, steward-context-labels, steward-context-login.
- steward-three-contexts x3 both widths: run2, run3, run4 (set.log in each): 6 PASS, 9.3-10.2 s.
- An earlier set (run1/, before the size-budget fix) passed the same rv64 cases and failed
  size-budget on littlefsd 1,143 of 1,142 lines (the removed constant); make then stopped
  before rv32, hence run2.

Review round (steward-red-3 OK with notes), head e7a5e9060 on main 6713d6ffa, logs run5/:
docs PASS, formatting PASS, prebuilt rv64+rv32 (313, 0 failed), size-budget PASS; both widths
PASS: steward-three-contexts (10.0 / 9.4 s), init-boot (6.8 s, P3 added to the gates), image-disk,
steward-home-quota (189.0 / 173.4 s), walfsd-boot, walfsd-quota, walfsd-confined-labelled,
walfsd-label-check, walfsd-one-volume (their manifests moved to 1,280), pipe-eight (the pipe
manifest; run5/pipe.log).


Second renewal: the last three walfsd entries moved to 1,280 (tests/data/beamlet-footprint/
manifest.json, tests/data/beamlet-compile-footprint/manifest.json, tests/data/beamlet/files.json).
Every walfsd entry in the tree, JSON-parsed (image/manifest.json and tests/data/**/*.json; the
listing is .worktrees/STEWARD3Q/.tmp/STEWARD3Q/walfsd-entries.txt): 21 entries in 13 files, all
`"pages": "1280"`, 0 below 1,280. Cases rerun on both widths after a fresh prebuilt (run6/, 313/0): PASS beamlet-footprint
(5.2 / 5.0 s), beamlet-compile-footprint (6.6 / 6.2 s), beamlet-files (1.8 / 1.8 s). The scan
lines (run6/<w>-beamlet-footprint.log, <w>-beamlet-compile-footprint.log), beamlet's unchanged by
the entry: rv64 footprint `heap beamlet 8804 of 11220 pages`, rv32 `5357 of 11220`; compile
rv64 `9135 of 11220`, rv32 `5542 of 11220`; stacks `stack beamlet 25832 / 24568 / 34280 / 34456
of 18 pages`, threads 1-4 as before; walfsd:data `heap 8 (9 on compile) of 64 pages`, `stack
15016 / 15768 of 14 pages`.

- Not run: the memory-scan cases (steward-session-ends, steward-ssh-two-principals), the whole
  bench, rv32 beyond prebuilt and the cases above; STEWARD3A's lease cases (its branch).

## Summaries checked

- README.md, GETTING-STARTED.md, docs/README.md, docs/TOUR.md, docs/SECURITY.md, docs/plan/m1..m3,
  docs/servers/init.md, docs/userland/sessions.md, agents.md: grep for in_flight, completion
  calls, multiplexed connections, per bucket, the contexts cap, two sessions: no claim about the
  servers' count of completion calls or the walfsd entry's pages; the cap claims (SECURITY.md R82
  row, m2-usable-shell.md) are about the steward's cap and stay true. No change.
- docs/servers/serving.md "Buckets", R26: the keying by account stays; the page states no count.
  No change.
- image/README.md: updated (the cap per principal, the sum 5 for the image and the constant 6,
  the walfsd entries' 1,280 pages).

## Pages and files changed

- servers/walfsd/src/server.rs, servers/littlefsd/src/server.rs: `in_flight: 6` with the
  derivation on `limits`'s doc comment; BUDGET 4 MiB / 3 MiB with the arithmetic. (A first cut
  named the count `IN_FLIGHT`: one code line over littlefsd's size ceiling, 1,143 of 1,142, so the
  tests read `limits(1).in_flight` instead and no line is added.)
- servers/walfsd/src/server_tests.rs, servers/littlefsd/src/server_tests.rs:
  `the_limits_fit_the_budget`.
- tools/testbench/Cargo.toml (dev-dependency redoubt-walfsd), tools/testbench/src/case.rs (the
  manifest test); Cargo.lock.
- image/manifest.json, tests/data/steward/idle.manifest.json, vault-launch.manifest.json,
  tests/data/boot-profile/manifest-unverified.json: walfsd entries 1,280 pages.
- tests/data/steward/three-contexts.manifest.json, tests/steward-three-contexts.toml: new.
- docs/servers/littlefsd.md (Admission line: the derivation and the arithmetic; status list +2),
  docs/servers/walfsd.md (Admission line: the budget arithmetic; Serving status list +3),
  docs/servers/steward.md (Contexts status line + the case), image/README.md.
- No steward code change.
