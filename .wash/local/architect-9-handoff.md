# Architect handoff (architect-9, 2026-10-03)

Working rules: architect-7-handoff.md's first lines. Watch lists: `architect-9-notes.md`.

## Rulings and their files

- K16 data region: 1 MiB, no shrink (`K16-data-region-ruling.md`).
- K16 walks: a live-PID mask, a dense stride queue, a marked reconcile if still needed; 512
  stays unless a walk cannot follow live objects, then the owner decides; table-size walks are
  converted (`K16-walks-ahead.md`).
- K21: the slice (ii) stands (2.7 ms); the flush is not the cost; no memory-layout edit.
- FSD2 (`FSD2-gaps-ruling.md`): every pair checked; `pair_room` per `new_pair`, 0 outside the
  charged root; RESERVE = 0; live roots are never removed or renamed over; rule 5 amended:
  `charge = max(quota, held + reserve)`.
- INIT4 net-attacks: bucket lines dropped; badge counting withdrawn.
- SMP3 and SMP2 briefs: a moving thread needs no fence; R12 is water-filling, the floor leaves
  out capped budgets, nothing changes at one hart.

## Merge checks

Done: FSD1, INIT3 (3 status lines), K21 at 55e1e73f7 (if red's flush trace is clean), RT1 at
a6a09ffc8 (rewrap serving.md:527-530), FSD2 at 28bf55ec6 plus `one_commit_splits_only_as_far_as_its_room`.

Owed:
- FSD2's rule-5 commit: its tests and the two Quotas lines.
- B7: architect-6's list.
- INIT4.
- K16: the mask and queue; R10 <= 30 ms; the audit's 31.6 ms; the visited-vs-live run; the
  1 MiB rows.
- After INIT4: the init step's M1 page edit (INIT4-implementer.md "What closes").

## Open with you

- K19: with the owner.
- RT2's brief: the node body (architect-8-notes.md).
- SMP2's checkpoint: the four model scenarios come to you before kernel work. Rule on them;
  the implementer does not adjust the rule.
- Later: files.md, "a file open across a rename".

## What consumed my context

The SMP2 floor design, reading kernel and littlefs code to rule, and the merge diffs (use
`git diff -U0`).
