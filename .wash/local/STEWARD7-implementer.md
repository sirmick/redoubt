# STEWARD7: a principal's leases are derived from the manifest; steward-latency at N = 16

Node STEWARD7 (parent AGENTS). Tier A (admission caps in every shared server; the bench's
derivation). Size S–M. Reviewer: steward-red-4. Ruling: `.wash/local/wave-2026-10-18-rulings.md`
"F2" (thread STEWARD3V-budgets Q4); the rules it builds on: `STEWARD3Z-ruling.md` Q2 (the
derivation form), `STEWARD3V-report.md` (the entry rule and its arithmetic), steward.md R82.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. A `steward-latency` run with an N = 16 round may pass 1.5 h: launch it
  detached (`setsid nohup`) and watch its log; copy the job log aside before any rerun.
- Messages at most 2000 bytes; detail in a file. Read reports by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic; an admission constant
  is derived from the manifest, stated as the derivation, held by a bench host test (3Q). A
  miss is a finding, not a tuning; no target moves. Not a compiled constant raised to a case's
  N; not a new manifest field.

## The symptom

`tools/testbench/src/case.rs` `the_servers_admit_what_a_manifest_lets_one_account_hold`:
`const LEASES: u64 = 1; // no manifest names it`. The servers' `limits` carry the same term:
erofsd `state` 11 = 2 per session × (console + 3 contexts + spare) + 1 lease; walfsd/bootfsd 8 =
5 + 1 + 2. One session with N leases holds 2 + N at `erofsd:system` and 1 + N at its home and
`/boot`, so N <= 7 is admitted and N = 16 is refused its eighth home connection (`a step failed
(Refused)`), whatever the entries' pages (STEWARD3V part 2; scheduling.md's N = 16 sentence).

## The rule

A principal's live leases are bounded by what its unlabelled share holds in `agent` budgets,
pages and processes both, the smaller:

    leases(p) = min( floor(share_pages / sizes.agent.pages),
                     floor(share_processes / sizes.agent.processes) )

with `share` the principal's unlabelled sub-budget as `init`'s `holds` derives it
(`servers/init/src/check.rs`: an equal part per label set less a budget's own cost), and for a
sponsored principal its sponsor's. It is an upper bound: sessions carved from the same share
only lower what is left for leases, and a sponsored principal's sessions are leases of that
share. The kernel's carve (R7) refuses past it, so the steward gains no rule, the core no row,
the manifest no field. Image: alice (81,920 pages, two label sets) 40,959 / 5,248 → 7, processes
32 / 2 → 16: 7; bob (no label set) 32,767 / 5,248 → 6, 20 / 2 → 10: 6.

The per-account caps read it: erofsd `state` >= 2 × sessions + leases; walfsd, bootfsd,
littlefsd >= sessions + leases (+ the spare 3Q gives), the largest over every manifest in the
tree, as the test already takes the largest sessions term. The servers' `limits` carry the
term the test derives (3Q's shape: the constant and its derivation in a comment beside it,
the test holding them equal or above); where a bucket grows, the entry's pages follow
STEWARD3V's rule (`every_manifest_entry_holds_its_buckets_at_their_caps` says which).

## The latency case

N = 16 is one principal's session with 16 live leases. By the rule alice's unlabelled share
must hold 16 × 5,248 + her console session's 11,392 = 95,360 pages; with her two label sets
her budget is about 190,722 pages (745 MiB) and `processes` at least 2 × (16 × 2 + 10) = 84.
Do the memory rule's arithmetic on `tests/steward-latency.toml`: the users budget beside the
five other principals (16,384 each) and the system servers under `memory_mib` 1536; if it fits,
size her so, add `round.(15, 8)` (D = 8 at N = 16, as STEWARD4's Q3 set it), its `LATENCY-COUNT`
lines and the description, and recompute the manifest's entries by 3V's rule (the servers'
buckets at the larger caps: show each). If it does not fit, do not raise `memory_mib` yourself:
record the largest N the rule and the machine give, with the arithmetic, as the round you add,
and say so in the report; the owner's question is already filed.

## Tier A invariants the reviewer checks

- Every per-account cap is at least the derived need for every manifest in the tree, held by
  the bench host test; no server's `limits` carry a number the derivation does not produce.
- Nothing admits more than the manifest lets one account hold: the term is an upper bound of
  live leases by the kernel's own carve, stated as such.
- The steward's own share still holds only its own connections (3Z); no bucket per principal
  appears anywhere.
- No target or definition moves; the N = 16 column is measured or stated unreached with its
  arithmetic.

## Cases

- Host: the bench test with the derived term (largest over manifests), the servers' per-entry
  tests at the new caps, `every_manifest_entry_holds_its_buckets_at_their_caps` on every
  manifest including the resized latency one.
- Bench: `steward-latency` (seed 3, by name, both widths, one hart; two harts on rv64) with the
  new round: every `LATENCY-COUNT N=16` line, the table's column; the steward set,
  `steward-many-sessions`, `steward-six-principals`, `aio-many-reads-two`, `walfsd-quota`,
  `init-boot`, both widths, unchanged in verdict.

## Pages

- steward.md "Leases": the bound as arithmetic (one sentence and the image's numbers).
- serving.md "`admit`" and walfsd.md's derivation: the lease term's new form; erofsd.md,
  bootfsd.md, littlefsd.md, keyd.md, ipd.md where their admission paragraph names the term.
- scheduling.md "With the real steward": the N = 16 column measured, or the sentence replaced
  by the largest N the manifest gives with its arithmetic; the paragraph's N list.
- testbench.md if the latency manifest's memory moves. Summaries checked: say which.

## Owned paths

`tools/testbench/src/case.rs` (the test), each shared server's `limits` and its tests,
`tests/data/steward/latency.manifest.json`, `tests/steward-latency.toml` (the round, the counts,
the description, the memory comment), the pages above. Hotspot: scheduling.md's table and the
latency toml are also STEWARD5's and STEWARD8's; the last to land re-measures; tell the
orchestrator which cells you changed.

## Gates

Host tests (`cargo test -p testbench`, the six servers, `redoubt-steward-server`); guest builds
of every rt-linking package both widths; the bench cases above both widths; `set
CASES=formatting`, `set CASES=docs`, `size-budget`, `unsafe-budget`. Report each with its exit
code, the derivation table (server, term, cap before and after, entry pages), and the memory
arithmetic for the latency manifest.

## Checkpoint

After the host tests are green and before the latency run: one progress note with the
derivation table and whether N = 16 fits the machine.
