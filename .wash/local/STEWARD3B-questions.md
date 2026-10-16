# STEWARD3B questions (steward3b-implementer, 2026-10-16)

Base 2d831801b. The core's request and approval_channel rows, guards and effects exist and are
traced (request.trace, channel.trace); I add no table row. What is missing is the embedding: the
wire's shape for the channel, the server's channel, sshd's `approve` user, and the notice. The
page's design (steward.md "The powerbox and approvals"; the brief point 2) leaves these open.

## Q1: the wire of `approval_opened`, `pending`, and the notice (nothing of the wire changes without your word)

Today: `3 approval_opened(principal: string, key: bytes) → channel: u64`;
`5 pending(channel) → request, hash, labels, text`; no approval-waiting send.

Proposed:
- `3 approval_opened(key: bytes, console: handle[0] endpoint, reader: handle[1] endpoint) →
  channel: u64`. The SSH user is `approve`: it names no principal, so `sshd` cannot. The
  **server** (protocol.rs) resolves the principal from the key id: the one whose `approval_keys`
  hold it, else the one whose `login_keys` hold it (so the core's `approval_key` guard, the
  keeper, answers `bad_key` for a login key: the core, oracle, traces and model are untouched),
  else `""` → the core's `unknown`. `sshd` mints two connections to the channel's console as a
  login does (`Consoles { steward, relay }`): `console`, through which the steward's main loop
  writes screens (bounded: `Connection::attach_within`, `SCREEN_US`; a write that times out is
  dropped, the person retypes `pending`) and may send `ended`; `reader`, through which the
  steward's reader thread for that channel blocks in `read` for the person's lines.
  Alternative (a2): keep `principal: string` and have `sshd` send `""`; same server lookup.
- `5 pending(channel) → -` (empty reply): the screens are `Output::Screen`s written to the
  channel; the call only asks for them. `6 approve`, `7 deny` unchanged. These three are sent on
  badge 2 by the steward's own reader threads (a send right the steward mints on its own
  endpoint, as it mints `EXITS` for its watchers); nobody else holds badge 2. steward.md's "each
  a `handed` badge in the manifest" is wrong for badge 2 (the manifest hands none): I would
  reword it: badge 2 is the steward's own, for its channel readers.
- `18 approval_waiting` (no fields, no reply): the steward's send on a session's `notices`
  endpoint for `Notice::ApprovalWaiting` (Notified::Session); the shell prints
  `[an approval is waiting]` (Notices.line, badge 1 only, as `lease_ended`).

## Q2: the channel in the server (my design, for your check)

- On `ApprovalOpened` answered Ok the server keeps `channel → (console, reader)`, starts the
  reader thread (stack as the watchers', handed the reader console, the channel id and the
  badge-2 send right through a one-shot endpoint), and raises `Pending { channel }` at once, so
  the person sees what waits. `Notified::Channel(c), ApprovalWaiting` → the server raises
  `Pending { c }` (a redraw). The screens of one event are written as one block per channel.
- The reader thread buffers bytes to `\n` (CR dropped), parses `approve <id:16 hex> <hash:64
  hex>`, `deny <id:16 hex>`, `pending` (the parser is `servers/steward/src/approval.rs`, the
  fuzz target `servers/steward/fuzz` as `servers/init/fuzz`, its corpus replayed by a host
  test), and calls the steward on badge 2; it writes the answer on the reader connection
  (`approved <id>` / `denied <id>` / `refused: hash_mismatch` …). A line it cannot parse answers
  `unknown command`. No echo (the channel has no line discipline): a residual.
- `ApprovalClosed` (sshd's call when the client closes): the server drops the entry and closes
  both consoles; the thread's `read` meets the end and exits. The steward's exit ends sshd's
  watch, as for sessions.

## Q3: the screen (what the approver sees)

```
pending approvals for alice:
request 8f3a9c21d4e5f607 hash 3b2c...64 hex
  request from alice (agent-1, labels []): start an agent labelled [7] for 10 min; reason (untrusted): "..."
type: approve <id> <hash> | deny <id> | pending
```
or `no approval is waiting for alice`. The text line is the core's `Rendered.text` verbatim
(render.rs: printable ASCII); the id and hash in hex; the header names the channel's principal.

## Q4: a granted labelled agent's namespace (own.rs)

`binding()` binds a lease's slot 1 to the sponsor's unlabelled `~/project` whatever its labels;
a labelled lease (R25) could read or write nothing there. Proposed: `Child::Lease { labelled }`;
a labelled lease's slot 1 is its label set's volume (`own.vault`, fresh, root, no quota carve, as
a vault session's `/vault`), at `/project`; the Echo agent then reads `/project/in` and writes
`/project/out/result` in the vault. agents.md "Leases" gets the sentence.

## Q5: the bench

Case 4 must type a random id and hash back. `Step::Expect` keeps its match's capture groups;
`send` expands `$1`..`$9` (`$$` a literal `$`). testbench.md's session block documents it; a
host test covers the expansion. Nothing else of the bench.

## Q6: the pending-cap case (brief 6) against the real rule

`fair_share` = `PENDING_CAP / live` with live = sessions + leases of the domain. alice's
unlabelled domain holds her console session, her SSH session and the agent: share 1. So the case
is: the agent submits PENDING_CAP + 1 notes, the **second** is `cap` (its share); a second agent
(live 4, share 1) still gets one; alice's SSH session's own note is dropped at its exit, and the
next screen lists only the agents'. The brief's "the last is refused" assumed the whole cap: I
follow the rule as the core keeps it, and the case's description says so.
