# The steward's transition tables

One file per machine of the steward's policy core, each included by
[the steward's page](../../../docs/servers/steward.md#machines), which is the specification. This
note is the practical guide to reading and writing one; the generator (`libs/steward/gen`) enforces
every rule here.

## Writing a table

Put a line `<!-- steward: NAME -->` directly above the table, where NAME is the machine's name in
snake_case, unique across the files. The header must be exactly:

```
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
```

Then one row per transition:
- **From:** a state in backticks, several separated by commas, or `-`: the object does not exist
  yet, so the row creates it or refuses the event.
- **Event:** an event in backticks, or several separated by commas. `Done` and `Failed` are the
  two outcomes of the one `Done` event a batch reports: every step succeeded, or one failed. An
  event marked `(ahead)` is answered ahead of admission; every row naming that event carries the
  mark, so the embedder reads it from the table.
- **Guard:** `-`, or one or more guards separated by commas, each in backticks, alone or after
  `!`. Rows with the same From and Event are tried in order, and the first whose guards all hold
  (a guard after `!`: fails) is taken; the last row of each such group has the guard `-`, so every
  case has a row. A guard reads the store and changes nothing. A row with `!g` takes `g`'s reason
  for its refusal. A guard that carries a rule is one function of the `Policy` table, with a
  mutation in the model; a guard that reads only an object's kind (which crossing it is, what a
  request asks for, whether a lease was granted) carries no rule and has no mutation.
- **To:** a state in backticks, `=` for unchanged, or `-` when the From is `-` and nothing is
  created.
- **Effects:** the effects in the order they run, each in backticks, separated by commas, or `-`
  for none. Effects the embedder carries out (a budget, a scope, a connection, a launch, a read or
  write, a destroy) form the event's batch, run in order; the next event of a machine that started
  a batch is that batch's `Done` or `Failed`. Replies, notifications and audit records never fail a
  batch. No effect branches on an object's kind behind one name: each kind has rows of its own.
  `unreachable` alone says the embedder's guarantee excludes the event in that state (a batch's
  outcome arrives before any other event about its object). It is a steward bug: the server fails
  closed, exiting so that `init` restarts it, and the model fails the run.

Events one machine raises for another (`Granted`, a crossing's `Open`, the snapshot or failure a
read crossing passes to its request, `LockedOut`, `SessionEnded`) are the core's own: `decide`
runs them in the order they were raised before it returns. They are never a batch's steps and
never fail one, and one that names an object already gone is dropped without an answer.

Every pair of a non-final state and an event the machine takes has exactly one group of rows. A
final state (one no row leaves) removes the object; an event that names a removed object is
refused by the dispatch, with the same answer as an unknown one. The table ends at the first blank
line, and a row-like line right after it is refused.
