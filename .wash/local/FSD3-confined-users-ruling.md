# FSD3: a labelled shared server's users in a confined boot (architect-11)

Ruling: (1). The users rule counted every principal domain as a user of a shared server so that a
server a session may later reach is never missed. In a confined boot a session reaches a server
only through the steward, which grants within one label set by the same rule (init.md, the
confinement check: "the steward enforces the same rule for the budgets and grants it creates
later"). So the domains that may later reach a shared server are exactly those with its label
set. Counting the others refused every confined boot that has a labelled domain and any shared
server, which is what the gap shows; the rule never meant that. (2) would reverse fsd.md's
"required", and (3) leaves R34 unusable with labels. No owner choice: this states what R34
already means and weakens no wall, since a domain with another set still cannot be handed the
server's endpoint (the endpoint kind) and the steward still cannot grant it one.

The rule applies to every shared server, not only labelled ones: an unlabelled `consoled` or
`keyd` is used by the {} domains only, which also clears the gap's v3 (a {7} label set refused
at `consoled`'s device). Unconfined boots are unchanged, and so is the bucket rule, which still
sizes a shared server for every principal domain.

## init.md, the confinement check (FSD3's text at wp-fsd3:178-183)

Replace "and, for a shared server (one that takes `buckets=N`), every principal domain: the same
count as the bucket rule, so a server a session may later reach is never missed." with:

> and, for a shared server (one that takes `buckets=N`), every principal domain with the server's
> own label set. Only such a domain may later be granted a connection there, since the steward
> grants within a label set by this same rule, so a server a session may later reach is never
> missed; a domain with another set is not counted, and the bucket rule still sizes the server
> for every principal domain.

The rest of the paragraph stays. Rewrap to the page's width.

## Tests

- A host test in `init`, listed under "The confinement check" and R34 (each count + 1):
  `host:redoubt-init::confined_counts_only_a_shared_servers_own_label_set`: the gap's v3 manifest
  (alice with the label set {alice-secrets}; a {7} blkd, fsd and client; {} consoled and keyd)
  is accepted, and the same manifest with the client unlabelled is refused at fsd's endpoint.
- If any existing init test passed only because a principal's labelled domain counted against
  an unlabelled shared server, report its name and what it now shows; don't change its verdict
  silently.
- fsd-confined-labelled as ruled (FSD3-label-check-ruling.md) on both widths.
