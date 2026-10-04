# STEWARD1: the steward core's Elixir reference, a differential oracle in the bench

Tier A (the bench and the steward's policy), size M. Design: steward.md "Two embedders and a
reference" (the Elixir reference), QA STEWARD0-design, and STEWARD0's merge (22b659d9e). Needs
STEWARD0 (merged). Read steward.md "The policy core" whole first: it is the contract, and the
Rust core (`libs/steward`) is the authority. The reference is never authoritative.

Every cargo and bench command on this host runs inside the dev container:
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from your worktree.

## What it builds

1. **The toolchain where the bench runs.** GETTING-STARTED.md "beamlet" says the pinned OTP
   28.5.0.6 and Elixir 1.20.4 are in neither the repository nor the container. They live in the
   untracked `toolchains/`, found by `userland/otp/tools/env.sh` and `BEAMLET_TOOLCHAINS`. A bench
   case must run in `cargo testbench` in the dev container, so the container needs them.
   - Find how the dev image is built (`./dev.sh`).
   - Propose, before anything else, how it gets the two pinned toolchains, pinned by version and
     checksum.
   - **Checkpoint 1: stop and report this proposal.** A changed dev image is a project-wide
     choice, and I may take it to the owner. A case that SKIPs where the toolchain is missing is
     not acceptable as the result: an oracle that does not run catches nothing.
2. **The trace format** (`libs/steward/trace/`, a host-only crate outside the shipped core, or
   tests; say which and why):
   - an input of events, one per line, a canonical text encoding of `Event` (now, random words,
     reply, kind);
   - an output: after each event, the effects in order, the audit records, and a canonical dump
     of the store read through `inspect` (domains, index, fixed, the exited flag).
   - Both sides write the output, and the check compares it byte for byte. Specify the encoding in
     a short page section (steward.md, under "Two embedders and a reference", as `####`; this page
     allows only its fixed `##` headings).
3. **The Elixir reference**, `libs/steward/elixir/`. It sits beside its tables, as the wire
   codec sits in `libs/wire/elixir`, not in `userland/shell`'s Mix tree. The shell is Tier B and
   ships on the box. The reference is a test oracle bound to the core's tables, never on the box.
   Compile it with `elixirc`, with no Mix project and no Hex dependencies.
   - `decide/2` as multi-clause functions over a `defstruct` state.
   - The generator writes its clause skeletons from the same tables, as
     `libs/steward/elixir/gen/<machine>.ex`. They are checked in, and the drift check covers them.
     This is the half STEWARD0 left: steward.md says "with the Elixir reference it writes that
     reference's clause skeletons too".
   - The guards, effects, `render` and the binding hash are written by hand in Elixir, from the
     page and the tables, not translated line by line from the Rust: an oracle that copies the
     code copies its bugs.
   - The hash is SHA-256 via `:crypto` on beamlet.
4. **The traces.**
   - Hand-written traces that together take every row of every table at least once. Use the
     generator's row list to check coverage, and fail on a row no trace takes.
   - Traces recorded from the model's families (P1 to P16) at a fixed seed: the model writes the
     events it drives.
5. **The bench case.**
   - A kind (or a host-tests mechanism) that compiles the reference with the pinned `elixirc` and
     runs it on beamlet (`userland/otp`, built as `userland/shell/setup.sh` builds it), over
     every trace, comparing with the Rust core's output.
   - Run the plain host beamlet with a code path, as `userland/shell/setup.sh` sets it up, not
     the fake-kernel platform (`fake`): the reference is pure and does no I/O, and the fake
     kernel is for beamlet's Redoubt platform. Read traces from files and write output to
     stdout.
   - **A divergence fails the case**, and the report names: the trace file; the index of the
     first event whose output differs, and its input line; the first differing line from each
     side (Rust's, then Elixir's), in the canonical encoding; and the rest of that event's
     output from both. Stop at the first divergence in each trace and go on to the next trace,
     so one run shows every trace that differs.
   - The same case runs `libs/wire/elixir/run-vectors`. Its default beamlet path is stale: it
     looks for `../beamlet`, but beamlet is `userland/otp` now.
   - **The harness can fail.** A host test shows that the comparison fails on a changed output
     line. Also record one negative run, with one Elixir guard broken, that the case catches.

## Pages (they move with the code)

- steward.md "Two embedders and a reference": the reference goes from planned to built, with its
  tests in the status block, and the trace encoding is added.
- wire.md: the "partly tested" status line (`run-vectors`, which no bench case runs) and its
  residual "The Elixir codec's differential run is outside the bench" go.
- testbench.md: the new kind or mechanism, under "Cases".
- GETTING-STARTED.md "beamlet": what the container now provides.
- The size budget only if a counted crate grows.

## Owned paths

- New: `libs/steward/elixir/`, `libs/steward/trace/` (if a crate), the traces, the new case
  file.
- Changed: `libs/steward/gen` (the Elixir backend), `libs/wire/elixir/run-vectors`, the bench's
  case-kind code in `tools/testbench`, the dev image files once checkpoint 1 is answered,
  `model/` only to record traces, the pages above, `Cargo.toml` members, `Cargo.lock`.
- Anything else is a question to the orchestrator. A gap the page does not settle is a QA
  question to the Architect, never improvised.

## Acceptance

- `cargo testbench` whole, with the new case running (not skipped) in the dev container on a
  fresh checkout.
- Every row of every table taken by a trace.
- Rust and Elixir outputs equal on every trace.
- The negative run caught.
- The report says how long the case takes, every difference found between the Rust core and the
  reference and which side was wrong, and any row the families never reach.
