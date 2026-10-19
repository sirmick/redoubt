# STEWARD9: the latency case's deadline lease follows the measured launch

Node STEWARD9 (parent AGENTS, needs STEWARD8). Tier B (the latency case's parameters and a page
sentence; no code). Size S. Reviewer: the orchestrator's pick (the editor, or steward-red-4 since
the case is the steward's). Ruling: `.wash/local/wave-2026-10-18-rulings.md` "STEWARD9";
background `.wash/local/B82-report.md` (a lease's deadline runs from its request).

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. A `steward-latency` run (now longer) exceeds 1.5 h: launch it detached
  (`setsid nohup`), copy job logs aside before any rerun.
- Messages at most 2000 bytes; detail in a file. Read reports by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic; a bound is measured once
  on both widths. No target moves. You touch no steward, kernel or server code — only the case
  and the page sentence. STEWARD8 lands first (same file); start on its merged tip.

## The symptom

`tests/steward-latency.toml`: the probe starts kernel-deadline leases `start.(2_000_000)` and a
5 s deadline class, sized to twice an older two-hart launch (~2.5 s). B82's rule: a lease's
deadline runs from its request, and the launch (image read, map, start) takes time; if the
launch exceeds the deadline, the deadline passes between `process_create` and `process_start`
and the launch answers `NotPermitted`. The launch now measures 6.62 s on rv32 at two harts
beside three spinners (~5 s beside fifteen at one hart; rv64 worst 3.95 s), so a 5 s deadline
lease dies inside its own launch: a forbidden line ends the boot before the trace dump, and the
rv32 N = 3/4 rows that now complete cannot be judged net of audits.

## The rule

The case's deadline-lease length and the sleep after it are derived from the measured worst
launch over both widths — the same measurement STEWARD8 took for `launch_timeout`/`MAX_CALL_MS`,
so the case's derived numbers all come from the one launch measurement:

- **deadline lease = 2 × the worst launch** (both widths; ~14 s for 6.62 s), one value used on
  both widths (the larger), so a deadline reliably fires on a running VM — the quantity the
  deadline notice measures (the time from the lease's deadline to the sponsor's `killed` notice);
- **the deadline leases overlap; the round waits once.** Do **not** sleep a full deadline after
  each of the 16 deadline leases serially — that inflates the run to ~1746 s of guest time and
  overruns the sched-trace ring (it stops at ~1394 s on rv32, dumps `dropped N`, and the oracle
  judges nothing: the ring records 4.2–7.8 k records a guest second whatever runs, so it scales
  with wall time, and the ring must not grow for one case — it is a kernel resource and rv32's
  memory is at its 1536 limit). Instead launch the deadline leases close together, let their
  deadlines fire as a cluster, and have the round wait **once** for the cluster's notices; the
  deadline notice is still measured per lease (concurrent expiry is the more realistic load and
  moves no target). A five-round run then fits well inside the ring.

State the derivation in the toml's comment (the lease as 2× the worst launch; the per-round wait,
not per lease) and on `scheduling.md` where the case's parameters are described. `timeout_secs`
stays the case's bound (a miss is a finding). With this, the rv32 N = 3/4 (and N = 16 where
reached) deadline-notice rows are measured net of audits and the table filled; fill them from one
run each at the pinned seed on both widths, as the other rows are.

**If the deadline leases' sequencing is in the probe** (`servers/steward/src/bin/steward.rs`, the
`latency-probe` arming), not the toml's Elixir, that part is Tier A: stop and tell the
orchestrator, which re-tiers this package (still one small package, steward-red-4 then). If no
five-round shape keeps the run inside the ring (measure the record rate and the per-round time
first), stop and tell the orchestrator before changing the kernel trace: the fallback is a Tier A
trace that records only from the first round's marker and only the event classes the oracle judges
(the destructions and wakes), which is a different node.

## Tier B check (the reviewer)

- The deadline lease is tied to the measured launch, not a fresh guess; the number and its
  arithmetic are on the page and in the comment, and match STEWARD8's launch measurement.
- No deadline lease outlives its launch any longer (no `NotPermitted`, no forbidden line) on
  either width at either hart count in a completed run.
- No target moves; the deadline-notice rows are measured, the targets unchanged.

## Owned paths

`tests/steward-latency.toml` (the deadline-lease length, the post-sleep, the comment), its probe
constants in `servers/steward/src/bin/steward.rs` **only if** the deadline class is a probe
constant rather than a toml value — if it is in the steward binary, that part is Tier A and the
orchestrator re-tiers or splits it; say so before touching it. `docs/kernel/scheduling.md` "With
the real steward" (the parameter sentence, the filled rows). Not the latency manifest (STEWARD7's
shape stands), not other cases.

## Gates

`steward-latency` by name, both widths, both hart counts (detached); `set CASES=formatting`,
`set CASES=docs`. Report each with its exit code, the worst launch you used, the derived lease
and sleep, and the filled rv32 rows with their log paths.

## Note

If the deadline class turns out to be a probe constant in the steward binary (not a toml value),
stop and tell the orchestrator: that makes the edit Tier A, and it folds into a steward package
rather than this Tier B one. Check first.
