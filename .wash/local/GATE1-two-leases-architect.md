# GATE1-notice-two-leases: the Architect's answer

## Ruling

Not yet a kernel finding. Both widths miss only once H is live at D's deadline, but the run did
not keep the terms that say where the time went (R10's X-Y, the wakes, lease end). No package is
cut and no target moves until the split below is in. The split is GATE1's own work: a fixture
and trace reading, with no kernel change, so it widens nothing and weakens nothing.

## The candidates, and how the split tells them apart

Measure each of the 9 D leases at seed 4, rv64 first, with the post-check's stdout kept. Use
`sched-trace`'s X, Y, K (pick) and R (requeue) records and K15's audit records. Every term is net
of the audits inside it.

| Term | Span | What a large value means | Owner |
| --- | --- | --- | --- |
| a | deadline -> X | expiry ran late: breaks budgets.md#deadlines | kernel package, cut on this evidence |
| b | X -> Y | R10's walks grow with a second full lease live. K16's brief measures the pump at ~1.2 ms per walk at full fill, and ipc.md's residual says delivery walks every thread twice over. Both scale with H's processes and threads, not with D's | kernel: walks by live thread (below) |
| c1 | Y -> the agent's notice received | the steward stand-in's wake competes with H's runnable budgets | see c2 |
| c2 | the agent's notice -> the sub-agent's | the gap is 20-40 ms gross today, against +8 ms with H destroyed first | see the three readings below |

The three readings of c1 and c2:

- **The stand-in did fixture work between the two receives** (checks, `gone`, `victim_control`).
  Then the sub-agent's sample includes fixture time. This is F, as in GATE1-notice-late §4. The
  fix is GATE1's: each notice is received by a thread already blocked on that slot's exit
  endpoint.
- **K records show H's budgets running between the two receives, at a pass at or below the
  stand-in's.** That is R12 as designed under two full leases. It is a design question for the
  owner, decision_request with a recommendation: the 40 ms target against how a floor wake
  competes.
- **A budget with a higher pass runs first, or the stand-in's own receive entry is long with no
  picks.** A long receive entry means the notice path, delivery's walks, scales with H's table.
  Either one is a kernel defect, and I cut it on the evidence.

My prior is that b, or the receive entry, carries the walks. That is K16 commit 1's subject, and
a second full lease is exactly the load it was told to measure against. The split decides it.

## If the walks are the cause: the package

K16 commit 1 ("walks by live thread, values unchanged") is the fix, but K16 needs GATE1, and GATE1
would then need it: a cycle. So I split commit 1 out as **K18**:
- Tier A, kernel, size S-M. Its brief is K16's commit 1 and its measure ("no target may
  regress"). GATE1's two-lease run at seed 4 must pass on both widths.
- Order: K18 needs K17. GATE1 resumes on K18's merge, unchanged. K16's remaining commits then need
  GATE1 as before; K16's brief loses commit 1 and keeps its "worst walk, measured".

Until then, GATE1 holds its WIP fold on wp-gate1 and changes no target or attacker.
