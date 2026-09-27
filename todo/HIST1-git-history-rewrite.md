# HIST1: a clean, auditable history from the fork point

Rewrite the history since the fork so that every commit message reads cleanly and process churn
is gone, while every real change keeps its place, its date and its author. Runs with no package in
flight (the plan graph holds K7, S2 and BL1 behind it). The owner reviews `hist/clean` locally and
decides the force-push. This file goes away in the rewrite's last commit.

## What the owner decided (2026-09-26 and 27)

- **Reword and fold, not a component squash.** Keep the real sequence of changes: a rewritten
  history that nobody built or reviewed is worse for an audit than an untidy one.
- **Every surviving commit keeps its tree,** except where a fold or a removed path changes it, and
  every such change is listed in the mapping table, so the rewrite can be checked mechanically.
- **Owner-decision commits stay** (open questions, owner answers, design reviews), reworded. The
  one exception is the collusion flip-flop (`7aea39315`, `b0bc1c4c4`, and `42eeedca5` which
  tightened the result): fold into one commit stating the final position.
- **Process churn leaves the history.** QA saves, claims-table edits, merge bookkeeping, handoff
  files, agent-tooling tuning. The QA itself is kept: its final form (`.wash/qa/`, one file per
  thread) arrives in the commit that moved the process to `.wash/`, and nothing in it is lost.
- **Messages** in the book's voice: `<component>: <what>`, a body saying why, no package IDs,
  thread names, answer numbers or review labels in the text. Owner decisions are stated in plain
  words. Where a record exists, provenance goes in trailers (`Plan-Node:`, `QA:`).

## Facts

- Fork point: `c0254413a` (bunnie; merge of xous-core PR #1002, 2026-09-15). Upstream history up
  to and including it is untouched.
- About 610 commits since the fork, 14 of them merges; 57 are beamlet's own history, brought in by
  a subtree merge (`646b4ff9b`). Authors: mick, and 8 commits by Michael Cloonan, which keep his
  authorship.
- A one-line-per-commit survey is in `target/hist/compact.txt` (regenerate it at the tip).

## Method

1. **Paths removed from every commit** (with `git filter-repo`, on a scratch clone), so that
   commits holding nothing else become empty and drop out:
   `docs/WORKSPACE-QA.md`, `.wash/QA.md`, `docs/inventory/qa-log.md`, every `*-HANDOFF.md`,
   `.pi/`, `docs/ARCHITECT-NOTES.md`, and `docs/SWARM.md`, `docs/PROJECT.md` and a root
   `PROJECT.md` before the commit that moves the process to `.wash/` (`1bbc5d2be`, reworded).
   `QUESTIONS.md`, `ANSWERS.md` and their archive copies stay: they are the owner's decisions.
2. **Commits dropped:** the empty commit `04289a39e`; the change-and-revert pair `63b3632bc` and
   `c5884cda1`; the move of the old docs to `docs/legacy` and its later deletion, collapsed into
   the one deletion.
3. **Commits folded** into the work they belong to: parked WIP (`877a13e95`, `262464111` into
   the first state that builds), clippy- and fixup-only commits into the commit they fix, the
   README.html and Pages churn into one Pages commit, the collusion flip-flop into one.
4. **Every remaining message rewritten.** The early `xous64:` prefix becomes the real component;
   beamlet's commits gain `beamlet:`; the large vague commits of 2026-09-22 get messages that
   describe what they did.
5. **A mapping table** (`.wash/local/hist1/map.tsv`): one row per old commit: old hash, action
   (keep, reword, fold into X, drop, emptied by path removal), new subject; and for each fold, the
   commits it absorbs. The owner reviews this table before anything is built.
6. **Build** on branch `hist/clean` from the approved table. Then move any live branch with
   `git rebase --onto`.
7. **Preserve the old history:** tag the old tip `archive/pre-rewrite` locally and write a bundle
   to `~/redoubt-pre-rewrite.bundle`. Delete nothing.

## Checks (the reviewer's)

- The tip tree of `hist/clean` equals the old tip, except for the removed paths.
- Every kept or reworded commit's tree equals its old commit's tree minus the removed paths; every
  fold's tree equals the last commit of its group, likewise.
- Upstream history up to `c0254413a` is untouched; authorship is kept (Michael Cloonan's commits
  stay his).
- Every commit on the first-parent line passes `cargo check` for its workspace members on rv64,
  and rv32 where it built then; the tip passes the full bench and the docs checker.
- Each message matches its diff and carries no process labels.
- `archive/pre-rewrite` and the bundle exist.

BLOCK on any mismatch.

## Team

| Role | Tier | Job |
| --- | --- | --- |
| Builder | frontier (Opus, high) | drafts the mapping table and messages in batches by component, then builds `hist/clean` |
| Reviewer | frontier (Opus, high), read-only | runs the checks above; reviews the table before the build |

## Publishing (the owner's call)

1. Review `hist/clean` locally: `git log --stat c0254413a..hist/clean`, and a fresh-clone build.
2. Force-push on the owner's word. Old commit links break; the mapping table says where each went.
3. Whether `archive/pre-rewrite` is published is the owner's call.
4. Delete the stale branches (`worktree-agent-*`, old `wp-*`, `d3-backup-tip`, `dev`) and the
   `redoubt-design-v1..v4` tags, or move the tags to their rewritten commits.
