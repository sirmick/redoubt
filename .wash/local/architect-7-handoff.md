# Architect handoff (architect-7, 2026-10-03)

Read the earlier handoffs first, newest first (architect-6, architect-5, architect-4, architect-2,
then architect-handoff.md); their working rules stand:
- pages go on main, staged by path;
- run doccheck before a commit: `./.wash/local/in-dev bash -c 'cd /work && cargo run -q -p redoubt-doccheck'`;
- QA bodies are at most 2000 bytes;
- no thread names or dates on pages;
- give exact lines to a package rewriting a page;
- nothing goes on a page before its code;
- end your turn after a decision_request.

My owed edits and watch lists are also in `.wash/local/architect-7-notes.md`.

Habits that served:
- Read the code before ruling. The FSD3 labels gap, the netd fault trigger and the SMP residency
  design all came from code the question did not name.
- Check a QA answer's `reply_to`. I sent one under the wrong id and had to resend.

## Rulings of this stretch, and where each lives

| Thread / request | Ruling | Where |
| --- | --- | --- |
| K21-dma-pool | (a) A fixed DMA pool: 1024 pages (4 MiB), always, taken from the top of RAM at boot before `boot_budgets`, the kernel's. Its frames are `DMA_OWNER`'s for life, with a 1024-bit bitmap; first fit, linear in 1024. Charging to the caller is unchanged. Quarantined runs stay out until reboot. Exact lines for devices.md, budgets.md and memory.md (including the diagram). dma-reset-quarantine's new verdicts: allocated + 2×RUN_PAGES = pool; no overlap; OutOfMemory with budget room. | `.wash/local/K21-dma-pool-ruling.md`; resolved; in K21 |
| gate step (message) | It closed with GATE1's merge (d45ef88eb). Nothing outstanding. | the orchestrator set it done |
| INIT3 brief (message) | Blame waits for the steward (the owner, INIT1-design), so a fault is printed "blamed on nobody: no steward". New rules, with exact init.md lines: a restart repeats the boot's own steps (keyd's check, consoled's attach, bootfsd's push and seal); an exit during the boot is restarted and counted (the reboot rule, more than 5 in 60 s); a quarantined driver is found by `device_info` on init's kept handle, then the box reboots; a restarted consoled forgets the servers' consoles (two residuals). init releases grants through libs/client's `Grants`. Six cases; the reboot cases end at the next boot's first line. netd's restart case moved to INIT4. | `.wash/local/INIT3-implementer.md`; nodes INIT3 and INIT4 edited |
| INIT4 brief (message) | Only netd and ipd leave the rig. The bench puts the disk and card on fixed slots (net0 0x10007000/7, disk0 0x10008000/8). Clients get scopes from ipd `scope=` plus handed badges (no steward stand-in). A judge program is the reporter, and clients park. bench-virtio-legacy-off moves to the tester. netd-restart uses a test feature triggered once from outside. The rig is deleted. Size L. Exact lines for testbench.md, netd.md, ipd.md and init.md. | `.wash/local/INIT4-implementer.md`; node INIT4 |
| M2 "several harts" line (K21's red review) | Held until K21 merges; text in the notes file. **Reword it to SMP1's design if the owner picks Q1 = one hart per budget:** "the eviction before a frame is freed", not "shootdown". | `architect-7-notes.md` |
| fsd design (message) | No owner question: one per volume is R47, and persistence is fsd.md's Purpose. FSD1, host-only, M, no needs: args `labels=`, `buckets=` and handle `volume`; 4 KiB blocks; a blank range is formatted and an unmountable one served as `corrupt` (never a restart loop); new wire error `corrupt` (8); remove by generation; one public fid resolver in the 9P skeleton (tell RT1). **Gap for FSD3:** a confined labelled fsd carries its volume's labels, and blkd's ranges are `NO_LABELS`, so blkd refuses its writes. FSD3 passes range labels from init and mints the range from `volumes` (INIT2 parses but does not mint). | `.wash/local/FSD1-implementer.md`; nodes FSD1 (launched), FSD2 and FSD3; the fsd step needs FSD3 |
| SMP1-design | Brief written on my recommendations. Owner's decision_request **pending** (id 7f4400f4…): Q1, one hart per budget (rec) or full shootdowns; Q2, targets gated at 1 and 2 harts with 4 recorded (rec). Design: steps 1-3 plus residency (a process's translations live only on its budget's hart; a destruction evicts a running budget with a lock-free acknowledgement before freeing frames; kernel-half unmaps too); FIFO ticket lock; device interrupts on the boot hart; one build (the smp feature and the spike deleted); MAX_HARTS 8; trace hart field; `--smp N`. **K19 does not gate SMP1** (a destruction runs whole under one lock); it gates step 5. K16 and K21 are needs. | `.wash/local/SMP1-implementer.md`; nodes SMP1 and SMP2 (both under M2, needing the orchestrator's override of M1) |

Carried from architect-6 (unchanged): INIT2's review and the two restored bench cases (R33
`init-refuses-budget-handle`, R34 `init-refuses-confined-server`); RT1's node; B7's node;
GATE1-trace-ring; K19 awaiting the owner (destroy-simplify.md).

## When the owner answers SMP1-design

- **Q1 = one hart per budget:**
  - write SMP1's page lines into the brief: memory.md's "One hart" residual becomes residency
    and eviction; processes.md's flush sentence; boot.md "The other harts stay parked" becomes
    "started by the kernel"; scheduling.md's line, already in the brief;
  - rewrite M2's "Several harts" step 4 on main, from "shootdowns by ASID" to residency and
    eviction, together with the held K21 line once K21 has merged.
- **Q1 = many harts:** rewrite rule 7 of the brief as IPI shootdowns on unmap, lend and reply,
  with ASIDs and concurrent stride accounting. That is probably L+, so split it.
- **Q2:** put the hart counts in SMP2's body and write its brief. The targets' sentence goes on
  scheduling.md's "Measured on QEMU, on one hart" residual when SMP2 lands.
- Return the thread to the orchestrator with decision_refs.

## Open, and what waits on what

- **Merge chain:** INIT1, then K21, then INIT2, then RT1 and INIT3, then INIT4.
- **K16:** held at commits 1 and 4.
- **B7:** reported.
- **FSD1:** just launched.
- **SMP1:** waits on the owner, K16 and K21.
- **FSD2 and FSD3** briefs are owed (notes file).
- **The init step** closes after INIT4. I owe the M1 page edit: move the init item to Progress,
  fix the stale "Not built" line, and mark R33's row built.
- **K19:** the owner's call.

## What to watch at each merge

- **K21:**
  - the pool lines as in K21-dma-pool-ruling.md;
  - dma-reset-quarantine's verdicts;
  - architect-6's K21 list (the free-list sentence, scan-bounds with the `alloc-first-fit`
    negative, the audit, no RAM scan in `release_owned_frames`);
  - then land the M2 line.
- **INIT2:** architect-6's list (handoff), plus R33 and R34 each naming its one bench case.
- **INIT3:** the page lines in INIT3-implementer.md; reboot cases end at the next boot's line;
  `init: restarted NAME, console N`; no netd case.
- **INIT4:**
  - the fixed slots in qemu.rs;
  - case names kept;
  - the rig deleted;
  - the page lines in the brief;
  - then the init step's M1 edit.
- **FSD1:**
  - fsd.md's Arguments, Mounting and Corruption lines;
  - wire row 8;
  - the resolver in ninep.rs only;
  - no init or blkd change.
- **SMP1:** per the brief, after the owner's answers.
- **RT1 and K16:** architect-6's lists.
