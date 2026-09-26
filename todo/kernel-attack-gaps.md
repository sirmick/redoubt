# Kernel attack gaps

Claims the kernel pages state as built that no bench case attacks, or that only the model
attacks. Each page says "partly tested" for these. The top-level set moves this file to
`docs/todo/kernel-attack-gaps.md`, which `kernel/boot.md` links; until then it is working material.
Format: page, section: the claim, and what no case attacks.

## ipc.md
- R1 (flow): a call or send between user budgets with different labels is attacked only in the model.
- R2 (fair waiting): turns between several groups are attacked only in the model.
- What `receive` returns: a record made unwritable while its thread waits is not attacked.
- How a call completes, R13: completion races between harts are not attacked.

## README.md
- What the kernel keeps: that no driver, file system, naming or policy code sits in the kernel rests on the source map, not a case.
- The no-cruft gate has no self-check: no planted forbidden name, `allow(dead_code)`, unused feature or second `PAGE_SIZE` is shown to fail it.
- The TCB's line count is measured, not pinned by a case; only `unsafe` has a ceiling.
- Nothing checks that every `kernel/src` file appears in the source map.
- The unsafe ratchet cannot show that every on-target TCB source file is listed in a budget.

## objects.md
- What objects cost: an endpoint's one page to its owner is attacked only in the model (`R6EndpointsFree`).
- What objects cost: a device object's one page to `system` is attacked by nothing.
- What objects cost: the saved-context pages (1 on rv32, 2 on rv64) are pinned by no case; the model's cost table is rv64's.
- `mint`: `Dead` from a message source whose call's stamp was destroyed has no case and no mutation.
- R9 (stamps): a handle minted from a call taking the call's handle's stamp (not the caller's budget) is attacked only in the model (`R9MsgStampIsSenderBudget`).

## budgets.md
- R6 (charging): an endpoint's page charge is attacked only in the model.
- Class is trust, not order: that the scheduler never reads class is argued from the code.
- Deadlines: a process entering the kernel in a tight loop to put its deadline off is not attacked.
- R10 (destruction): destroying the budget a device object is charged to (the device destroyed, every handle closed) is not checked by a case.

## timer.md
- Time: that `time_now` counts from the kernel's start is not checked (only monotonic, never early, linear with `rdtime`).
- Expiry: the order at an equal instant (timeouts before deadlines; (pid, tid); budget id) is attacked only in the model (`ExpireBudgetsFirst`).
- Budget deadlines: a destroyed child's later deadline leaving the list is shown only by the kernel surviving past it.
- Failure and restart: a boot with no `Time` tag (or 0) powering off is not attacked (R17's gap too).
- The hart timer: a stale early hint costing one early interrupt and missing nothing is argued, not attacked.

## devices.md
- Device objects: the one page a device object costs its owner is not measured; a DMA device past `MAX_DMA_DEVICES` (16) getting no object is not attacked.
- `dma_alloc`: the `MAX_RUNS` (32) runs-per-device limit is not reached by a case.
- Reset before reuse: that the reset precedes pooling inside the kernel is attacked only in the model; the reset and quarantine cases run only on rv64.
- Devices handed to the first program: the loader's refusal of a device tree with no console, or a console with no interrupt, is not attacked.
- R5 (interrupts): masking a fired source is attacked only in the model (QEMU's 16550 raises per byte); completing the claim before masking, and billing interrupt time to the IRQ object's owner, are not attacked.
- R18 (device authority): the kernel's refusal of a malformed `Devs` entry and of a `Grnt` boot argument is not attacked.
- Failure and restart: destroying a device object's owner budget (IRQ waiters get `Dead`, source masked, handles swept) is not attacked.

## processes.md
- Processes and PIDs: that PIDs are drawn at random is not attacked.
- Threads: `thread_create` refused with `OutOfMemory`, and the first thread returning from its entry (a fault), have no case.
- Creating and starting: `OutOfProcesses` from `process_create` and `OutOfMemory` from `process_start` have no case.
- Exit notices: a notice dropped because its exit endpoint was destroyed has no case.
- R21 (crash blame): blame after the blamed sender's budget is destroyed has no case; a thread holding a parked call that receives a send and then faults (blames nobody) is covered only in parts.
- PID pinning by untaken notices (a cross-budget `OutOfProcesses`) has no case.

## memory.md
- Backing and zeroing: that a frame freed with data comes back zero is attacked only in the model (`R11NoZeroing`); no case can tell which frames it was handed.
- Where `map_anon` puts pages: a full placement area, and a request that fits only at the area's end, are not attacked; the search's worst-case cost is not measured.
- `map_fixed`: its cases run on rv64 only.
- Instruction fetch after mapping: no case can see a missing `fence.i` (QEMU keeps fetch coherent).
- Lending at the page-table level: a lend within one process is not attacked across harts or across that process's teardown.
- R11 (memory): W^X on device registers and `dma_alloc` pages is not attacked (and does not hold: `todo/device-mapping-exec`); the absence of any physical-address argument is argued from the call table.
- R19 (kernel W^X): no case plants a writable kernel code page to show the boot check stops; the case boots rv64 only.
- R22 (range cost): only `map_fixed`'s huge length is attacked; `unmap`, `set_flags`, `process_map` and lends with huge ranges are not, and `map_anon`'s search is an exception no case measures.

## scheduling.md
- One flat stride queue: round-robin among one budget's threads is not attacked.
- Preemption points: an interrupt's wake not preempting, and another budget's deadline preempting, are not attacked.
- The current minimum and ties: clauses 1 and 4 are checked on the target only when a run happens to tie; the host tests and model attack them.
- Charging: interrupt handling billed to the device object's owner is not attacked.
- Responsiveness: decision wake plus R10 time is not asserted as one sum; `budget_destroy` call-to-return is recorded, not asserted.
- Charging: floods of weight-0 budgets with deadlines beyond the 64 of `sched-timer-flood` are not attacked (their destruction is billed to nobody).
- R23 (no test channels): no case builds the production kernel and scans it for the trace.
- Failure and restart: a picked thread dying before the switch, and a full queue, are not attacked.

## boot.md
- Firmware: the bench never falling back to QEMU's own firmware is not attacked.
- The argument block: the kernel's refusals of a malformed block (a tag past the end, a second `MREx`, a bad `Devs` entry, a `Grnt` tag) are not attacked.
- R16 (image confinement): an image cut short inside its segment data, a writable and executable segment, and a bundle of more than 63 programs are not attacked; the truncated-image case runs on rv64 only.
- R17 (fail closed): a short or missing seed and a missing timebase are not attacked (every QEMU boot supplies both); nor an initrd under 64 bytes.
- Failure and restart: a reboot through `system_reset` (the chain rerun, the bundle verified again) is not attacked.
