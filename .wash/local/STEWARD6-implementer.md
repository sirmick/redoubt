# STEWARD6: the steward's calls to shared servers inside a batch are bounded

Node STEWARD6 (parent AGENTS). Tier A (the steward). Size S. Reviewer: steward-red-4. Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "F3"; evidence `.wash/local/B88-report.md` ("Cause").

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; the fmt gate is
  `make -f scripts/jobs.mk set CASES=formatting`, the docs gate `set CASES=docs`. No edit of any
  kind while prebuilt builds or cases run ("tree changed").
- Messages at most 2000 bytes; detail in a file under `.wash/local`. Read question files and
  reports by section, never whole; never read a QA file's base64 tail.
- Pages carry no dates, package ids or decision ids; a built section has no `**Open:**`; every
  `### R<n>` has a SECURITY.md row; a number on a page shows the arithmetic that produces it.
- A miss is a finding, not a tuning: no target moves; a bound is measured once on both widths,
  never guessed. The core (`libs/steward`) is not touched here.
- STEWARD5 follows you in `servers/steward/src/bin/steward.rs`: land small and clean.

## The symptom

`servers/steward/src/bin/steward.rs`: `unattached(name)` returns `Connection::new(..)`
(`libs/rt/src/client.rs:122`: `within(endpoint, FOREVER)`), so every `new_connection` the
steward mints at a shared server (`fresh`, `carve`, `fresh_carved`) waits without bound;
`server(name)` attaches with `Connection::attach`, the same `FOREVER`, so the launch step's
reads of the beamlet image from `erofsd:system` and a push's `stat` and read of its source wait
without bound too. Its disconnects are bounded (3Y, `CLEANUP_US`) and its console writes are
(3U, `CONSOLE_US`). B88's block-trace: the steward's longest call out was 1.43 s under two
spinner VMs loading code; a server that never answered would hold the loop for ever, serving
nobody, with `init` seeing the steward alive.

## The rule

Every call the steward makes to a shared server inside a batch is made within one bound,
`SERVER_US`: a mint by `new_connection`, the launch's image reads, a push's `stat` and read, a
crossing's mint. A call the server did not take within it fails the step; the batch's existing
failure path answers the caller (`failed` to `start_agent` and `submit`, a refused login), the
core rolls the batch back, and nothing inside the steward retries: the sponsor or the person
does. 3W's distinction stands: a mint's `Timeout` wakes no parked step (`minted` already maps
`Sys(Timeout)` to `Timeout`).

The bound is measured, not guessed. Run `steward-latency` (seed 3, both widths, one and two
harts, `--keep-failed`) with a temporary line per call out (`steward: <server> <op> took N us`,
removed before the commit, or the kernel's `block-trace` feature B88 added), take the worst
per-call wait, and set `SERVER_US` at twice it rounded up to a second; state the measurement and
the arithmetic on the page. Expect the order of 3 s (B88's 1.43 s); if the arithmetic gives a
different number, that number stands. `HELLO_US` (2 s) and `RELAY_US` (1 s) are the precedent
for a bound on the loop's own calls.

## Tier A invariants the reviewer checks

- No call the steward makes to a shared server inside a batch waits `FOREVER`; the kept
  connections (`servers`, `carved`) are created `within` the bound, so every later call on them
  is bounded too.
- A step the bound fails is a failed step and nothing else: no partial grant survives the
  rollback (the core's existing path), no retry inside the steward, a console line says it in
  the batch's words (`a step failed (Timeout)`).
- The bound is never reached on a quiet box: `steward-latency` N = 1 and the steward set keep
  their figures and verdicts.

## Cases

- Host (`servers/steward/tests/steward.rs`, the Recorder): a server that never answers a mint
  fails the login's step within the bound and the next caller is served; the launch's image read
  the same (`start_agent` answers `failed`); a push whose source server never answers answers
  `failed`; a mint answered after the bound wakes no parked step.
- Bench: the steward set (`steward-ssh-two-principals`, `steward-lease-ends`, `steward-end-lease`,
  `steward-declassify`, `steward-push`, `steward-server-restart`, `steward-blame-lockout`,
  `init-boot`), both widths, unchanged in verdict; `steward-latency` N = 1 figures unmoved (run by
  name; its rows at N = 3 and 4 are STEWARD5's to move, not yours).

## Pages

- steward.md "Residual risks": beside "A context's relay holds the steward's one thread for a
  bounded time", the new sentence: a call to a shared server inside a batch holds the loop at
  most `SERVER_US`, the measurement and the arithmetic; the step fails and the caller is told.
- steward.md "Authentication and sessions": one clause: a login whose server does not answer in
  time is refused and the person retries. agents.md "Leases": one clause for `start_agent`.
- Summaries checked for the claim (README.md, GETTING-STARTED.md, docs/servers/README.md,
  m3-agents.md): say which you read and that nothing moved.

## Owned paths

`servers/steward/src/bin/steward.rs` (the connection constructors and `SERVER_US`), its tests;
docs/servers/steward.md, docs/userland/agents.md. Not `cleanup.rs`, `console.rs`, `drive.rs`
beyond what the bound needs, not `libs/steward`, not `libs/rt`.

## Gates

Host tests (`cargo test -p redoubt-steward-server`); release builds rv64 and rv32 with and
without `latency-probe`; the bench cases above both widths; `set CASES=formatting`, `set
CASES=docs`, `size-budget`, `unsafe-budget`. Report each with its exit code, the measured
per-call waits with their logs' paths, and the arithmetic that set the bound.

## Checkpoint

After the measurement, one progress line as an `instruction`-class message to the orchestrator
is not needed; put the worst wait and the bound in the report. Ask on a QA thread only if the
measurement gives a bound the page cannot state in one sentence.
