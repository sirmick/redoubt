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

---

# Answers to 28-55 (owner, 2026-09-19; confirmed)

**All Rec, except 37, 51 and 55 (changed) and 33, 54 (clarified) below.**

## Changed

**37. Blame with several open calls: blame the most recent, not all.** Blaming every account with
an open call on the faulting thread brings back round 2's bystander problem: a server thread that
holds many open calls (consoled's blocking reads, ipd's receives) would blame everyone waiting on
it when Bob's request crashes it.
- Blame goes to the account of the call the faulting thread **took most recently** (the kernel
  records it per thread when `receive` delivers a call). The exit notice's `blamed_account` is
  that one account.
- Stated residual: delayed corruption can still misattribute; the consequence is a logout.
- `mint` accepts any message id among the caller's open calls (as Rec); a `send` is never open.

**51. Writing up: drop it entirely.** Blind write-up is not needed: data enters a vault by the vault
session reading it *down* from an unlabelled volume (read-down is allowed). So:
- **Every write needs equal labels** (caller's label set = object's). Reads still need the object's
  labels ⊆ the caller's.
- This removes append-only blind writes, the fixed error text, and the name-existence leak through
  `Tcreate`.
- `check(caller, object, read|write)` becomes: read ⇒ object ⊆ caller; write ⇒ object = caller.

**55. A panic with open calls counts as a fault,** blamed per answer 37: the account of the most
recently taken open call, not every open call's account.

## Clarified

**33. Leases.** Accept `MAX_LEASE` = 24 h as a constant in KERNEL-SPEC.md (a spec change,
HISTORY.md). Sub-agents are budgets **inside** the agent's own budget, so R10 already ends them no
later than the agent; an agent holds only its own budget handle, so it cannot create siblings. The
steward refuses lease requests above `MAX_LEASE` rather than clamping them silently.

**54. Declassification reads.** Accept, stated precisely: the steward (system class, unlabelled)
creates a short-lived reader budget carrying exactly the item's labels, which reads and snapshots
the item and returns it to the steward (system class, so R1 does not apply). No standing universal
reader; the steward itself stays unlabelled.

## Accepted as recommended
28, 29 (including the INIT.md name rule: 1-64 bytes of `[a-z0-9_:+-]`, starting with a letter), 30,
31, 32, 34, 35, 36, 39, 40 (the `arg` register carries the startup page's address; no fixed
address), 41 and 42 (one rule: status 1 = `Malformed` everywhere), 43, 44, 45, 46, 47, 48, 49, 50,
52, 53. (38 was already answered by 17.)

Notes:
- **50** is a strong find: a launcher never passes its own connection to a child; each child gets a
  fresh connection. State it in CAPABILITIES.md and INIT.md as a rule, not a convention.
- **53** adds one kernel notice (last handle with a badge closed). Keep it minimal: same delivery path
  and label rule as exit notices, one pending slot per badge, charged to the endpoint's owner.
