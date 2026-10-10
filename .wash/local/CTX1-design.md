# CTX1 design checkpoint: named session contexts over SSH

Read: .wash/local/CTX1-brief.md; docs/userland/sessions.md; docs/servers/sshd.md (whole);
docs/servers/steward.md (machines, manifest lines, authentication and sessions, protocol,
authority, R36/R37, failure and restart, residuals); libs/steward/tables/session.md;
libs/wire/tables/steward.md; servers/sshd/src/{lib.rs Login, console.rs}; K23 (6ab5b9845, init
reaps `users` and relaunches) and K26 (0b1ddf839, sshd's `watch` ends a dead steward's channels).
Branch: wp-CTX1 off main 93f933c7a, no code yet.

## 0. What changes for every session

All sessions become contexts. `ssh alice@box` is the context with the empty name (shown as
`default`). So closing SSH no longer ends a session: it detaches it, and `exit` (the VM's exit),
`end_context`, the idle timeout, a lockout or the steward's end ends it. `steward-session-ends` and
the "closing the channel ends the session" sentences (sessions.md "How to use it", sshd.md
"Ending", steward.md login/`channel_closed`) change with it. The console session on the UART
stays as it is: not a context, no name, reopened when it ends.

## 1. The name: parsed in sshd, checked again in the steward

- **Grammar:** `principal[+label][.context]`, in that order, each part 1 to 64 bytes of
  `[a-z0-9_-]` starting with a letter. `alice.work+tax` does not parse (after `.` only a context's
  charset, which has no `+`). No case folding. sshd's `Login::parse` gains `context:
  Option<&str>`; host tests pin the accepted and refused forms (rule 1).
- **Principal names:** init's manifest check refuses a principal whose name holds `+` or `.`
  (`.` is not in the name charset today; `+` neither), and one equal to a reserved name. Reserved:
  `approve` (and any later terminal name); a login `approve+x` or `approve.x` is refused by both
  sshd's parse and the steward (rule 7).
- **Question A:** today's name rule (init.md "Names", sshd's `name`) allows `:`. The brief says the
  charset *stays* `[a-z0-9_-]`. I propose principals, labels and contexts lose `:` (it is for
  server names like `walfsd:data`), checked by init for principals and labels. Confirm.
- **sshd does not decide.** It parses only to split the user name, never refuses on the parse
  before the signature: a name that does not parse goes to the steward as principal `""`, which
  is never valid, so every refusal takes the same path (rule 3, below).
- **The steward checks again** (rule 2): `login` gains `context: string` (and `from: string`,
  section 3). The steward re-checks each part's charset and length, the reserved names, the key
  against the principal alone (`login_key`), then the label's ownership (`owns_labels`), then
  looks up or creates the context. The label and the context are chosen only after the key has
  matched.
- **Rule 3, no enumeration.** Before authentication sshd answers key queries by `keyd`'s `holds`
  alone (as today), so nothing about principals leaks there. After a verified signature, an
  unknown principal, an unknown or unowned label, a bad context name, a reserved name and a
  malformed user name all get the steward's one refusal (`bad_key`) on the one path: the key-id
  hash is computed and compared in every case, the label and context steps run only after it
  and are pure table lookups. Contexts are created on first use, so "unknown context" is only a
  malformed one. **Question B:** confirm contexts are free names created on first login (up to
  the cap), not declared in the manifest.

## 2. The steward's context table and its states

- **Identity** (rule 4): `(account, label set, name)`. Table per domain `(account, label set)`,
  as sessions are numbered today, so nothing is shared across a principal's label sets (R37).
- **Entry:** name, the session id (random, R36), the steward instance's generation (section 6),
  the session budget, the relay's control endpoint (section 4), state, `detached_at`, the
  attached channel's session id at sshd, and `from` of the attached client.
- **States** (the session machine grows; one table, generated as today):
  `Starting -> Attached <-> Detached -> Ending -> Ended`.
  - `Login` with no live context of that identity: as today's row, to `Starting`, then `Attached`.
  - `Login` for an `Attached` context: `Takeover` (stays `Attached`, effects `notify_old`,
    `detach_channel`, `attach_channel`, `notify_new`, `reply_login`).
  - `Login` for a `Detached` one: `attach_channel`, `notify_new`, `reply_login`, to `Attached`.
  - `ChannelClosed` (and sshd's end) from `Attached`: `detach_channel`, to `Detached`.
  - `Detach` from the session's own badge: as `ChannelClosed`, and sshd closes the channel.
  - `IdleExpired` from `Detached` (the steward's timer): to `Ending`.
  - `Exited`, `EndSession`, `EndContext` (section 7), `LockedOut`: to `Ending`, as today.
  - `Login` past the cap: `refuse` (`cap`), nothing carved (rule 6).
- **Model:** the steward core is modelled; the new events join `steward_policy` and
  `steward_noninterference`, with mutations (`PolicyAttachOtherSet`, `PolicyEvictOldest`,
  `PolicyListOtherSet`, `PolicyEndOtherSet`, `PolicyAttachAcrossGenerations`) and the Elixir
  reference and hand traces for every new row. This is most of the package's size.

## 3. Attach and takeover across sshd's channels

- The VM's `/dev/cons` is no longer sshd's channel console: it is the context's **relay**
  (section 4), which holds the current channel's console connection while attached.
- **Attach:** sshd mints the channel's console as today and passes it in `login`; the steward
  hands it to the relay (`attach(console)` on the relay's control endpoint), so the bytes go
  VM <-> relay <-> sshd. The steward replies to `login` with the session id, the name and the
  labels, as today.
- **Takeover** (rule 5): the steward has the relay write one line to the old channel (`[context
  alice.work taken over from 203.0.113.7 at 1234.5 s]`), then `detach` (the relay drops the old
  console connection) and sends `ended` on it, so sshd closes the old SSH channel with status 0;
  then attaches the new channel and the relay writes `[context alice.work: reattached; taken over
  from 198.51.100.2]` to it. Both terminals are told.
- **The client's address:** sshd reads `ipd`'s `/tcp/N/remote` for the connection and passes it
  as `from` in `login` (printable `a.b.c.d:port`, checked by the steward).
- **Question C, "the time":** the box has no wall clock until M6 (beamlet's `system_time_us` is
  `None`). I propose the time since boot (`time_now`), labelled as such. Confirm, or drop it.
- **Labels, checked twice** (rule 4): the steward attaches only a channel whose login's label set
  is the context's (it is part of the identity, so a different set is a different context); and
  the relay runs in the context's budget, so its writes to the new channel's console carry the
  context's labels and meet sshd's label check (R25/R67) a second time. The machine case
  attacks it.

## 4. The detached console buffer, and who holds it

Three places it could live; I recommend (c).
- **(a) In sshd**, a console per context instead of per slot. sshd's state is per connection
  slot (4 slots, two threads each); detached consoles outlive slots, so sshd would need threads
  or a multiplexed server per detached context, and an sshd restart (which today only drops
  channels) would end every context's console. Cheapest in processes, most change to sshd.
- **(b) In the steward**, as the brief says: the steward serves every context's `/dev/cons` and
  relays every byte. That puts the policy server on every session's byte path, with its work
  paid by the steward (a stated residual today), one more hop per keystroke, and a thread or a
  multiplexed server per context in the steward.
- **(c) A relay per context, a small native program `consrelay`, which the steward launches in
  the context's own budget beside the VM** (recommended). It serves the VM's `/dev/cons`
  (the `consol` protocol: bytes, `size`, `resize`, the interrupt), holds the current channel's
  console connection while attached, and the buffer while detached. Its pages and CPU are the
  principal's (R6), its sends carry the context's labels (the second label check comes free),
  and it dies with the context's budget. The steward stays off the byte path and holds only
  each relay's control endpoint (`attach(console, note)`, `detach(note)`; a new wire table).
  sshd is unchanged but for `from` and the name.
- **The bound** (rule 9): 64 KiB of output per context while detached, the newest kept. Past it
  the oldest are dropped and counted. On reattach the relay drops forward to the first line
  boundary (no half escape sequence at the top), writes `[N bytes of output dropped while
  detached]` if N > 0, replays the buffer, and reports the new window size as a `resize`, so the
  shell redraws its prompt. While detached a write never waits (the VM is never blocked by a
  missing terminal); input waits, as with nothing typed.
- **Question D:** (c) adds a program and a protocol; the brief puts the buffer in the steward.
  Confirm (c), or name (a) or (b).

## 5. The cap and the idle timeout: manifest keys

- Per principal, optional, in the manifest's `principals` entry: `"contexts": { "max": N,
  "detached_secs": S }`; defaults `max` 4 and `detached_secs` 86400 (one day), from a
  manifest-wide `steward.contexts` default if given. init checks `max` 1 to 16 and `S` 60 to
  604800, and writes them into the principal's manifest line as `contexts=N detached=S` (the
  core's parser and writer, as today).
- **The cap is per (principal, label set)** (rule 6): live contexts, attached or detached, the
  default included. A login that would create one more is refused `cap`; nothing is evicted.
  The check runs after authentication and before any carve, so a failed login allocates nothing.
  The session size times `max` must fit the label set's sub-budget; init checks that.
- **The idle timeout** ends a context `S` seconds after it was last detached; an attach clears
  it. The steward's loop already waits with a deadline; the earliest detached deadline is its
  next timeout. (A budget deadline would do it in the kernel, but a deadline cannot be cleared
  on reattach.)

## 6. The restart rule (rule 8): contexts end with the steward

Recommended: **contexts end with the steward**, as sessions do now (K23).
- **How:** unchanged mechanism. `init` reaps `users` (every context's budget, its VM and relay
  with it), the new steward starts on an empty `users` (its start check), and sshd's `watch`
  (K26) ends every attached channel with `the steward is gone`. The steward's table is in its
  memory only, so nothing can reattach to a dead context; each entry also carries the steward
  instance's generation (a random 64-bit word drawn at start), so the rule holds in the code as
  well as by the reap, and a test can attack it.
- **Cost:** a steward crash (a bug: the steward is in the trusted base) ends every detached
  context's work, as it ends every session today. Detached work is lost on a bug, not on
  disconnect.
- **The alternative, re-adoption by (id, generation), costs:** `users` is no longer reaped, so
  K23's "a dead steward's sessions go with it" and its start check go; the new steward needs a
  handle to each surviving budget and process, which the dead steward's handle table took with
  it, so the kernel needs a way to find a child budget by an id the steward set (or `init` must
  hold them); the table (ids, generations, names, relay endpoints) must outlive the steward,
  in `init` or a store we do not have before M6; the shared servers' connections the dead
  steward minted for those sessions die with its fresh connections (K23's disconnect), so every
  surviving VM's namespace would be dead anyway and need rebuilding. That is kernel, `init` and
  storage work for a crash that is a bug. Not in CTX1.

## 7. Authority for listing and ending contexts

- **Through the session's own `steward` connection**, the named handle every session already
  holds (a minted badge stamped with the session's account and labels by the kernel, R14). New
  operations on that badge class: `contexts` (list), `detach`, `end_context(name)`. The steward
  answers from the caller's domain `(account, label set)` as the kernel stamped it, never a
  claim. No new handle: the badge is the grant, and it dies with the session.
- `contexts()` lists the caller's domain only: name, attached or detached, seconds since its
  last detach. Never another label set's, never another principal's (R37: a vault's contexts
  are invisible to the unlabelled side and the reverse; the cap is per set, so counts do not
  cross either).
- `end_context(name)` ends a context of the caller's domain only (its own included); any other
  name is the same `unknown` as one that does not exist. `detach()` detaches the caller's own.
- **A labelled session starts nothing** (P8) still holds: listing, detaching and ending start
  nothing. **Question E:** the brief says the steward "grants this through a handle". I propose
  the existing `steward` connection is that handle; a separate revocable handle buys nothing
  that destroying the session does not already revoke. Confirm.

## 8. Tests per rule, and the machine cases

Host tests (sshd core, the steward core and server, init's check, consrelay):
1. canonical parsing: accepted and refused forms in sshd and again in the steward
   (`alice.work+tax`, a 65-byte part, upper case, `:`, an empty part, `approve.x`).
2. the steward refuses a login whose context or label is set for another principal, and checks
   the key before either (a login with a wrong key and a valid label/context is `bad_key`).
3. every pre-attach refusal is one reply on one path (the steward's same `bad_key`, the key-id
   hash computed in each case); a malformed name reaches the steward as `""`.
4. `alice.work` and `alice+tax.work` are two contexts; a login of one never attaches the other;
   the relay's console write on a channel of another label set is refused by sshd (R25).
5. takeover: the old channel gets the line and `ended`, the new one the line with `from`.
6. the cap: the next login is `cap` and the oldest is still `Detached`; no carve was made.
7. reserved names refused by sshd, the steward and init.
8. after a steward restart a login of the same name starts a new context (new generation), and
   an entry of another generation never attaches (`PolicyAttachAcrossGenerations`).
9. consrelay: the bound, the drop count line, the cut at a line boundary, writes never wait
   while detached.
Plus the model's mutations (section 2) and the reference traces.

Machine cases, both widths (new `[[session]]` steps in the bench as needed):
- `steward-context-reattach`: alice.work defines `x = 41`, disconnects, reconnects, `x + 1` is
  42; output written while detached is replayed.
- `steward-context-takeover`: two connections to alice.work; the first sees the takeover line and
  its channel ends; the second sees the line with the first's address.
- `steward-context-cap`: `max = 2`; alice.a and alice.b detached, alice.c refused `cap`, alice.a
  still reattaches.
- `steward-context-labels`: alice+tax.work exists; `alice.work` is a different, new context, and
  no output of the vault context reaches the unlabelled channel.
- `steward-context-restart`: with the restart probe, a detached alice.work is gone after the
  steward restarts, and alice.work is a fresh VM (its state is not there).
- updated: `steward-session-ends` (closing detaches; `exit` ends), `steward-restart-ssh`.
- The idle timeout: a host test with a short `detached_secs`; a machine case at the 60 s minimum
  if wanted (it waits, so it is quiet-class and slow).

## Size and tier

Tier A, L: the steward core, its tables, model, reference and traces; sshd's parse and `from`;
a new program (consrelay) and protocol; init's manifest check; the shell's three commandlets;
five new machine cases and two changed. I would split it: CTX1a (core, model, reference, sshd
parse, init check, no relay: a detached context keeps no output), CTX1b (consrelay, the buffer,
takeover lines), CTX1c (shell commandlets and pages). Question F: split or one package.
