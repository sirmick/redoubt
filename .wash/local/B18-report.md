# B18 report: the heavy host cases fanned out, measured and bounded

Branch `wp-B18`, worktree `.worktrees/B18`, base `647f37dbe`, head `9e596306d` (nine commits,
clean tree, not pushed). The second round follows the orchestrator's answer: (a), plus splitting
the steward families out and bounding each mutation (the last section).

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

All were run through q or jobs.mk.

- `cargo test -p testbench`: 138 passed, rc 0.
- docs, formatting, no-cruft, size-budget and unsafe-budget, as jobs.mk targets: rc 0.
- rt-miri: rc 0 both as `cargo testbench --exact rt-miri` (92.3 s) and as `rv64/rt-miri`.
- model-mutations and model-host-tests: rc 0 as jobs.mk targets, twice each (alone and in the
  acceptance run). jobs.mk runs `cargo testbench --exact <case>` under `q run`. They were not also
  run outside jobs.mk: each is 12 to 33 min, and the command is the same.
- Elixir cases: rc 0 as jobs.mk targets.
- Acceptance: see below.

## Acceptance against the 15-minute tail

Not met. The model's steward families set a floor the fanout cannot get under; every other part of the tail is now about 2 min.

Command (16:15:21 to 16:57:57, `real 42m36s`):

    make -k -f .worktrees/B18/scripts/jobs.mk -C .worktrees/B18 quiet-rv64 quiet-rv32 rv64/model-mutations \
      rv64/model-host-tests rv64/rt-miri rv64/elixir-oracles rv64/bench-elixir-oracles-broken-guard \
      rv64/host-tests rv64/steward-host-tests rv64/memory-host-tests rv64/littlefs-host-tests

The machine was shared with other tenants throughout. `q log` (end times, ran):

| case | ended | ran |
| --- | --- | --- |
| rv32/bench-ssh-guest | 16:15:21 | 0.2 s |
| rv64/client-host-tests | 16:15:24 | 2.8 s (FAIL, see below) |
| rv64/rt-host-tests | 16:15:35 | 11.1 s |
| rv64/r4-host-tests | 16:15:37 | 2.1 s |
| rv64/bench-ssh-loopback-deadlock | 16:15:42 | 5.1 s (FAIL, see below) |
| rv64/bench-ssh-guest | 16:15:55 | 12.8 s |
| rv64/bench-elixir-oracles-broken-guard | 16:16:24 | 5.2 s (warm cache) |
| rv64/elixir-oracles | 16:16:29 | 4.8 s (warm cache) |
| rv64/steward-host-tests | 16:16:31 | 2.1 s |
| rv64/littlefs-host-tests | 16:17:00 | 23.2 s |
| rv64/host-tests | 16:17:00 | 35.4 s (was 1,387 s on the quiet set) |
| rv64/memory-host-tests | 16:17:04 | 32.8 s |
| rv64/rt-miri | 16:17:26 | 74.5 s |
| rv64/model-mutations | 16:30:35 | 849.9 s (697 s when its jobs had the machine to themselves) |
| rv64/model-host-tests | 16:57:57 | 2,505.1 s |

- Without the two model cases, the tail ends at 16:17:26: 2 min 5 s.
- With model-mutations it is 15 min 14 s.
- model-host-tests sets the whole: its steward_policy job waited for an 8-core lease behind the
  mutation jobs, then ran about 30 min.
- The two quiet failures were each rerun alone on the quiet set:
  - bench-ssh-loopback-deadlock: PASS, 0.2 s. The first run said the bench could not read its
    sshd.log.
  - client-host-tests: it fails about 1 run in 8 even alone (7 passes, then a FAIL in a loop of
    `q run --quiet -- cargo testbench --exact -v client-host-tests`). The failing test is
    `libs/client/tests/aio.rs` `a_server_that_breaks_its_hold_loses_the_session_at_the_margin`,
    which panics at aio.rs:493 and :505.
  - `cargo test -p redoubt-client` alone passed 6 runs of 6.
  - The branch does not touch `libs/client` (no diff from base), so this flake predates B18 and
    belongs to the client's aio work.

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

## Second round: the split and the mutation bound

Commits 7–9:

- `b333260c1 testbench: a host-tests case may name the tests it runs`. This adds the `filter`
  field: libtest name filters, placed before the `--skip`s. A fanout of each test makes jobs only of the
  tests that `filter` takes.
- `e2bd17f10 tests: the model's steward families get a case of their own, and a mutation they catch late fails`.
- `9e596306d docs: the steward families' case and cost, the mutations' cap, and a host-tests filter`.

**The split**

- `model-host-tests` runs every model test except `mutations_are_caught` and the two steward
  families. It is one cargo test in the bounded class (4 cores): PASS in 139.4 s, the build
  included, so it has no deadline.
- `steward-model-host-tests` is new and runs only `properties::steward_policy` and
  `properties::steward_noninterference` (`tests = ["properties"]`, `filter`).
  - It is a fanout of each test, 8 cores and `MODEL_THREADS=8` per job, so it is in the fanned
    class, not the bounded one: a non-fanned case cannot set the model's thread count.
  - PASS: 1,473 s and 1,435 s of run time. Its wall was 4,200 s because the 8-core leases queued
    on a busy machine.
  - `timeout_secs = 2369` per job (1,777 s measured earlier, and a third).
- On the page: the steward families' per-seed cost is stated as the steward model's own, and a
  residual risk names the follow-up (making a seed cheaper, and whether PolicyDeclassifyUnfit's
  catch depth is a coverage weakness). The page carries no package ID.

**The mutation bound**

- `STEWARD_CAP = 500` seeds per steward family, per mutation. The kernel families keep 20,000,
  because their seeds cost under 1 ms.
- The bound is in seeds, not seconds: deterministic, and stated on the model page.
- A mutation not caught within the caps fails the test by name. A lower cap can only fail more
  mutations, never hide one.
- Run through jobs.mk: FAIL, `1 of 147 jobs failed: PolicyDeclassifyUnfit`. The other 146 passed.
  - The longest job is now that failing search, 200 s on 4 threads.
  - Next were R2OneCursor at 164 s and PolicyAgentOtherSet at 64 s.
  - Wall was 1,018 s on a busy machine; about 12 min with the machine to itself.
  - `timeout_secs = 267` per job.
- **model-mutations now fails on this branch, by design, until the steward model's follow-up.**
  Merging it turns the case red in every train. You might want to hold the merge, or mark the
  case `whole_run = false`, until the follow-up lands; that is your decision.

**The three slow mutations**

All times are with the old family order and one core, from the first measurement.

| Mutation | Caught by | Seed | Time |
| --- | --- | --- | --- |
| PolicyDeclassifyUnfit | steward_policy | 4709 | 1,720 s on 1 core (582 s on 4 threads) |
| R2OneCursor | steward_noninterference | 345 | over 48 min on 1 core before the reorder; 164–181 s on 4 threads after |
| PolicyAgentOtherSet | steward_noninterference | 96 | over 48 min on 1 core before the reorder; 57–68 s on 4 threads after |

- The next slowest at one core were PolicyEndLeaseAdmitted (steward_policy seed 224, 125 s) and
  PolicyDeclassifyLive (seed 153, 62 s).
- The kernel families catch as late as seed 8187 (R4OverdrawOnDelivery) but take only 8.6 s.
- The steward families' per-seed cost is 0.6 s (steward_policy) and 0.94 s
  (steward_noninterference) on one thread, in release and dev alike.

**Tail with the split** (from the measurements above)

- Everything except the model's mutation and steward cases ends in about 2 min 5 s.
- model-host-tests takes 139 s.
- model-mutations takes about 12 min with the machine to itself, and fails on PolicyDeclassifyUnfit.
- steward-model-host-tests takes about 25–30 min of run time with its own 8-core leases; it is
  the one case past 15 min.

**Gates at the new head (9e596306d), all rc 0:** cargo test -p testbench (138 passed); docs; formatting; no-cruft; size-budget; unsafe-budget; model-host-tests and steward-model-host-tests as jobs.mk targets. model-mutations rc 1 by design (PolicyDeclassifyUnfit caught too late).

## Third round: rebase onto main, one --exact, the steward case by name only

- **Rebase:** onto main `ec902d464`, which includes B19 (`fdafcf2cb`), the plan commit
  (`f7ce1e9b6`), BEAM8 and BEAM3.
- **`--exact`:** B19's survived (`Args::exact` plus `selected()` in main.rs, and its usage line in
  testbench.md). Mine was dropped from the bench commit: it had the same meaning.
- **jobs.mk** has one shape: B19's recipe (target/prebuilt when it is there, else cargo testbench,
  always `--exact`) plus my fanned and Elixir classes and the arch-less cases run once.
- **The steward case is renamed** `steward-model-host-tests`, as asked, with `whole_run = false`.
  The bench page says it runs only by name until a cheaper steward seed brings it under 10 min.
- **New commit** `scripts: jobs.mk's train targets leave out the cases run only by name`.
  jobs.mk ran every case by its exact name, so `cases-*` and `quiet-*` ignored `whole_run`:
  worst-walk and sched-cluster-old-control ran in trains, and the steward case would have too.
  The aggregate targets now leave such cases out; each keeps its own target.
- **Head** and gate results: see the result message.

## Final: known-late list, one core a job, head de588ed49 (16 commits on main 29238720c)

- **Known-late list.** `late = ["PolicyDeclassifyUnfit"]` is in model-mutations, with its seed
  (4709) and MODEL1 in a comment.
  - Its job runs with TESTBENCH_LATE=1, which lifts the steward cap.
  - A pass is reported "caught late, known" in the result and as LATE in the log.
  - Any other late mutation fails by name, and so does a known one no longer caught at all.
  - An entry that names no job fails the case.
- **Cores.**
  - Every mutation job asks 1 core.
  - The late job asks `late_cores = 4`, and `MODEL_THREADS = "{cores}"`; the bench substitutes each
    job's own core count for `{cores}`.
  - Its deadline is 1,552 s: 582 s on 4 cores, doubled because q may grant half the ask after a
    minute, plus a third. A run at 776 s got 2 cores and expired.
- **model-mutations (Q_PRIO=8): PASS, rc 0, 692 s of wall, 148 jobs.**
  - PolicyDeclassifyUnfit: LATE, 598 s on 4 cores.
  - R2OneCursor: 340 s; PolicyEndLeaseAdmitted: 130 s; PolicyAgentOtherSet: 101 s.
- **The steward case** is renamed steward-model-host-tests, with `whole_run = false`. It was not
  rerun on this head, as instructed: its measurement stands (1,473 s and 1,435 s of run time;
  deadline 2,369 s per job).
- **jobs.mk.** The `cases-*` and `quiet-*` targets leave out `whole_run = false` cases. It keeps
  B19's prebuilt recipe and `--exact`, which survived; mine was dropped.
- **Gates at this head, all rc 0:**
  - `cargo test -p testbench`: 146 passed.
  - docs, formatting, no-cruft, size-budget, model-host-tests and model-mutations.
  - The quiet set, both widths: 32 s of wall. All six passed, bench-ssh-loopback-deadlock and
    client-host-tests included.
