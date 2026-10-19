# B89: sshd-login-bound orders on the accepts it saw

Node B89 (parent AGENTS). Tier B (the bench case; re-tiers to an SSH3 finding only if a slot is
genuinely held). Size S. Reviewer: the orchestrator's pick. Ruling:
`.wash/local/wave-2026-10-18-rulings.md` "B89". Evidence:
`.tmp/train-74/fail-rv32-sshd-login-bound.console.log`. The case is SSH3's.

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run. Copy the failing console log aside before a rerun.
- Messages at most 2000 bytes; detail in a file. A flaky test is a bug; a case orders on events,
  not the host's clock (the memory rule). You touch the case, not sshd's code — unless the
  investigation shows a slot is genuinely held (below), which is a finding, not a fix you make.

## The finding

`sshd-login-bound` (rv32, train 74 run 2) timed out at 900 s waiting for a fourth `no login
within 5 s, closed` line. The console shows connections 1, 2, 3 accepted on slots 0-2 and closed
at their 5 s bound, then the fourth dial — held 11.60 s by the bench peer against the others'
5.9 s — accepted only after slot 0 freed, as a reused `connection 1 on slot 0`. So only three
connections were ever concurrent (slot 3 was never used), and the fourth never produced a
distinct accept line. The case's `expect` list (four `on slot` lines, then four closures) assumes
all four are accepted before any closes; the loaded rv32 host delivered the fourth SYN after the
first had already timed out. `buckets=4` is right — slot 3 was simply never reached — so this is
a case-ordering bug, not a slot-count bug.

## The fix

Make the case order on the accept events, not on all four arriving before any closes. Either:

- **(preferred) serialize the dials:** dial connection k+1 only after connection k's `on slot`
  line appears. Four quick dials complete well within the 5 s bound, so all four connections stay
  open and are accepted on slots 0-3 before any reaches its bound; then expect the four closures.
  This guarantees four concurrent unauthenticated connections by construction (the rule SSH3
  tests), and slot 3 is actually used.
- or **match the accepts seen:** expect the `on slot` lines in any order and then their closures,
  tolerating a reused connection number, so a late dial does not hang the case.

Prefer serializing: it tests the intended four-concurrent-slots pressure deterministically, where
matching-as-seen only stops the hang.

## The escalation (do not paper over)

If, once the dials are serialized so all four are dialed before any closes, the fourth connection
**still** cannot get a distinct slot — slot 3 unreachable while three are open — then a slot is
genuinely held after the shell's line or by something else, and that is an **SSH3 correctness
finding**, not a case flake. Stop, record what holds the slot (the console, the slot's state), and
raise it to the Architect on a QA thread (B89-slot or SSH3-slots); do not loosen the case to pass
over a real held slot.

## Cases and gates

- `sshd-login-bound` both widths, run **alone** and in a **loaded set**, 20 runs green beside a
  loaded host (the flaky-test bar: a case that still flakes is not fixed).
- `sshd-loopback-*`, `steward-ssh-two-principals` unchanged in verdict.
- `set CASES=formatting`, `set CASES=docs`.

Report each with its exit code, and the console of one serialized run showing four distinct
accepts on slots 0-3 before any closure.

## Owned paths

`tests/sshd-login-bound.toml` (the dial sequencing / expect ordering); its staged data if any.
Not `servers/sshd` (unless the escalation fires, which is a finding, not your edit), not other
cases. `docs/servers/sshd.md` only if the slot behaviour is found wrong (the escalation).
