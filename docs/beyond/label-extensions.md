# Label extensions

## Idea

- **Taint-on-read:** a budget that reads labelled data gains the label at that moment and loses
  what the label forbids (its network, say).
- **Integrity labels:** low-integrity data, such as a model's output, cannot reach a high-integrity
  sink without an endorsement.

## Why it is not a goal

Labels are fixed when a budget is created, and reading above one's labels fails
([the servers](../servers/README.md#labels)): an unlabelled agent that stumbles on labelled data gets
an error and keeps its network, and nobody can taint someone else. Taint-on-read would let an
accidental read cut an agent off, let labelled data be planted on others to cut them off, and leak
through siblings started before the read. Integrity labels answer a different threat, bad input
reaching a trusted consumer, which no milestone claims to stop.

## What it would need

- For taint: a rule for everything the budget already shares (siblings, open connections, parked
  calls) at the moment of the read, and a defence against planted labelled data.
- For integrity: a second label dimension in the kernel and in the label check, and an endorsement
  step with its own approval.

**Attack cases:** for taint, planting labelled data on a victim does not cut it off; for integrity,
unendorsed low-integrity data never reaches a high-integrity sink.
