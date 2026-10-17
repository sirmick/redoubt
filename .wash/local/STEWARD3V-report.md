# STEWARD3V report (steward3v-implementer, 2026-10-16)

Branch `wp-STEWARD3V` on main c63bbacbf; worktree `/home/mcloonan/redoubt/.worktrees/STEWARD3V`.
Logs under `/home/mcloonan/redoubt/.tmp/STEWARD3V/`.

## What was delivered

- **The rule in code.** `libs/rt/src/server/admit.rs`: `Limits::need` and `Admission::need`
  (every bucket at its caps, in bytes; `fits` is `need <= budget`), `LAUNCHED_STACK_PAGES` (128,
  the stub's `MAX_STACK_PAGES`), `budget_of(pages, heap_pages)` and `budget(&Startup)`: the entry's
  pages less its heap cap and the stack's bound, times the page; `None` when the block names no
  budget. Host test `a_servers_budget_is_its_entrys_pages_less_its_heap_cap_and_stack`.
- **The six servers derive their budget from the block, no compiled `BUDGET`:** `erofsd`,
  `walfsd`, `littlefsd` through `redoubt_fileserver::program::start` (its `fits` takes the budget);
  `bootfsd` and `keyd` in their programs (`BAD_LIMITS` on no budget); `ipd` through
  `Config::sizing(budget)` (`OWN_USE` gone: the heap cap holds the stack's tables and the sockets'
  buffers; `Sizing.need` carries the arithmetic). Each server's `server.rs` states the entry
  rule with the image's arithmetic in place of its constant.
- **Per-server host tests** (`the_image_entry_holds_its_buckets`, littlefsd's
  `the_manifests_entry_holds_its_buckets`, keyd's renamed from
  `the_limits_are_sized_as_containment_says`, ipd's `every_scope_and_the_milestone_fit` extended):
  the bucket's bytes, the image's count's need, the entry's budget, how many such buckets it
  holds. Program tests write the image's `budget_pages`/`heap_pages` into their blocks as `init`
  does; erofsd's, bootfsd's and keyd's refusal tests add a block naming no budget and one a page
  short of the count's need (and bootfsd's: one page more serves).
- **The bench host test** `every_manifest_entry_holds_its_buckets_at_their_caps` (tools/testbench,
  beside 3Q's `the_servers_admit_what_a_manifest_lets_one_account_hold`): every manifest under
  `image/` and `tests/data/`, every entry of the six programs with a `buckets=N`: pages >=
  ceil(need / 4096) + heap cap + 128. It found five entries short: `tests/data/erofsd/corrupt.json`
  (four erofsd volumes) and `read-only.json`, 256 pages with 4 buckets (222 + 128 = 350): raised to
  512 by the rule.
- **The six-principal manifest** `tests/data/steward/six-principals.manifest.json` (STEWARD4's
  dropped shape: alice labelled, bob, carol, dave, erin, mallory, agent sponsored; keys carol,
  dave, erin added to `tests/keys`, a staged home each; buckets keyd 10, consoled 9, bootfsd 10,
  ipd 11, walfsd:data 10, walfsd:alice-secrets 9, erofsd:system 10; walfsd's entries 1,792 by the
  rule, the rest keep the image's pages, which hold) and `tests/steward-six-principals.toml`: the
  boot, alice's console session, erin (the sixth) logs in over ssh, evaluates and exits 0.

## The arithmetic (image/manifest.json), stack at its bound 128

| server | bucket (B) | count | need (B / pages) | heap | rule gives | entry | budget left (pages) | holds |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| erofsd:system | 226,816 | 6 | 1,360,896 / 333 | 26 | 512 | 1,024 | 870 | 15, not 16 |
| bootfsd | 165,888 | 6 | 995,328 / 243 | 2,304 | 2,816 | 4,096 | 1,664 | 41, not 42 |
| walfsd:data | 626,688 | 6 | 3,760,128 / 918 | 64 | 1,280 | 1,280 | 1,088 | 7, not 8 |
| walfsd:alice-secrets | 626,688 | 5 | 3,133,440 / 765 | 64 | 1,024 | 1,280 | 1,088 | 7 |
| keyd | 4,096 | 6 | 24,576 / 6 | 8 | 256 | 256 | 120 | > MAX_BUCKETS |
| ipd (seven defaults; the image names no overrides) | 712,704 | 7 | 4,988,928 / 1,218 | 74 | 1,536 | 4,096 | 3,894 | 21, not 22 |
| littlefsd (aio and beamlet manifests) | 626,688 | 4 | 2,506,752 / 612 | none | 768 | 1,024 | 896 | 5, not 6 |

Six principals: erofsd 10 → 554 + 26 + 128 = 708 → 768 (keeps 1,024); bootfsd 10 → 405 + 2,304 +
128 → 3,072 (keeps 4,096); walfsd 10 → 1,530 + 192 = 1,722 → 1,792; walfsd 9 → 1,377 + 192 → 1,792;
keyd 10 → 10 + 136 → 256; ipd 11 → 1,914 + 202 → 2,304 (keeps 4,096); consoled 9 under 3Z's
constant (holds init's and 10 on rv64, 11 on rv32).

## Design questions (thread STEWARD3V-budgets, architect-2)

Q1 the stack: the startup block carries no stack and a program cannot read its own; built on (a),
the server subtracts the launched bound (128) and the rule names it; (b) a wire field was the
alternative. Q2 an uncapped heap counts 0. Q3 consoled keeps its constant. Unanswered at this
writing; swapped if ruled otherwise.

## Pages

serving.md (`admit`: the rule, status 15), walfsd.md (the entry rule in derivation form, the
image's arithmetic, status 37), erofsd.md, bootfsd.md, littlefsd.md, keyd.md, ipd.md (each
admission paragraph restated with the image's arithmetic; the renamed/added tests), init.md
(Heaps: the entry rule beside the memory rule), consoled.md (its entry-rule sentence with the
constant as the budget and the stack's bound). Summaries checked: README.md, GETTING-STARTED.md,
docs/plan pages, testbench.md, SECURITY.md, image/README.md, steward.md: no claim names a server's
budget constant; no change.

## Gates (every command through `scripts/q run` / `scripts/jobs.mk`; logs in `.tmp/STEWARD3V/`)

Head **da8babfbc** on main c63bbacbf; `git status --short | wc -l` = 0; 63 files (51 changed,
12 new), +532 −201 among the tracked ones.

| gate | result |
| --- | --- |
| host tests: rt, fileserver, erofsd, bootfsd, walfsd, littlefsd, keyd, ipd, client (`host-final.log`, then ipd's sizing rerun after the ipd figure fix: 2 passed) | 450 passed, 0 failed |
| testbench `the_servers_admit_what_a_manifest_lets_one_account_hold`, `every_manifest_entry_holds_its_buckets_at_their_caps` | both pass |
| guest builds, 3Z's 26 rt-linking packages, rv64 and rv32 (`guest-*.log`) | rc 0 both, no non-vendor warnings |
| `set CASES=` steward-ssh-two-principals steward-context-cap steward-lease-ends steward-three-contexts steward-many-sessions steward-six-principals walfsd-quota aio-many-reads-two init-boot beamlet-footprint beamlet-launch beamlet-lookup-host beamlet-natives-attack beamlet-natives beamlet-serve job-interrupt-line job-interrupt-native job-interrupt-ssh job-kill pipe-carries pipe-eight pipe-hostile-output pipe-interrupted pipe-never-reads pipe-no-authority pipe-sixty-four pipe-stage-handle piped-build piped-host-tests, both widths (`gates1.log`) | 55 PASS; beamlet-lookup-host FAIL (real: userland/otp/redoubt's fixture wrote no budget; fixed, PASS alone 65 s, `lookup-host-rerun2.log`) |
| steward-six-principals rv64 alone (`six4.log`, console `six4-console.log`, erin's ssh log in the run dir) | PASS: every server starts at its count, alice's console session, erin logs in, evaluates, exits 0 |
| docs (`make -f scripts/jobs.mk set CASES=docs`) | PASS (before and after the commit) |
| fmt `cargo +nightly fmt --all --check` | clean |
| size-budget | PASS against the commit: libs/rt 4048 → 4059, libs/fileserver 573 → 574, each with its `Size budget:` line; rerun on da8babfbc at the reviewer's note, `size-budget-final.log` (rv64; the target runs under rv64 only, jobs.mk) |
| unsafe-budget | PASS |

Not run: the whole bench (the train's); steward-latency (waits for STEWARD4: its case and probe).

## For STEWARD4 (through the orchestrator)

The latency manifest's entries under six principals (keyd 10, consoled 9, bootfsd 10, ipd 11,
walfsd:data 10, walfsd:alice-secrets 9, erofsd:system 10): walfsd's two entries 1,792 pages; every
other entry keeps the image's pages (erofsd 768 by the rule ≤ 1,024; bootfsd 3,072 ≤ 4,096; keyd
256; ipd 2,304 ≤ 4,096; consoled under 3Z's constant). The manifest's `public` must list only the
programs the case ships (`crossing` is STEWARD4's). My six-principal manifest is
`tests/data/steward/six-principals.manifest.json`, built from STEWARD4's dropped shape with alice
81,920 pages / 60 processes / contexts 3 and the five others 16,384 / 20 / contexts 1 (one
session's share), so the users budget of a 1 GiB machine holds them.

## Residuals and notes

- The server's check counts the stack at 128 pages, conservative by 128 less the entry's stack
  (walfsd.md says so); a wire field would make it exact (thread STEWARD3V-budgets Q1 (b), not
  taken; provisional ruling from the orchestrator, the Architect's confirmation pending).
- consoled's budget stays 3Z's constant (its entry-rule sentence counts the stack's bound).
- Cargo.lock: the testbench's two new dependencies (keyd, ipd).
- Fixtures that run a shared server's program on the host now write a budget: the servers' own
  tests, libs/client's, userland/otp/redoubt's. A fixture that forgets one hangs its test at the
  fake kernel's stuck-wait (60 s), which is how beamlet-lookup-host found the last one.
