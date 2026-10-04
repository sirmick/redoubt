# architect-11 notes

Watch lists from architect-10-notes.md and architect-9-notes.md stand.

- FSD3 Q3 (answered by the orchestrator): testbench disk.rs packs the GPT through blkd's host
  Image and each littlefs partition through host-only redoubt_fsd::pack (Fsd's create/write
  path); case.rs [disk] recipe/stage; qemu.rs writes it; mkimage calls `testbench --pack-disk
  RECIPE OUT`. Consistent with brief item 4 (fsd's own volume code, no second writer); no ruling.
  Check at FSD3 merge: pack() is host-only (cfg, not in the rv image); image/README.md says
  mkimage packs through the testbench's --pack-disk and fsd's pack; testbench.md's [disk] keys
  (recipe, stage) and the --pack-disk flag; the littlefs diff case result reported.
- FSD3 case 3 ruling (a): fsd-confined-labelled keeps only the confined write/read; new case 9
  fsd-label-check (unconfined, unlabelled caller refused read+write at fsd, {L} file unchanged).
  Lines in FSD3-label-check-ruling.md: fsd.md section status tested (31) with 5 bench lines;
  serving.md R25 tested (8). Check at FSD3 merge.
- BEAM1 heap limit ruling (BEAM1-heap-limit-ruling.md): B, heap and ETS each budget/16; budget
  >= 2x VM's own use (rv32 beamlet budget -> 4,096 pages); budget_pages= required (BAD_ARGS);
  init does not check it (args opaque, init.md:78); follow-up todo beamlet-budget-from-startup.md
  (startup-block field). Check at BEAM1 merge: page sentence, todo file, limits() doc comment.
- FSD3 confined users ruling (FSD3-confined-users-ruling.md): (1) for every shared server, users
  in a confined boot = principal domains with its own label set; bucket rule unchanged. init.md
  line given; test confined_counts_only_a_shared_servers_own_label_set (+1 confinement + R34).
  Check at FSD3 merge.
- FSD3 red P2: code stands (mount walk at every start); brief rule 5 meant no whole-volume *block*
  check. fsd.md:415 replacement + fsd-restart description sent; P1 page line: init.md Volumes
  bullet gains "and no entry is handed a badge at the endpoint a `blkd` receives on." Check at merge.
- FSD3 Sharing::Server: keep + re-aim confined_refuses_a_server_instance_serving_two_label_sets at
  a device-less {} blkd under a {7} fsd/volume (Server). If unreachable, delete + init.md:165 bullet
  out + sentence after "names the kind." Check at merge which way it went; lists/counts follow.
- K16 c8 pump ahead (K16-pump-ahead.md): pump walks every live thread (3 find_thread + next_sender
  per receiver, O(R_e x T)); not covered by ruling 2; redesign = per-endpoint receiver list,
  per-group sender FIFOs, notice list, own package. If R10 > 30 ms net: decision_request (rec:
  merge with residual numbers + redesign package; alts lower MAX_THREADS / MAX_PROCESS_COUNT).
- IPC3 cut (plan rev 329-330): brief IPC3-implementer.md; per-endpoint intrusive lists for pump,
  next_sender, notices, irq waiters, destruction reach; receiver order -> model's FIFO (ipc.md
  sentence); layout checkpoint. SMP1 needs IPC3 (hotspot line in SMP1-implementer.md).
- ASID bound (owner, K16-asid-bound-ruling.md): MAX_PROCESS_COUNT < 2^ASID_BITS (9 rv32, 16 rv64);
  value 512 -> 511 (PIDs 1..=511; 512 is 10 bits); assert in arch/riscv/process.rs; memory-layout
  satp para + processes.md:44 + every PID count -1. rv64-toward-16-bits = own package after IPC3.
  Check at K16 round 2.
- Hardware bounds sweep (hardware-bounds.md): missing = ASID (K16), MAX_THREADS<=u8, user half +
  Sv39 canonical asserts, loader dt.rs silent truncation at 32 devices, harts (added to SMP1 brief
  item 1). Recommended HW1 (S, needs K16) with boot.md "Hardware bounds" section; brief on request.
- HW1 cut (plan rev 331): HW1-implementer.md; boot.md 'Hardware bounds' section text in brief; R17 +3 tests. Check at HW1 merge.
- BEAM1 merge check at 69e335a45: OK + 2 edits (beamlet.md:243 'its tests run on the host'; budgets.md:195-197 rewrap). INIT5 brief updated to BEAM1's sentence + beamlet-boot.
- BEAM1 restart loop: (a) wording; beamlet.md sentence + budget-flood description sent. BEAM2 brief must ask: is the console shell a restartable server, with what limit.
- K16 c8 numbers: R10 11.7 s, pump 6.9-11.5 s, expiry 28.9 s (pumps nested), reconcile 0.32 s. IPC3 acceptance + expiry; K22 (marked reconcile, sched.rs) proposed, cut on orchestrator's word. Lower limit: ~500 live threads total for 30 ms -> not real.
- K22 cut (plan rev 335): K22-implementer.md (marks; owns sched.rs, ptable.rs, libs/stride). SMP1 needs IPC3+K22. K16 r2 at K16-7's tip: c8 lines state numbers + conditions, name IPC3 and K22; ASID 511 lines.
- BEAM2 brief drafted (BEAM2-implementer.md, plan rev 338): two disks (volumes.disk), system.index format, R75 verified userland on boot.md, parked start, docs strip default + switch; owner question pending: confined labelled domains vs shared userland disk (R34).
- Handoff written (architect-11-handoff.md). Owner pending: K16 choice, h/1 docs, ASID package after SMP1, confined userland-disk question (not yet put).
