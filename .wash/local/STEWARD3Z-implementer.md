# STEWARD3Z: a connection the steward mints is admitted in its principal's bucket, not the steward's

Node STEWARD3Z (parent STEWARD3, needs STEWARD3Q). Tier A (admission is a shared server's
authority). Size M. Reviewer: steward-red-4. Ruling behind it: `.wash/local/STEWARD4-ruling.md`
"Q12"; evidence `.wash/local/STEWARD4-report.md`.

## Context rules (read these first)

The standing rules of every launch: `q run` / `make -f scripts/jobs.mk` for every build and boot;
no edits while prebuilt cases run; messages at most 2000 bytes; pages carry no dates or ids; a
built section has no `**Open:**`; every `### R<n>` has a SECURITY.md row. The core (libs/steward)
is not touched here. STEWARD4 is open beside you: its latency manifest and the steward's
`drive.rs`/Machine are the hotspot; coordinate through the orchestrator before touching either.

## The symptom

`libs/rt/src/server/admit.rs:13-14`: "the 9P skeleton counts a connection a client minted for
itself in the share of the connection it minted it from, so minting more connections gains
nothing". Right for a client minting for itself; wrong for the steward, which mints for others:
every connection it mints at `erofsd:system` (a session's shell and relay, a lease's) is a
`State` of the steward's own share there (an account-0 key: a bucket per root badge,
admit.rs:75), cap 8. So the box holds eight steward-minted system-volume connections in all
(2 sessions × 2 + 3 leases + 1 = 8; 4 sessions × 2 = 8): a fifth session or a fourth lease is
refused on today's image, and STEWARD4's latency case cannot reach N = 4. 3Q derived the
per-account caps from the manifest; this cap is the minter's and escaped the derivation.

Second symptom: consoled's client pool is a compile-time constant (`servers/consoled/src/
server.rs:42-53`: `limits(buckets)` against `BUDGET` 2 MiB, `MAX_THREADS` consoles a bucket), so
a six-principal manifest refuses nine buckets and the boot reboots: serving.md's rule that the
manifest sizes each server's buckets to what it serves is not kept by consoled.

## The design

1. **Primary: a minted connection's admission belongs to the account it is minted for.** The
   serving library (admit.rs, the 9P skeleton's `mint_at`) charges a connection minted by a
   root-badge holder for another party to the bucket of the account that first calls on it:
   at the mint the `State` sits in a small `pending` pool bounded by the minter's own share
   (so a flood of mints by one client is still bounded by its share, R26), and at the first
   request on the new connection it moves to the caller's account bucket, where 3Q's caps
   apply. A pending connection nobody ever calls on is freed by the minter's disconnect (3Y's
   cleanup) or the scope's death as today. The rule is one sentence on serving.md "`admit`":
   "a connection minted for another account is admitted in that account's bucket from its first
   request; until then it holds one of its minter's pending slots". Why primary: it keeps
   "accounts, not badges" (serving.md) whole, adds no bucket per principal at any server, and
   makes 3Q's derivation the whole truth with no new input.
2. **Fallback: a root badge per principal at each shared server**, taken only if (1) cannot be
   made sound in the library (a reviewer's objection to the pending pool, or a State that cannot
   move between buckets without a copy): init hands the steward's entry one root badge per
   principal per handed endpoint (the manifest's principals: static, bounded; badges below
   `FIRST_MINTED_BADGE`, never reused at one endpoint, written by init as a steward line
   `badge "SERVER" principal=P id=N`), and the steward mints a principal's connections from that
   principal's badge, so the `State` lands in that badge's account-0 bucket. Cost, stated if
   taken: a bucket per principal per shared server, so 3Q's bucket sizing gains a principals
   term everywhere. Say in the report which you took and why.
3. **consoled follows the manifest** as 3Q did for the file servers: `buckets` from the
   manifest's `buckets=N`, `BUDGET` and the heap cap from `Limits::fits` with the count,
   `init` refusing a count the server cannot fit (serving.md's rule); the image's and the test
   manifests' consoled entries recomputed, the arithmetic on consoled.md.
4. **3Q's derivation** (walfsd.md, littlefsd.md, the bench host test) says, after this, that the
   steward's own share holds only the steward's own connections (its kept fresh connection per
   server and the crossing's, STEWARD4), and the per-account term covers every session's and
   lease's connection whoever minted it.

## Amendment (architect, after STEWARD4's latency run, round 3)

A `disconnect` 3Y's cleanup drops at a busy server (four timeouts; seen at `erofsd:system`
under two spinner VMs) leaves that connection's `State` held until the server's restart. Under
(1) that slot sits in the account's bucket, not the steward's share: say so in serving.md's
residual, in 3Y's words ("until the server's restart"). What frees it is STEWARD3W (a parked
step retried at the server's next answer; `.wash/local/STEWARD3W-ruling.md`), not this node:
touch neither cleanup.rs nor the retry constants here.

## Tier A invariants the reviewer checks

- A connection minted for a principal is admitted in that principal's account bucket and no
  other; the steward's own share holds only its own (its kept connections).
- No connection outlives its account's bucket: the scope's death (3R) and the steward's cleanup
  disconnect (3Y) free it; a pending slot is freed with its minter's disconnect.
- A flood of mints from one client is bounded by that client's share before any first call
  (R26): the pending pool's bound is the minter's share, stated and tested.
- No server gains a per-principal bucket under (1); under (2) every such bucket is sized by the
  manifest and refused by init when it does not fit (R26's undersized-server residual).
- consoled's limits are the manifest's, not constants; a count that does not fit is refused at
  boot, never silently truncated.

## Cases

- host (libs/rt, the fake): a connection minted by a root badge for another account is in the
  minter's pending pool, moves to the caller's bucket at its first request, and a second
  connection minted for the same account counts in that bucket; eight connections minted for
  eight accounts hold nothing of the minter's share after their first calls; a minter flooding
  mints past its share is refused the ninth (R26); a pending connection is freed by the
  minter's disconnect.
- host (init, if (2)): the per-principal badges handed and the steward lines written.
- boot: `steward-many-sessions` (four sessions and four leases hold system-volume connections at
  once under two principals, both widths); the six-principal manifest of STEWARD4's latency case
  boots (consoled sized by the manifest); `steward-latency` regains N = 4 and 16 and its table on
  scheduling.md is completed with the stand-in's columns beside (run by you on STEWARD4's case
  once both are on main, or by STEWARD4 if it lands after you: the orchestrator says which).
- the steward set (steward-ssh-two-principals, steward-context-cap, steward-lease-ends,
  pipe-sixty-four in its class), aio-many-reads-two, walfsd-quota, both widths; 3Q's admission
  host test extended with the new term.

## Pages

- serving.md "`admit`" (the rule), R26's section (the pending bound), "Residual risks" (the
  residual "a root badge's share caps everything minted from it" leaves).
- consoled.md (limits from the manifest, the arithmetic); walfsd.md, littlefsd.md (3Q's
  derivation restated); init.md (`buckets=N` for consoled; the steward lines if (2)).
- steward.md "Authority" (what the steward's own share holds); SECURITY.md rows for R26 and
  R77 (tests), R25 unchanged.
- scheduling.md "With the real steward": the completed table, when the latency case regains N.

## Owned paths and the hotspot

libs/rt/src/server/admit.rs, minted.rs, ninep.rs (`mint_at`) and tests; servers/consoled;
servers/init (consoled's count check; the badges if (2)); servers/steward only for (2) and for
the kept-connection bookkeeping's doc comment; tests/ (the new case, the manifests' consoled
entries); the pages above. **Hotspot with STEWARD4:** its latency manifest and case (you
complete its table; do not edit its toml while it is open: send the orchestrator the lines) and
servers/steward/drive.rs (STEWARD4's crossing steps; you should need nothing there under (1)).

## Gates

The rt host tests and fuzz target; the init and steward host tests; the boot cases above both
widths; the steward set; `cargo testbench docs`; fmt; the unsafe ratchet (libs/rt's budget);
the size budget. Report each with its exit code, and the table completed or the reason it is
not.

## Checkpoint

After the rt host tests of (1) are green, one progress line: the rule as implemented, the
pending pool's bound, and whether (2) is needed.
