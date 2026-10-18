# SSH3: sshd's connections are bounded in time before login and sized by the manifest

Node SSH3 (parent AGENTS). Tier A (sshd). Size S. Reviewer: steward-red-4 or the orchestrator's
sshd red. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "The sshd step and SSH3"; the rules
it builds on: serving.md "`admit`" (3Q), walfsd.md's entry derivation (3V), steward.md R82,
sshd.md "Sessions over SSH" (listening, the box's platform) and "Residual risks".

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run.
- Messages at most 2000 bytes; detail in a file. Read pages by section.
- Pages carry no dates or ids; a number on a page shows its arithmetic; a bound is measured once
  on both widths, never guessed; an admission constant is derived from the manifest. A built
  section has no `**Open:**`.
- `sunset` is used as published with one patch (sshd.md): no change to the vendored crate.

## The symptom

sshd.md "Residual risks": "Connections are first come, first served. `sshd` serves four
connections at once, whoever opened them, and closes any beyond; how many one principal may
hold is not decided, so one principal's open connections can keep another's logins out until
they end." `servers/sshd/src/bin/sshd.rs`: `SLOTS = 4` compiled, "within `ipd`'s default of
five parked calls a bucket"; a connection that never authenticates holds its slot until the
client hangs up.

## The rule

1. **Before login a connection holds a slot for a bounded time.** A connection that has not
   completed authentication within `LOGIN_US` is closed by `sshd` (the platform ends the
   connection; `sunset` sees a closed transport). The bound is measured: twice the slowest login
   the bench records (sshd.md's measured paragraph: 2.3 s for a second principal's login beside
   a running session; take the worst over `steward-ssh-two-principals` and `launch-system` on
   both widths at main), rounded up to a second, the arithmetic on the page. Expect the order of
   5 s; the number the arithmetic gives stands. After login a connection is a channel, bounded
   by its session (R82: a login past the context cap is refused and the connection ends), so no
   per-principal slot rule is needed and none is added.
2. **The slot count follows the manifest.** `sshd`'s entry carries `buckets=N` as every shared
   server's does, and its slots are derived from it: one parked read per slot and the listener's
   accept within `ipd`'s parked calls a bucket (ipd.md "Sizing": 5 by default), so `SLOTS` =
   the parked calls a bucket less one, times `sshd`'s buckets at `ipd`; `ipd`'s own `sshd` bucket
   sizing (init.md, ipd.md) is the source. The image keeps 4 if the arithmetic gives it; the
   number and its derivation go on sshd.md "The box's platform". A manifest whose entry does not
   fit the slots' stacks (`DRIVER_STACK_PAGES` 48 + `READER_STACK_PAGES` 4 per slot, twice the
   measured peak, as the comment states) is refused at start (`BAD_LIMITS`, `init` reboots) as
   every server refuses an entry it cannot hold.

## Tier A invariants the reviewer checks

- A connection that has not authenticated is closed at the bound, whatever bytes it sent or
  withheld; no state of it survives the close; the slot serves the next connection.
- Nothing before login is told anything new: the close is silent (no banner, no reason), as
  every pre-authentication refusal is uniform (sshd.md "A key is tried twice").
- The slot count is derived from the manifest and `ipd`'s parked-call bound, held by a host
  test; no compiled count remains.
- Channels after login keep every rule they have (R67, R68); the bound does not run on an
  authenticated connection.

## Cases

- Bench: a client that connects and never authenticates (the bench's `ssh` with a key the
  manifest does not list and a long `ConnectTimeout`, or `nc`-style bytes if the bench has one)
  holds a slot at most the bound, and a principal's login after it succeeds: the verdict is the
  next login's session line, never the attacker's output (rule F); four such connections then
  a login, both widths. A manifest sizing `sshd`'s buckets boots and holds the slots; one that
  cannot is refused at start (`init-refuses-*` family).
- Host (`redoubt-sshd`'s host platform, `tests/` in `servers/sshd`): the bound closes an
  unauthenticated connection and not an authenticated one; the slots' derivation.

## Pages

- sshd.md "The box's platform" (the slot derivation and the bound with their arithmetic),
  "Residual risks" (the first-come residual narrowed to what remains: after login, R82 bounds
  it; before, the bound), status lines; init.md if `buckets=N` for `sshd` needs a sentence;
  ipd.md "Sizing" if its `sshd` bucket is named. SECURITY.md rows unchanged unless a test moves.

## Owned paths

`servers/sshd/src/bin/sshd.rs` (the bound, the slots), `servers/sshd/src/lib.rs` only if the
core must learn the close, its tests; `tests/` (the new case, the manifests' `sshd` entries);
`image/manifest.json`'s `sshd` entry if it gains `buckets=N`; the pages above. Not the vendored
`sunset`, not the steward.

## Gates

`cargo test -p redoubt-sshd`; release builds rv64 and rv32; the sshd and steward SSH cases
(`sshd-loopback-*`, `steward-ssh-two-principals`, `steward-login-refused`, `steward-restart-ssh`,
`sshd-restart-detaches`, `launch-system`) both widths; `set CASES=formatting`, `set CASES=docs`,
`size-budget`, `unsafe-budget`. Report each with its exit code and the two derivations.

## Checkpoint

After the measurement of the slowest login: one progress note with the bound's arithmetic.
