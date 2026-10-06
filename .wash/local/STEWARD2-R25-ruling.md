# STEWARD2: minting a connection at an unreadable root (R25) — ruling (a)

**Ruling: (a).** Minting a connection at the caller's own root is not a read, so R25 does not
forbid it.

## Why, citing the rule

R25 (docs/servers/serving.md:548-555): information flows from an object to a caller only if
the object's labels are a subset of the caller's; "metadata (a qid, a `stat`, a directory
entry) is a read of its node". What flows to the minter from `new_connection` is `conn:
handle, id: u64` (libs/wire/tables/ninep_common.md, row 2): no qid, no stat, no name. With an
empty root nothing is walked, and the root's existence is the minter's own attach root, known
to it by construction. So the minter learns nothing.

What the minted handle lets its holder learn is decided at the holder's first request:
`Tattach` goes through `root()` (libs/rt/src/server/ninep.rs:792-799) and `may_read` with the
holder's labels, and every request after is checked the same way
(`labels_are_checked_on_every_request`). A non-empty root keeps every step's check against
the minter (ninep.rs:844-847), so no name leaks. Handing the connection on is the kernel's R1
between the two budgets, as for any handle.

(b) is rejected: the rule is about what flows, not who the caller is; a role test would be a
second rule to keep and to attack. (c) and (d) are not needed.

The brief's Q3/Q4 reading stands: the steward grants connections, the server checks the
session's requests.

## The change

One condition in `make_connection`: with an empty cleaned path, the root is taken without
`may_read`; with a non-empty path, as today. `libs/rt` is the implementer's for this by the
ruling; the report says so.

## Page lines

- serving.md:392-394, the `new_connection` sentence gains: "A mint at the caller's own root
  (an empty path) walks nothing and reads nothing: the reply carries a handle and an id, no
  qid, and the first read of that root is the holder's `Tattach`, checked against the holder's
  labels like every request after it, so a server may hand out a connection to data it cannot
  read itself and learns nothing by it."
- R25's paragraph gains: "Minting a connection is not a read: nothing of the node flows to the
  minter, and the holder's requests are checked against the holder's labels."

## Test, on R25's status list

A host test (`redoubt-rt`): an unlabelled minter mints at a labelled root (succeeds); the
minter's own `Tattach` on the minted connection is refused; a labelled holder's succeeds; a
mint with a non-empty path into that root is refused at its first step.
