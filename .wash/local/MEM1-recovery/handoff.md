# MEM1 orchestrator recovery checkpoint

The original implementer was paused after repeated queued checkpoint instructions did not
produce a saved handoff. This is an orchestrator snapshot, not the implementer's completion
report. Source is preserved in place; do not reset or replay the backup onto it.

- Worktree `/home/mcloonan/redoubt/.worktrees/MEM1`, branch `wp-MEM1`, HEAD
  `e9f2fcb958f4229ba90a009999b95fd03897c058`. Implementation is uncommitted.
- `manifest.json` records exact dirty paths, untracked paths and patch hash. `tracked.patch`
  and `untracked.zip` back up the source snapshot; `implementer-report.md` preserves the
  predecessor's `/tmp/MEM1-report.md`. Read its test log but reconcile stale permission text.
- At pause there was no qemu-system process. Machine measurements were not reported as run.
  No in-flight QEMU needs resuming. Do not repeat the completed full host suite without reason.
- Reported PASS: full host-tests (model-host-tests 382.6s), client-host-tests including every
  paint unit through 65535 on a 128-page launch and 129-page no-call refusal; init-host-tests;
  init-build and client-build both widths; formatting; unsafe-budget; no-cruft.
  Exact retained logs/commands need locating before final acceptance. Some earlier attempts
  failed and were repaired; preserve that distinction from the report.
- Size gate reported stub 366/361, client 987/966, init 1957/1952. Scoped exact ceilings may
  be updated with measured reasons after trimming and review; no speculative headroom.
- Initial image stack fields are placeholders, not measured final declarations. Need rv32/rv64
  init-boot stack measurements, init-refuses-stack/bound checks, final declarations, final
  gates/review and clean logical commits. Whole Tier A bench remains mandatory.
- Provisional red review: OK with notes, no concrete mechanism flaw. Scanner bool-table
  simplifier finding was withdrawn; keep the simple bounded table. Final review still required.
- Docs changes already exist. Architect ruling: label loader-init's 32-page stack distinctly
  (one backed page, 31 reserved); client stacks default16, allowed1..128, all selected pages
  backed, max envelope0x7FF80000..0x80000000. Do not replace init's32 with128. Kernel reserve
  INIT_PAGES1024 is fixed, preflight bound depends on final manifest; interim508 is not final
  measured evidence. Check 'at least doubles' against final bound<=512. State actual init
  client-library Launch use, not all client APIs boot-tested. Public paint is a driven-path
  measurement, not an adversarial proof.
- OWNERSHIP NOW: BEAM7 owns QEMU focused machine-test slot and docs writer window. MEM1
  successor can inspect existing docs but waits to edit them/run QEMU until its slot. Host
  work and source fixes remain authorized. MEM1 owns Boot.memory/Kind::Boot in case.rs;
  BEAM7 owns HostTests sections in its separate worktree and build.rs. No blanket file lock.
- Current peers: architect-2; sched1-implementer-2; beam7-implementer-2;
  mem1-red, mem1-simplifier, mem1-editor (editor reviewing current docs provisionally).
  Use current main SWARM/PROJECT, not old process copies. Never push/stash/blanket-stage.

Next: reconcile snapshot quickly, preserve coherent source in exact-path checkpoint commits,
refresh root `.wash/local/MEM1-report.md`, identify next missing measured gate and request a
focused machine slot when BEAM7 releases it. No fresh design checkpoint or repeat permission
question is needed for the settled implementation or previously approved scoped size edits.
