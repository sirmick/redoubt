# A shared content store

## Idea

One store shared by every principal, holding packages by the hash of their contents: stored once,
deduplicated, garbage-collected, with read-only code pages shared between users.

## Why it is not a goal

It is ruled out as a channel. A store shared by everyone lets one principal learn what another has
installed: add a blob and time it, or probe whether a hash is already there. Defending that needs
uniform charges and restricted visibility, and the benefit, deduplication, is worth little with a
handful of principals. Each principal's packages live in its own directory
([packages](../servers/pkg.md#per-principal-packages-and-profiles)).

## What it would need

- A reason that outweighs the channel, and the owner's decision.
- Every principal charged as if the blob were theirs alone, whoever stored it first.
- No operation whose answer or timing depends on another principal's blobs.

**Attack cases:** a principal cannot tell, by any answer or timing, whether another has stored a
given blob.
