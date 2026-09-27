# The history rewrite of 2026-09-27

The history since the fork from xous-core (`c0254413a`) was rewritten once, so that every commit
message says what changed and why in plain words, and process bookkeeping is out of the code's
history. Nothing about the code changed: the rewritten tip's tree is identical to the old tip's
(`1ee46ba33`), and every change keeps its place, its author and its dates.

## What changed

- **Messages.** Every message was rewritten: the real component in the subject, the reason in the
  body, no package IDs, question numbers or review labels. An owner decision reads
  `Owner decision: ...`. The trailers naming who wrote a commit are kept.
- **Process files left the history.** Removed from every commit: the QA saves, handoff notes,
  agent-tooling configuration (`.pi/`), the Architect's notes, the old DOC1 QA log, and
  `docs/legacy/`; and, before the process moved to `.wash/` (`09756df73`), every earlier copy of
  the process pages and the claims table. The QA itself is kept, one file per thread, in
  `.wash/qa/` from that commit on.
- **A few commits went.** 53 held only removed files, 35 were folded into the commit they belong
  to (a fixup, a commit that did not build, bookkeeping), and 3 were dropped (an empty commit and
  a change with its revert).

## Checking it

- The old history is the tag `archive/pre-rewrite`.
- [`history-rewrite.tsv`](history-rewrite.tsv) maps every old commit to its action (`reword`,
  `fold:<target>`, `emptied`, `drop`) and its new commit.
- For every new commit, its tree equals its old commit's tree (for a fold, the last folded
  commit's) with the removed paths deleted. There is one exception: at `ab18061d7`,
  `tests/programs/src/bin/sched-latency.rs` is as it was before the dropped change and revert on
  either side of it.
