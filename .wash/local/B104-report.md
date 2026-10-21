# B104 report: the fingerprint writes no index; testbench.md's guest-time counts recounted

Branch `wp-B104`, worktree `.worktrees/B104`. Base 4750dd61c, head 4ebc5cda4. Tier B, tooling + docs.

## Commits

1. `4cf017da9` testbench: the tree's fingerprint reads the tree and never writes the index
   (`tools/testbench/src/prebuilt.rs`, `docs/testbench.md` "Building once": prose + status line)
2. `4ebc5cda4` docs: the bench's guest-time counts recounted from the case files
   (`docs/testbench.md` "Which cases run in guest time", and the 68 in "On a shared host")

## The fix differs from the node's suggestion

The node's suggestion was `--no-optional-locks`. **It does not stop the write.** Measured on git 2.53.0:
`git --no-optional-locks diff HEAD --binary` still rewrites `.git/index` when a tracked file's
stat is stale. Porcelain `git diff` refreshes the index through `refresh_index_quietly`, which
does not check the optional-locks setting. Both runs below were on the new host test:
- with the flag: `the_fingerprint_writes_no_index ... FAILED` (index rewritten)
- with plain `git diff HEAD --binary` (the old code): FAILED the same way

The fix: the fingerprint takes the changes from `git diff-index -p --binary HEAD` instead. That is
plumbing, which never refreshes or writes the index. It produces the same bytes as `git diff HEAD --binary`.
On this worktree with the change uncommitted both printed the same output (sha256 8778a306…). A file whose
times moved but whose bytes did not prints nothing under either command. `rev-parse` and
`ls-files --others` never wrote the index: checked one by one on a scratch repository, and only
`diff` changed it.

New host test `testbench::the_fingerprint_writes_no_index`. It commits a file, moves the file's mtime back an hour so the
index is stale, runs `fingerprint`, and asserts that the index's bytes are unchanged and that no `index.lock` exists.
It passes with the fix and fails with the old command (shown above). This is the property that rules out a
leftover lock: a command that never writes the index never takes `index.lock`.

Other git calls in the bench (`fmt.rs` `ls-files`, `size.rs` `show`/`merge-base`/`log`) never
write the index; left unchanged.

## Recount: script and command

Script `/home/mcloonan/redoubt/.tmp/B104/bin/counts.py` (B101's counts.py, plus a count per
`icount` value and the `-mttcg` names among the no-reason cases). Command:

    python3 -I /home/mcloonan/redoubt/.tmp/B104/bin/counts.py /home/mcloonan/redoubt/.worktrees/B104/tests

Output at the head:

    boot 352, icount 187, host clock 165
      icount = 'shift=3,sleep=off': 180
      icount = 'shift=3': 6
      icount = 'no-such-option': 1
    disk: 138
    net: 70
    input: 28
    none: 15 [asid-cost-host, console-one-writer, consrelay-footprint, deadline-flood-billed-traced-mttcg,
      deadline-placement, redoubt-ipc, sched-capped-mttcg, sched-exit-churn-mttcg, sched-latency-tcg,
      sched-lock-contention-4-mttcg, sched-wake-no-preempt-harts-mttcg, smp-evict-mttcg,
      smp-lock-wait-4-mttcg, smp-shootdown-mttcg, timeouts-tcg]
    none and -mttcg: 8
    -mttcg elsewhere: beamlet-schedulers-mttcg (disk and input)

The launch note said 351 boot cases, 164 on the host clock, disk 137 and net 69. The tree at 4750dd61c has one more of
each (352, 165, 138, 70). B101's new lock-wait case is one of them. The six `shift=3` cases each have `[disk]` and `[userland]`
(grep). `bench-qemu-early-exit` is the `icount = "no-such-option"` self-check.

Paragraph changes:
- 145 of 226 → 187 of 352, with the 180, 6 and 1 split
- disk 38 → 138, sockets 8 → 70, input 4 → 28, stated as counted under each reason a case has
- the host-time list: added `deadline-placement` and the 8 `-mttcg` cases with no other reason.
  The paragraph had named 4 of those 8 and `console-one-writer`.
- "On a shared host": "A boot case without `icount` (68 of them)" → 165. This figure was stale in the same way.

## Open item for the orchestrator

`consrelay-footprint` runs on the host clock with no disk, socket or input, and its file gives
no reason. The page now says exactly that. Whether to move it under `icount` is not this package's call.

## Gates (run from `.worktrees/B104` with the launch-env exports)

- `make -f scripts/jobs.mk set CASES="host-tests docs formatting"`: exit 0. Results: docs PASS 7.4 s, formatting PASS 27.7 s,
  host-tests PASS 69.8 s (its packages include `testbench`).
- `q run --cores 4 -- cargo test -p testbench --bin testbench fingerprint`: 3 passed (and the
  two negative runs above, each 1 failed)
- `make -f scripts/jobs.mk prebuilt`: exit 0. rv64: 368 cases, 0 failed. rv32: 354 cases, 0 failed. No
  `.git/worktrees/B104/index.lock` afterwards.
- `make -f scripts/jobs.mk set CASES=rustsbi-boot`: exit 0, PASS on both widths in 0.6 s from the
  prebuilt directory, so a run accepts the new fingerprint.
- rustfmt +nightly --check on prebuilt.rs: exit 0 (formatting case above too).

## Affected summaries checked

- `docs/testbench.md` "Building once": updated (prose names diff-index and why; status line
  names the new test).
- `docs/testbench.md` "Which cases run in guest time" and "On a shared host": updated (above).
- `README.md`, `GETTING-STARTED.md`, `CONTRIBUTING.md`, `docs/README.md`: grepped for fingerprint, prebuild,
  index.lock, icount and guest time. No match except GETTING-STARTED's SSH host-key fingerprint, which is unrelated. No
  change needed.
- No other page links to or quotes these sections (grep for the anchor and the old figures).
- Read in full: `tools/testbench/src/prebuilt.rs` and `docs/testbench.md`.

## Fold round 1 (sched5-editor's P2, P2, P3); new head 337199cc9

Branch rebuilt from 4750dd61c: `164cb0f2a` testbench fingerprint, `337199cc9` docs recount
(tree equal to the pre-fold state checked with `git diff`).

- P2 renames/prefixes: the command is now `const CHANGES = diff-index -p --binary -M HEAD`.
  A scratch repository had an edit, a staged add, a deletion, a staged `git mv` with an edit, a mode change and a binary edit.
  On it, `git diff HEAD --binary` and `diff-index -p --binary -M` had the same sha1 (3bfe2145…). Without -M the sha1 differed (7b3a11ca…).
  Plumbing output was unchanged under `-c diff.noprefix=true`, `diff.mnemonicPrefix=true` and
  `diff.renames=false`, so a checkout's diff.* configuration no longer moves the fingerprint.
  `core.abbrev` changes both commands alike.
  New host test `the_fingerprints_changes_are_git_diffs` covers that same set of changes. It runs git with the diff.* settings pinned to git's defaults,
  checks that each header is present, and requires byte equality with `git diff HEAD --binary`.
  Negative run with -M removed: FAILED (`no "rename from renamed"`).
- P2 equal fingerprint: `the_fingerprint_writes_no_index` now also asserts the fingerprint after
  the mtime move equals the one taken before it.
- P3: "every case named `-mttcg` but `beamlet-schedulers-mttcg`" (9 -mttcg files; 8 in the
  no-reason list per counts.py).
- "Building once" prose now says `git diff-index -M`, what `git diff` prints with renames
  included; the status line names the new test.

Gates: `q run --cores 4 -- cargo test -p testbench --bin testbench fingerprint`: 4 passed.
`make -f scripts/jobs.mk set CASES="host-tests docs formatting"`: exit 0 (docs 2.0 s, formatting
27.5 s, host-tests 65.8 s, all PASS). rustfmt +nightly --check: exit 0.
