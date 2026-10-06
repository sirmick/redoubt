# B10: the model's tests take their thread count from a bound, and the two wall-clock bounds load can fail go

Tier A (the model's harness and the bench's keeper: test tooling, no kernel or server code).
Size S. Needs nothing. Why now: `model-host-tests` runs alone under docs/testbench.md "On a
shared host" and closes the gate for 380 to 3,024 s per whole bench, the long runs being its
own oversubscription; and the 14 `ssh-loopback` cases sit in the alone class for a wall-clock
bound in the keeper, not for anything they measure.

Run everything natively on this host. Your own `host-tests` runs are host-clock cases: run them
with nothing else of yours beside them.

## Context rules (read these first)

- **Read only** `model/tests/common/mod.rs`, `model/tests/map_fixed_contracts.rs`,
  `tools/testbench/src/ssh.rs` from `GUESTS_GO` to the end of `leftover_guest`, and the page
  sections named below. Not the model's families, not the kernel.
- **Don't open `.wash/qa/*.md`.**
- **Keep the report under 1900 bytes,** detail in `.wash/local/B10-report.md`.

## Reading list (only these)

- `docs/testbench.md`: "On a shared host" (the paragraph after the seed sweep), "Sessions and
  the loopback server" (its first paragraph).
- `docs/kernel/model.md`: the paragraph that names `REDOUBT_MODEL_SEQUENCES`, if any; otherwise
  the "How to use it" section.
- `model/tests/common/mod.rs` (`sequences`, `run`); `model/tests/map_fixed_contracts.rs`
  (`huge_len_is_refused_promptly`); `tools/testbench/src/ssh.rs` (`GUESTS_GO`,
  `leftover_guest`).

## The design

1. **One bound for the whole test binary.** `run` spawns `available_parallelism` threads per
   test, and cargo's harness runs tests in parallel on top, so a bound on cargo's threads
   bounds nothing. The rule: *a bound on cargo's test threads is a bound on the whole binary.*
   `run`'s thread count becomes, in order: `MODEL_THREADS` if set (a positive integer; anything
   else is a test failure naming the variable); else **1** if `RUST_TEST_THREADS` is set, so the
   binary's threads are cargo's count and no more; else `available_parallelism`, as today, for
   a developer's unbounded run. Document the three in `run`'s doc comment and on model.md beside
   `REDOUBT_MODEL_SEQUENCES`. No other harness change: seeds, blocks of 64, the stop flag and
   the panic-as-I14 rule stay.
2. **The one wall-clock bound in the model becomes a count.** `huge_len_is_refused_promptly`
   asserts `elapsed < 1 s` to catch a regression from one range lookup to a walk of 2^26 pages.
   Measure the work, not the time: the model's ghost state gains a counter of pages the
   `map_fixed` path visited (incremented where `tables_needed` or an overlap scan steps a page;
   read through the model's inspection, never by the kernel model's logic), and the test asserts
   the refusal visited **zero** pages. Keep the comment's reasoning, rewritten for the count. If
   the counter cannot be placed in a few lines, say so and stop: a wider bound is not the fix.
3. **The keeper's bound becomes a wait to the case's deadline.** `leftover_guest` names and
   kills a guest still running `GUESTS_GO` (5 s) after its sessions ended, and reports it as a
   failure. Under load a guest's shutdown can take longer than 5 s with nothing wrong, so this
   is a wall-clock bound a shared host can fail. Change it to wait until the case's own
   deadline (`timeout_secs`), and only then kill and report: the case's deadline is already the
   one host-clock exposure the rule allows. The message keeps its form with the elapsed time.
   Hotspot: B9 ("the keeper's host test races /proc under load") touches the same function's
   host test; whichever lands second rebases.
4. **The classes move.** With 1 to 3 in, `model-host-tests` is a `host-tests` case whose crates
   assert no wall-clock bound, so it runs beside other work under `RUST_TEST_THREADS` (the
   bounded class); the operator's scheduler needs no new mode. The `ssh-loopback` cases never
   measured with the host's clock: their `sshd` runs in inetd mode through `ProxyCommand` and
   binds no port, the guest's stdio is a virtio-serial port, and 13 of the 14 assert no time;
   their exposures are the deadline and (until 3) the keeper's bound. Those 13 are class (ii)
   cases: a pass is a verdict, a failure that is only the deadline is rerun alone. The 14th,
   `bench-ssh-loopback-deadlock`, expects its timeout (`timeout_secs = 0.2`, a `must_fail` on
   the mark it never gets), so it stays alone beside `bench-ssh-guest`; the page on main says
   so. Write "13 of the 14", never "no such case".

## The cases

- The model's own tests: `cargo test -p redoubt-model` unbounded; then with
  `RUST_TEST_THREADS=4` and `MODEL_THREADS` unset, with the binary's thread count observed at or
  under 4 plus the harness's own (report how you observed it: `/proc/<pid>/status` `Threads:`
  sampled, or `ps -L`); then `MODEL_THREADS=2`.
- `huge_len_is_refused_promptly` passes with the count; a deliberate break (one lookup per page,
  on a branch, not committed) fails it with the count in the message: record the run.
- `tools/testbench`'s tests for the keeper (B9's and any existing): a guest that ends late but
  before the deadline is not reported.
- `cargo testbench model-host-tests` under `RUST_TEST_THREADS=4` beside another case, timed, in
  the report with the unbounded time beside it.
- The docs check.

## Page lines (exact text in the report)

- **testbench.md**, "On a shared host": delete the sentence "The model's property runs spawn a
  thread per host core inside each test (`model/tests/common/mod.rs`), on top of cargo's own
  parallelism: that case runs alone, and oversubscribes the host alone, until its runs take
  their thread count from the same bound." In the host-tests sentence, `redoubt-model` leaves
  the list of crates that assert a bound (the list becomes `redoubt-rt`, `redoubt-client`,
  `redoubt-keyd` and `redoubt-consoled`; `redoubt-ipd`, `redoubt-model` and `testbench` only
  read the clock). The `ssh-loopback` clause moves as the paragraph on main now says (the
  Architect's edit: see the thread).
- **testbench.md**, "Sessions and the loopback server": one sentence that a guest still running
  after its sessions is given until the case's deadline, then killed and reported.
- **model.md**: beside `REDOUBT_MODEL_SEQUENCES`, `MODEL_THREADS` and the rule of point 1.

## Owned paths

`model/tests/common/mod.rs`, `model/tests/map_fixed_contracts.rs`, the model's ghost counter
(`model/src/ghost.rs` or where the inspection lives: the smallest site), `tools/testbench/src/ssh.rs`
(`GUESTS_GO`, `leftover_guest`), the pages above. **Not yours:** the model's families and
mutations, the kernel model's logic, the scheduler in `.wash/local`.

## Gates

The model's host tests; `tools/testbench`'s host tests; the docs check; fmt; the unsafe
ratchet (unchanged); the size budget (unchanged). Report each command with its exit code and the
timings of the design's point 4.

## Not here

Any change to what the model checks, its sequence counts or its million-run acceptance; the
operator's scheduler; B9's `/proc` race (rebase around it).

## Checkpoint

None: report at the end.
