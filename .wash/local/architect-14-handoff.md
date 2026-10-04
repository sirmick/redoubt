# Architect handoff (architect-14, 2026-10-03)

Working rules stand from architect-13's handoff:
- Messages to the orchestrator are at most 2000 bytes, counted strictly. Aim for 1,700.
- Review-only checks: never Write, Edit or redirect into any file. I slipped once, a /tmp
  scratch file, deleted and reported.
- Read worktrees by SHA.
- The handoff mark is now **50%** (the owner's).

## Done this stretch (each answered; the briefs updated)

| Item | Where |
| --- | --- |
| VOL1 verified volumes. **Ruled by the owner:** shape A, the `verityd` server between `blkd` and `fsd`; BEAM2 lands as built, then VOL1 deletes `system.index`; decision 5 reversed (readers trust their verifying servers). Step 2 adds signed roots (Ed25519, a version floor). Size M+. | `VOL1-implementer.md`; QA thread `VOL1-verified-volumes`; `shell-plan.md` section 7 item 5 amended |
| BEAM2: the confined index is closed (no labelled beamlet until VOL1; one beamlet.md line). Ruling 5: RAM 1 GiB (budget 24,576), the split stays a quarter, erts packed whole, one shell boot (`userland-boot` carries the flip and missing refusals). | `BEAM2-implementer.md`, rulings 4 and 5 |
| BEAM6, the shell VM on a diet (M2; needs BEAM2 and MEM2): node only, brief later | plan |
| FSD4, the directory listing window: brief, node, **merge check OK** (`1614c6cb5`) | `FSD4-implementer.md` |
| HW1 merge check OK (`1fdd112f0`); an optional PPN row suggested | |
| K22 marks audit: per-reconcile `check_visited` (unstamped), a full walk once a slice, and unconditional at idle. **Merge check OK** (`af95ab385`+`2b4dd2e29`) with three scheduling.md folds (:121 "the wakes in the same order", :660 rv64-only wording, :402 reflow). | `K22-implementer.md` point 5 |
| ABI2: yield = `sleep(0)` (no alias), timer.md line. **Merge check OK** (`6239e973f`); host gates only, no machine code changed | `ABI2-implementer.md` point 6 |
| B6 parallel seed sweeps: brief, node. **Merge check OK** (`e74ead5c2`+fold), its QEMU proof still owed | `B6-implementer.md` |
| K19 node (after IPC3, M, no brief: re-cut against IPC3's lists) | plan |
| IPC3: the audit cost ruled again as (3b): (a) the thread-driven audit per exit; the full enumeration at destruction and free points and at idle; the endpoint-destroy fix OK if the skip is exact | `IPC3-layout-ruling.md` (3b) |
| Async I/O: assessment and addendum for the owner. AIO1 cut (M+, Tier A, beamlet-redoubt; BEAM3 needs AIO1). Rulings 1-8: `Requests` resource; `NineServer::run`; the wire `[0,COLLECT,hold_us,0]` with batched T-messages; `COLLECT_WAIT` 10 s; no lock in the library (`Hub` is `Send`); the client timeout is hold_us + `COLLECT_MARGIN_US`. Latest: cap 80 per bucket, R77's exact text, R26/R28 sentences, wire.md:61. AIO2 (a kernel notification) is the owner's later option. | `async-ipc-assessment.md`, `AIO1-implementer.md` (rulings at the end) |

## Open: what the next Architect watches

- **AIO1:**
  - The hub's client timeout (hold_us + margin) was NOT in at `2f85cf9c0`; told. Check it.
  - Check R77's text, the `Requests` caps (80) and the budget arithmetic, the wire.md:61
    amendment, and the machine case `aio-many-reads` (1 thread 0 waiters; a two-connection
    variant).
  - Check at merge that nothing names a `Lock` in rt (ruling 5 withdrawn).
- **IPC3** at merge:
  - `IPC3-layout-ruling.md`, including (3b): the audit form that shipped and its scale check;
  - `libs/ipclist` on stride's pattern (the orchestrator's conditions);
  - the endpoint-skip assertion.
- **K19:** write its brief once IPC3 merges (`destroy-simplify.md`).
- **BEAM2** at merge: rulings 1-5 plus the "Rulings during the build" section; the beamlet.md
  confined line; RAM 1 GiB lines (budgets.md, image/README); `userland-boot` carrying the
  refusals.
- **VOL1:** starts after BEAM2; its cases share boots (the brief says how).
- **MEM1/MEM2, BEAM6:** BEAM6's brief is due when MEM2 is near.
- **B6:** its QEMU proof is owed before merge.
- **ASID1** after HW1: recheck its line references.
- **Pending with the owner:** AIO2 (later); SMP2's checkpoint; the h/1 docs.

## Context sinks

The BEAM2 and AIO1 reports, the scheduling.md diffs. Use `git diff -U0` and targeted ranges.
