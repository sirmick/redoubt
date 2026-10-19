# STEWARD11: the destroy-by-index guard holds uniformly (the Scope arm)

Node STEWARD11 (parent AGENTS, needs STEWARD10). Tier A (the steward's serving path). Size S.
Reviewer: steward-red-5. Ruling: `.wash/local/wave-2026-10-18-rulings.md` "STEWARD10 review
notes"; builds on STEWARD10's `.wash/local/STEWARD10-implementer.md` (the deadline-as-witness
guard).

## Context rules (read these first)

- Every build and boot through `scripts/q run` or `make -f scripts/jobs.mk`; fmt gate `make -f
  scripts/jobs.mk set CASES=formatting`, docs gate `set CASES=docs`. No edit while prebuilt
  builds or cases run.
- Messages at most 2000 bytes; detail in a file. Read source before editing. STEWARD10 lands
  first (same file, `drive.rs`); start on its merged tip.
- Pages carry no dates or ids; a security property lands with a test whose verdict comes from the
  system. The core (`libs/steward`) is not touched.

## The gap

STEWARD10 gave the lease and crossing destroy-by-index paths the deadline-as-witness guard: a
destroy reads `kernel.now()` = `t` immediately before it, and at `t >= d` the kernel owns the
destruction (the deadline passed, the index may be recycled) so the steward never destroys by
that index. `servers/steward/src/drive.rs:550` `Made::Scope(b) => (b, FOREVER)` still destroys a
scope **by index, unguarded**: a scope has no deadline (`FOREVER`), so `t < d` always and the
destroy always runs by index. It is unreachable today — scopes are never destroyed by handle
(effects.rs names the budget token; confirmed in STEWARD10's review) — but an unguarded
destroy-by-index is exactly STEWARD10's bug, and the next change that makes the Scope arm
reachable would reintroduce the silent wrong-object destruction. The invariant "the steward never
destroys by an index R10 may have revoked and recycled" should hold at **every** destroy-by-index
site in `drive.rs` by construction, not everywhere-but-one.

## The rule

Apply STEWARD10's shape to the Scope arm so the invariant is uniform. A scope carries no deadline,
so the deadline witness does not apply directly; its witness is that its handle still names the
scope the record holds. Concretely, match STEWARD10's settled form: a destroy is attempted only
when the record's handle is current, and `BadHandle`/`WrongObject` from the scope destroy are
treated as STEWARD10 treats them (BadHandle benign — the scope is gone, drop the record;
WrongObject an invariant tripwire, since no inline install should make the index name another
object). The exact form is yours, matching STEWARD10's so the two read as one mechanism; do not
invent a second idiom. If the cleanest uniform form is simply "every `Made::*` destroy goes
through the same guarded helper," do that and the Scope arm inherits it.

## Tier A invariants the reviewer checks

- Every destroy-by-index site in `drive.rs` (lease, crossing, scope) refuses to destroy an index
  that may have been recycled; none destroys a live wrong object.
- The Scope arm's change does not alter a reachable path's behaviour (it is unreachable-by-handle
  today), only closes it by construction.
- The mechanism reads as one with STEWARD10's (no second idiom).

## Cases

- Host (`servers/steward/tests/steward.rs`, the fake kernel): a scope record whose handle index
  was reused for another object yields no wrong destruction — the reused object survives, its
  liveness the verdict (as STEWARD10's lease test). A normal scope destroy (the reachable path)
  is unchanged.
- Bench: the steward set both widths unchanged in verdict; `steward-latency` (with STEWARD9 if it
  has landed) unchanged.

## Pages

steward.md "Failure and restart": the cleanup bullet states the invariant holds at **every**
destroy-by-index site (not only the lease/crossing arms). No other page claim moves.

## Owned paths

`servers/steward/src/drive.rs` (the Scope arm and any shared guarded helper), its tests; steward.md
the one bullet. Not `libs/steward`, not the kernel, not `tests/steward-latency.toml` (STEWARD9's).

## Gates

Host tests; release builds rv64 and rv32; the steward set both widths; `set CASES=formatting`,
`set CASES=docs`, `size-budget`, `unsafe-budget`. Report each with its exit code and the scope
host test's verdict.
