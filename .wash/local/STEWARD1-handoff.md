# STEWARD1 handoff (implementer 1 → next), 2026-10-02

Branch wp-steward1, worktree /home/mcloonan/redoubt/.worktrees/steward1, rebased on main
(ca5a6437b). The image commit is on main (87581a5e2): in-dev now runs rev 4, with OTP 28.5.0.6 and
Elixir 1.20.4 under /opt/toolchains (BEAMLET_TOOLCHAINS). Checkpoint-1 record:
.wash/local/STEWARD1-toolchain.md.

## Done (commits on the branch, oldest first)

- 117eeb1b3 steward-gen: the Elixir backend. `libs/steward/gen/src/lib.rs` `elixir(m)` writes
  `libs/steward/elixir/gen/<machine>.ex`. Each has `rows(from, event)` clauses: row data
  `{line, guards, effects, to}` with the table line, or `:no_row`. The drift check and the orphan
  check cover the directory. This is a clean commit.
- 34f931bcb WIP: the trace crate `libs/steward/trace` (`redoubt-steward-trace`, host-only, a
  workspace member) and the Elixir reference `libs/steward/elixir/{steward,guards,effects,render,trace}.ex`.
- c71328da8 WIP: 13 hand-written traces in `libs/steward/trace/traces/`, and the reference's
  `done` line.
- af9387c8b WIP: the model records events. `Steward.recorded` holds them, and `policy.rs` has
  `steward_policy_events`/`steward_noninterference_events` (refactored into
  `policy_sequence`/`noninterference_runs`). `model/examples/steward-traces.rs` writes the trace
  files: seeds 1–4 for policy, 1 for noninterference. The trace crate has `record.rs`.
- Fold the three WIPs into logical commits before acceptance (SWARM rule 10).

## Verified

- `cargo test -p redoubt-steward-trace`: 5 pass, among them `a_changed_output_line_is_caught`.
  `cargo test -p redoubt-steward-gen`: 10 pass.
- Dev loop scripts, untracked, in target/: `target/steward-dev.sh` runs the reference on host BEAM
  and records the model traces; `BREAK=guard` gives the negative run.
  `target/steward-beamlet.sh` runs it on beamlet. Run each as
  `/home/mcloonan/redoubt/.wash/local/in-dev target/steward-dev.sh`.
- All 19 traces (13 hand, 6 model) are equal Rust vs Elixir, on BEAM and on beamlet. Beamlet takes
  about 0.12 s for the hand traces and 1.2 s with 16 model seeds.
- Every row of every table is taken by the hand traces. The check fails on an untaken row.
- Negative run, `BREAK=not_locked`: caught. It diverges at blame.trace event 11, where Rust gives
  StartFailed and Elixir AgentStarted. Exit 1.

## Findings for the report

1. lease.md row 15 (`- StartAgent !not_locked`) cannot be taken within the embedder's guarantee.
   A lockout ends every routed caller in the domain, and nothing is routed there again until the
   window passes. blame.trace takes it only with `now` going back, and says so. Report it as a
   design note (a defence-in-depth row); it may be worth a QA question to the Architect.
2. The model's families at seeds 1–4 never reach 42 rows; 16 seeds reach 39 of them, no better.
   They include crossing rows 14–24 (copy out, write), request 42/43 (approved declassification
   and push), lockout rows, and the unreachable rows. List them from the `never reached` lines of
   `steward-trace check`.
3. No divergence was found between Rust and Elixir. The first runs only caught harness bugs in the
   reference.

## Next steps

1. **Bench mechanism.**
   - Add a new case kind in tools/testbench (say `elixir`) that runs scripts and judges their exit.
     Before them it checks the toolchain: source userland/otp/tools/env.sh; `erl` must exist,
     `releases/28/OTP_VERSION` must be 28.5.0.6 and `elixir --version` must say 1.20.4. A miss
     FAILS, never SKIPs (Architect's condition 2).
   - New case file `tests/steward-elixir-reference.toml` (or similar). It runs:
     - `libs/steward/elixir/run-traces` (to write: like target/steward-dev.sh, but on beamlet,
       built with `RE_ENGINE=rust userland/otp/tools/build-beamlet`; the -pa paths are stdlib,
       kernel, erts, crypto and compiler, plus the elixir ebin; see target/steward-beamlet.sh;
       supports `--break GUARD`);
     - `libs/wire/elixir/run-vectors`, after fixing its default BEAMLET_DIR to `$repo/userland/otp`
       and its doc comment path (`redoubt/wire/...` is stale too).
   - Add `redoubt-steward-trace` to tests/steward-host-tests.toml's packages.
2. **Pages.**
   - steward.md "Two embedders and a reference": the reference planned → built. Add the tests to
     the status block of "The policy core" (status lines are checked by doccheck; see testbench.md
     on test IDs). Add a `####` "The trace encoding" section (the grammar is in
     `libs/steward/trace/src/{input,output}.rs`; describe input lines, output blocks, `hash=shown`,
     `row`/`done`).
   - wire.md: drop the "partly tested" status and the residual.
   - testbench.md "Cases": the new kind's row.
   - GETTING-STARTED is already done in the image commit.
3. **Gates.**
   - Size budget: model/src grew by about 25 lines over its max 10158, so raise tests/size-budget.toml
     model max_lines to the new count.
   - No unsafe added.
   - fmt: `in-dev cargo +nightly fmt --all --check`.
   - doccheck: `in-dev cargo run -q -p redoubt-doccheck`.
   - `in-dev cargo testbench --allow-skip` on both widths: exactly one SKIP
     (bench-ssh-loopback-openssh), and the new case RUNNING.
4. Fold the WIPs, then report: the case's time, findings 1–3, and the negative run.

## Traps

- Elixir: `quote` is a special form (the helper is `qs`), and `String.slice` counts graphemes, so
  use `binary_part`. beamlet prints start/0's return value after the output: the `done` line
  handles it.
- Maps: the reference sorts every iteration explicitly (`Enum.sort`) to match BTreeMap order.
- The model is no_std. Recording is a per-Steward field, not a global.
- in-dev mounts the main checkout at /work; /tmp on the host is not visible inside. Use target/.

## Additions (orchestrator's checklist)

Branch tip: af9387c8b, everything committed, tree clean apart from untracked target/. The private
tag redoubt-dev:steward1 is removed.

### Brief deliverables

| # | Deliverable | Status |
| --- | --- | --- |
| 1 | Toolchain in the image | done, on main 87581a5e2 |
| 2 | Trace format | in progress: the code is done, the page section (`####` under "Two embedders and a reference") is not written |
| 3 | Elixir reference + generated skeletons | done (WIP 34f931bcb; skeletons in clean 117eeb1b3) |
| 4a | Hand traces + coverage | done (WIP c71328da8) |
| 4b | Model-recorded traces | done (WIP af9387c8b) |
| 5 | Bench case | not started; host test of the comparison done; negative run done by hand via BREAK |
| — | Pages (steward.md, wire.md, testbench.md) | not started |
| — | Size budget, gates, fold | not started |

### The Architect's five conditions

1. **Met.** Versions and sha256s are pinned once, as ARGs; `sha256sum -c` gates the build, with
   no fallback.
2. **Not yet.** The bench kind's toolchain check is still to write: it must FAIL on erl missing,
   OTP_VERSION not 28.5.0.6, or `elixir --version` not 1.20.4.
3. **Met.** The evidence (`command -v erl` → /opt/toolchains/...) is in STEWARD1-toolchain.md.
4. **Met.** GETTING-STARTED and the Dockerfile header are in the image commit.
5. **Met.** The builder deps are absent from the final image; +178 MB layer; 135 s build.

### Trace encoding decisions, and why

- **One crate, host-only.** `libs/steward/trace` is a crate, not tests. It needs a bin the bench
  script calls (`steward-trace run|check`) and a writer the model example uses. It lives outside
  the shipped core (std), so the size and unsafe budgets of `redoubt-steward` are untouched.
- **Input.** Manifest lines (`principal`, `keyd`, `servers`, `sizes`), then one line per event:
  `event now=N random=[..≤8, zero-padded] reply=N Kind field=value...`.
  - Objects are `kind@account/labels#id`.
  - Strings are quoted, with the escapes `\" \\ \n \xNN`.
  - A Done's result is `ok=[budget(n),scope,connection,process(n),bytes("..."),done]` or
    `failed=step,error`.
  - `hash=shown` takes the hash of the last screen shown for that request, so hand traces need no
    SHA by hand. Both sides substitute it the same way.
- **Output.** A `boot` block, written once: principals, fixed, carves. `fixed` never changes, so it
  is not repeated per event. Then per event: `event N`, the outputs in order, the batches and their
  steps, `exit`, and `store` followed by a full dump through inspect. The dump covers domains in
  manifest order with objects by id, routes, ids, channels, and `exited`.
  - Each side writes its own output; the check compares it byte for byte.
- **Coverage.** The reference also emits `row MACHINE LINE` for each row it takes, using the table
  line from the generator. These lines are not compared output. The Rust check matches them
  against the generator's row list. Coverage therefore comes from the reference: the Rust core
  has no row hook, and adding one would change the shipped core. Since the outputs are equal on
  every trace, the same rows were taken.
- **End marker.** The reference ends with `done`; the check refuses a run cut short. beamlet
  prints start/0's value after the output.
- **Model traces.** These are recorded fresh on each run by the example, not checked in, so they
  never drift from the model.
- **Negative run.** A `break` file in the traces root names one guard, which the reference then
  holds always.

### First thing for implementer 2

Write `libs/steward/elixir/run-traces`, from target/steward-dev.sh plus target/steward-beamlet.sh,
then the bench kind with the toolchain check (condition 2) and the case file. Then do the pages,
the gates, and the fold.
