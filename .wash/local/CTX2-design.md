# CTX2 design note: relay, detach, reattach, takeover, the bounded buffer

Builds on CTX1a (wp-CTX1, a624f8c92) and the approved CTX1 design
(.wash/local/CTX1-design.md, sections 2 to 4; answers A to F). Read: sshd's console.rs and the
login and `ended` paths in bin/sshd.rs; the steward's console handling (bin/steward.rs `console`,
`release`, `consoles`); consoled.md "The `consol` protocol"; libs/wire/tables/consol.md; ipd.md
`/tcp/N/remote`; serving.md `new_connection`. No code yet.

## 0. What a person sees

- `ssh alice.work@box`, work, then close the terminal: the VM keeps running. Output written
  while nobody is attached is kept, the newest 64 KiB.
- `ssh alice.work@box` again: `[context alice.work: reattached; N bytes of output dropped while
  detached]` (the drop count only when N > 0), the kept output replayed, then the prompt redrawn.
- A second login while the first terminal is still attached takes over. The old terminal gets
  `[context alice.work taken over from 198.51.100.2:51234 at up 2h13m]`, and its SSH channel
  closes with status 0. The new terminal gets
  `[context alice.work: reattached; taken over from 203.0.113.7:40022]`.
- `exit` at the prompt ends the context, as today. The UART console session is unchanged: not a
  context, and reopened when it ends.
- sessions.md, sshd.md "Ending" and steward.md (login, `channel_closed`) say plainly that closing
  SSH detaches. steward-session-ends changes with them: closing detaches, `exit` ends.

## 1. Today, and why a relay

The VM's `/dev/cons` (slot 4) is a connection to sshd's per-channel 9P file (console.rs `Cons`).
The steward receives it in `login` and binds slot 4 to it. That file and its server die with the
channel's slot. So the VM cannot keep one `/dev/cons` across channels unless something that
outlives the channel serves it. The approved choice (CTX1 design, section 4 (c)) is a relay per
context, `consrelay`, launched by the steward in the context's own budget beside the VM.

## 2. consrelay

A small native Rust program (`servers/consrelay` or `userland/native/consrelay`; Tier A, since
it holds and forwards a console capability) on the serving skeleton.

- **Toward the VM:** it serves `/dev/cons` as one 9P file, with the semantics of sshd's `Cons`
  (read parks with no input, write, stat length 0, refusals). It also serves `consol` `size` and
  `resize`, as far as sshd serves them (consol is still planned; until then `size` answers the
  last pty size and `resize` parks).
- **Toward the channel:** it is a 9P client of the attached channel's console file, exactly as
  the VM is today. Its calls are stamped with the context's budget, so sshd's label check (R25,
  R67) holds a second time for free: a vault context's relay cannot write to an unlabelled
  channel.
- **Toward the steward:** a control protocol, a new wire table `consrelay`, accepted only on the
  steward's root badge (the VM's minted badge is malformed there, as in every badge-class table):
  - `attach(console: handle, note: string)`: drop any current channel, write `note`, replay the
    buffer, take the channel. The handle is a connection to the new channel's console (section 4).
  - `detach(note: string)`: write `note` to the current channel if any, then drop it. Further
    output goes to the buffer.
  - The relay sends nothing to the steward. Its death is seen as the budget's (section 6).
- **Threads:** three. The server thread owns all state. A reader thread does blocking reads of
  the channel's input and sends each chunk to the server's endpoint. A writer thread does
  blocking writes to the channel from a queue, so a slow channel never stalls the server or the
  VM. On detach the server marks the channel gone, and the reader and writer threads see their
  calls fail when the channel's connection dies.
- **The bound** (rule 9): 64 KiB of output kept while detached, newest kept, the rest dropped and
  counted, and a write never waits while detached. On attach the relay cuts forward to the first
  `\n` (no half escape sequence at the top), writes the note with the drop count, replays, then
  answers any parked `resize` with the new size so the shell redraws. Input while detached: a
  read parks, as with nothing typed.
- **Memory:** the buffer and the stacks are allocated at start, so a VM that later uses up the
  budget's pages cannot starve the relay mid-session. The relay's image comes from `/boot`, as
  the VM's does.

## 3. The steward's side

- **Launch:** the session batch gains, before the VM's launch, an endpoint created in the
  session's budget, consrelay's launch receiving on it, and a minted badge on it bound into the
  VM's slot 4 in place of sshd's console. The steward keeps the endpoint's root badge as the
  relay's control connection. New core steps: `CreateEndpoint` (in a budget) and `Mint`, or one
  `LaunchRelay` step if the embedder can do both. Question G.
- **The console handle and `ended`:** the steward keeps the channel's own connection (sshd's
  badge), as today, so `ended` on release still comes from the steward and sshd needs no change.
  It gives the relay a second connection to the same console, minted with `ninep_common`
  `new_connection` on that handle. So the end of a channel never depends on the relay being
  alive. This needs the steward's `new_connection` call admitted by sshd's console skeleton:
  `LIMITS.buckets` is 2 today (sshd's own and the session's). Question H.
- **Attachments:** each login answers a fresh attachment id (random, R36) as the reply's
  `session`. sshd already uses it only to name the channel to the steward. `channel_closed(id)`
  detaches only if `id` is the context's current attachment; a superseded id answers ok and
  changes nothing. Without this, the old channel's close after a takeover would detach the new
  one.
- **States** (session machine): `Starting -> Attached <-> Detached -> Ending -> Ended`.
  - `Login` with no live context of that name: as today, then `Attached`.
  - `Login` to an `Attached` context: takeover. The effects are `detach_relay(note to old)`,
    `end_console(old)` (the steward's `ended` on the old channel's handle, which sshd closes with
    status 0), `attach_relay(new, note)`, `reply_login`. CTX1a's `in_use` row becomes this row.
  - `Login` to a `Detached` context: `attach_relay`, `reply_login`, to `Attached`.
  - `ChannelClosed` of the current attachment: `detach_relay`, `end_console`, to `Detached`.
  - `Exited`, `EndSession`, `LockedOut`: to `Ending`, as today. The attached channel gets `ended`
    when its handle is released.
  - Guard `current_attachment` keeps the stale-close rule. It is the rule's keeper, with a
    mutation.
- **The note's facts:** `from` is a new `login` field. sshd reads `ipd`'s `/tcp/N/remote` for the
  connection (`addr`, `port`), and the steward checks it is printable `a.b.c.d:port`. The time is
  answer C's: the wall clock if there is one, else `up 2h13m` since boot. The steward builds both
  notes, so the relay renders only text it was given (printable, capped as `FIELD_CAP`).

## 4. Labels, checked twice (rule 4)

- The identity is (account, label set, name), so a login in another label set is another
  context, and no row attaches across domains. A mutation that looks a context up across
  domains cannot be written (the handler sees one domain), and is retired with the reason, as
  for the five on steward.md.
- The relay runs in the context's budget, so each of its writes to a channel carries the
  context's labels and meets sshd's R25 check. The machine case attacks this through a hostile
  path: a vault context's relay is handed an unlabelled channel by a broken embedder build
  (bench feature), and sshd refuses its writes.

## 5. Model and reference

- New events and rows in the core, the Elixir reference and the hand traces. A new property
  P18:
  - an attached context has exactly one current attachment;
  - a close of a superseded attachment changes nothing;
  - a takeover tells both channels;
  - the relay and console of an attachment are its domain's.
- Mutations: `PolicyStaleCloseDetaches` (current_attachment), `PolicyTakeoverUntold` (the old
  channel not told or not ended), `PolicyAttachWithoutEnd`. Reach and catch tables re-measured,
  counts set again, as in CTX1a.

## 6. Failure

- **VM exit or `exit`:** `Exited`, then `Ending`. Destroying the budget kills the VM and the
  relay; the steward releases the attached handle with `ended`.
- **The relay dies alone** (a bug): the VM's `/dev/cons` calls fail and the shell ends, as when
  a console dies today. The steward sees the relay's exit endpoint and ends the context. The
  watcher thread watches both processes' exits (one more watcher per context).
- **Steward restart:** contexts end with it (CTX1 section 6, K23). Unchanged here; CTX3 adds the
  generation.
- **sshd restart:** every channel drops, and the steward gets no `channel_closed`. Contexts stay
  attached to dead channels until the next login of the name takes over, which works, since
  takeover ends the old console regardless. CTX3's idle expiry would never fire for such a
  context; sshd's restart could send `channel_closed` for all, or the steward could detach on
  `watch`'s end. Question I.

## 7. Questions

- **G.** One `LaunchRelay` step in the core, or the general `CreateEndpoint` and `Mint` steps?
  I propose the general steps: the kernel model already has endpoints and mints, so the model
  checks them.
- **H.** The relay's channel connection: the steward mints it from sshd's console with
  `new_connection` (sshd's `LIMITS.buckets` from 2 to 3, so the steward's account is admitted),
  or the steward passes the original and relies on the relay to send `ended`? I propose
  `new_connection`: the channel's end then never depends on code in the user's budget.
- **I.** sshd's restart while contexts are attached: (a) leave them attached until the next
  login takes over; (b) detach all on `watch`'s end from the steward's side. I propose (b).
- **J.** The session's size. The relay is one more process in the session budget, and the
  manifest's `sizes.session` has `processes: 2`, today the VM and one native program the
  session launches. Either `processes` goes to 3, or the relay counts as the second and a
  context launches one native program fewer. The pages are about the relay's image, 64 KiB of
  buffer and three stacks, around 40 pages. I propose `processes: 3` and the pages added; that
  moves CTX3's cap arithmetic, which init checks.

## 8. Tests

- Host tests:
  - consrelay: the bound and drop count, the line cut, writes never waiting while detached, a
    replay on attach, a refused control call from the VM's badge;
  - the core: every new row, the stale-close rule;
  - sshd: `from`;
  - the steward server: attach and takeover batches, and the minted connection.
- Machine cases, both widths:
  - steward-context-reattach (state kept, output replayed);
  - steward-context-takeover (both terminals told, the old channel ends with status 0, the new
    terminal sees the old address);
  - steward-context-labels (a vault context and an unlabelled one with the same name, no output
    crossing; the relay's label attack);
  - steward-session-ends updated (closing detaches, `exit` ends);
  - steward-restart-ssh updated.
- The bench's session `name` (CTX1a) lets a case log in to one name twice.

## Size

L. consrelay with its wire table, about 600 lines and tests. The core's states, rows, effects,
model and reference, about 500. sshd's `from`, the steward's batch and the embedder, about 300.
Five machine cases and the pages.

## Decisions (orchestrator, on the design checkpoint)

- **G:** one `LaunchRelay` step in the core, not general `CreateEndpoint` and `Mint` steps. The
  core's authority stays narrow and the model's surface small; generalize when a second user
  appears.
- **H:** as proposed. The steward keeps sshd's console handle and mints the relay's connection
  with `new_connection`, so ending the old console on takeover is the steward's act. sshd's
  console `LIMITS.buckets` goes from 2 to 3, and sshd.md says why.
- **I:** as proposed. When `watch` ends on an sshd restart, the steward detaches every attached
  context; they keep running detached, and the next login reattaches. A test for it.
- **H, changed in phase 4 (orchestrator, 2026-10-08):** `new_connection` from the steward
  deadlocks: the Attach step runs before the steward answers `login`, while sshd's slot thread,
  the only one serving the channel's console, waits in that call. So (c): sshd mints two rooted
  connections at login and sends both (`login` gains `relay: handle[1]`); the steward keeps the
  first and hands the second to the relay in `attach`; sshd honours `ended` only on the
  steward's badge (`console::Consoles::may_end`), so neither the relay nor the VM (which now holds
  the relay's console, not sshd's) can end the channel. LIMITS stays buckets 2, state 2: two
  mints per login, one login per channel.
- **J:** the relay is a third process in the session budget. Measure its pages and size them by
  B32's rule (twice the peak, rounded up to 128); CTX3 redoes its cap arithmetic from that
  number.

## The red's attack notes to hold (.wash/local/handoffs/steward2-red.md, "CTX1 attack notes")

1. `login_key` stays the first guard; `in_use`, `locked_out` and `cap` refusals come only after
   it, and failed authentication stays cheap.
2. Takeover only after the new login is fully authenticated in the core. The old channel is told
   and ended, with `from` passed through the terminal guard (printable, capped). The old sshd
   slot is freed (K26's GONE path). No window where both channels are attached. A context never
   attaches a channel of another label set, and sshd's channel labels check it again.
3. The 64 KiB buffer is in the relay, charged to the context's budget, never to the steward's
   heap. steward-sub-budget-flood still holds with detached VMs counting.
4. Ids are forgotten on GONE (K26 generation), so a stale channel cannot attach to a new context
   of the same name.
5. Keep P17 (including the non-name contexts) when adding the takeover ops.
6. A new reserved name lands in both lists (sshd `Login::RESERVED`, libs/steward
   `manifest::RESERVED`).
7. steward-context-login's `InUse` line becomes the takeover's lines.

## Implementation plan (implementer's, from reading the code)

### Where the relay's endpoint lives
The kernel charges an endpoint to the budget that created it and frees it only when that budget
is destroyed (kernel/src/endpoint.rs `new_endpoint`, budget.rs `free_owned_endpoints`). If the
steward created the relay's endpoint, each context would cost a page of the steward's budget
for good (red note 3). So **consrelay creates its own endpoint**, in the context's budget. The
steward makes one **hello endpoint** at its start, and per relay mints a hello badge stamped
with the session's scope; it hands that to the relay at launch. The relay mints two badges on
its endpoint, the VM's and the steward's control badge, and sends both on the hello badge. The
LaunchRelay step waits for that hello, bounded (2 s), so the step yields `Made::Relay { control,
vm }`. Connect for slot 4 of a context binds the VM's badge; the console session still gets
consoled's.

### Core (libs/steward)
- Session gains `attachment: u64` (the current attachment's id, 0 when detached; the first one
  is the session's own id, so replies and tests keep their id) and `from: String` (that
  attachment's client address). Index gains `attachments: id -> (domain, session id)`, held in
  `fresh` and removed on detach and forget.
- New state `Detached`. `Running` means attached; the console session is always Running.
- Login rows: login_key, owns_labels, not_locked, then `!context_free` -> `take_over` (raises
  `Raised::Attach { object, attachment, from, key }` for the live session; the provisional one
  ends with Nothing). The takeover is therefore decided only after full authentication: its row
  comes after every authentication row.
- `Login` (new context): carve_session, create_scope, **launch_relay**, **attach_relay**,
  connect, launch.
- Session rows for the raised `Attach`:
  - `Running`: detach_relay (with the note to the old channel), attach_relay (with the note to
    the new one), audit_attached; the reply comes on `Done`.
  - `Detached`: attach_relay, audit_attached, to `Running`.
  - `Starting`: refuse (`in_use`).
- `ChannelClosed(id)` looks the id up in `attachments`:
  - `Running` with `!current_attachment`: `= | reply_ok`, a superseded close changes nothing;
  - `Running`: detach_relay, to `Detached`;
  - `Detached`: `= | reply_ok`.
- New external event `SshdGone`, raised by the server when sshd's `watch` ends, detaches every
  Running context (`Detach` event; the rows mirror ChannelClosed).
- Steps:
  - `LaunchRelay { token, budget }`;
  - `Attach { relay, console: Token, note }`: the embedder takes the login's channel console,
    keeps it under `console`, mints the relay's connection from it with `new_connection`, and
    calls relay `attach`;
  - `Detach { relay, console, note }`: relay `detach(note)`, then release the console, which
    sends sshd `ended`.
- Session batches now finish in Running and Detached too: `Done` rows there are empty or reply,
  and `Failed` ends the context.
- Notes, built by the core from `now`, `from` and the context's name:
  - `[context NAME taken over from FROM at up XhYm]` to the old channel;
  - `[context NAME: reattached; taken over from OLDFROM]` to the new one;
  - `[context NAME: reattached]` after a detach.
  `NAME` is the context's name, or `default`. The relay adds its own drop-count line.
- `Record::Attached { session, key, from, took_over }` for each attach after the first.
- Mutations:
  - PolicyTakeoverBeforeAuth: login_key passes when the named context is live;
  - PolicyBothAttached: detach_relay does nothing;
  - PolicyStaleCloseDetaches: current_attachment passes.
- P18:
  - every attach's key is the principal's login key (P2 extended to Attached records);
  - a relay has at most one console attached, tracked by the model's embedder from the steps;
  - a superseded close changes nothing;
  - P17 is kept.

### Phases (each a WIP commit and a handoff update)
1. Core, tables, gen, Elixir reference, traces, model (host).
2. Wire: the `consrelay` table; `login` gains `from`; sshd reads `/tcp/N/remote`.
3. consrelay: a library with host tests, and its binary.
4. The steward embedder: hello endpoint, LaunchRelay, Attach and Detach, the console's
   `new_connection`, SshdGone on `watch`'s end. sshd's `LIMITS.buckets`.
5. The image: consrelay on `/boot`; measure the relay's pages; `sizes.session`.
6. Machine cases, then pages.
