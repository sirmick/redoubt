# SHELLN1: the shell's notices listener takes a lease_ended before it prints

Node SHELLN1 (parent AGENTS). Tier B (the shell, inside one session's budget). Size S. Reviewer:
the shell/editor (the orchestrator's pick, shell track). Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "STEWARD12 burst finding"; evidence
`.worktrees/STEWARD12/.tmp/STEWARD12/burst-finding.md`.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting` (nightly fmt in `userland/otp` too), docs gate `set
  CASES=docs`. No edit while prebuilt builds or cases run.
- Messages at most 2000 bytes; detail in a file. One VM is one trust domain; this is the shell's
  own code (Tier B), no steward/kernel/wire change.

## The finding

STEWARD12 measured the deadline_notice burst: the steward's `lease_ended` send is a **rendezvous**
with the sponsor's shell VM, and `Redoubt.Shell.Notices` takes each notice only **after** printing
the previous `[lease ended]` through the relay. So under a burst of N concurrent lease expirations
the last notice reaches the sponsor ~N × the shell's take after the deadline (581 ms of sends at
D = 6, up to 2.2 s deferral at D = 16 two harts), and one send of six was **lost** on rv64/1 (a
send past the steward's `NOTICE_US`, 100 ms, is dropped). The deadline_notice target is "the
sponsor told, less the deadline", so the sponsor's consumption rate is on the measured path — and
it is the shell's to fix, not the steward's (the steward cannot make the sponsor consume faster).

## The rule

`Redoubt.Shell.Notices` **takes each `lease_ended` (and `server_restarted`) send promptly** —
receive it, enqueue it, and print `[lease ended]` (or the restart line) **asynchronously** — so
the steward's send completes in ms whatever the print and the relay cost. The shell prints the
queued notices in order on its own, decoupled from the take. So a burst's last notice is ~N × (ms)
within the steward's `NOTICE_US` (no notice lost), and the steward's loop is not blocked on the
slow rendezvous (largely fixing STEWARD12's deferral of other events behind the burst's sends).

Keep the print order (the notices print in the order they arrived); the async print must not drop
a queued notice (it is the sponsor's record that a lease ended). The take is the fast path; the
print is the slow path behind it.

## Cases

- Bench: `steward-deadline-burst` (STEWARD12's, D = 6 both widths; the D = 16 rv64 variant) with
  the shell change — the last `[lease ended]` within a **stated ms bound** (measured, twice the
  worst take, on the page), **all N lines present** (none lost), both widths. The verdict is the
  shell's printed lines and the steward's sends all returning (no `dropped` count).
- Host: the shell's ExUnit (`test/redoubt/...`) — the listener takes a notice (enqueues it)
  before the print completes, and prints queued notices in order; a slow/blocked printer does not
  make the take miss.

## Pages

- `docs/userland/shell.md`: the notices listener takes-then-prints (the decoupling, the order).
- `docs/kernel/scheduling.md` / `docs/servers/steward.md`: the burst residual STEWARD12 stated
  (the last notice ~N × the sponsor's take, the lost notice) is **replaced** by the measured bound
  once this lands. Summaries checked: say which.

## Owned paths

`userland/shell/lib/redoubt/shell/notices.ex` (or wherever `Redoubt.Shell.Notices` lives) and its
tests; the pages above. Not the steward, not the kernel, not the wire tables, not
`tests/steward-latency.toml` (STEWARD9's). `steward-deadline-burst` is STEWARD12's case — coordinate
through the orchestrator if it is still open; otherwise this node owns it after STEWARD12 lands.

## Gates

The burst cases both widths; the shell's host tests; `set CASES=formatting`, `set CASES=docs`.
Report each with its exit code, and the last-notice bound measured before/after with log paths.

## Measure after

Whether the steward's sends still block its loop enough to want the sends off the loop (a notice
thread, STEWARD12's option 2) once the shell takes fast, or it is subsumed. Report the loop's
deferral under the burst after this change; if it is gone, STEWARD12's (2) is not needed.
