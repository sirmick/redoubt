# BEAM1: beamlet runs on the machine, under `init`

Tier A: beamlet's platform boundary, a new thread primitive in the runtime, and the bench. Size
M-L. Needs: the `init` step (done) and API1 (done). Start from main. Run every cargo and bench
command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The step (`beamlet-redoubt`) is cut into five packages:
- BEAM1, this one: the VM on the kernel, with the console, the clock, randomness and modules
  read from `/boot`.
- BEAM2: the userland disk and the shell on the UART console.
- BEAM3: the I/O threads, and files over 9P.
- BEAM4: the natives, and the shell launching a native program.
- BEAM5: `gen_tcp` over `/net`.

Build nothing here that a later package removes.

## Context rules (read these first)

- **Don't read whole files.** Run `grep -n`, then Read a range. `userland/otp/vm/src/vm.rs` is
  long: read only `Config`, `spawn`, `run` and the `EMBEDDED` list.
- **Don't open `.wash/qa/*.md`, other packages' reports or other briefs.** This brief holds
  what they decided. If you must open a QA file, read it only up to its checkpoint comment:
  `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench and cargo output.** Use `cargo testbench --list | awk '{print $1}'`, and read
  boot logs under `target/testbench/last/` only through `grep` or `tail`. A cross-build of
  beamlet prints a lot: keep only `tail -30` and the `error` lines.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/BEAM1-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/userland/beamlet.md`: "The `Platform` boundary", "The console, the clock and
  randomness", "beamlet on Redoubt", and the first two paragraphs of "Asynchronous underneath".
- `docs/userland/native.md`: "The client library", only its table of modules.
- `docs/testbench.md`: "The case file", "Starting a case's programs" and "The servers' cases
  under `init`".
- `userland/otp/redoubt/src/lib.rs`, whole (233 lines): the platform as it runs on the fake
  kernel. Also `src/bin/fake-redoubt.rs`, `run` only.
- One servers' case under `init` with a program that is not a server: `tests/netd-restart.toml`
  and `tests/data/net/restart.json`, the `judge` entry. Copy its `keyd`, `consoled` and
  `bootfsd` entries from `tests/init-servers.toml`'s manifest.
- `libs/rt/src/handle.rs`, `thread_create` only, and `libs/rt/src/start.rs`, the `entry!`
  macro.

## Settled rules

1. **One program, `beamlet`,** a `[[bin]]` of `beamlet-redoubt` beside `fake-redoubt`, built for
   the machine with `no_std`/`no_main` and the runtime's `entry!`.
   - Its `main` is `fake-redoubt`'s `run`, given a `Threads` and a `Modules` for the machine.
   - It takes its module and function from its startup block's arguments: `MODULE [FUNCTION]`,
     as `fake-redoubt` takes them.
   - It carries the natives `fake-redoubt` carries (`beamlet-crypto`, `beamlet-re`). Make them
     non-optional dependencies of the `beamlet` binary, not of the `fake` feature only.
   - The process exits with the code `run` returns.
2. **One scheduler thread in M1.** The kernel runs one hart until M2, so a second scheduler
   would cost a thread and buy nothing. beamlet-vm's multi-scheduler path needs `std`, which the
   machine does not have, so don't port it. The two scheduler threads in the page's split come
   with several harts (SMP3). BEAM3 rewords the split; you change nothing there.
3. **Threads on the machine: one safe primitive in the runtime.** Add `redoubt_rt::thread::spawn`.
   - It takes a `Box<dyn FnOnce() + Send>` and a stack size in pages, takes the stack from
     `map_anon`, and calls `thread_create` with an `extern "C"` trampoline. The trampoline runs
     the closure, then `thread_exit`.
   - Its `unsafe` is the one `Box::from_raw` in the trampoline, with a `// SAFETY:` line. The
     unsafe budget's line for `libs/rt` rises by that one, with its reason.
   - Its stack is not freed when the thread ends, and the doc comment says so. The platform's
     threads live as long as the VM.
   - This is a new file, `libs/rt/src/thread.rs`. RT2 is rewriting `libs/rt/src/server/`: don't
     touch that directory or `lib.rs` beyond the one `mod` line.
   - The machine's `Threads` is that call. The reader thread is the only thread BEAM1 starts.
4. **Modules from `/boot`, by file name.** The machine's `Modules` reads `/boot/<file>` whole,
   through the client library's `file`, on a connection to `bootfsd`. A name `bootfsd` lacks is
   `None`.
   - This stays: BEAM2 adds the hash check against `system.index`, which is read from `/boot`
     too, and moves the objects to the userland disk.
   - No archive format: each module the case needs is a public entry of its own, within
     `bootfsd`'s 64 entries and 8 MiB.
5. **No grants beyond the page's table.** `files` and `programs` stay `None` on the machine
   (`enotsup`, `eacces`) until BEAM3 and BEAM4. `system_time_us` stays `None`.
6. **Started by `init` like any program in a case,** with its own console from `consoled`, a
   budget from the case's manifest, and `bootfsd`'s endpoint handed to it.
   - Measure the pages one VM needs to run each case, and size the budget at about twice that.
   - Report the binary's size, and the pages the cases' VMs used.
7. **The bench builds beamlet from its own workspace.**
   - `userland/otp` is a workspace of its own, outside the root's (root `Cargo.toml`
     `exclude`). A case's `programs` entry gains a way to name a binary there.
   - Keep it one field, for example `workspace = "userland/otp"`. Build it with B7's per-run
     target directory and the reported executable, exactly as root packages are built
     (`tools/testbench/src/build.rs`, `binary`).
   - The vendored crates check (`vendor-check`) already covers beamlet's lockfile: keep it
     green.

## The cases

Every case boots `init` with `keyd`, `consoled`, `bootfsd` and `beamlet` on rv64. The test
modules live in `userland/otp/redoubt/tests/` as Erlang source, compiled by the case's build
step the way the Elixir oracles' are (`tools/testbench/src/elixir.rs`, the pinned toolchain).
Each is a public entry, with the few OTP modules it needs, and no more.

1. **`beamlet-boot`:** the VM runs a module that writes a line to the console, then returns
   `ok`.
   - Expected, in order: `init`'s started line for `beamlet`; the module's line under its
     `[con …]` tag; and `init`'s line that `beamlet` exited with code 0.
   - **Checkpoint:** send one progress line with the branch, the binary's size, the pages used
     and this case's lines. Then go on.
2. **`beamlet-console`:** the bench types a line (`[[input]]`, once the module prints its
   prompt). The module reads it, echoes it, and then sleeps 200 ms with `receive after`. It
   prints the microseconds `erlang:monotonic_time` advanced across the sleep, which must be at
   least 200,000 (the clock and `idle`). It also prints 32 random bytes from
   `crypto:strong_rand_bytes/1`, as hex.
   - The bench judges: the echo matches; the elapsed time is at least 200,000; and the hex is
     64 digits and not all zeros.
3. **`beamlet-heap-flood`** (the attack): the module allocates without limit.
   - The verdict is the system's (rule F): `init`'s line that `beamlet` ended, and no other
     server ended.
   - After that, a second program in the same manifest, started after `beamlet` ended,
     still gets its console line through `consoled`. Use a `tests/programs` binary that
     prints and exits.
   - `forbid`: `init: rebooting` and any other server's end.
   - Report how the VM ended. An `OutOfMemory` raised as an Erlang error that kills the VM
     is fine; a panic in the platform is a finding, so report it, don't hide it.

rv32: try the cross-build of `beamlet` for rv32 once case 1 passes on rv64, and report what
breaks. Don't fix it. Whether beamlet runs on rv32 is the owner's question, and I put it to them
with your report.

## Page lines (exact; each in the commit that makes it true)

- **beamlet.md, "beamlet on Redoubt":** "Status: planned · M1 (separation and containment)"
  becomes:
  > Status: built · partly tested: files, programs, `/net` and the natives are not built, and the
  > modules are read from `/boot` unchecked; rv64 only · tested: bench:beamlet-boot,
  > bench:beamlet-console, bench:beamlet-heap-flood

  If doccheck wants the `<details>` form for three or more tests, use it, with the same clauses.
- **beamlet.md, "beamlet on Redoubt",** after the table's paragraph that begins "TCP is Plan 9's
  `/net`", add:
  > On the machine, beamlet is the program `beamlet`, started like any other with a console, a
  > budget and a connection to `bootfsd`. It runs one scheduler thread until several harts
  > ([several harts](../plan/m2-usable-shell.md#several-harts)), and starts its threads with the
  > runtime's `thread::spawn`.
- **beamlet.md, "The console, the clock and randomness":** in its status, "it runs in no boot"
  becomes "it runs in a boot in bench:beamlet-boot and bench:beamlet-console". Keep the rest of
  the clause.
- **beamlet.md, the same section's last bullet:** "On the fake kernel it is seeded from the host
  for a person's run, and fixed for a test's, so a test repeats." Add after it: "On the machine it
  is the kernel's own."
- **native.md:** the runtime's table gains a row:
  > | `thread` | `spawn`: a closure on a thread of this process, with a stack from `map_anon` that outlives it |
- **testbench.md, "The case file":** the `programs` example gains the workspace field, in the
  form the code takes. Send me the line.
- **The "Open" under "beamlet on Redoubt"** (the timer's counter frequency): if `time_now`'s
  microseconds served every case, say so in the report, and I close it.

## Owned paths

- `userland/otp/redoubt/**` (the `beamlet` binary, the machine's `Threads` and `Modules`, the
  test modules), and `userland/otp/Cargo.toml` and `Cargo.lock` as the build needs.
- `libs/rt/src/thread.rs`, and one `mod` line in `libs/rt/src/lib.rs`.
- `tests/beamlet-*.toml` and their manifests under `tests/data/beamlet/`.
- `tools/testbench/src/{case.rs,build.rs}`, only for the workspace field and the test modules'
  compile step.
- `tests/unsafe-budget.toml` (the one line) and `tests/size-budget.toml` (beamlet-redoubt's row,
  if it has one).
- The page lines above.

**Not yours; ask first:**
- `libs/rt/src/server/**`: RT2's.
- `servers/fsd/**`: FSD3's.
- `userland/otp/vm/**`: beamlet's interpreter. If the VM needs a change to run `no_std` on the
  machine, stop and tell me what and why.
- `servers/init`, the kernel and `libs/client`.

## Gates

- `beamlet-boot`, `beamlet-console` and `beamlet-heap-flood` on rv64.
- The whole bench on both widths, alone (one whole bench at a time).
- The testbench's host tests, `rt-host-tests`, and beamlet's own host tests (`cargo test` in
  `userland/otp`, the `fake` feature included).
- `vendor-check`, `cargo fmt --check`, and the size and unsafe budgets.
- doccheck.

Report each command with its exit code. The report lists:
- each case's lines and result;
- the binary's size and the pages used;
- the unsafe line;
- the rv32 attempt;
- each page line, as written.
