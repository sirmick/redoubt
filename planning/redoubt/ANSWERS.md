# Answers to QUESTIONS.md (owner, 2026-09-19)

**All Rec, except 2, 13, 14 and 23 below.** Each answer goes into the named note with a HISTORY.md
entry, as QUESTIONS.md says.

## Changed

**2. Open calls per thread.** One call per thread is too tight: servers like `consoled` and `ipd`
hold many requests open at once (a blocking read per terminal, a receive per socket), and with
`MAX_THREADS` = 31 that caps them at about 31 clients each.
- A thread may hold several taken-but-unreplied calls, up to a per-process limit, new constant
  `MAX_OPEN_CALLS` = 64.
- Each open call is charged one page to the receiving process's budget, like any object.
- Beyond the limit, `receive` returns `Busy`.
- Restate I5's R3 bound per open call (at most `MAX_LEND_PAGES` per open call), not per thread.

**13. Object costs.** Rec, with one correction to confirm: a handle is about 24-32 bytes (object
reference, 64-bit badge, 64-bit stamp), so a handle table page likely holds 128 handles, not 256.
The kernel implementer confirms the figure and it goes into the cost table.

**14. Errors and the order of checks.** Do not make the model's README normative. Copy the error
table and the order of checks **into KERNEL-SPEC.md**; the spec stays the single owner, and the
model conforms to the spec (not the other way round). The decoding-errors-first rule (`BadHandle`,
then `TooLarge`, then `InvalidArgument`) is part of what gets copied.

**23. JSON integers.** No "either". The manifest's schema fixes each field's type:
- 64-bit quantities (ids, accounts, byte and page sizes, deadlines) are always strings;
- small counts (weights, depths, restart limits) are always numbers.

A value of the wrong JSON type is an error. Fix INIT.md's example to match.

## Accepted as recommended
1, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 15, 16, 17, 18, 19, 20, 21, 22, 24, 25, 26, 27.

Notes on a few:
- **17** is a good catch: key the pending-request cap, `WAIT_CAP` and R2's round-robin by
  (account, label set), not by account alone.
- **24:** the amended tenet 3 should say what is allowed precisely: host-only test oracles and fuzz
  drivers, in crates outside the workspace build, never linked into anything that runs on the
  machine.
- **26:** attack success is asserted by the system (kernel, victim or a clean power-off), never by
  the attacker's own output; apply this to every attack case, not only WP-E1.
