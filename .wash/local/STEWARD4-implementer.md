# STEWARD4: declassification and push, and the latency targets measured with the real steward

Tier A (a trusted system server, and the responsiveness targets). Size M. Needs STEWARD3 and
K19 (the destruction path as it will stay; a lease's end is measured against it). Don't start
until the node's needs are met.

Two halves: the last of the core's events, `Submit` with `Declassify` and `Push` content and the
crossing machine's batches; and the responsiveness targets re-measured on a boot where the real
steward, not a stand-in, decides and destroys.

Run everything natively on this host, under the job pool's rules (docs/testbench.md "On a shared
host"). Timing cases are guest-time cases (icount, a pinned seed): they share the pool; their
`timeout_secs` expiry alone is no verdict.

## Context rules (read these first)

- **Don't read whole files.** steward.md by section; `libs/steward/tables/crossing.md` whole.
- **Don't open `.wash/qa/*.md`.** STEWARD3's report: its batch-runner section only.
- **Pipe bench output;** read boot logs through `grep` or `tail`; the latency logs begin with
  hex dumps and hold thousands of `SCHED-TRACE` lines.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1900 bytes,** detail in `.wash/local/STEWARD4-report.md`.

## Reading list (only these)

- `docs/servers/steward.md`: "Declassification and push", R42, "Guards and effects" (the
  `DECLASSIFY_MAX`, the crossing's rows: read, copy out, write), "Machines" (the crossing).
- `docs/servers/init.md`: "The confinement check" (the reader and writer budgets as the named
  exception's edges).
- `docs/kernel/ipc.md`: R1 (a labelled budget answers, never starts; the `system` class).
- `docs/kernel/scheduling.md`: "Responsiveness" whole (the targets, the workload, the sweeps),
  R12's residual "The steward decision wake is late by whole slices".
- `tests/sched-latency.toml` and `tests/programs/src/bin/sched-latency.rs`: the stand-ins and
  what each measures; `tools/testbench/src/sched_oracle.rs`: the post-check's targets (by
  symbol: the `post_check` argument names).
- `libs/steward/tables/crossing.md`; `libs/steward/src/effect.rs` (`Read`, `Write`, `Bytes`).

## The design

### Declassification and push

1. **Declassify**, from a session carrying exactly the item's label, by the label's owner. The
   core's `Submit` opens a **read crossing**: `CreateBudget` with exactly the item's labels and
   a deadline (a short one: the core's `crossing` size), `Read { through: reader }`: the steward
   `call`s a reader program in that budget, which reads the item from the labelled volume's
   `fsd` and fills the steward's lend; the steward, unlabelled, never reads the volume itself
   (R1: the labelled budget only answers). The reader is a small native program in the bundle
   (`crossing-reader`: open, read whole, reply; refuse over `DECLASSIFY_MAX` + 1 bytes so the
   lend is bounded), launched through the stub into the crossing budget with one connection,
   the volume's, rooted at the item. `DestroyBudget` closes the crossing. The core checks size
   and printability (`item_fits`), hashes the snapshot and freezes the request; the approval
   screen shows the whole item (STEWARD3's channel). On `Approve`, the **copy out** writes
   exactly the snapshot (`Bytes::Literal`, never a re-read: `PolicyDeclassifyLive`) to the
   unlabelled volume through the steward's own `fsd` connection (`through: None`).
2. **Push**, from an unlabelled session of the target label's owner: the steward reads the
   source itself (`through: None`; it is unlabelled), snapshots and hashes it; the screen shows
   source, target, size and hash, not the bytes; on approval, a **write crossing**:
   `CreateBudget` with exactly the target label set, `Write { through: writer }`: a
   `crossing-writer` program (the reader's twin) takes the snapshot in the steward's lend and
   writes it to the labelled volume, then the budget is destroyed. A labelled session's mount of
   a shared unlabelled volume in a confined deployment is declined with the push offered: the
   steward's namespace build in STEWARD2 gains that refusal when `init`'s manifest is confined
   (the `confined` key, init.md).
3. The crossing budgets are the confinement check's named exception's edges: each carries
   exactly one label set and dies after one item; the steward's audit line for a crossing names
   the labelled side's domain.

### The targets with the real steward

4. **A new boot case, `steward-latency`**, under `init` with the real steward: alice's
   session starts N agents (1, 4, 16, as `sched-latency`'s sessions) each a spinning VM in a
   lease, and the steward is driven by the case's `end_lease` calls and lease deadlines. It
   measures what the stand-in measured, from the steward's side: the decision wake (the steward
   runs again after its timeout; the server exposes it as the time from the deadline it armed to
   its `time_now` at wake, printed as the stand-in prints it), R10's cost per destroy (the
   trace), the deadline notice (the `killed` notice's arrival against the lease's deadline), and
   a lease's end from the decision (decision wake + destroy). The targets are
   scheduling.md's, unchanged: decision wake p50 25 ms / p99 95 ms, deadline notice p99 40 ms,
   R10 p99 30 ms, a lease's end p99 125 ms, judged by `sched_oracle` net of audits as today, on
   one pinned seed with the target stated from a seed sweep. `sched-latency` (the stand-ins)
   stays: it is the kernel's case and runs without the servers.
   - A miss is a finding, not a tuning: stop, keep the run, and return it on the thread with the
     trace's account of where the time went (slices behind sessions, the steward's own work per
     request, audits). No target moves here.
5. **The steward's work per request** is bounded and reported: the case prints the steward's
   time per `end_lease` and per `Submit` (its own `time_now` brackets), and the report states
   the worst as a residual on steward.md ("Steward work is paid by the steward").

### The rules it keeps

R42 (one approved item, exactly the snapshot, through a reader carrying its labels), R1 (the
reader and writer only answer), R34 (a confined deployment's push is the only input path), R12
and the Responsiveness targets (unchanged numbers, now with the real decision-maker).

## The cases (both widths; system verdicts)

1. **`steward-declassify`** (`[[session]]`): alice's vault session declassifies a 40-byte text
   item; `approve@box` shows it; on approval it appears on the unlabelled volume byte for byte
   (the case reads it from alice's unlabelled session); the reader budget is gone (the audit).
   A 300-byte item and a binary item are refused at submission (`TooBig`, `NotPrintable`). The
   vault session modifies the item after submission and before approval: what lands is the
   snapshot (the kernel's and `fsd`'s lines, the case's byte compare).
2. **`steward-push`**: alice's unlabelled session pushes a 4 KiB file into `{alice-secrets}`;
   the screen shows size and hash, not bytes; on approval the vault session reads it whole; the
   writer budget is gone. A vault session's `submit` of a push is refused (`exact_labels`); bob
   cannot push into alice's label (`NotOwner`).
3. **`steward-confined-push`**: the confined image (init's `confined` manifest): a vault
   session's mount of the shared unlabelled volume is declined and the push is the way in.
4. **`steward-latency`**: point 4, on a pinned seed, both widths, with its seed sweep recorded
   in the report (`--sweep 1..20` under the pool).
5. **Host:** `crossing-reader`/`writer` refuse an oversized item and a second request; the
   crossing batch's rollback (`Failed` → `Closing` → `Closed`, STEWARD0's C11) destroys a
   partial crossing.

## Page lines (exact text in the report)

- **steward.md:** "Declassification and push" and R42 statuses to `built · tested`; the
  residual "Steward work is paid by the steward" gains the measured worst per request; the
  Residual "The steward decision wake" entries on scheduling.md gain the real-steward numbers.
- **scheduling.md** "Responsiveness": a paragraph "With the real steward" naming
  `steward-latency`, its table of p50/p99 at N = 1, 4, 16 beside the stand-in's, and the
  sweep; the targets table unchanged. R12's status gains the case.
- **init.md:** "The confinement check" status: the steward's half built with case 3.
- **SECURITY.md:** R42's row; R12's row gains `bench:steward-latency`.
- **testbench.md:** the `steward-latency` case where `sched-latency` is described.

## Owned paths

- `servers/steward/**`; `servers/crossing-reader`, `servers/crossing-writer` (new, small; or
  one program with a mode: say which and why); `tests/programs` for the latency case's agent
  module; `tools/testbench/src/sched_oracle.rs` only to accept the new case's lines (the
  targets' names unchanged).
- The cases and pages above.

**Not yours:** the core's rows and constants; the kernel; `sched-latency` and its targets;
`fsd`.

## Gates

The whole bench on both widths under the pool's rules, `steward-latency` on both; the host
tests; `elixir-oracles`; fmt; the unsafe ratchet; the size budget (two new programs); doccheck.
Report each command with its exit code, and the sweep's table.

## Not here

The audit file and its signing (M3, M4); retention and chaining (M5); a target change of any
kind.

## Checkpoint

After `steward-declassify` is green on one width, one progress line with the branch; and again
with `steward-latency`'s first full run (pass or miss) before any sweep.
