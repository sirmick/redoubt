# architect-12 notes

Watch lists from architect-11-notes.md (and 10's, 9's) stand.

- BEAM2 brief confirmed complete against main + wp-fsd3 (a5b505e97); fixed reading list (fsd.md has
  no "Mounting": "Volumes, connections and labels", Read-only ranges bullet). R75 free. Recheck at
  FSD3 merge (recipe, pack, case.rs [disk]). R34 confined/userland-disk owner question: not put;
  offered to orchestrator.
- ASID1 brief written (ASID1-implementer.md; plan node body rev 345). Rulings in it beyond the
  orchestrator's list: (a) a destruction moves satp off the dying space and flushes its ASID before
  freeing frames (a walker through a freed table could cache a garbage G leaf, which no ASID flush
  drops); (b) map_page_in reports a linked table -> whole-ASID flush (spec: non-leaf change rs1=x0);
  (c) DMA window leaf (map_kernel_page) gains G; G never on the PROCESS_AREA path; (d) checked-build
  log of unflushed writes, empty at every return to user; (e) QEMU's TLB is not ASID-tagged
  (write_satp flushes; implementer confirms), so the saving cannot show on QEMU and the negatives
  trip through the audit. Check at ASID1 merge: the page lines in the brief, flush table as built.
- SMP1 brief amended for ASIDs: needs + ASID1; item 7 rewritten: per-PID stale mask (u8, MAX_HARTS)
  for harts that ran a process but are not running it (lazy flush before next install, no IPI);
  idle hart moves satp to PID 1 (no flush); shootdown flushes (x0, pid) / kernel half (addr, x0);
  new stale-mask case + negative smp-no-stale-mask; memory.md and memory-layout.md residual lines
  rewritten. M2 page item 4 text is ASID1's to write (removes "There are no address-space ids").
- HW1 brief: ASID_BITS row added conditional on ASID1 having merged.
- INIT5 check (b7d73c4f8): OK + budgets.md test-list edit (tested 8, the bound.rs test); whole bench owed.
- FSD3 check (a5b505e97): OK + fsd.md Authority edit (the console init gave it); optional testbench.md sort.
- K16 r2 check (b81d94dde): OK, no edit. Expiry confirmed with IPC3: IPC3 brief amended (item 6 the
  expiry, owns time.rs expire_due, next_timeout no longer a "remaining walk", worst-walk loses
  must_fail/whole_run and runs on rv32 at what RAM holds, the two todo pages deleted).
- Owner 2026-10-03: BEAM2 R34 default ruled (one read-only attachment per label set, R34 unchanged); brief's section rewritten, beamlet.md line stated (no Open). Check at BEAM2 merge.
- ABI1/ABI2 cut (owner 2026-10-03; plan rev 362, parent M1). ABI1 (S, no needs): redoubt_sys::Transport,
  Ecall impl (same one unsafe), rt one installed transport (HostKernel->Transport, install_transport,
  feature installed-transport; no_std slot deferred to the backend), init's 2 DeviceInfo calls via rt,
  beyond/README.md no-MMU direction line. ABI2 (S, needs ABI1 for shared files): ipc.md lend/transfer
  as ownership (exact text), R3/how-a-call-completes lines, fake refuses given-up pages, doc-only type
  changes; no kernel change (take_buffer/give_buffer_back/move_buffer verified). Check both at merge.
