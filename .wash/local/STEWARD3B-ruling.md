# STEWARD3B ruling (thread `STEWARD3B-channel`): the wire and the namespace; Q2-Q6 checked

Question: `.wash/local/STEWARD3B-questions.md`.

## Q1, the wire: accepted as proposed

- `3 approval_opened(key: bytes, console: handle[0] endpoint, reader: handle[1] endpoint) →
  channel: u64`. The SSH user `approve` names no principal; the server resolves it from the key
  id: the principal whose `approval_keys` hold it, else the one whose `login_keys` hold it (so
  the core's `approval_key` guard answers `bad_key`, and the core, oracle, traces and model stay
  untouched), else `""` → `unknown`. R38 holds: only the approval key opens a channel, and the
  refusal tells a login key from an unknown one only inside the steward (sshd shows one refusal).
- `5 pending(channel) → -`: screens are outputs written to the channel; the call asks for one.
  `6 approve`, `7 deny` unchanged. The three come on badge 2 from the steward's own reader
  threads: badge 2 is a send right the steward mints on its own endpoint (as `EXITS`), held by
  nobody else; steward.md's "handed in the manifest" is rewritten for it.
- `18 approval_waiting` (no fields): a send on a session's `notices` endpoint; the shell prints
  `[an approval is waiting]`, badge 1 only, as `lease_ended`.

Two things to state with it: the number of reader threads is the core's cap on open channels
(name the constant), each a thread of the pool's stack size, so the steward's thread count stays
bounded and declared (MEM1's stack rule, beamlet.md's table of bounds has the shape); and a
reader's call on badge 2 is served by the main loop, which never waits on a reader (its screen
writes are bounded by `attach_within`), so the pair cannot deadlock: say so in the doc comment.

## Q4, the namespace: accepted, with the scope stated whole

A labelled lease's slot 1 is its label set's vault, whole, at `/project` (fresh, root, no quota
carve: the vault's own quota, shared as a vault session shares it). R25 contains it: nothing of
the unlabelled `~/project` is reachable, and nothing outside the label set. agents.md "Leases":
"a labelled agent's `/project` is its label set's vault: the vault is the task's scope; an
unlabelled agent's is its sponsor's `~/project`, with a quota carved from the home." The
unlabelled carve (STEWARD3A's `sizes.agent.quota`) does not apply to a vault lease.

## Q2: accepted

One reader thread per open channel blocking in `read` on sshd's second connection; the main
loop writes screens on the first with `attach_within` (`SCREEN_US`), a timed-out write dropped
and the person retypes `pending`; `ApprovalOpened` raises a `Pending` at once; a `Notified
ApprovalWaiting` redraws. Bound the reader's line length (a line longer than the parser's
longest form is refused on the channel, not read further into memory).

## Q3: accepted, with a usable binding code

The screen: a header, per request `request <id> hash <hash>` and `Rendered.text` verbatim, a
`type:` line. A person cannot type 64 hex characters reliably; the binding needs the hash the
person saw, not its length. The screen shows beside the full hash its first 8 hex as the code
(`code 3fa9c0e1`), and the server accepts `approve <id> <hash>` with the full hash or that
8-hex code, which it expands to the full hash from the last screen it wrote on that channel
(the one place it can come from); an id or code not on the last screen is `HashMismatch` to the
channel only (C9). The core's `Approve` still takes the full hash: nothing of the core moves.
Say why on sshd.md's `approve@box` paragraph: the code binds the approval to the screen the
person saw, as the full hash does, and the screen is the steward's alone to write.

## Q5: accepted

The bench's `send` expands `$1..$9` from the last `expect`'s captures: a testbench change with
its host test and a line on testbench.md's session table; steward-red-2 reviews it with the
package.

## Q6: accepted

The case follows the core's rule (`fair_share = PENDING_CAP / live`): with alice's console
session, her SSH session and the agent live, the agent's share is 1 and its second note is
`cap`; a second agent still gets one; a dead session's requests are gone from the next screen.
The brief's "the last is refused" assumed the whole cap; the case's description says what the
rule is and the page states the share.
