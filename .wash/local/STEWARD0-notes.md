# STEWARD0 notes (architect-3)

The design is not half-written. It was finished and committed before the handoff instruction
arrived: `docs/servers/steward.md`, "The policy core" (under Interface), commit 99d01496f. The
brief is `.wash/local/STEWARD0-implementer.md`, and the answer is on QA `STEWARD0-design`. The
plan has STEWARD0 (L, needs nothing) and STEWARD1 (M, the Elixir oracle and a bench case running
Elixir on beamlet, needs STEWARD0); the `steward` step needs STEWARD1.

## What the next Architect does

1. **Review STEWARD0's tables at its checkpoint** (commit 1: `libs/steward/tables/*.md`) before
   any code. Check each machine:
   - every waiting state has a `Done` failure row;
   - `Approve` appears only from `Rendered` on the rendering channel;
   - every guard named is in the page's guard table;
   - edges (approval grant, crossing, lease supervision) are the only rows touching a second
     domain;
   - an edge's audit record carries the labelled side's domain.
2. **Write STEWARD1's brief** when STEWARD0 is near done. Points for it:
   - The bench has no case that runs Elixir. `./test-shell` runs the shell's tests on beamlet
     outside the bench, and `libs/wire/elixir/run-vectors` is in no case.
   - The pinned OTP/Elixir live in `toolchains/` (untracked; `userland/shell/setup.sh` and
     `userland/otp/tools/env.sh` find them).
   - Need: a trace format both the Rust core and Elixir run, a canonical dump of states, effects
     and audit records, and a host-tests case that runs it.

## Choices made in the design (not owner choices; reopen only with evidence)

- Ids come from the random words the event carries. The core holds no generator or counter.
- Effects are batches with tokens. A batch stops at the first failure and comes back as one
  `Done`.
- Mutations work by swapping entries in a `Policy` function table. The shipped crate has no
  mutation feature.
- Four mutations are retired as unwritable by type: `PolicyBlamePerAccount`,
  `PolicyCapPerAccount`, `PolicyCarveFromUnlabelled`, `PolicyNarrowToSessionBudget`. Three are
  added: `PolicyApproveOtherChannel`, `PolicyApproverExceeds`, `PolicyEndLeaseFromVault`.
- The binding hash is SHA-256 via the vendored `sha2` (already a workspace dependency through
  `sshd`).
- The model keeps its kernel plumbing, volumes, ideal `keyd` and mixer. `ConnectionShares` and
  `AdmissionBucket` move to `model/src/serving.rs`.

## Open, not yet ruled

- The steward's wire table (`libs/wire/tables/steward.md`) does not exist, though the page says it
  is included. It is the steward step's, written from the event set.
- Projects (M5) and run-time principals (M5) are outside the M1 core. Its domain key already fits
  them.
