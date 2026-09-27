# Paths the docs checker names that no longer exist

## What

The docs checker names two paths that are gone. Its process-reference rule skips
`docs/SWARM.md` and `docs/PROJECT.md`, which have left the book, and its list of directories that
hold no pages still names `docs/legacy`, which has been deleted.

## Why it matters

Neither changes a verdict today: a page that does not exist is never checked. But an exemption
that points nowhere is a door left open: a page added later under either name would skip the
rule it names without anyone deciding it should.

## Where

`tools/doccheck/src/lib.rs`: `milestones_and_process` (the two page paths) and `EXCLUDED`
(`docs/legacy`).

## Done when

- The process-reference rule applies to every page, with no path exempt.
- `EXCLUDED` lists no directory that does not exist, or is removed if it is empty.
- The checker's fixtures still show each rule firing on its bad fixture and not on the good tree.
