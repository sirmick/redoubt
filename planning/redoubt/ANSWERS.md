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

---

# Answers to 56-68 (owner, 2026-09-19; confirmed)

**All Rec, except 57 (decided) and 58 (changed).**

**57. Which call a fault blames:** the most recently taken call that is **still open**. This is
what KERNEL-SPEC.md already says (serving account) and follows answer 31: a replied call is
finished and no longer carries blame.

**58. A panic in a thread with no open calls, while other threads of the process hold some: blame
nobody.** Falling back to another thread's most recent open call brings back bystander blame (that
call's sender did nothing to the faulting thread). The exit notice says `faulted` with
`blamed_account` 0 and `blamed_labels` empty; the crash counts only toward the restart rate limit
and, past it, the reboot. Stated residual: corruption left behind by a replied call can crash an
idle thread later without anyone being blamed.

Accepted as recommended: 56 (`receive`'s record carries each handle's kind), 59, 60, 61, 62 (the
badge-slot cost confirmed in K2), 63 (the steward enforces `MAX_LEASE`; the kernel knows only
deadlines), 64, 65, 66, 67, 68.

---

# Answers to 69-101 and the revisited 56, 57, 58, 62, 64 (owner, 2026-09-19; confirmed)

**All Rec, with 81 option (a) and a note on 84.**

**81. Pinned open calls: option (a).** An abandoned-call notice (the flag lives in the open call's
own page; the server replies to free it), and at `MAX_OPEN_CALLS` `receive` refuses only calls,
still delivering sends and notices. Not (b): letting a call's timeout lapse once taken would let a
hostile server pin a caller until the server dies, the hole round 3 closed with timeouts. Also as
recommended: `admit`'s caps sum to less than `MAX_OPEN_CALLS` with headroom, and parked calls get a
server-side deadline. With 69 accepted, this is the one kernel notice besides exit notices.

**84. User work inside servers: accept, with a note on the steward.** Servers doing work for users
run in the stride queue with a manifest weight and bound one request's work. The steward keeps
strict system-first ordering (so logout and ending a lease stay responsive, item 90), but it also
works for users: it must bound the work any single request can cause and rely on its per-account
caps, and CONTAINMENT.md states the residual (steward work is paid by the steward, not the
requester).

Accepted as recommended: 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 82, 83, 85, 86, 87, 88,
89, 90, 91, 92, 93, 94, 95, 96 (moot with 69), 97, 98, 99, 100 (the answer-14 order was a
classification; the spec's positional order stands), 101.

Revisited: 56 revised Rec accepted (check handle kinds by use; the table's kind is documentation);
57 and 58 are replaced by 82 (`serve(msg_id)`, no fallback); 62 is moot with 69; 64 as recommended.

---

# Answers to 102-115 (owner, 2026-09-19; confirmed)

**All Rec, except 103 (replaced).**

**103. No `first` flag; no strict priority at all.** One stride queue for every budget. `init`,
the steward and drivers get large weights in the manifest instead of running first.
- A driver woken by an interrupt re-enters at the minimum pass (R12), so it runs within about one
  `SLICE`; strict priority only matters for a driver that spins, which is a bug to find, not a mode
  to support.
- The steward's large weight keeps logout and ending a lease prompt (answers 84, 90).
- Removes: the `first` flag, the rules for setting it, and the two-tier ordering in R12. Class now
  means trust only (R1's label-check exemption, `budget_usage`), never scheduling.
- Stated cost: up to one `SLICE` of latency for drivers and the steward under load.
- RESOURCES.md, KERNEL-SPEC.md (R12, Budget fields) and INIT.md (manifest weights) change
  accordingly, with a HISTORY.md entry.

Accepted as recommended: 102 (`MAX_HANDLES` = 4096, `TooLarge`), 104, 105, 106, 107, 108, 109,
110, 111 (charge table pages in use; the model follows), 112, 113, 114, 115.

---

# Answers to 116-119 (owner, 2026-09-19; confirmed)

**All Rec.**

- **116.** Handles that would take a receiver past `MAX_HANDLES` are the same as any cost it cannot
  pay: the message is `Refused` to its sender (answer 72). A reply's handles that do not fit give
  the caller `OutOfMemory` and the reply is delivered without them (question 107).
- **117.** `new_connection` gains `quota: u64`; the `ninep_common` error table gains `3 refused`
  (root missing, permission denied, a cap reached, quota exceeded), leaving code 2 for `not_yours`;
  a connection a client mints for itself counts in the share of the connection it came through, so
  minting badges cannot escape a fair share. NAMESPACES.md and CONTAINMENT.md say so.
- **118.** Each server's manifest sizes its bucket count to the (account, label set)s it serves, so
  the cap does not bind in normal use; CONTAINMENT.md states the residual channel for a server
  sized smaller. Byte quotas live in `fsd` behind the grant and disconnect hooks, not in the shared
  library; the `quota` field stays on the wire.
- **119.** Amend tenet 6 as proposed: a build of the same sources with debug assertions and
  overflow checks on is not a special build, and the bench boots chosen cases with it. The shipped
  configuration is still what most cases boot. It has already earned its place (the undefined
  behaviour in the argument-block read, two latent SMP bugs).

---

# Answers to 120-126 (owner, 2026-09-19; confirmed)

**All Rec, with an addition to 120.**

**120. Domain separation, plus the bundle signature itself.** Accept both recommendations (a
domain for the package container in milestone 2; `init` refuses a manifest that gives `keyd` the
key the loader verifies the bundle with, since `keyd` cannot see that itself). **In addition, give
the bundle signature its own domain now**, in milestone 1: the loader verifies a signature over
`domain || length || tar` rather than the bare archive. The loader and the signing tool are small
and no production key exists yet, so it is cheap now and awkward later, and it closes the
cross-protocol signing hole from both sides rather than relying only on `init`'s check.
VERIFIED-BOOT.md states the container, with a HISTORY.md entry.

- **121.** Accept: WIRE.md states the pattern once (a typed protocol that mints a narrower
  capability names its grant and release operations), and CONTAINMENT.md says a launcher releases a
  child's grants when it receives the child's exit notice, as it disconnects its connections.
- **122.** Accept: manifest arguments are opaque strings passed through unchanged; each server's
  note defines its own; `init` validates only their count, length and encoding.
- **123.** Accept: `bootfsd` serves only the entries the manifest marks public (programs, module
  archives), never the manifest itself. INIT.md states the residual: in milestone 1 the seeds live
  in `init`'s memory and in the bundle image, at the same trust as the bundle. Milestone 2 seals
  them to the machine and generates them at first boot rather than shipping them.
- **124.** Accept: no session or lease holds `keys` in milestone 1. The worked example's row goes,
  and both mentions are marked milestone 2, where a principal's key comes with the one message
  shape it may sign (answer 95).
- **125.** Accept: `keyd` keeps the `audit` purpose, WP-S2 signs each audit record, the file carries
  the signatures, and verification is an operator tool in milestone 2.
- **126.** Accept: a server draws its first minted badge at random above 2^63 (`random`), so a
  restarted server never reissues a badge a client still holds. The 9P skeleton and `keyd` change
  together.
