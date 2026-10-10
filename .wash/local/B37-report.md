# B37 report: userland test time

Branch wp-B37, worktree /home/mcloonan/redoubt/.worktrees/B37, on main 36d1450f9. One commit,
9b97e47b7 (step 2). Not pushed. The raw timings are in /home/mcloonan/redoubt/.tmp/B37: measure.log
and the step-*.log files.

## Step 1: where the time goes

How the timing was run:
- The B37 tree, under q leases: `./test-shell` at 8 cores, each difftest suite at 4, cargo at 8.
  `./test-shell`'s stages were timed through a copy kept outside the tree.
- Machine cases ran one after another through `make -f scripts/jobs.mk rv64/<case> rv32/<case>`,
  from prebuilt. That is 29 cases, every one that boots beamlet or the shell (the beamlet-*,
  userland-*, steward-*, boot-profile*, pack-*, init-boot and image-disk cases), on both widths.
- **cold** is a fresh worktree. **shell change** is one Elixir module edited
  (userland/shell/lib/redoubt/shell/printer.ex). **beamlet change** is one VM source edited
  (userland/otp/vm/src/lib.rs).
- tools/elixir-tests could not run here: there is no reference/elixir-1.20.4 checkout.

| step | cold | shell change | beamlet change |
| --- | ---: | ---: | ---: |
| build-beamlet (pcre2) | 12.7 | — | 11.6 |
| cargo test (userland/otp) | — | — | 22.0 |
| ./test-shell, whole | 90.7 | 60.7 | 70.9 |
| · setup (beamlet build, mix) | 14.0 | 0.5 | 8.7 |
| · formatting | 0.3 | 0.3 | 0.3 |
| · native (cells, beamlet-screen) | 4.2 | 0.3 | 0.7 |
| · on_beamlet (ExUnit on beamlet) | 28.3 | 28.0 | 28.0 |
| · on_beam (ExUnit on the BEAM) | 11.9 | 11.7 | 11.8 |
| · entry_point | 0.1 | 0.1 | 0.1 |
| · terminal (pty) | 2.4 | 0.2 | 0.7 |
| · on_fake_kernel (riscv build of beamlet-redoubt, its fake tests, a shell run) | 29.5 | 19.5 | 20.5 |
| difftest, serial, all suites | 121.3 | — | 36.3 |
| · atomvm / crypto / elixir / erlang / net / ssl | 88.4 / 8.1 / 3.3 / 8.8 / 11.2 / 1.4 | — | 20.2 / 6.8 / 0.8 / 2.2 / 5.6 / 0.7 |
| prebuilt | 394.7 | 117.1 | 207.7 |
| machine cases, 58 runs one after another | — | 764 | 790 |
| **whole gate list** | | **≈ 942 s (15.7 min)** | **≈ 1138 s (19 min)** |

- The shell change's formatting stage failed only on my unformatted timing edit; its time stands.
- The atomvm suite fails 6 tests, which is B34's known set.

The machine cases, both widths together, for the shell change (the beamlet change's are within a
few seconds of these):

| cases | seconds |
| --- | ---: |
| steward-restart | 376 |
| steward-ssh-idle | 107 |
| steward-restart-ssh | 42 |
| steward-ssh-two-principals | 34 |
| steward-sub-budget-flood | 25 |
| steward-vault-launch | 24 |
| userland-read-only | 20 |
| userland-boot | 19 |
| the other 21 cases | 117 |

Where the time goes:
- **The machine cases: 81 % of a shell change's list.** They run one after another, and half of
  their time is two cases that wait by design. steward-restart has 13 restarts, 14 s apart, to
  stay under the reboot rule. steward-ssh-idle has 45 s of quiet.
- **prebuilt.** 117 s after an Elixir-only change, 208 s after a beamlet change.
- **./test-shell.** About 61 to 71 s, of which:
  - on_beamlet takes 28 s;
  - on_fake_kernel takes 20 s, and rebuilds and tests beamlet-redoubt's Rust even when only
    Elixir changed;
  - on_beam takes 12 s.
- **The difftest.** 36 s serial even with nothing to recompile, and 121 s cold.

## Step 2: the parallel difftest (9b97e47b7)

`tools/difftest` keeps its output, its SKIP rules and the order of its failure diffs.

- **Parallel runs.** All suites' tests run at once through xargs -P, as many as `nproc` allows
  (the q lease; DIFFTEST_JOBS overrides it). Each test has its own root.
- **The cache,** under build/difftest/<suite>/cache:
  - per source file, its compiled modules, keyed on its bytes, the suite's .hrl files and the
    toolchain (the erl and elixir paths, and expect.escript);
  - per module, its oracle result (or "not a test", for no start/0), keyed on the module's name,
    its file's key, its .stdin and .args, and the suite's helper files (sources with no start/0).
- **Only misses run.** Misses are compiled in parallel. Their oracle runs in parallel chunks
  through `expect.escript DIR MODULE...` (new: an optional module list), and the console tests run
  alone with their input. Entries the tree no longer names are pruned. DIFFTEST_FRESH=1 empties the
  cache.
- **Verdicts.** Each job writes its own verdict, and the verdicts are gathered in the serial order.

Numbers. The serial figures are the old script on the same tree, at 12 cores, both with
nothing changed:

| run | serial | parallel, 12 cores | parallel, 4 cores |
| --- | ---: | ---: | ---: |
| cold (empty cache) | 121 s (the measured cold run) | 31.0 s | — |
| warm, nothing changed | 38.6 s | 8.9 s, 8.8 s | 11.4 s |
| one test edited (erlang/arith.erl) | (recompiles and re-oracles the whole erlang suite) | 9.3 s | — |
| erlang alone, warm | 2.2 s | 2.4 s | — |

**Counts.** Every run gave 518/524 passed, 6 failed and 18 skipped, the serial run's counts. The
6 failures and their diffs are the serial run's, with one exception: atomvm/test_node's
*expected* value. The old oracle ran every module of a suite in one BEAM, so an earlier test's
state gave test_node `{'EXCEPTION',error,{badmatch,43}}`; run in a chunk it returns 0. It fails on
beamlet either way. An oracle that depends on what ran before it is a test-isolation defect of the
old oracle, and the chunked one shares it, only less often.

**A bug found and fixed on the way.** The first version keyed the oracle on the file, not the
module, so the two modules of elixir/otp.ex shared an entry, and a warm run lost Elixir.OtpTest
(523 counted, not 524). The key now includes the module.

**Gates on 9b97e47b7.**
- The full difftest's counts are the serial run's (above).
- ./test-shell passed every stage.
- docs: exit 0.

**Docs.** GETTING-STARTED.md's beamlet section says how the difftest parallelises and caches;
userland/otp/README.md has a one-line note. The tests' pages say nothing about the difftest's
mechanics, so they needed no change.

### Step 2's red notes, folded (wp-B37 now dc052b1ea, amended from 9b97e47b7)

1. **Cross-module calls.** A module's oracle key now includes every source of its suite, which
   replaces the helper-file hash. Editing atomvm/echo.erl changed pingpong's oracle key (checked
   by its cache entry name) and reran the suite's oracle.
2. **Isolation.** The oracle runs on one BEAM per module (`xargs -n 1`), with no chunks, so
   membership no longer depends on the lease. test_node's expected value is `0` at both 12 and 4
   cores.

Numbers (/home/mcloonan/redoubt/.tmp/B37/difftest2.log, dt2-*.log). Four SMP2 boots were running
beside these runs.

| run | 12 cores | 4 cores |
| --- | ---: | ---: |
| cold (DIFFTEST_FRESH=1) | 47.6 s (was 31.0, chunked) | 70.6 s |
| warm | 9.1 s, 9.1 s | 11.4 s |
| one test edited, erlang/arith.erl (reruns its suite's oracle) | 11.0 s | — |
| one test edited, atomvm/echo.erl (491 sources) | 26.5 s | — |

- **Counts:** every run gave 518/524, 6 failed, 18 skipped.
- **The failures, at both core counts:** atomvm test_binary_to_term, test_code_all_available_loaded,
  test_code_server_nifs, test_display_string, test_node and test_unicode, which is B34's set.
- **Docs:** GETTING-STARTED.md's difftest paragraph now says the results each come from a BEAM
  of their own, and that an edited test's suite is asked again. The docs gate passed (rc=0).
- **wp-B37-cuts is rebased onto dc052b1ea**, with the same four cuts: 603401637, 2deb2586f,
  63221a31a and 4d3d126f6.

## Step 3: proposals, not built

For a checkpoint. The savings below are measured where I give a number, and estimated where I say
so.

1. **Run the machine cases concurrently, each under its own q lease.** One after another they take
   764 s. Run together, they would take about steward-restart's 190 s, since the two long cases
   mostly wait, and their verdicts are icount-held. jobs.mk would need a target for a case set,
   because the rule today is no -j, so this is the owner's call. Estimated saving: about 570 s on
   every list that runs them.
2. **Choose the machine cases by what changed.** For an Elixir change outside the driver, Term and
   beamlet, run none. For a driver or Term change, run userland-boot and userland-read-only on both
   widths (39 s) plus the steward session cases. For a beamlet change, run all of them. Measured
   saving for a shell change outside the driver: 764 s, plus 117 s of prebuilt.
3. **./test-shell, on_fake_kernel.** Skip its riscv build and its cargo tests of beamlet-redoubt
   when nothing in userland/otp or libs/ changed, and keep its shell run on the fake kernel.
   Estimated saving: about 15 s of its 20 s.
4. **./test-shell, on_beamlet and on_beam concurrently.** They are separate VMs. They need
   separate scratch, and both today use userland/shell/_build. Estimated saving: about 12 s.
5. **prebuilt after an Elixir-only change (117 s).** Profile what it rebuilds: the userland
   volume for both widths, verity trees, and possibly every program. A userland-only rebuild path
   could cut most of it. This needs a measurement first.
6. **The two waiting cases.** steward-restart's 13 restarts are needed to reach ipd's cap of 12,
   and 14 s is what keeps it under 5 restarts in 60 s. steward-ssh-idle's 45 s is the behaviour
   it tests. Neither should be cut; proposal 1 hides them.

With 1, 3 and 4, a shell change's list would go from about 942 s to about 330 s (estimated). With
2 as well, an Elixir change outside the driver would take about 60 s. A beamlet change's list
would go from about 1138 s to about 470 s (estimated) with 1 and the new difftest.

## Step 3: the cuts (branch wp-B37-cuts)

wp-B37-cuts is in the same worktree (/home/mcloonan/redoubt/.worktrees/B37). It branches off
wp-B37 at 9b97e47b7, because step 2 was still in review: main was 36d1450f9. wp-B37 itself is
unchanged. Four commits, in the approved order:

1. **1ebf59f93, jobs.mk `set`.** `make -k -f scripts/jobs.mk set CASES="a b"` runs rv64/ and
   rv32/ of each listed case at once, under the file's own -j64. Each case still goes through its
   class recipe: its own q lease, --quiet for the quiet ones, --lock net for the net ones. An
   empty set prints "set: 0 cases" and exits 0. docs/testbench.md "On a shared host" has the
   one-sentence rule. The docs gate passed (rc=0).
2. **baeb3f5f0, on_fake_kernel.** cargo still builds both targets; when nothing changed that is
   a no-op of about 0.1 s. The stage then hashes every executable cargo names
   (`--message-format=json`) and vm/tests/fixtures/*, which limits.rs loads at run time, and
   skips the 19 s of tests when the hash equals the one recorded after the last pass
   (userland/otp/target/test-shell-fake-kernel). The shell run on the fake kernel always runs.
   Checked: a new test in redoubt/tests/pack.rs made the tests run, and so did reverting it.
3. **196365000, on_beamlet ‖ on_beam.** `mix test` runs in the background into a mktemp log,
   which is printed after beamlet's live run, and each VM keeps its own verdict. Their scratch was
   already separate: beamlet uses _build/beamlet-root, _build/beamlet-test and its own /dev/shm
   xdev; BEAM uses ExUnit's userland/shell/tmp and _build/test. The old "beamlet goes first"
   protected nothing, because every stage ran anyway. GETTING-STARTED.md's test-shell line is
   updated.
4. **a5b5bc309, scripts/shell-cases [BASE].** It reads `git diff --name-only BASE` (by default
   the merge-base with main) and the untracked files. It prints the 29 kind="boot" cases whose
   toml names Redoubt.Shell or beamlet, and lists on stderr each path that selected them.
   - **Run them:** userland/shell/lib/redoubt/shell/driver*, lib/redoubt/term*,
     lib/redoubt/screen*, userland/otp/, servers/, and **any path it does not place**
     (conservative).
   - **Run none:** the rest of userland/shell, docs/, *.md, .wash/.
   - **My reading of the rule:** "term*, screen*" means lib/redoubt/term* and screen*. There is
     no shell/term* or shell/screen*.
   - GETTING-STARTED.md has the usage line.

**Cut 5, profile only, nothing built.** prebuilt took **116 s with nothing changed** in the tree
(58.5 + 56.8 s), against 117 s after the Elixir-only change. So the cost is fixed per-case
overhead, not Elixir.
- **How it was measured:** strace -f of an rv64 prebuild (122.8 s under strace; 59 s
  without). Trace in /home/mcloonan/redoubt/.tmp/B37/pb.strace.
- **cargo build is 97.5 s of the 122.8.** There are 1,399 invocations, of which only 192 are
  distinct (kernel 224×, loader 224×, test-programs 189×, init 90×, …), each a no-op of about
  0.04–0.07 s.
- **Elixir and Erlang:** erlc and beam.smp take about 13 s under strace, once per width.
- **The rest:** packing and signing, about 12 s.
- **Proposal:** memoise `Builder::cargo` per run, keyed on (workspace, package, bin, features,
  profile, target), as `Builder::userland` already does for the disks. The tree is fixed for a
  prebuild: the fingerprint is checked before and after. Estimated prebuilt 116 s → about 40 s,
  and a whole `cargo testbench` would save the same per case.
- **Its risk:** in a non-prebuild run, an edit made mid-run would no longer reach later cases.
  This is Tier A code (the bench), so it waits for the go-ahead.

**Remeasured** (/home/mcloonan/redoubt/.tmp/B37/measure2.sh, measure2.log, m2-*.log). The machine
was otherwise idle. Seconds:

| step | shell change (step 1) | shell change, printer.ex (now) | driver change, driver.ex (now) | beamlet change (step 1) | beamlet change (now) |
| --- | ---: | ---: | ---: | ---: | ---: |
| build-beamlet | — | — | — | 11.6 | 5.1 |
| cargo test (userland/otp) | — | — | — | 22.0 | 19.2 |
| difftest | — | — | — | 36.3 serial | 9.7 (8 cores) |
| ./test-shell | 60.7 | 30.0 | 29.8 | 70.9 | 43.4 |
| machine case set (shell-cases) | 29 | 0 | 29 | 29 | 29 |
| prebuilt | 117.1 | — | 116.4 | 207.7 | 150.0 |
| machine cases (58 runs) | 764 serial | — | 205.5 at once | 790 serial | 206.1 at once |
| **whole list** | **≈ 942** | **30** | **≈ 352** | **≈ 1138** | **≈ 434** |

- **Results:** all 58 case runs passed in both phases with cases. The difftest gave B34's known
  518/524 with 6 failed. ./test-shell's formatting stage failed only on the unformatted timing
  edit, as in step 1. Every other stage passed.
- **The beamlet edit was a comment.** It left beamlet-redoubt's binaries byte-identical, so
  on_fake_kernel rightly skipped its tests. A code change would add about 19 s.
- **What bounds the machine cases now:** steward-restart (about 188 s), with the quiet cases
  running one at a time beside it.
- **With cut 5's memo** (estimated): the driver list would be about 280 s and the beamlet list
  about 360 s.
- **Flake:** the console.rs host-clock test failed twice in ./test-shell at 8 cores, beside
  SMP2's boots. Both failures came while I was developing, not during the remeasure. It passed
  alone through --quiet (13/13).

## Step 3, cut 5 built (462f88666 on wp-B37-cuts)

- **What changed:** Builder::cargo keeps each build it ran, keyed by its directory and arguments,
  with the binary cargo reported, as Builder::userland keeps each staged disk. A request it has
  already seen is answered without running cargo. A failed build is not kept.
- **New host test:** testbench::a_build_asked_for_again_in_a_run_is_not_run_again. It breaks the
  fixture's Cargo.toml after the first build. The identical request is then still answered with
  the first binary, and a request that differs only in features fails.
  - Mutation check: with the lookup disabled, the test fails (panics at its second build).
- **Changed test:** in interleaved_builds_each_pack_their_own_binary, the "second build compiles
  nothing" check now uses a fresh run in A's directory. Within one run the memo would make that
  check vacuous.
- **Gates:**
  - `cargo test -p testbench`: 158 passed.
  - Clippy: no new warning in build.rs or main.rs.
  - docs rc=0.
- **testbench.md "Building once":** has the rule, and the new test is on its status line.

**Measured** (wall time, `make -f scripts/jobs.mk prebuilt`, both widths):

| prebuilt | before | after |
| --- | ---: | ---: |
| nothing changed | 116 s (58.5 + 56.8) | 43 s (21.6 + 20.8) |
| Elixir-only change (printer.ex) | 117 s | 43 s (21.6 + 21.2) |

All 458 entries built, with 0 failed. The 29 shell machine cases (58 runs) passed from the new
pieces in 204 s, run at once (/home/mcloonan/redoubt/.tmp/B37/memo-cases.log).

**Remaining risk:** in a long plain `cargo testbench` run, an edit made mid-run no longer reaches
later cases. The page states that a run takes the tree as it was when it began. Prebuild already
refuses a tree that changed while it built.

## The console.rs flake: not the quiet class

beamlet-redoubt tests/console.rs `an_end_of_input_already_waiting_ends_the_idle_that_takes_it`
fails at line 226, `assert_eq!(p.console_read(), ConsoleInput::Nothing)` (got Eof).

- **Why it fails:** `console_read` (src/lib.rs:525) sends the first read (`start_reading`) and
  then takes whatever has already completed. With empty input, the fixture's console thread
  answers the read with the end at once. When that thread wins the race before
  `take_completed`, the platform rightly returns Eof.
- **So it is a race in the test, not a wall-clock bound:** the test assumes the answer arrives
  later. Running it alone only makes the race rarer, and a pass alone is not a verdict.
- **Its clock assertions are safe under load:** they are a lower bound (idle returns by its
  deadline) and "more than 5 s of a 10 s deadline left", which has 5 s of margin.
- **Where it runs:** the test is part of beamlet-lookup-host (bounded class, every
  beamlet-redoubt test) and of ./test-shell's on_fake_kernel.
- **The fix belongs in the test:** hold the end back until after the first read. For example, use
  an input Read that blocks on a channel the test releases after `console_read` returns Nothing.
  That is beamlet's console package's file, so I propose it as a follow-up and have not added a
  quiet-class entry.

## shell-cases corrected after the red's BLOCK (716aaa889, amended from 4d3d126f6)

The first mapping was wrong. Machine cases reach the shell well past its driver, so "other
Elixir runs none" let an Elixir change skip cases it can break. **The 30 s figure for a printer.ex
change in the step 3 table is withdrawn.**

**The mapping now:**

| path changed | cases run |
| --- | --- |
| userland/shell/test/, docs/, *.md, .wash/ | none |
| the rest of userland/shell (lib, mix.exs, config, help, setup.sh) | the shell's set, 14 cases |
| userland/otp/, servers/, any path the script does not place | beamlet's set, 30 cases |

- **The shell's set:** every boot case naming Redoubt.Shell or waiting for its prompt `(N)> `,
  plus three that reach the shell otherwise: beamlet-footprint, beamlet-launch and
  steward-vault-launch.
- **Beyond the formula you gave:** matching on the prompt adds userland-read-only (named in the
  red's finding), steward-vault-session, steward-sub-budget-flood, steward-restart-ssh and
  steward-session-ends.
- **steward-session-ends** names neither beamlet nor the shell, and it was not in the old 29. It
  waits for the prompt, so it is now in both sets, which is why beamlet's set is 30.

**Checked by hand on the script:**

| edit | cases |
| --- | ---: |
| printer.ex, mix.exs, help/ | 14 |
| test_helper.exs | 0 |
| docs/userland/shell.md | 0 |
| vm/src/lib.rs | 30 |
| servers/steward | 30 |

**Remeasured, printer.ex change, the shell's set at once.** The machine was idle. Logs are
/home/mcloonan/redoubt/.tmp/B37/m3-*.log.

| step | before (step 1) | now |
| --- | ---: | ---: |
| ./test-shell | 60.7 | 41.0 |
| prebuilt (with the cargo memo) | 117.1 | 52.1 |
| machine cases | 764 (29 cases, serial) | 236.9 (14 cases, 28 runs at once, all passed) |
| **whole list** | **≈ 942** | **≈ 330** |

- **What bounds the cases now:** the shell's set holds six quiet steward cases (ssh-idle,
  ssh-two-principals, sub-budget-flood, vault-session, vault-launch, session-ends). Over both
  widths that is 12 runs, which go one at a time on the quiet core set.
- **Formatting:** it failed only on the timing edit, as before. Every other stage passed.
