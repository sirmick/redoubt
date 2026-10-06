# B10 report: the model's threads from a bound, a counted refusal, the keeper's wait

Branch wp-B10, from main 995781152 (fast-forwarded from edd953085 before the first commit, since
main had rewritten the "On a shared host" paragraph). Never pushed.

## Commits

- 1ef53e763 model: a hostile map_fixed length is refused by count, not by time
  (model/src/ghost.rs, model/src/kernel.rs, model/tests/map_fixed_contracts.rs)
- e0ad53b05 testbench: the keeper gives a loopback guest until the case's deadline
  (tools/testbench/src/ssh.rs, docs/testbench.md: the keeper sentence, its status line)
- 6593bc288 model: a bound on cargo's test threads bounds the whole test binary
  (model/tests/common/mod.rs, docs/kernel/model.md, docs/testbench.md "On a shared host")

## Design points

1. `threads()` in model/tests/common/mod.rs: `MODEL_THREADS` (positive integer, else the test
   fails naming it; the message is also printed to stderr first, because `quiet_panics` silences
   the panic hook), else 1 if `RUST_TEST_THREADS` is set, else `available_parallelism`. Seeds,
   blocks of 64, the stop flag and panic-as-I14 are unchanged.
2. `Ghost::pages_walked`: incremented once per vpn in `tables_needed`, the one per-page walk on
   `map_fixed`'s path (`range_free` is one `BTreeMap::range` lookup and steps no page). The test
   reads the delta through `w.k.ghost` and asserts 0. `tables_needed` takes `&mut self` now (all
   callers already had it). To stay inside the model's size ceiling (the first version was 10242
   of 10240 lines), its inner key loop became one `extend(... .filter(...))`, same behaviour.
   This touches kernel.rs, outside the brief's named ghost.rs: the increment has to sit where the
   walk is. Nothing in the kernel model reads the count.
3. `leftover_guest(case_dir, deadline)`: `run` passes the case's deadline; the probe in
   `loopback_usable` passes now + PROBE_TIMEOUT (30 s, the probe's own bound; it has no case
   deadline). Message form kept: "... still ran {elapsed}s after its sessions ended (pid ...;
   killed)". The loop is `leftover(program, named, deadline)` so a host test can drive it with
   `sh`.
4. Pages below.

## The deliberate break (not committed)

Inserted `let _ = self.tables_needed(pid, first..first + n);` before the budget check in
`map_fixed`, ran
`jobserver share cargo test -p redoubt-model --test map_fixed_contracts huge_len`:

    test huge_len_is_refused_promptly ... FAILED
    assertion `left == right` failed: the refusal walked 66060288 pages
    test result: FAILED. 0 passed; 1 failed; ... finished in 2.29s
    exit 101

kernel.rs restored from a copy afterwards; confirmed absent from the diff.

## Cases

- map_fixed_contracts: 10 passed (before and after the size fix); model lib tests: 7 passed.
- Threads, observed by sampling `/proc/<pid>/status` `Threads:` every 0.2 s of the `properties`
  binary (debug, REDOUBT_MODEL_SEQUENCES=20000), run under `jobserver bounded` (RUST_TEST_THREADS=4):
  - MODEL_THREADS unset: max 9 threads over 7098 samples (main + 4 test threads + 1 runner thread
    each: 4 busy); 6 passed, 1468 s.
  - MODEL_THREADS=2: max 13 over 3602 samples (main + 4 + 8); 6 passed, 744 s.
  - MODEL_THREADS=0 and =x: the test fails, stderr `MODEL_THREADS must be a positive integer, not "0"`
    (and `"x"`).
  - The unbounded run (one thread per core) was not run separately: under the pool it would be an
    `all` job, and the timed run below plus tonight's alone runs cover the unbounded and bounded
    times. Say if you want it run.
- testbench: `cargo test -p testbench --bins ssh::` 4 passed, including
  `the_keeper_waits_to_the_deadline` (a 1 s-late guest under a 60 s deadline is not reported; a
  stuck one at a 200 ms deadline is named and killed) and `the_keeper_finds_what_names_the_case`.
  B9 has not landed on main; whichever lands second rebases.

## Gates (make -f .wash/local/jobs.mk, on the branch before the size fix unless noted)

- docs: PASS, rc=0
- rv64/unsafe-budget: PASS, rc=0 (unchanged; the model is uncounted)
- rv64/formatting: PASS, rc=0
- rv64/host-tests: rc=0. The target is a substring filter, so it ran every host-tests case in
  the bounded class (RUST_TEST_THREADS=4) beside other members' work: all 16 PASS, among them
  host-tests 35.8 s (testbench's tests) and **model-host-tests 2841.7 s**.
- rv64/size-budget: FAIL first (model 10242 > 10240); after the fold into 1ef53e763, PASS, rc=0.
- rv32 build: not run (no kernel, loader, sys, rt or server change).
- Whole bench: not run.

## Model suite wall time

- Before (tonight's logs, alone, unbounded, one thread per core over cargo's threads): 377.4,
  377.8, 378.8, 379.4 s (MEM1, VOL1), and 2998.4 s once (SCHED1 rv64).
- After (bounded, RUST_TEST_THREADS=4 so 4 busy threads, beside other jobs): 2841.7 s.
  A single bounded run is slower than an alone run on 24 cores, as expected from 4 cores against
  24; what it buys is that nothing waits for it. If its time on the gate's critical path matters,
  MODEL_THREADS raises it within the shared class (e.g. RUST_TEST_THREADS=4 MODEL_THREADS=2 ran
  the properties binary in half the time of MODEL_THREADS unset). That choice is the scheduler's
  (jobs.mk, not mine). The separate timed `jobserver bounded cargo testbench model-host-tests`
  run was stopped as a duplicate once host-tests had produced this measurement.

## Page lines as written

testbench.md "On a shared host" (main's text from 995781152 kept for the ssh-loopback clause):
"Those are a `host-tests` case whose crates' tests assert a wall-clock bound (`redoubt-rt`,
`redoubt-client`, `redoubt-keyd` and `redoubt-consoled` do; `redoubt-ipd`, `redoubt-model` and
`testbench` only read the clock), which no tolerance would make load-proof; ..." and the
sentence "The model's property runs spawn a thread per host core ... from the same bound."
deleted.

testbench.md "Sessions and the loopback server" (the keeper bullet, where the 5 s rule lived,
rather than a new sentence in the first paragraph): "**The keeper:** once a case's sessions have
all exited, any `qemu-system-riscv64` whose command line names the case's directory (the run's
own) has until the case's deadline to go, as a loaded host may slow a guest's shutdown with
nothing wrong; one still running then is killed, and the case fails, naming it." Status list
gains `host:testbench::the_keeper_waits_to_the_deadline`, tested (15) -> (16).

model.md, after `REDOUBT_MODEL_SEQUENCES`: "A bound on cargo's test threads is a bound on the
whole binary: each test's runner spawns `MODEL_THREADS` threads if it is set (a positive integer;
anything else fails the test, naming it), else one if `RUST_TEST_THREADS` is set, else one per
core." The runner paragraph's "runs seeds on every core" became "on several threads".

## Summaries checked

- README.md, GETTING-STARTED.md, model/README (none), tools/testbench README (none): grep for
  model-host-tests, RUST_TEST_THREADS, "every core", "five seconds": no hits outside
  docs/testbench.md; no change needed.
- docs/kernel/model.md runner paragraph and the sequences paragraph: updated.
- docs/testbench.md shared-host paragraph, keeper bullet, status list: updated.
- .wash/local/jobs.mk still lists model-host-tests in `alone`: not mine (the scheduler); it
  should move to `bounded` when this merges.

## Risks

- The counter catches a walk only through `tables_needed`; a future per-page scan written
  elsewhere on the path would not count. The comment on the test says what it counts.
- A keeper failure now only shows at the case's deadline, so a leaked guest holds the case up to
  `timeout_secs` before it is reported.

## Round 2: red notes 1-2 (folded; branch now 6560ae241, 516de2db5, 5ce1c9e14)

Note 1: the counter is now `Ghost::map_steps`: one per page `tables_needed` walks and one per
lookup `range_free` makes (`range_free` takes `&mut self`, body one line, so the model stays at
its ceiling). The test asserts the refusal took 0 steps; its comment claims "all 2^26 pages" only
for `tables_needed` first and "counts its lookups" for the overlap check first; `fresh_tables`'
own guard unchanged. Recorded break, overlap check first (`let _ = self.range_free(pid, first,
n);` before the budget check, not committed): `the refusal took 1 steps over its range`, exit
101. The tables_needed-first break (66060288) was recorded on round 1's counter; the walk is the
same.

Note 2: a floor, `GUESTS_GO` (5 s) restored: the keeper waits until max(deadline, sessions' end +
5 s). Why the floor and not reordering: the ordering already reports a session's own failure
first (`failure.or(leftover)` in `run`; a timed-out session is `Stop::Failed`, so it is the
verdict and the keeper's message is only the fallback), so the remaining defect was the shrunken
grace near the deadline, which only the floor fixes. Doc comment now says both. Test: a stuck
guest given a deadline of now is killed no sooner than GUESTS_GO (elapsed measured, not the
printed seconds, so load cannot flake it). Page: "has until the case's deadline, and at least
five seconds, to go, ...; one still running then is killed, and the case fails, naming it, unless
a session has already failed it."

Gates (round 2): docs rc=0; rv64/formatting rc=0; rv64/size-budget rc=0; rv64/unsafe-budget
rc=0; `jobserver bounded cargo test -p testbench --bins` 90 passed rc=0; model lib 7 + 
map_fixed_contracts 10 passed; `RUST_TEST_THREADS=4 make rv64/model-host-tests` PASS 2821.9 s,
rc=0 (alone class, wall 3032 s with the queue).

## Merge gate (head 373396887 = `git rebase --signoff 27e74ec3b`)

Rebase: no conflict. `git range-diff 995781152..5ce1c9e14 27e74ec3b..HEAD`: each of the three
patches identical, the only change an added `Signed-off-by: Michael <sirmick@gmail.com>`.
`git diff 5ce1c9e14 HEAD` is not empty: it is exactly main's 995781152..27e74ec3b (MEM1's merge:
docs/testbench.md +45, tools/testbench/{Cargo.toml,case.rs,main.rs,memory.rs,qemu.rs}), none of
it in ssh.rs, model/ or my hunks. model-host-tests not rerun (the rebase touched nothing under
model/; testbench/src changed only through main, so testbench's unit tests were rerun: 96 passed).

Shared (make, one invocation): aborted-text rv64 0.9 / rv32 0.9; exit 0.1 / 0.1; forbid 0.2 /
0.2; host-key 0.1 / 0.1; openssh 33.3 / 30.1 (first run builds the guest's case initrds). All PASS rc=0.
Alone (`jobserver all cargo testbench --arch W bench-ssh-loopback`, since the name is a prefix of
the others and the filter is a substring): rv64 and rv32 rc=0, all 7 PASS, bench-ssh-loopback
0.1 s, deadlock 0.2, openssh 3.4 / 3.5.
Alone (make): bench-ssh-loopback-deadlock rv64 PASS 1.4 s, rv32 PASS 0.2 s; bench-ssh-guest rv64
PASS 1.2 s (smp=1), rv32 rc=0 with no case (the case is rv64 only).
