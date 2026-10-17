# STEWARD3Z rulings

Thread `STEWARD3Z-admission`. Brief: `.wash/local/STEWARD3Z-implementer.md`.

## Q1: consoled's per-bucket caps (design point 3)

**Ruled: (A).** Caps follow who holds what. Architect's territory: a serving-library mechanism
that exists (`Admission::with_overrides`, serving.md "Overrides": a named account-0 root badge
gets caps of its own), applied to one more server; no manifest field, wire or ABI change.

Why not (B): it keeps a 255-console cap on every principal's bucket, which is a cap for
`init`'s one badge and nobody else's need, and it ties fit to the entry's pages rather than to
what a bucket serves: serving.md's rule is that the manifest sizes each server's buckets to
what it serves, and 3Q's rule is that a cap is derived from the manifest, not tuned.

Conditions, all of them 3Q's rules applied to `consoled`:

1. **The default bucket's `state` is derived, not chosen.** The page states the arithmetic:
   the consoles one account can hold at once under the manifest (its console session, its live
   contexts up to the cap per label set over its label sets, a lease's console if a lease is
   minted one; say which, from the steward's slot table, do not assume), plus the spare 3Q
   gives every derivation; `files` is `CONSOLE_FIDS` × that. 8 and 16 stand only if that
   arithmetic produces them or shows them above the need; the bench host test holds
   per-bucket `state` ≥ the derived need for every manifest in the tree, as a new term beside
   3Q's (erofsd 8 → 11, bootfsd/walfsd ≥ sessions + leases).
2. **`init`'s override is its root badge's alone.** `init` is the first root badge at
   `consoled` and refuses any handed `consoled` (consoled.md "Started by `init`" says so; cite
   it): the override's cap is `MAX_THREADS` consoles and `CONSOLE_FIDS` × `MAX_THREADS` fids,
   as the comment on `limits` argues today. The worst-case rule of `with_overrides` (each
   override slot `max(override, default)`) is what the fit is computed from.
3. **The fit is on the page with its arithmetic.** `BUDGET` 2 MiB holds `init`'s bucket at its
   caps plus 10 default buckets on rv64 (11 on rv32), bytes shown as consoled.md shows them
   now; the six-principal manifest's 9 fit; a `buckets=N` that does not fit is `BAD_LIMITS` at
   start and `init` reboots, as init.md says (unchanged). The entry's pages follow walfsd.md's
   rule (budget + heap cap + stack, rounded up to 256): 1,024 stays if the numbers do.
4. **Under design point (1)** a session's console, minted by the steward for the principal,
   lands in that principal's bucket at its first request, so `buckets=N` counts the manifest's
   principals plus `init`'s and the steward's own; say the count rule on consoled.md once.

Pages: consoled.md (the two bucket kinds and their arithmetic, the count rule); SECURITY.md's
R26 row if its tests change; init.md unchanged unless the count rule needs a sentence.

## Q2: the other shared servers' budgets under six principals (design point 3, continued)

**Ruled: (iii) for STEWARD3Z; (i) as a follow-up node, STEWARD3V.** Mine to rule: a serving
rule and the manifest-to-server derivation; no target, capacity figure, wire or ABI moves (3V
picks no maximum: it removes a compiled wall, and the image's entries keep their pages if the
derivation gives them).

Why (iii) now: 3Z is sized M for admission's ownership (who a minted connection belongs to)
and consoled; five servers' budget rules are a package of their own, and 3Z's gate stays
honest: `steward-many-sessions` (four sessions and four leases under two principals, a
three-principal manifest that fits today's budgets) proves the re-homing; the six-principal
boot and `steward-latency` at N = 4 and 16 move to 3V. scheduling.md's "With the real steward"
keeps N = 1, 2, 3 beside the stand-in's 1/4/16 with Q12's sentence until 3V completes it;
STEWARD4's table waits for 3Z and 3V both. The 3Z node body still names the latency gate: the
orchestrator moves that sentence (the node is active; I do not edit it).

Why (i) over (ii): (ii) raises five constants to ten and the next manifest hits the same wall;
serving.md's rule is that the manifest sizes each server's buckets to what it serves, and 3Q's
that admission constants are derived from the manifest. Under (i) the manifest is the one
source: a shared server's usable budget is its entry's pages × PAGE_SIZE less its heap cap and
stack, checked at start with `Limits::fits(COST, budget)` against `buckets=N` (refused
`BAD_LIMITS`, init reboots, as today), and the per-entry rule on walfsd.md inverts to its
derivation form: "an entry's pages are what its count of buckets at their caps needs, plus the
heap cap and stack, rounded up to 256; a case's manifest sizes by the same rule, never by what
its case happens to use". The spirit of today's rule (sized by a rule, not by use) is kept; what
changes is that the count enters it. The image's entries: walfsd 1,280 pages gives 1,202 pages
of budget (≈ 4.7 MiB), which holds its six buckets (6 × 626,688 B) with room for a seventh, so
the image's pages stay as they are unless the page's arithmetic wants them tighter (it does
not: pages are declared, twice the peak is the memory rule, this is the admission rule). The
latency manifest's walfsd entry becomes what ten buckets need (~1,614 pages → 1,792 rounded),
and so for each of the five.

3V's scope: erofsd, bootfsd, walfsd, littlefsd, keyd, ipd (every shared server with a compiled
BUDGET; consoled is 3Z's and already derives); each page's admission paragraph restated with
the derivation and the image's arithmetic; init.md's entry rule; the bench host test holding
every manifest in the tree to the rule (pages ≥ the count's need), beside 3Q's terms; the
six-principal manifest's entries recomputed; `steward-latency` regains N = 4 and 16 and the
table is completed (coordinated with STEWARD4 through the orchestrator, as 3Z's brief says).
