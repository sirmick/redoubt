# Package coordination

Owns package state and execution rules. [BUILD-PLAN](BUILD-PLAN.md) owns remaining deliverables
and acceptance; [STATUS](STATUS.md) describes current behavior. User instructions govern the
current session; this document describes the project's swarm workflow.

## Roles and execution

- The orchestrator maintains claims, assigns independent work in isolated worktrees and integrates
  one package at a time. The kernel track has one writer. Reconcile external kernel work before a port.
- The resident architect resolves questions from the specifications and records genuine owner
  decisions through `.pi/skills/architect-qa/SKILL.md`. Implementers do not invent answers.
- Each active package has one resident implementer and three resident reviewers: defensive,
  simplifier and code/documentation editor. They retain context through fixes and review rounds
  until acceptance or explicit abandonment. Refresh the diff and evidence each round. Implementers
  own their paths/tests; reviewers inspect without editing. The orchestrator commits and integrates.
- Stage only owned files; inspect other sessions' changes before staging. Do not use `git add -A`
  or `git commit -a` in a shared worktree.
- Start packages when their dependencies are merged. Stop only dependent work for an unresolved
  design decision. Update the owning contract and approval record when a decision is accepted.
- Acceptance requires the package's tests and attack cases, a green full bench, rv32 compilation,
  no undocumented unsafe and no unexplained ratchet increase. Rebase and retest before integration.
- Review may be batched for small changes; TCB/security work gets its own risk-bounded round and
  defensive reader. A package merged before review is `merged (review due)`, not done. Clear review
  debt before the next wave; record only outstanding debt here. Evidence belongs with the change.

[PROJECT.md](../PROJECT.md) owns Wash setup, profiles and exact MCP examples. Use the bulk
`workspace_configure` interface with stable member keys, `package` tags and resident lifetimes;
reuse sessions via `assignment_update`. Completing an assignment does not end a resident.
End all four package residents with `member_control` only after acceptance/abandonment.
Ephemeral agents are for bounded auxiliary tasks. `.pi/agents/` retains role responsibilities;
PROJECT overrides legacy fresh/ephemeral lifecycle assumptions. Workflow scripts are references,
not an automatic Wash runner. Acknowledge inbox messages and end the turn after setting
`member_update.waiting`; never poll. Preserve running Wash and keep build artifacts on the project root's filesystem.

## Questions and acceptance evidence

QA is a first-class Wash record, surfaced as a live Questions Markdown tab. Wash is its sole
writer; configure `qa_document` as `docs/WORKSPACE-QA.md` when setting up the workspace.
Wash creates or loads that filename, then writes complete Markdown history after QA changes and human answers.
Reusing it restores Wash checkpoints (threads, attribution and pending owner decisions); ordinary
Markdown is preserved. Reassign reopened unfinished questions to the current team. Conflicting
active owners or corrupt checkpoints fail without overwriting the file. The Questions
tab shows its path and any write failure. Concurrent implementers append through MCP rather
than editing or committing the generated file. Open a stable package thread using `message_send.qa`; carry `thread_id` and `reply_to`
on questions and answers between implementer, Architect and reviewers. Wash records the actual
authors. Replies append atomically; reassignment/blocking/resolution/reopening require the current
thread revision through `member_update.qa_updates`. Reconcile conflicts, never overwrite history.
A tracking update alone does not wake the next responder; send a linked actionable message too.

The resident Architect alone writes formal QUESTIONS/ANSWERS and specification changes through
`.pi/skills/architect-qa/SKILL.md`. Cite settled decisions; send unresolved owner choices through
`decision_request` linked to the thread. Only actual owner responses authorize decisions; answers
are recorded but do not close QA automatically. Update formal contracts, link decision references,
return work to the implementer, and verify with the package reviewers. Only the orchestrator or
that package's reviewers may resolve, with evidence; pending human decisions prevent closure.
Reopen with a reason when evidence changes. No unresolved blocking QA at package acceptance;
record deferred nonblocking questions explicitly. Store accepted evidence in the owning project
records; the orchestrator can commit the continuously generated Markdown as review evidence.
Check qa_document_status before acceptance/teardown; write failures retain backend records
and retry automatically, including after teardown and backend restart. workspace_end attempts
the final save and returns its status. Preserve and reuse the generated filename after teardown.

Browser refresh preserves backend workspace/QA and running residents. Backend restart pauses
recovered members for deliberate reconciliation/resume. Tab selection and unsent drafts need not
survive refresh. Do not silently switch approved reviewer models/providers. Pending approvals
appear in Needs you and open the member tab; do not bypass them with broad auto-approval.

## Claims

This ledger owns package state. `waiting` means an unmet dependency or unresolved contract;
`ready` still requires the shared review gate and a scoped assignment. `building`, `review`,
`merged` and `folded` describe execution/integration, not acceptance. BUILD-PLAN distinguishes
integrated prerequisites from final gates. A Wash `active` item may represent unaccepted integrated
work; it does not mean an implementer is running. Merged rows retain dependency and evidence links.

| Package | State | Branch | Notes |
| --- | --- | --- | --- |
| SV1 | merged | recovery-server-checks | Five D1/R4 bench registrations and server unsafe coverage restored; three reviews complete; source behavior unchanged |
| M0 | merged | recovery-model | Current-contract host model recovered; three reviews, full bench and 5,001,000-sequence run complete; see model/VALIDATION.md |
| M1 | merged | recovery-model | Joint host-model recovery with M0: flat scheduling, IPC outcomes and record traces; native replay remains C1; question 171 remains open |
| W1 | merged | wp-w1 | d52896bee |
| W2 | merged | wp-w2 | 3715363a9 |
| W3a | merged | wp-w3 | 612a0a599; review R-1 complete |
| A1 | merged | wp-a1 | 44f1780a1 |
| A2 | merged | wp-a2 | c98034520 |
| A3 | folded | | into wp-k2 (answer 103; one flat weighted stride queue) |
| L1 | merged | wp-l1 | 25ab39296 |
| T1 | merged | wp-t1 | 987bacbed |
| T1b | merged | wp-t1b | 6cd067a39 |
| T1c | merged | wp-t1c | fe807fc4b; checker and runtime 9/9 reduction each passed three reviews. Server roots restored by SV1; G1 listed every on-target source (enumeration criterion met) |
| V1 | merged | wp-v1 | 05955bf86 |
| K0 | merged | wp-k0 | f7b9fdd16 |
| K0b | merged | wp-k0b | e30d43304 |
| K1 | merged | wp-k1 | e1d2c6216 |
| K2 | merged | wp-k2 | 95788dcd0 (carried A3) |
| K3 | merged | wp-k3 | 12c52c2d7 |
| K4 | merged | recovery-k4 | Native lifecycle integrated; three reviews and full bench complete; bundle-file readback remains acceptance gate for R2/R3 (answer 169) |
| G1 | merged | wp-g1 | Review debt cleared 2026-09-23: 26cba3022 reviewed, unsafe coverage complete, R11 write-without-read rule (Wash QA G1-coverage, G1-write-without-read); three rounds, all OK with notes. Answer 166 settles K5's contract (Wash QA G1-q166) |
| K5a | merged | wp-k5a | 7526e8571: `map_fixed` (answer 172) in the kernel, redoubt-sys and the model, call 26; fixes a `tables_needed` under-count for ranges of 1 GiB or more (shared with `process_map`). Attack cases map-fixed-attack and map-fixed-tables; the R11MapFixedSkipsOverlap mutation is caught. Three review rounds; red team, editor and simplifier all OK (Wash QA K5a-review-1, K5a-review-2) |
| K5 | merged | wp-k5 | 33db4858e (tip 1784a7f91): kernel-owned timer, timeouts, budget deadlines, one preemptive stride queue with inheritance and free-weight carving; redoubt-stride crate checked against the model; fence.i; pinned latency targets met on both widths. Five plan rounds, final red review MERGE (Wash QA K5-code-review-final). Follow-ups: K5-r10-destroy-cost, K5-carve-lead-rescale, K3-irq-level-latch |
| K5b | waiting | | answer 173 (question 147): DMA device reset and frame quarantine; plan drafted during K5, implemented by the kernel writer right after K5; gates R3 driver restart and off-bench D3 |
| K6 | waiting | | needs K1-K5, R1b |
| IPC1 | review | wp-ipc1 | Implementation in fe807fc4b, its TCB rounds complete; 26cba3022's shared record validator reviewed in G1. Host model recovered; native replay, K5 timer, serving-path and concurrency gates remain open; native exit covered by K4 |
| R1 | merged | wp-r1 | 8298608af (carried the answers 39-42, 50-53 part of R1b) |
| R1b | merged | wp-r1b | 86117e7af |
| R1c | merged | wp-r1c | cd65fa610; joined `Parked` to the 9P skeleton (recovery of `5d29d136e`, answers 156-158); reviewed R-R1c |
| R1d | waiting | | typed parking awaits decision 163; needs R1c (merged); blocks `resize` in B2a |
| R2 | merged | wp-r2 | d6b1809a7: flat-binary stub at 0x1FF0_0000 maps ELF segments with `map_fixed`, frees the image, jumps. Bench case stub-launch (rv64, rv32): hostile ELFs and 32 fuzzed headers hurt only the child. Four review rounds; red team and editor OK (Wash QA R2-review-4). Test-coverage follow-ups in Wash QA R2-followups. R1b and K4 lifecycle integrated; production handoff completes K4 bundle-readback acceptance |
| R3 | waiting | | needs R2/K5 and relevant device/startup/confinement decisions; infrastructure enables later packages, full-server boot/blame acceptance awaits their integration |
| R4 | merged | wp-r4b | 69466924c; bootfsd and consoled recovered from wp-r4; reviewed R-R4b |
| B1 | ready | | R1b/R4 merged; scope Platform interfaces before launch; native acceptance additionally needs R3 startup/public modules and K5 |
| B2 | waiting | | IEx on the UART; needs B1 and integrated R3 startup infrastructure |
| B2a | waiting | | the console library (`consol` codec, `Redoubt.Console`/`.Key`, answer 162); needs B2, R4 |
| B2b | waiting | | `Redoubt.Ed`, `Shell.top()`; needs B2a |
| D1 | merged | wp-d1b | 8681f2648; blkd recovered from wp-d1, reviewed R-D1 (editor BLOCK fixed; red team 4/4 OK) |
| D2 | waiting | | D1/L1/R1b merged; held-fid and consumed filesystem contracts need reconciliation (including Q131); boot acceptance needs R3 |
| D3 | building | wp-d3 | External claim unverified; no source supplied in recovery review |
| S1 | merged | wp-s1 | 14bcc6e9d |
| S2 | waiting | | needs integrated R3 startup infrastructure, B1 and D2; coordinate R3 blame and S3 session protocol tables before implementation |
| S3 | waiting | | needs D3/S1/S2/B2; full console acceptance also needs B2a/R1d (Q163), per-channel resize/abandonment and cleanup |
| C1 | waiting | | needs integrated M1/K1–K5/T1/IPC1 and R3 bundle handoff; supplies replay to IPC1 acceptance, does not wait for it; affected traces await contract disposition |
| E1 | waiting | | needs everything (milestone) |

## External work constraints

Do not merge the supplied remote branches wholesale or re-port D1/R4, K4 lifecycle or the host
model: that recovery is integrated. D3 remains an unverified external claim; reconcile ownership
and source before replacement or acceptance. Exact historical tips and restrictions are in the
[recovery inventory](archive/2026-09-22/ASTRA.md#11-remote-branch-recovery-review--2026-09-22).
Current acceptance gaps belong to BUILD-PLAN and STATUS.

## Review debt

G1 cleared the debt that gated the next wave (2026-09-23; evidence in its commit and Wash QA).
Valid earlier reviews stand: T1c's checker (R-T1c) and runtime unsafe reduction, and IPC1's design,
host and kernel rounds ([assessment §5–§9](archive/2026-09-22/ASTRA.md#5-owner-approved-ipc-follow-up--2026-09-22)).
Review completion alone does not close IPC1's acceptance gates. Completed model validation is in
[model/VALIDATION.md](../model/VALIDATION.md).

Outstanding follow-ups, none blocking:

- **Kernel `print!` panic re-entry.** A panic inside `print!`'s `write!` re-enters through the
  panic handler's `println!` while the first `&mut OUTPUT` is live; the handler then powers off.
  Proposed fix (red team): an `AtomicBool` `PRINTING` that makes `handle_panic` write straight to
  the stateless SBI console.
- **`R11AllowsWriteOnly` is caught only through `set_flags`.** The mutation also disables the
  model's `process_map` check, which no sequence yet shows killed on its own. The kernel side is
  covered by `write-only-attack`. Split the mutation or add a `process_map` sequence.
- **`process_map` backs its source before refusing bad flags.** `ensure_range_exists` runs before
  the W+X and write-without-read refusals, so a refused call can still charge the caller for
  demand-reserved source pages. Only the caller's own budget changes, as with its other
  refusals; moving the flags check first would change error precedence, so decide with the
  Errors table.
- `libs/abi`'s 44 undocumented unsafe uses remain legacy debt for K6, as does the legacy
  `UpdateMemoryFlags` call (now refusing write-without-read too).

## Cross-cutting review records

Use [the dated assessment](archive/2026-09-22/ASTRA.md) for past evidence and
[the active follow-up list](../ASTRA.md) for unresolved
findings. Keep completed checkpoint narratives out of this ledger.
