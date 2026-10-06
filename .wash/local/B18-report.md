# B18 report: the heavy host cases fanned out, measured and bounded

Branch `wp-B18`, worktree `.worktrees/B18`, base `647f37dbe`. Head and acceptance numbers: see the
end.

## Commits (base..head)

1. `testbench: a host-tests case may run as many jobs, each on cores q leases; --exact names one case`
   (`tools/testbench/src/{fanout.rs (new), case.rs, build.rs, main.rs, elixir.rs}`)
2. `scripts: jobs.mk runs each case once and by its whole name, and gives fanned and Elixir cases their own cores`
3. `tests: rt-miri runs each file as a job of its own, with a deadline`
4. `tests: the Elixir oracle cases have a deadline`
5. `tests: the model's mutations run as one release job each, and its other tests one job per test`
   (`model/examples/mutations.rs` (new), `model/tests/mutations.rs`, `tests/model-mutations.toml` (new),
   `tests/model-host-tests.toml`)
6. `docs: host-tests fanouts, --exact, the Elixir deadline, and what each heavy host case costs`
   (`docs/testbench.md`, `docs/kernel/model.md`)

## What was delivered

- **Bench fanout** (`host-tests` field `fanout = { each, env, values, cores, vars }`), with three
  kinds of `each`: `"value"` gives one job per line a command prints, set in `env`, over the case's one test file;
  `"file"` gives one job per entry of `tests`; `"test"` gives one job per test of every built test binary (libtest
  `--list`), each run `NAME --exact`. The tests are built once (`cargo test --no-run
  --message-format=json`). Each job runs its binary directly, from its package's directory, as cargo
  would (under Miri, which cannot run a binary itself, `cargo miri test --test FILE`), through
  `scripts/q run --cores N` when `q ping` answers, else serially. `timeout_secs` is each job's
  deadline (coreutils `timeout`, inside the lease, so a wait for cores is not counted). The case log
  `<run>/<case>.log` lists every value with libtest's own time and the wall time; each job's output is in
  `<run>/<case>/<value>.log`. The verdict is the conjunction, and it names every failed value. New host-tests
  fields: `skip`, `profile`, `fanout`, `timeout_secs`. Elixir cases gained `timeout_secs` over
  their scripts together.
- **`--exact`**: runs the one case the filter names whole.
- **jobs.mk**:
  - Every target runs `--exact`.
  - Arch-less cases (47, all host kinds) run only under rv64, and their rv32 targets are a no-op.
  - New class `fanned` (2 cores for the build; each job leases its own).
  - The Elixir cases are bounded (4 cores, not 1).
  - rt-miri and host-tests left the quiet class.
- **model**:
  - The `mutations` example lists `Mutation::ALL`.
  - `REDOUBT_MODEL_MUTATIONS` takes a whole name alone.
  - `R2OneCursor` and `PolicyAgentOtherSet` now try `steward_noninterference` first. The families and caps are unchanged.

## Findings that shaped it (measured)

- **Train 4's 1,387 s quiet `rv64/host-tests`** was the substring filter: it ran every
  `*-host-tests` case, the model's included. Hence `--exact`.
- **Every arch-less case ran twice per train** (rv64 and rv32); this includes every host-tests and
  Elixir case.
- **The Elixir oracles** are almost all compile time:
  - Cold run: beamlet 16 s and the model's steward-traces example 12 s; elixirc, the BEAM and the
    oracles together take under 1 s.
  - In the train the cases ran on 1 core (boot class), at 154 s and 138 s.
  - On 4 cores, cold, both at once: 32.8 s and 34.5 s.
  - Cargo's cache already shares the builds. Nothing at run time was worth sharing, so no runtime
    sharing was added.
- **rt-miri**: the 12 files fanned out take 92 to 96 s of wall, bounded by `heap` (59 to 96 s
  depending on load). `connection` takes 20 to 25 s. Concurrent `cargo miri test` runs only
  briefly block each other on locks.
- **The model** (the real tail):
  - On one thread, `steward_policy` costs 0.6 s per seed and `steward_noninterference` 0.94 s per
    seed. Release and dev cost the same, so the cost is in the model's code, not an unoptimised
    dependency.
  - The kernel families cost under 1 ms per seed.
  - At the default counts that is 6 to 7 core-hours.
- **Mutations at 1 core each**:
  - 144 of the 147 took 0.2 to 125 s, 400 core-s in total.
  - PolicyDeclassifyUnfit is caught at steward_policy seed 4709 after 1,720 s.
  - R2OneCursor and PolicyAgentOtherSet were still uncaught after 48 min. They searched other
    families' full caps first.
  - Probes showed that kernel_sequence, budget_lifecycle, scheduler_fairness and flood all miss
    R2OneCursor. steward_noninterference catches it at seed 345, and PolicyAgentOtherSet at seed 96.

## Measured results (fanned, through jobs.mk, the worktree's)

| Case | Wall | Longest job | timeout_secs |
| --- | --- | --- | --- |
| model-mutations (147 jobs, 4 cores each, release) | 697 s (11 min 39 s) | PolicyDeclassifyUnfit 582 s; R2OneCursor 181 s | 776 per job |
| model-host-tests (69 jobs, 8 cores each, dev) | 1,997 s (33 min) | steward_policy 1,777 s; steward_noninterference 1,469 s | 2,369 per job |
| rt-miri (12 jobs) | 92 to 96 s | heap 60 to 96 s | 128 per job |
| elixir-oracles / broken-guard | 34.5 / 32.8 s cold | n/a | 46 |

Before: model-host-tests (mutations included) took over 3 h in debug and over 3 h in release;
rt-miri took 133 s on the quiet set; the Elixir cases took 154/138/50 s, twice per train.

## Gates

See the result message for the final exit codes; all were run through q or jobs.mk.

- `cargo test -p testbench`: 138 passed, rc 0.
- docs, formatting, no-cruft, size-budget and unsafe-budget, as jobs.mk targets: rc 0.
- rt-miri: rc 0 both as `cargo testbench --exact rt-miri` (92.3 s) and as `rv64/rt-miri`.
- model-mutations and model-host-tests: rc 0 as jobs.mk targets.
- Elixir cases: rc 0 as jobs.mk targets.
- Acceptance: see below.

## Acceptance against the 15-minute tail

ACCEPTANCE_PLACEHOLDER

## Documentation check

- `docs/testbench.md` was updated:
  - the usage block (`--exact`);
  - "On a shared host": deadlines and the jobs.mk classes, plus a new table of the heavy host
    cases' costs;
  - the kinds table fields;
  - a fanout paragraph;
  - the Elixir oracles section (the deadline, and why nothing more is shared);
  - rt-miri's paragraph;
  - the status lines (new host tests).
- `docs/kernel/model.md` was updated: the cost of the default run, 146 corrected to 147 variants,
  the preferred family, the name filter, and `model-mutations`.
- Checked with no change needed:
  - `README.md` and `model/README.md`: neither names the cases or the test.
  - `GETTING-STARTED.md`: its `jobs.mk` example targets still exist.
  - `CONTRIBUTING.md`: no mention.
  - `docs/kernel/invariants.md` (lines 48 and 93) names `mutations_are_caught`, which still exists,
    and its claim is unchanged.

## Design problems and open risks

- **The model's steward families are the floor.** model-host-tests needs 33 min of wall even on
  8 cores per family, and about 15 min would need about 16 cores each. A separate model package
  could make a steward seed cheaper; lowering the seed counts would be a coverage decision for the
  owner or the Architect. This was asked as a question to the orchestrator, recommending (a); no
  answer had arrived when this report was written.
- **Deadlines are measured on a mostly idle machine.**
  - The Elixir deadline includes cargo builds, which can wait on another build's lock in a busy
    train.
  - A host-tests job's deadline includes the job's `cargo miri` lock wait (rt-miri only).
  - The docs say that a deadline expiry beside other work is rerun alone, as for a boot.
- **rt-miri is no longer quiet.** That rests on the files asserting only hang guards: a 60 s guard
  in parked_write, and polls of 20,000 × 1 ms in parked. Under Miri on a loaded core these are
  slower but generous.
- **The fanout's `"test"` mode runs test binaries directly, so doctests are not run.** The model
  has no doctests (checked: no rust code blocks in model/src docs).
- **A mutation job holds 4 cores even for a 0.2 s job.** q's fairness then allows 2 such jobs at
  a time while another tenant waits.

## Next step

The orchestrator's answer on the model's steward cost; review.
