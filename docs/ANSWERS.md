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

---

# Answers to 150-153 (owner, 2026-09-22)

**All Rec.** These record the owner's use-case direction of 2026-09-22 (HISTORY.md's "The use case,
the high/low pair, and the game" entry and the "back out collusion prevention" commit), which was
written straight into the notes with no numbered record. The decisions are the owner's, made on
2026-09-22; the answers below restate them and pin the semantics they left implicit, and add no
mechanism.

## Clarified

**150. The isolation unit is the label set, not the capability set.** The direction, now defined as
a named property rather than one sentence: capabilities bound authority, labels bound information
flow, and data moves only along labels. Two budgets with different handle sets but equal label sets
are **one trust domain** — a handle passed between them is not a crossing — and two budgets with
differing label sets are the smallest domains the OS distinguishes (R1). TENETS.md, The use case,
keeps the sentence and now says "one trust domain"; CONTAINMENT.md, Labels, opens with the property;
CAPABILITIES.md, Agents, item 6 states it and points at CONTAINMENT.md. No mechanism change: this is
what R1 and `check` already implement.

**152. `confined` is a per-boot flag.** It is one top-level boolean, applied to the whole manifest,
not per domain. `init` compares **label sets** (a budget's labels, a volume's label set, as the
manifest declares them; sets differ when not equal, so `{a}` differs from `{}`, from `{b}` and from
`{a,b}`; a system server such as the steward carries none and is a domain of its own) and refuses
the **boot** when differing sets share a `servers` entry, a `volumes` entry, an endpoint name in
`receives`/`handed`, an `ipd:*`/`netd` instance, or a core; it also refuses a confined manifest in
which a labelled domain reads a shared unlabelled volume. Refusal is a boot failure, not a warning
(TENETS.md 2). INIT.md's manifest table gains `confined` (a boolean) and The boot manifest carries
the precise rule; WP-R3 implements and tests it.

**153. The steward push.** Defined in CONTAINMENT.md, Push, as the exact mirror of declassification,
**one item per push**: the target label's owner triggers it through the powerbox with an out-of-band
approval; the steward, unlabelled, reads the source and writes the item into the labelled volume
through a short-lived **writer budget** carrying exactly the target label set (a write needs equal
labels, and the steward holds none); no standing path, queue or batch; audited with the request's
labels. The confined domain cannot trigger, name the item for, or pull a push, so the lower side
cannot make a push happen or choose its timing — that is what closes the B-to-A channel. `check` is
unchanged; the steward declines a labelled session's mount of a shared unlabelled volume and offers
the push instead. Stated residual: a push is one human action, so a confined domain's input rate is
a human approval rate. WP-S2 builds it.

## Accepted as recommended

**151. Covert communication is stated once, canonically, in TENETS.md (The adversary);** every other
note points at it. CONTAINMENT.md's Covert and timing channels section now opens by pointing at the
tenet instead of re-arguing it, and the channel table's out-of-scope wording says so too. The design's
exact and only claim is **zero intentional (software-mediated) paths across a label boundary**, by
construction; the only zero is placement. TENETS.md's covert-channel wording is unchanged; the only
TENETS.md edits this tranche makes are the property and citation additions 150-153 name.

Notes:
- All four are recordable as accepted: they were owner-decided on 2026-09-22 and reapplied here only
  to add the semantics WP-R3, WP-D2, WP-D3 and WP-S2 need.
- **150** and **151** are clarifications of wording already present; **152** and **153** add detail
  to INIT.md's confinement rule and CONTAINMENT.md's push, and both name their implementing package.

---

# Answers to 154-155 (owner, 2026-09-22)

**All Rec.** Both are derivations from the existing design, raised by the WP-W3 split: W3a (the
9P opcode floor) is being built and its implementation uncovered the second one.

## Accepted as recommended

**154. WP-W3b is dropped: answer 115 is already satisfied.** `redoubt-rt`'s
`Record<const N: usize>(pub [u64; N])` is built as `Record([0; N])`, a written stack array, so
every record it passes is already backed and the runtime owes no change. The "untouched page is
`InvalidArgument`" assertion is kernel behaviour and is not reachable through the `HostKernel`
fake, so it cannot move into `redoubt-rt`'s host tests; it already lives in two real-boot cases —
`budget-syscall-attack` (WP-K1: a page reserved and never touched, `budget_usage` on it fails)
and `lend-untouched-page` (WP-K0). No runtime change, no new case; BUILD-PLAN.md deletes WP-W3b
and WP-W3a is the whole of WP-W3. KERNEL-SPEC.md (ABI, Records) is unchanged.

**155. The `fsd` message `copy` is renamed `copy_file`.** `copy` camel-cases to `Copy`, and
`Copy` is in the generator's `RESERVED_TYPES` for a real reason: every generated type derives
`Copy`, so a message type named `Copy` would shadow the derive. The reservation stays; the
message is renamed, in `docs/NAMESPACES.md` (`copy_file`, opcode 17, reply `count: u64`
unchanged) and in the `docs/USERLAND.md` row that names it. This is folded into **WP-W3a**,
because WP-W3a's acceptance ("the `fsd` table must still generate") cannot hold until the rename
lands. The alternatives lose: prefixing generated types changes every codec and call site for
one word, and narrowing `RESERVED_TYPES` breaks the derive.

Notes:
- The `copy` failure was latent, not new: `cargo test -p redoubt-wire-gen` was already red on
  `redoubt`, because the `fsd` typed-operations tables reached `docs/NAMESPACES.md` without the
  generator learning their `<!-- wire: fsd ninep -->` marker, and the parse died there before it
  could reach `copy`.
- **WP-W3a carries both:** the marker and opcode floor (answer 113), and this rename (answer 155).

---

# Answers to 156-159 (owner, 2026-09-22)

**All Rec.** Four decisions raised by porting `consoled` onto the current runtime. They are
derivations from R1b's parked-call design and CONTAINMENT.md's shared-server-library section (answers
81, 82, 104 already decided the intent); none changes frozen behaviour, and 156's mechanism was
written in WP-R4's own runtime commit and never merged.

## Accepted as recommended

**156. A 9P server that must wait: restore the three-way read and the skeleton's hand-back.** R1b
removed the old `Read { Done(usize), Wait }` and left `FileServer::read` returning a `usize`, so a
server cannot say "nothing yet, no end" — the one thing a console read needs, since `0` means EOF. The
read path gets the three-way enum back: `Read::Done(n)` / `Read::Wait`; `answer_in_place` returns
`Replied`/`Waiting`/`NoRoom` instead of `Option<()>`; and `serve_parking` is `serve_with` except a
`Read::Wait` request is handed back unanswered, with its T-message still in its lend, so the server
parks it (`Parked`) and serves it again later. `serve`/`serve_with` keep answering every request, so a
server that returns `Wait` without `serve_parking` gets a `Rerror`, not a stranded caller. A held
request's **handles are closed when it is handed back and its handle list emptied with them** (they
were delivered into this process; holding them grows the table, and a second serving would close
indices naming something opened since). Serving it again re-reads it from the lend, so the skeleton
keeps nothing of a held call: a fid clunked meanwhile makes the second serving an `Rerror`. Only
`read` waits in milestone 1. NAMESPACES.md owns it (Holding a call).

**157. A new package owns the join; `Parked` stops owning its `Admission`.** **WP-R1c ("join `Parked`
to `NineServer`")**, owned by `libs/rt`, with its own review round — not WP-R4b, which is a server
port and must not carry a shared-library API change behind its acceptance. R1c recovers the runtime
half from WP-R4's own commit `5d29d136e` ("the runtime pieces the first two 9P servers need"), which
was written and never merged. It adds `NineServer::admission_mut` and `NineServer::share_of`, and
**`Parked<T>` stops owning an `Admission`**: `Parked::new(longest)`, with `&mut Admission` passed to
`park`, `resume`, `resume_first`, `expired` and `abandoned`. Reason: fids and parked calls must be
charged in the same buckets and shares (`admit`, CONTAINMENT.md), and two tables do not compose. This
updates `libs/rt/tests/parked.rs`. CONTAINMENT.md's shared-server-library section says so.

**158. `consoled` is the right first user of the join.** A read with no input parks; it is not
answered `0` (which would look like a closed console) nor an error the client must poll. It is the
smallest possible first user (one file, one wait condition), the work exists (157), and `ipd` needs
the same join, so special-casing `consoled` would be thrown away. NAMESPACES.md, The console.

**159. Two test-harness breaks: one fixed on its own, one folded into R1c.** (a) `servers/keyd/tests/
keyd.rs:8` (and the two ported servers) include the runtime's test helper at `../../rt/tests/common/
mod.rs`, a stale path from the reorganisation that resolves to `servers/rt/...`; **`cargo test -p
redoubt-keyd` is red on `redoubt` today because of it**. Fixed as **its own one-line commit on
`redoubt`** (a pre-existing bug in a merged package; a red test masks other regressions). (b) The
ported servers' `tests/vectors.rs` need a **server-side conformance runner**
(`libs/rt/tests/common/vectors.rs`, `vectors::run(&mut NineServer, &Caller) -> Counts`, with the
`waiting` count) that does not exist in `redoubt`; **recover it as part of WP-R1c**, since it drives
the skeleton and its `waiting` count is precisely 156's new observable. NAMESPACES.md: every 9P server
runs the corpus.

Notes:
- 156's mechanism is not new design: it was written in `5d29d136e` and dropped. R1c and the note
  restore it, so WP-R4b's `consoled` port is a port again, not a redesign.
- **No design mechanism changed.** 157's `Parked` signature changes are an API shape the note now
  states; nothing in KERNEL-SPEC.md, CAPABILITIES.md or TENETS.md is touched.

---

# Answer to 162, and the status of 160-161 (architect, 2026-09-22)

**160 and 161 are open** — 160 is the owner's (whether milestone 1 carries a resize push at all),
161 is the orchestrator's (it changes BUILD-PLAN.md, which the architect does not edit). Both carry a
`Rec`. **162 is answered** below; it is a consistency fix, not a new mechanism.

## Accepted as recommended

**162. `console_size` is `Option`, and the Redoubt side is pinned.** The implemented trait method is
right as written: `Some((cols, rows))` when the platform knows a size, `None` when it does not, with
the trait default `None`, so a platform that says nothing is honest rather than silently claiming
80×24. What was missing is what the Redoubt platform answers and where a size comes from:

- On Redoubt the size comes from the console server, not from the startup block (which has no size
  field: `startup` carries `version, handle_count, namespace, handles, argv`). The Redoubt platform
  asks its `/dev/cons` connection with the `consol` `size` call (opcode 16) and caches the answer. A
  server that does not serve `consol` refuses the opcode as `Malformed` (WIRE.md, code 1), and the
  platform answers `None`, which `Redoubt.Console.size()` reports as `{:error, :unknown}`.
- **`consoled` takes its size as a manifest argument**, `cols,rows`, two decimal numbers, defaulting
  to `80×24` when absent. INIT.md's `servers` arguments are opaque strings each server's note defines
  (answer 122), which is how `keyd` already takes `name,purpose,seed`; `consoled`'s own note defines
  this one. `sshd` answers from the SSH pty-req instead (WP-S3), the one place a size can change.
- **`USERLAND-API.md` owns the Redoubt side of the `Platform` contract** — what each method must
  answer here, and which `Redoubt.*` module wraps it. `userland/otp/DESIGN.md` keeps owning the trait
  itself, as it already documents `platform.rs`.

Applied to `USERLAND-API.md` (The console and the `Platform` contract), `NAMESPACES.md` (The console),
and `HISTORY.md`. Nothing in KERNEL-SPEC.md, CAPABILITIES.md or TENETS.md is touched; no mechanism
changed.

Notes:
- The `on_resize(callback)` entry in `Redoubt.Console` is removed with 160's Rec: a callback whose
  delivery the same table describes as "via GenServer `handle_info`" is two contracts in one row, and
  with no push there is nothing to deliver.
- **`libvterm/`** (an untracked C git clone at the repo root) is **reference only** — read for its
  terminal state-machine and key tables, never built, never linked (TENETS.md 3). Recorded in
  `USERLAND-API.md`'s console section and ignored like the other vendored reference trees
  (`.gitignore`). This is an orchestrator action (the file is not the architect's); proposed below.

---

# Answer to 160 (owner, 2026-09-22), and question 163 (architect)

**160 is decided by the owner; 163 is open.** The owner overruled 160's earlier recommendation and
chose the general rule with `resize` kept in milestone 1. Specifying it revealed a mechanical gap,
recorded as 163.

## Accepted as recommended

**160. A server pushes an unprompted event by parking a call the client made.** The IPC primitives
are caller-initiated — a `call` and a `send` both start at the client, a `reply` answers a call the
server already took — so a server has no way to speak to a process that is merely reading a file. The
design's answer is that a server which has news and a client that wants it meet by the client
**calling and waiting**: the client makes a call meaning "tell me when this happens", the server
**parks** it (the machinery WP-R1c landed), and answers it when the event occurs. The parked call *is*
the push channel: no endpoint is handed over, no `send` is needed, and WIRE.md's rule holds (every
milestone 1 typed message is a `call`).

- **Where it lives:** NAMESPACES.md, Holding a call — the section that already describes parking now
  says *why* a server parks (not only "the file server asked"), and states this as the design's answer
  to server-initiated delivery.
- **What it costs:** a parked call holds one of the caller's `MAX_OPEN_CALLS` and one of the server's
  admission slots (its bucket and share) for as long as it waits, which is why parked calls are
  capped, reported abandoned, and may carry a deadline.
- **`resize` is an instance.** `consol` opcode 17 `resize` is a `call` with no fields: the client
  calls it, the server parks it, and answers `cols, rows` when the window changes. Same shape as
  opcode 16 `size`, so the table stays all-`call`s and needs no `kind` column. A client that never
  calls it misses changes (it should re-read `size` when it redraws); a client re-calls `resize` after
  each reply to wait for the next; one parked `resize` per connection is the client's own business and
  the server keeps no per-client resize state; on a UART, where nothing resizes, `consoled` parks a
  `resize` for ever rather than refusing it, since a wait is honest and `size` is there for a client
  that would rather not wait.
- **`Redoubt.Console`** gets `await_resize/1` — a **message**, not the removed `on_resize(callback)`:
  the caller is re-called and `{:console_resize, cols, rows}` arrives as a message, which is how this
  VM delivers anything to a process.

## Open

**163. A parked *typed* call is not possible yet.** The park mechanism reaches only the 9P `read`
path: `serve_parking` hands a request back only when `answer_in_place` returns `Answer::Waiting`, and
that comes only from `FileServer::read` returning `Read::Wait`. A typed opcode goes to the server's
own dispatch (`Result<(), Error>`, always replies) and the typed `Answer<R>` has no "wait". So
`resize` is specified but not buildable until the typed dispatch can hand a request back. **Rec:**
extend it in its own `libs/rt` package (WP-R1d), owned by WP-B2a, so one park mechanism serves both
entry points; until then WP-B2a builds opcode 16 `size` and not 17 `resize`. **Alt:** make `resize` a
9P file (`/dev/cons-size`, whose `read` parks) — needs no `libs/rt` change but splits one concern
across two mechanisms. Open for the owner or the orchestrator to schedule.

Notes:
- 160's earlier Rec (drop `resize` from milestone 1) is **overruled**; the record keeps the reasoning
  in QUESTIONS.md 160, which now carries the owner's decision in its `Answered` line.
- Applied to NAMESPACES.md (Holding a call, The console), USERLAND-API.md (The console and the
  `Platform` contract, `Redoubt.Console`), QUESTIONS.md (160 closed, 163 opened) and HISTORY.md.
  Nothing in KERNEL-SPEC.md, CAPABILITIES.md or TENETS.md is touched.

---

# Answers to 167-168 (owner, 2026-09-22)

**Both Rec accepted.** The owner explicitly approved both IPC recommendations after the
orchestrator summarized them. Questions 164-166 remain open; this approval changes neither
confinement nor capability closure nor scheduling. The architect specifies the exact return
encoding and completion ordering below as the mechanical elaboration of these approved outcomes,
not as an additional statement attributed to the owner.

## Accepted as recommended

**167. Caller ownership and reply validity are independent of status.** Preserve R3, R4 and R4b,
including abandonment consuming the lend, server death returning a live caller's lend, and R4
delivering partial replies on `OutOfMemory` (answers 49, 70, 81, 107, 116). Every `call` return
carries lend disposition and reply presence in registers, even on error. The exact encoding is
owned by KERNEL-SPEC.md, IPC return registers: `a0` retains the existing status, `a1` is 0 `none`,
1 `returned`, 2 `consumed`; `a2` is 0 `absent`, 1 `present`; `a3..a7` are zero. No new error code,
input record, call number or wire protocol is introduced.

- Before decoding a recognized `call`, initialize `none` for raw lend `(0, 0)` and `returned`
  otherwise, with `absent` reply. Thus even an earlier bad argument cannot hide retention; this
  does not validate the alleged memory range. Only post-receipt abandonment consumes a lend.
- `present` means the complete output record committed. Output records are validated as readable
  and writable before delivery, then checked again at completion. Restore the lend before output
  so a record inside it remains supported. A failed output commit reclaims newly installed reply
  handles and their otherwise-unused table pages, returns the lend, and reports `InvalidArgument`
  with `absent`; that failure overrides an attempted partial reply's `OutOfMemory`.
- CAPABILITIES.md, IPC, owns the safe runtime's consuming-buffer contract: return ownership only
  when retained; disarm consumed buffers; expose or close every handle in a committed partial
  reply before translating or discarding an error. Other facades must preserve this accounting.

**168. A successful reply reports delivery or discard and the installed slots.** KERNEL-SPEC.md,
IPC completion and IPC return registers, specifies `reply` success as `a0 = 0`, `a1 = 0 discarded`
or `1 delivered`, `a2 = installed-handle mask`, `a3..a7 = 0`. Bits are positional, 0 through
`MAX_MSG_HANDLES - 1`; a discarded reply's mask is zero. `reply` errors keep the existing error
code and all-zero payload and leave the call open. A discarded reply successfully closes the call,
whether abandonment or failed caller-output commit caused the discard. A partial R4 reply is
delivered if its complete record commits, with only its installed slots set in the mask.

- CONTAINMENT.md, the shared server library, owns the transaction rule: retain new grants and
  connections provisionally, roll them and their admission charges back on discard, and check
  required returned-handle slots on delivery. `new_connection` and `keyd`'s `grant` need their
  returned capability; missing it rolls back that new resource. A multi-resource operation needs
  an explicit per-resource policy, not an assumption that syscall success delivered everything.
- Delivery is kernel record commit, not application acknowledgement or a promise against later
  revocation. Ordinary release/disconnect and admission bounds remain necessary for a client that
  disappears after delivery. Neither outcome rolls back prior non-provisional server effects.

**Follow-up: WP-IPC1.** Update `redoubt-sys`, the kernel completion paths, executable model,
`redoubt-rt` and its client/server wrappers, and existing grant/connection servers together. Cover
every lifecycle-table row, both widths' encoding, early errors, partial handle delivery, failed
output commit/rollback, subsequent address reuse and destructor behavior, and real-kernel server
cleanup. Model integration and real timer-based cancellation acceptance remain required before
declaring the package complete; host substitutes alone do not establish these boundaries.

Applied to KERNEL-SPEC.md, CAPABILITIES.md, CONTAINMENT.md, QUESTIONS.md and HISTORY.md. These are
specification changes awaiting implementation; no existing code is represented as conforming.

### Architect clarification after R-IPC1-design (2026-09-22)

The design reviewer asked whether "one kernel completion" protected mapping and lifecycle state
as well as handle tables. It must: answer 167's valid output commit and answer 168's single
delivery/discard outcome require validation, copying, handle installation/rollback and outcome
publication to be protected together against relevant mapping changes and teardown/abandonment.
KERNEL-SPEC.md, Output validity and rollback, now states that requirement explicitly. Equivalent
validated-frame pinning still needs completion arbitration; it cannot permit a reply to commit
while abandonment consumes the same lend. This is the architect's derivation of the accepted
outcomes, not a further owner decision. Post-commit concurrent changes remain outside the promise;
no encoding, status or ownership rule changes. WP-IPC1's concurrent-completion coverage verifies it.
