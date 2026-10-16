# STEWARD4: declassification and push, and the latency targets measured with the real steward

Refreshed brief (architect, 2026-10-16), replacing `.wash/local/STEWARD4-implementer.md`
(2026-10-05), which predates the whole STEWARD3 step. Read this file, not the old one; the old
one stays for the record. Node STEWARD4 (needs STEWARD3, K19); Tier A; size L; reviewer
steward-red-2.

## What changed from the 2026-10-05 brief

- The steward's second half is on main: scope-bound connections (STEWARD3R/3S: every 9P
  connection and ipd's grant minted in the session's or lease's scope; R41 built), leases and
  agents (3A: `start_agent`, `end_lease`, the sponsored `agent` principal, the `notices`
  endpoint, `sizes.agent` with `quota`, the 0-quota sub-root rule, a nested lease refused), the
  admission cap from the manifest (3Q), crash blame and the lockout (3C: `blame` on init's class,
  badge 3; `LockedOut`), the powerbox at `approve@box` (3B: `submit`, the pending cap and
  `fair_share`, the approval channel with its reader threads, the screen hash and its 8-hex code,
  `approve`/`deny`, `approval_waiting` 18), and server-restart re-binding (3X, train 45:
  `reconnected` 19, `server_restarted` 20, `rebind` 21, `SERVER_TIMEOUT` 3 s, a lease not
  re-bound). STEWARD3Y (deferred end-batch cleanup) follows as a small package and is this
  package's one hotspot.
- So the old brief's assumptions go: the approval channel exists (its screen is the core's
  `Rendered`, printable ASCII, with the hash and code); `Submit`'s `Content::Declassify` and
  `Content::Push` and the `crossing` machine's rows exist in the core and are traced (`request.md`
  rows 24-27, 42-43; `crossing.md`); the wire's `submit` (opcode 9) already carries `kind`,
  `labels`, `lease`, `item`, `source`, `what`, `reason`; the next free opcode is **22**.
- The stand-in latency case is unchanged, but what "a lease's end" measures moves with 3Y: see
  "The targets" below.

## Context rules (read these first)

- `.wash/local/implementer-context-sinks.md`'s rules as every launch: no whole-file reads of QA
  files, no `--list`, no boot-log hex; every build and boot through `q run` / the `make -f
  scripts/jobs.mk` targets; no edits while prebuilt cases run; messages at most 2000 bytes.
- Pages carry no dates, package ids or decision ids; a built section has no `**Open:**`;
  `mutation:` names must exist in `model/src/mutation.rs`; every `### R<n>` has a SECURITY.md row.
- The core's guards, rows and constants are the Architect's: a case that needs one changed is a
  question on the thread, not an edit. The core, its Elixir oracle and the model move together
  when they move at all (STEWARD0's rule); here they should not move.

## Reading list (only these)

- docs/servers/steward.md: "Declassification and push", "The powerbox and approvals" (as
  built), "Leases", "Failure and restart", R41, R42; "The steward's protocol".
- libs/steward/tables/request.md, crossing.md (the rows you drive), README.md (the machines'
  events); libs/steward/src/event.rs (`Content`), consts.rs (`DECLASSIFY_MAX` 256, `FIELD_CAP`),
  render.rs (what the screen shows), edges.rs (the crossing's edges).
- servers/steward/src: protocol.rs (routing by class; `submit`'s decoding), drive.rs (steps:
  `CreateBudget`, `Connect`, `Launch`, `DestroyBudget`, the `made` tokens), the Machine (the
  batches, `Kernel::connect(owner, slot, scope)`), approval.rs (the channel).
- libs/wire/tables/steward.md (opcodes 1-21 after 3X); docs/servers/serving.md "`admit`",
  "Multiplexed connections", R41's scope rule; docs/servers/init.md "The confinement check",
  R34 (confined placement), "Fresh connections per child".
- docs/kernel/scheduling.md "Responsiveness" (the targets and how they are judged);
  tests/sched-latency.toml and tests/programs/src/bin/sched-latency.rs (the stand-ins);
  tools/testbench/src/sched_oracle.rs (the post-check, net of audits).
- .wash/local/STEWARD3A-Q2-ruling.md (leases, the agent VM, `sizes.agent`), STEWARD3B-ruling.md
  (the channel, the code), STEWARD3R-ruling.md (scopes; the cutoff/cleanup rule),
  STEWARD3X-ruling.md (restart; the end-batch stall), the STEWARD3Y node body (plan.toml).

## The design

### Half 1: declassification and push

1. **Items.** The core names an item by `item: u64` and a push's source by `source: u64`; the
   server owns the mapping: `submit`'s `what` carries the path (a declassification: the item's
   path inside the vault, relative to the vault's root; a push: `source` is the path in the
   submitter's unlabelled home, `what` the target path inside the vault), and the server draws
   the item ids and keeps path beside id for the request's life. The core hashes `Content` (ids)
   and the snapshot; the binding the person approves is therefore the bytes, and the screen shows
   the path as the requester's text (`FIELD_CAP`, marked). Where a declassified item lands: the
   submitter's unlabelled home, `~/out/<name>`, `<name>` the item's last path element; an
   existing file there refuses the submission (`in_use`), never overwrites.
2. **Declassify** (`Content::Declassify`), from a session carrying exactly the item's label set,
   by the label's owner (the core's guards). `Submit` opens a **read crossing**: the crossing
   machine's `Open [reading]` → `carve_crossing` (CreateBudget with exactly the item's labels, the
   `sizes.crossing` limits and a deadline of the core's), `read_item` (`Read { through: reader
   }`: the steward `call`s a reader program in that budget, which reads the item from the vault's
   walfsd and fills the steward's lend; the steward, unlabelled, never reads the vault: R1), then
   `destroy_crossing`. The reader is a small native program in the bundle, `crossing-reader`
   (open, read whole, reply; refuse over `DECLASSIFY_MAX + 1` bytes so the lend is bounded),
   launched through the stub into the crossing budget with **one connection**, the vault's,
   minted in the **crossing budget's scope** (3R: `new_connection(root, quota, scope)` with the
   crossing budget as scope; root the item's directory, quota 0: it reads), so the crossing's
   destruction ends the connection in every table and the steward's deferred disconnect (3Y) is
   cleanup only. `Done` with the snapshot: the core checks `item_fits` (size and printability),
   hashes, freezes the request, `Rendered` shows the whole item (render.rs:73-75). On `Approve
   [declassifies]`: `open_copy_out`: the copy out writes exactly the snapshot
   (`Bytes::Literal`, never a re-read: `PolicyDeclassifyLive`) to `~/out/<name>` through the
   steward's own kept connection at walfsd:data (`through: None`), below the submitter's home
   carve; `audit_declassified`.
3. **Push** (`Content::Push`), from an unlabelled session of the target label's owner: `Submit`
   → `read_source` (the steward reads the source itself through its kept connection: it is
   unlabelled), snapshot and hash; the screen shows source, target, size and digest, not bytes
   (render.rs:77-81); a pushed item is not held to `DECLASSIFY_MAX` but to the lend's bound
   (`PUSH_MAX`, a server constant stated on the page; propose 64 KiB, one lend). On `Approve
   [pushes]`: `open_write`: a **write crossing** (`carve_crossing` with exactly the target label
   set, `write_item`: `Write { through: writer }`: `crossing-writer`, the reader's twin, one
   connection to the vault's walfsd minted in the crossing's scope, root the target's directory,
   the vault's own quota; takes the snapshot in the steward's lend and writes `what`), then
   `destroy_crossing`; `audit_pushed`. One program with a mode (`crossing read|write`) is
   preferred to two binaries: say so if you keep two.
4. **Confined deployments.** A labelled session's mount of a shared unlabelled volume in a
   confined deployment (init.md `confined`, R34) is declined by the steward's namespace build
   with the push offered: the binding table's slot for a shared unlabelled volume is left
   unbound for a labelled domain when the manifest is confined, and the login's reply says so
   (the shell prints one line). init.md "The confinement check" status moves to built with
   case 3.
5. **Failure.** `Failed` → `Closing` → `Closed` (`destroy_partial`): a reader that exits, times
   out or answers too much destroys the crossing and refuses the request (`failed`); a writer
   that fails after some bytes leaves the vault as it is (the partial file stated on the page:
   the vault's owner sees a short file named as the target; the next push overwrites it: say so),
   `audit_push_failed`. The crossing's deadline is the kernel's cutoff if the program hangs.
6. **Scopes and admission.** Every connection a crossing holds is minted in the crossing
   budget's scope (3R), so nothing of a crossing outlives it; the crossing's one connection
   takes one `in_flight` of the account's bucket (3Q): the cap's derivation gains "+ 1 crossing"
   and the host test's sum follows (the image's 6 becomes 7 if the arithmetic says so: recompute,
   state).

### Half 2: the targets with the real steward

7. **`steward-latency`**, a boot case under init with the real steward: alice's session starts
   N agents (1, 4, 16, as `sched-latency`'s sessions), each a spinning VM in a lease
   (`Redoubt.Agents.Echo spin`, 3A's), and the steward is driven by the case's `end_lease` calls
   and lease deadlines. It measures what the stand-in measured, from the steward's side: the
   decision wake (the steward runs again after its timeout: the time from the deadline it armed
   to its `time_now` at wake, printed as the stand-in prints it), R10's cost per destroy (the
   trace), the deadline notice (the `killed` notice's arrival against the lease's deadline), and
   **a lease's end from the decision**: decision wake + destroy, **ending at the kernel's
   destruction and the notice to the sponsor, not at the cleanup**. With STEWARD3Y the end batch
   replies after `DestroyBudget` and the disconnects are deferred; the measured end therefore
   excludes the deferred cleanup by construction, and the page says so in one sentence ("a
   lease's end is measured to its destruction and notice; the servers' records are freed after,
   off the measured path"). If 3Y has not merged when this case is written, measure the same
   instant (the destroy) from the trace and state that the reply then follows the cleanup.
   Targets are scheduling.md's, unchanged: decision wake p50 25 / p99 95 ms, deadline notice p99
   40 ms, R10 p99 30 ms, a lease's end p99 125 ms, judged by `sched_oracle` net of audits, one
   pinned seed, the target's seed sweep recorded. `sched-latency` stays: it is the kernel's.
   - A miss is a finding, not a tuning: stop, keep the run, return it on the thread with the
     trace's account of where the time went. No target moves here.
8. **The steward's work per request** is bounded and reported: the case prints the steward's
   own time per `end_lease` and per `Submit` (its `time_now` brackets), and the report states
   the worst as the residual's number on steward.md ("Steward work is paid by the steward").

### The rules it keeps, and what a reviewer checks (Tier A invariants)

- **R42 (one approved item):** what leaves a label set is exactly the approved snapshot, read
  through a crossing carrying the item's labels, copied out by literal bytes; no re-read after
  approval; one item per request; `DECLASSIFY_MAX` and printability at submission. Reviewer: the
  copy-out path has no read of the vault; the snapshot is the hashed bytes; the request is frozen.
- **Declassification only through a granted approval:** no step of the core or server writes
  to an unlabelled volume from labelled content except `open_copy_out` after `Approve`; the
  server never runs a step the core did not emit (the drive executes the core's batch and
  nothing else). Reviewer: grep the server for writes to walfsd:data outside the drive's steps.
- **Push crosses only by a grant and only upward into the target's labels:** the writer carries
  exactly the target label set; the write goes to the vault's walfsd through the crossing's
  connection; the source is read by the unlabelled steward; a vault session's `submit` of a push
  is `labelled`, a non-owner's is `not_owner` (the core's guards, unchanged). Reviewer: no path
  by which a labelled session names, pulls or triggers a push.
- **Nothing a session can do that its label set forbids:** R1 (the reader and writer only
  answer; a labelled budget never starts a call), R25 (a labelled connection touches nothing
  outside its set), R41 (every crossing connection minted in the crossing's scope), R34 (a
  confined domain's only input is the push). Reviewer: the crossing's `CreateBudget` labels equal
  the item's or target's set exactly; its connections' stamps are the crossing's; the confined
  refusal is the steward's namespace build, not a shell check.
- **R12 and the Responsiveness targets:** unchanged numbers, measured with the real
  decision-maker; the measured end excludes 3Y's deferred cleanup and says so.

## The cases (both widths; system verdicts)

1. `steward-declassify` (`[[session]]`): alice's vault session declassifies a 40-byte text item
   (`submit` kind declassify, `what` its path); `approve@box` shows the whole item with the hash
   and code; `approve <id> <code>` lands it at `~/out/<name>` byte for byte (alice's unlabelled
   session reads it; the case compares); the crossing budget is gone (the audit's
   `crossing-destroyed`, the kernel's lines). A 300-byte item and a binary item are refused at
   submission (`too_big`, `not_printable`). The vault session modifies the item after submission
   and before approval: what lands is the snapshot. A second declassification to the same
   `~/out/<name>` is `in_use`.
2. `steward-push`: alice's unlabelled session pushes a 4 KiB file into `{alice-secrets}` at
   `what`; the screen shows source, target, size and digest, no bytes; on approval the vault
   session reads it whole; the writer budget is gone. A vault session's `submit` of a push is
   `labelled`; bob's push into alice's label is `not_owner`; a push over `PUSH_MAX` is `too_big`.
3. `steward-confined-push`: a confined manifest (tests/data): a vault session's shared unlabelled
   volume is unbound, its login's line says the push is the way in, and a push lands.
4. `steward-latency`: point 7, pinned seed, both widths, the seed sweep in the report
   (`--sweep 1..20` through q, serialized with any whole bench: one bench at a time).
5. Host: `crossing` (the program) refuses an oversized item and a second request, and exits
   when its one call is answered; the server's drive runs `Failed → Closing → Closed` on a reader
   that dies (the fake kernel's exit notice) and destroys a partial crossing; the oracle traces
   for the request and crossing rows are already there: run-traces must show every row taken by
   the server's tests (no new rows).

## Page lines (exact text in the report)

- steward.md "Declassification and push" and R42 to built · tested with the cases; the figure's
  "planned" notes go; the `~/out/<name>` rule, `PUSH_MAX`, the partial-push statement, and the
  confined refusal; "Steward work is paid by the steward" gains the measured worst per request.
- scheduling.md "Responsiveness": a paragraph "With the real steward" naming `steward-latency`,
  its p50/p99 table at N = 1, 4, 16 beside the stand-in's, the sweep, and the sentence on what a
  lease's end measures (to the destruction and notice; cleanup off the path); R12's status gains
  the case; the targets table unchanged.
- init.md "The confinement check" to built with case 3; walfsd.md/littlefsd.md's admission
  derivation gains the crossing's term if the arithmetic moves it (3Q's host test follows).
- agents.md "Leases"/"Declassification" sentences where they name the planned shape;
  sessions.md the shell's confined line; SECURITY.md R42's row, R12's row gains
  `bench:steward-latency`, R34's row gains case 3; testbench.md where `sched-latency` is
  described; libs/wire/tables/steward.md if a row is added (none is expected: `submit` carries
  everything; if the confined refusal needs a reply field, opcode 22 is the next free).

## Owned paths and hotspots

- `servers/steward/**` (protocol.rs, drive.rs, the Machine: **hotspot with STEWARD3Y**, which
  changes the end batch's order and adds the deferred cleanup queue; rebase onto 3Y's branch when
  it exists, or land after it: the orchestrator says which); `servers/crossing` (new, one small
  program with a mode); `tests/`, `tests/data` (the confined manifest), `tests/programs` only if
  the latency case needs a program beyond 3A's Echo; `tools/testbench/src/sched_oracle.rs` only
  to accept the new case's lines (the targets' names unchanged); the pages above.
- **Not yours:** the core's rows, guards and constants; the kernel; `sched-latency` and its
  targets; walfsd beyond the admission constant's derivation; the shell beyond one printed line.

## Gates

The whole bench on both widths under the pool's rules (one whole bench at a time),
`steward-latency` on both with its sweep; the host tests (steward server, init, crossing, the
oracle's run-traces); `elixir-oracles`; fmt; the unsafe ratchet; the size budget (one new
program); doccheck. Report each command with its exit code and the sweep's table.

## Design questions before the build

None needs the owner: the targets do not move, no page leaves the book, the landing place
(`~/out/<name>`) and `PUSH_MAX` are the Architect's and stated above. Bring to the thread, not the
build: any core row or guard the cases need changed; the admission arithmetic if the crossing's
term moves the image's 6; 3Y's order if the measured end cannot be taken at the destroy.

## Not here

The audit file and its signing (M4), retention and chaining (M5), a target change of any kind,
leases re-bound after a server restart (3X's residual), `gatewayd` and keys in leases (M4, M5).

## Checkpoint

After case 1 is green on one width: one progress line with the branch, the crossing program's
size, and the measured steward time per `Submit`.
