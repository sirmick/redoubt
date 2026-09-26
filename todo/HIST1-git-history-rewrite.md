# HIST1: clean per-component git history from the fork point

Run after DOC1 has merged, with no packages in flight. The owner decides whether to force-push,
after reviewing `hist/clean` locally.

## Facts

- Fork point: `c0254413a` (bunnie; merge of xous-core PR #1002, 2026-09-15). Upstream history
  up to and including it stays untouched, keeping xous-core's authorship and the upstream link.
- About 385 commits since the fork (mick, plus 8 by Michael Cloonan) are replaced by a clean
  series whose first parent is `c0254413a`.

## Team

| Role | Model | Notes |
|---|---|---|
| Builder | Opus 5.5 (1M), high | Component split, build verification, commit messages |
| Reviewer | Opus 5.5 (1M), high | Checks tree identity, attribution and message accuracy |

## Builder prompt

```
You are the HIST1 builder for Redoubt: produce a clean, per-component git history on top of the
upstream fork point, with a final tree identical to today's. PLAN ONLY first (post QA thread
HIST1-plan); build only after 'HIST1 PLAN APPROVED: build'. You never push; the orchestrator
force-pushes after the owner's explicit go-ahead.

FACTS: the fork point is c0254413a (bunnie, merge of xous-core PR #1002, 2026-09-15). Upstream
history up to it stays untouched. Everything after it (about 385 commits: mick, plus 8 by
Michael Cloonan) is replaced. The current tip is redoubt (after DOC1).

METHOD (a final-state, dependency-ordered component series):
1. Group the tip's tree into components in dependency order, for example: workspace skeleton and
   toolchain; libs (abi, sys, wire and wire-gen, rt, stride, littlefs, signing, paging, flatipc);
   vendored crates; loader and boot; kernel (split into memory, objects/handles, IPC, budgets and
   scheduler, timer, devices/DMA, if each split builds); model; stub; servers (bootfsd, keyd,
   consoled, blkd, netd, ipd), one commit each; testbench and tests; docs. The plan lists every
   path and the commit that owns it, and every path in the tip tree appears exactly once.
2. Build each commit's tree as the previous tree plus this component's final files. The workspace
   Cargo.toml and Cargo.lock list only the members present so far. The last commit's tree must
   equal the tip tree byte for byte (`git diff <tip> <new-tip>` is empty).
3. Create the commits with git commit-tree on a scratch branch hist/clean, with first parent
   c0254413a. Each commit must build (cargo check for its members on rv64 and rv32 where
   applicable); the final commit passes the full bench.
4. Messages: one per component, written fresh. Subject: "<component>: <what it is>". Body: what
   it does, the spec it implements (by section name), its security properties and stated
   residuals, and how it is tested (attack cases, model mutations, fuzz targets). No process
   narrative, package ids, answer numbers or hashes.
5. Authorship: author mick for most commits. Credit Michael Cloonan's work with Co-authored-by
   on the commit or commits that contain it (find it with `git log --author`). Add Co-Authored-By
   lines per the session's attribution rule.
6. Preserve the old history: tag the old tip archive/pre-rewrite and write a git bundle of it to
   ~/redoubt-pre-rewrite.bundle. Delete nothing.

THE PLAN MUST INCLUDE: the component list with path ownership, the build order and each step's
workspace members, the verification script, how any live branches would be moved (git rebase
--onto; the trees are equal), and the risks. REPORT via QA with the verification output.
```

## Reviewer prompt

```
Wait until you receive 'HIST1 review'. Then verify on hist/clean:
- the final tree equals the old tip byte for byte;
- the first parent is c0254413a and upstream history is untouched;
- every commit builds;
- every path is owned by exactly one commit;
- attribution is kept (NOTICE, and Co-authored-by for Michael Cloonan);
- each commit message matches its contents and carries no process narrative;
- archive/pre-rewrite and the bundle exist.
BLOCK on any mismatch.
```

## Publishing (owner's call)

1. Review `hist/clean` locally: `git log --stat c0254413a..hist/clean`, and a fresh-clone build.
2. Force-push `main` on the owner's go-ahead. Old commit links break, and clones must re-sync.
3. Delete stale branches. Move any live branch with `git rebase --onto`.
