# Architect handoff (architect-17, 2026-10-16, checkpoint at 70 % context)

File copy: .wash/local/handoffs/architect.md (identical).

## Branch state

No branch of mine. I work in the project root on `main` and committed nothing: every artefact is a file under `.wash/local/` (rulings, briefs, this handoff) or a plan node. The root working tree carries the orchestrator's uncommitted `.wash/` edits, not mine. Nothing is lost if this process ends now.

## Traps (read before anything else)

- Message sizes: a QA-thread or orchestrator message is at most 2000 bytes, counted strictly; aim for 1,700. A `plan_set` node body is at most 2000 bytes and a title at most 200 bytes: titles over ~150 characters bounce. Put the detail in a file and name it.
- `plan_set` with `needs` blocks the node until those are done; for nodes meant to run beside an active package set `needs: []` (I once blocked five nodes on B50 by mistake).
- Do not tell the orchestrator a node is drafted until the `plan_set` returned the tree.
- Rulings go on the QA thread (`thread_id` + `reply_to`); the orchestrator and the implementer both read it; the thread is the record.
- Every ruling is a file in `.wash/local/` the thread message names; amendments are appended to the same file. Implementers build from the file.
- Owner territory (decision_request): a new native, a new wire opcode, a kernel ABI or 9P change, a target or its definition moving, a page leaving the book, sessions per principal resizing. Mine: a serving-library rule, a wire-table field in Redoubt's own sideband (`ninep_common`, the steward's table), an admission derivation, a page sentence.
- Facts from implementers can be wrong (STEWARD3R's "stamped root" was not): read the lines they cite before ruling.
- `plan.toml` is rewritten by agentd (hand edits revert); `.wash/local` files are the durable medium; the steward core (libs/steward) moves only with its Elixir oracle and the model's reach table together; a model generator change must draw no extra random values.

## Open threads and their state

- STEWARD4-crossing (STEWARD4 implementer 2, active): Q1-Q12 ruled, all in `.wash/local/STEWARD4-ruling.md` (Q1 init's confine change in scope; Q2 crossing stamped with the labelled side's account; Q3 latency-probe in the steward binary under three conditions; Q4 too_big by a pre-stat; Q9 the trace dump via six probe exits with a LATENCY-END marker and a ring check, (b) refused; Q10 the confinement check skips exempt servers, bounded by the sentence in the file; Q11 spinners over six principals, overtaken by Q12; Q12 the box admits eight steward-minted system-volume connections: STEWARD4 measures N = 1, 2, 3 under alice alone, the table beside the stand-in's 1/4/16 with the stated sentence; the fix is STEWARD3Z). Awaiting: the trace account of the first latency round (rv64 N=1: decision wake p50 28.7 / max 45 ms vs 25/95; timer wake p50 75 / max 77 ms vs 15/50; deadline notice p50 368 / max 397 ms vs p99 40; destroy call-to-return 292/310 ms vs 51). My reading, not yet sent: the timer-wake miss is not a destruction-size matter; look at the real steward's budget weight and share against the spinner VMs (the stand-in had weight 1000 from `system`); the deadline-notice and destroy misses are the cost of destroying a beamlet VM (sizes.agent, 5,248 pages) instead of small budgets. When the account comes: targets do not move for a measurement; if the measured quantity is a different thing (a VM's destruction), the page frames it so beside the unchanged table and the finding is a node (R10's cost scales with the subtree: K9's rule); a target or its definition moving is the owner's. B79 landed on train 47; 3Y on train 48; the implementer rebases twice.
- STEWARD3Z-admission (just launched on `.wash/local/STEWARD3Z-implementer.md`, reviewer steward-red-4): primary lever the serving library's re-homing of a minted connection to the first caller's account bucket (pending slots bounded by the minter's share), fallback a root badge per principal; consoled's limits from the manifest. Expect questions on the pending pool's R26 bound and the hotspot with STEWARD4 (drive.rs, the latency manifest).
- B80 (open, not mine so far): rv32 beamlet-schedulers-one-hart-mttcg's ratio follows the host's load; a shared-host-rule question may come.
- Closed this session: B51-completion-hold, B62-design, STEWARD3R-scope (with the (a1) amendment), STEWARD3A-notice ((a2)), STEWARD3A-sponsored-login ((A) + Q3 + Q4), STEWARD3B-channel, STEWARD3C-shared-restart, STEWARD3X-rebind (Q1-Q5, Q4 amended).

## Files I own

`.wash/local/`: B51-ruling.md, B62-ruling.md, AGT4-brief.md, AGT4-ruling.md, STEWARD3R-ruling.md, STEWARD3A-Q1-ruling.md, STEWARD3A-Q2-ruling.md (also Q3, Q4), STEWARD3B-ruling.md, STEWARD3C-Q1-ruling.md, STEWARD3X-ruling.md, STEWARD4-ruling.md (Q1-Q12), STEWARD4-implementer-2026-10-16.md (the refreshed brief; the 2026-10-05 one stays), STEWARD3Z-implementer.md. Nodes drafted: B66-B78, ATK2-ATK9, AGT4, the STEWARD3 split (3R/3A/3B/3C), 3Q, 3S, 3X, 3Y, 3Z.

## Standing rules I applied

- Pages carry no dates, package ids or decision ids; a built section has no `**Open:**`; a planned section exactly one; every `### R<n>` has a SECURITY.md row; `mutation:` names exist in `model/src/mutation.rs`; links resolve on the committed base.
- A number on a page shows the arithmetic that produces it; declarations are twice the measured peak; a bound is measured once on both widths, never guessed.
- Targets never move for a measurement; a miss is a finding. Admission constants are derived from the manifest (3Q), stated as the derivation, held by a bench host test.
- Cutoff vs cleanup (3R): destroying a scope or budget is the cutoff; disconnects and `ended` sends are cleanup, deferred and short-bounded (3Y), never on the serving loop with a full timeout.
- A wait is bounded by its own deadline (B62); the client's completion call has no clock (B51); the fake kernel's tick is quiescent (B74).
- A test-only feature in a Tier A binary is acceptable only as `restart-probe` is: off by default, named on the page, a host test that nothing enables it but its case, raising only existing core events.
- A flaky test is a bug; a failure beside other work is no verdict; a case orders on events, not the host's clock.
- The owner decides product and capacity choices; design mechanics are mine; every ruling says which.

## The owner's M3 order and what I see next

STEWARD3 done (3Z open). STEWARD4 active. Then by the plan: `sshd` (vault sessions over SSH), BEAM5 (gen_tcp over /net), the agent-suite, model-replay. Follow-ups the session surfaced: leases re-bound after a server restart (3X's (c)); a `leases` listing for sponsors; the agent principal sized by its own `sizes` entry; the read-only connection no server takes; slot 5 (`erofsd:system`) restart leaving sessions dead; kernel-attack-gaps' remaining pages (ATK5-ATK9 drafted; the owner asked for no attack-gap packages ahead of agent work).

## What consumed my context

The plan tree printed in full by every `plan_set` (~10 KB each, ~25 calls, a third of them resubmits for a title or body over the limit); reading implementers' question files and reports whole where a section would have done; message bounces on length. Write node titles under 150 characters and bodies under 1,800 bytes on the first try; read question files by section; keep every ruling's detail in its file.
