# WEIGHT1: the image runs the steward at 1,000 under `system`

Node WEIGHT1 (parent AGENTS). Tier A (a `system` weight in the trusted base's manifest: R12's
policy is the manifest's weights), size S, reviewer kernel-red-4 (one red; the editor's checks are
the red's, since the diff is a number and sentences). Base: origin/main 453a48bfc. Owner's answer
(Q1 in `.wash/local/owner-questions-2026-10-18.md`, "Owner's answers"): steward weight 1,000
under `system` in the image. Independent of SCHED5, B94, B95 and IPD2.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting` (it runs nightly fmt in `userland/otp` too), docs gate
  `set CASES=docs` (C11: no package ids in cases or comments). No edit, and no git operation,
  while prebuilt builds or cases run. Copy a failing job's log aside before a rerun.
- Messages at most 2000 bytes; detail in a file. Read reports and long pages by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic. A miss is a finding, not
  a tuning; no target and no bound moves.
- Change only the steward's weight. Any other weight (the read-path servers', `session`,
  `agent`) is the owner's and not asked: if a measurement points at one, stop and say so.

## The change

1. `image/manifest.json`: the `steward` entry's budget weight 100 → 1000.
2. Every manifest in the tree whose `steward` entry copies the image's (weight 100): today
   `tests/data/steward/{boot,confined.manifest,idle.manifest,many-sessions.manifest,
   six-principals.manifest,three-contexts.manifest,vault-launch.manifest}.json`,
   `tests/data/pipe/manifest.json`, `tests/data/boot-profile/manifest-unverified.json`; find
   them by grep, not this list, and list them in the report. `latency.manifest.json` is already
   1000. A manifest that runs the steward at 100 on purpose (a case about the weight) keeps it and
   says why in its case's comment; I know of none.
3. `system`'s sum: `init` refuses a manifest whose servers' weights exceed `system`'s; state the
   image's new sum (today ~4,900 plus the steward's +900) against 250,000 in the report.
   `servers/init/tests/manifest.rs` reads the image: run its host tests.

## Pages (sentences, not new rules)

- init.md "Weights" already says the steward gets 1000 beside the drivers: now true; no edit
  unless its sentence names a figure that moved.
- scheduling.md "With the real steward": "The steward runs at the workload's weight, 1,000 under
  `system`, in the case's manifest" → the image's own weight; the responsiveness table's
  condition (the targets stated with the stand-in at 1,000) now matches the image: say the image
  runs the steward at the weight the targets are measured at.
- `tests/steward-latency.toml` (and n4/n16, burst cases if they repeat it): the comment "(the
  image's is 100)" goes.
- steward.md "Residual risks", the launch bullet: add the owner's rule (Q2) with its reason, one
  sentence: a launching lease runs at the `agent` weight it keeps for its life, never above its
  sponsor's running leases, since a launch weight would let a flood of launches take CPU from
  running agents; the launch's measured cost and the derived client bound stand.
- Grep the book for the steward's weight as 100 (`weight 100`, `at 100`, "image's") and fix any
  sentence that names the image's figure. Summaries checked: say which.

## Cases (gate)

The short gate (both builds, touched crates' host tests, smoke set, docs, fmt, size, unsafe),
plus, both widths, every boot case whose manifest changed (grep `tests/*.toml` for `from = { path`
naming each changed json, and `image/manifest.json` / `image/boot.toml` users) and these, whose
verdicts or page figures depend on the steward's share or timing:
`steward-end-lease`, `steward-lease-ends`, `agent-login`, `deadline-in-launch`,
`sshd-login-bound`, `steward-six-principals`, `steward-restart`, `boot-profile`,
`beamlet-reduction-rate`, `beamlet-schedulers-one-hart`, `session-tcp-echo`, `shell-compile`.
The latency and burst cases run on their own manifest (already 1000): not re-run.

A figure a page states from one of these cases (the slowest login behind sshd's 5 s bound,
boot-profile's prompt time, the end-lease latency) is compared against the page: inside its
stated spread, no edit; outside it, re-measure on both widths and update the figure with its
arithmetic; a bound derived from it does not move unless the derivation does, then say so.

## Owned paths

`image/manifest.json` (the steward entry's weight, line ~84; IPD2 edits ipd's entry, line ~54,
in the same file: a textual neighbour, rebase whichever lands second), the `tests/data/**`
manifests above, the toml comments above, scheduling.md, steward.md, any page the grep finds.
Not `servers/`, not `kernel/`, not any other weight.

## Report

Each gate with its exit code; the manifests changed; `system`'s sum; the page figures compared,
with logs; what was deleted (the "image's is 100" sentences).
