# Kernel attack gaps

## What

Claims the kernel pages state as built that no bench case attacks, or that only the model
attacks. Each page's status line says "partly tested" for these and names the gap. One line per
gap: the page's section, the claim, and what no case attacks.

### ipc.md
- R1 (flow): a call or send between user budgets with different labels is attacked only in the model.
- R2 (fair waiting): the keying of groups by account, label set and (for account 0) budget is attacked only in the model, and on the kernel turns between groups only by `ipc-fair-label-sets` (two unlabelled groups beside a vault's turn); `redoubt-ipc` fills one group's cap.
- What `receive` returns: a record made unwritable while its thread waits is attacked for an exit notice (`process-attack`'s late record), an interrupt and an abandoned-call notice (`receive-bad-record`); for a message it is not attacked.
- How a call completes, R13 (one outcome per call): completion races between harts are not attacked.
- R14 (unforgeable sender): every case delivers account 0 and no labels; a non-zero account or a label set reaching the receiver unchanged is attacked only in the model (`MsgNoLabels`, `MsgAccountZero`).

### README.md
- What the kernel keeps: that no driver, file system, naming or policy code sits in the kernel rests on the source map, not a case.
- The no-cruft gate has no self-check: no planted forbidden name, `allow(dead_code)`, unused feature or second `PAGE_SIZE` is shown to fail it.
- The TCB's line count is measured, not pinned by a case; only `unsafe` has a ceiling.
- Nothing checks that every `kernel/src` file appears in the source map.
- The unsafe ratchet cannot show that every on-target TCB source file is listed in a budget.

### objects.md
- What objects cost: an endpoint's one page to its owner is attacked only in the model (`R6EndpointsFree`).
- What objects cost: a device object's one page to `system` is attacked by nothing.
- What objects cost: the header page is pinned by no case.
- `mint`: `Dead` from a message source whose call's stamp was destroyed has no case and no mutation.
- R9 (stamps): a handle minted from a call taking the call's handle's stamp (not the caller's budget) is attacked only in the model (`R9MsgStampIsSenderBudget`).

### budgets.md
- R6 (charging): an endpoint's page charge is attacked only in the model; the header page is pinned by no case.
- R10 (destruction): timeouts before deadlines at an equal instant is attacked only in the model (`ExpireBudgetsFirst`).
- Class is trust, not order: that the scheduler never reads class is argued from the code.
- Deadlines: a process entering the kernel in a tight loop to put its deadline off is not attacked.
- Root, system and users: no case checks the boot table (`root`'s 63 processes, the weights, `INIT_WEIGHT`).
- R10 (destruction): destroying the budget a device object is charged to (the device destroyed, every handle closed) is not checked by a case.

### timer.md
- Time: that `time_now` counts from the kernel's start is not checked (only monotonic, never early, linear with `rdtime`).
- Expiry: the order at an equal instant (timeouts before deadlines; (pid, tid); budget id) is attacked only in the model (`ExpireBudgetsFirst`).
- Budget deadlines: a destroyed child's later deadline leaving the list is shown only by the kernel surviving past it.
- Failure and restart: a boot with no `Time` tag (or 0) powering off is not attacked (R17 (fail closed)'s gap too).
- The hart timer: a stale early hint costing one early interrupt and missing nothing is argued, not attacked.
- A passed deadline comes first: a process entering the kernel in a tight loop to put its deadline off is not attacked (the same gap as budgets.md's).

### devices.md
- Device objects: the one page a device object costs its owner is not measured; a DMA device past `MAX_DMA_DEVICES` (16) getting no object is not attacked.
- `dma_alloc`: the `MAX_RUNS` (32) runs-per-device limit is not reached by a case.
- Reset before reuse: that the reset precedes pooling inside the kernel is attacked only in the model.
- Devices handed to the first program: the loader's refusal of a device tree with no console, or a console with no interrupt, is not attacked.
- R5 (interrupts): masking a fired source is attacked only in the model (QEMU's 16550 raises per byte); completing the claim before masking, and billing interrupt time to the IRQ object's owner, are not attacked.
- R18 (device authority): the kernel's refusal of a malformed `Devs` entry and of a `Grnt` boot argument is not attacked.
- Failure and restart: destroying a device object's owner budget (IRQ waiters get `Dead`, source masked, handles swept) is not attacked; nor is a device handle closing when the budget that stamped it is destroyed.

### processes.md
- Processes and PIDs: that PIDs are drawn at random is not attacked.
- Threads: `thread_create` refused with `OutOfMemory`, and the first thread returning from its entry (a fault), have no case.
- Creating and starting: `OutOfMemory` from `process_start` has no case.
- Exit notices: a notice dropped because its exit endpoint was destroyed has no case.
- R21 (crash blame): blame after the blamed sender's budget is destroyed has no case; a thread holding a parked call that receives a send and then faults (blames nobody) is covered only in parts.

### memory.md
- Backing and zeroing: that a frame freed with data comes back zero is attacked only in the model (`R11NoZeroing`); no case can tell which frames it was handed.
- Instruction fetch after mapping: no case can see a missing `fence.i` (QEMU keeps fetch coherent).
- Lending at the page-table level: a lend within one process is not attacked across harts.
- R11 (memory): the absence of any physical-address argument is argued from the call table.
- R19 (kernel W^X): no case plants a writable kernel code page to show the boot check stops.

### scheduling.md
- One flat stride queue: round-robin among one budget's threads is not attacked.
- Preemption points: an interrupt's wake not preempting, and another budget's deadline preempting, are not attacked.
- The current minimum and ties: clauses 1 and 4 are checked on the target only when a run happens to tie; the host tests and model attack them.
- Charging: interrupt handling billed to the device object's owner is not attacked.
- Responsiveness: `budget_destroy` call-to-return is recorded, not asserted.
- R12 (scheduling): the bound on a call's kernel time is attacked only for `map_anon`'s search and `map_fixed`'s range.
- R23 (no test channels): no case builds the production kernel and scans it for the trace.
- Failure and restart: a picked thread dying before the switch, and a full queue, are not attacked.

### boot.md
- Firmware: the bench never falling back to QEMU's own firmware is not attacked.
- The argument block: the kernel's refusals of a malformed block (a tag past the end, a second `MREx`, a bad `Devs` entry, a `Grnt` tag) are not attacked.
- R16 (image confinement): an image cut short inside its segment data and a writable and executable segment are not attacked; the truncated-image case runs on rv64 only.
- R17: a short or missing seed and a missing timebase are not attacked (every QEMU boot supplies both); nor an initrd under 64 bytes.
- Verified boot, R15 (verified boot) and R17: the two signature cases run on rv64 only.
- Devices handed to the first program: a DMA device past the sixteenth getting no object is not attacked (as devices.md's).
- Failure and restart: a reboot through `system_reset` (the chain rerun, the bundle verified again) is not attacked.

### memory-layout.md
- `satp` and the TLB: no case attacks a stale translation surviving an address-space switch or an unmap (one hart, global flush; argued from the code; attacked across harts in M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts)).
- Placement areas: the message area (`0x4000_0000`, 4 MiB) and `map_anon` area bases are not attacked as addresses (not security claims; listed for completeness).

### abi.md
- The kernel keeps every register outside a0-a7 across an `ecall`: no case attacks it.
- The order of checks after decoding: pinned by a case only for the first checks of `budget_create`, `budget_usage` (records before the handle: `budget-syscall-attack`), `call` (record before endpoint lookup: `ipc-outcomes`), `receive` (`WrongObject`), `serve`, `process_start` (count before record: `process-attack`). The rest of each row (stages 2 to 5) is not attacked.
- The kernel's order against the model's: no trace replay (planned for M1 (separation and containment)), so the rows that differ were found by reading only.
- A valid call number with bit 32 set on rv64: not attacked (`legacy-gone` does it for 0..=46 only).

### invariants.md
- I7 (every flow obeys R1): a message between user budgets with different labels: no case (the same gap as ipc.md's R1).
- I9 (pages W^X, zeroed, lends unmapped): reuse of a freed frame (a case cannot choose which frame it gets; `mem-attack` says so): model only.
- I11 (fair turns): turns among several groups on one endpoint: model only (the same as ipc.md's R2).
- I12 (ids never reused): budget and message ids never reused: invisible to a process, model only; endpoint, device and process-object ids: nothing attacks them, not even the model (it checks budget ids only).
- I13 (every blocking call returns by its timeout): timeouts on a multi-hart boot: `timeouts` has no `smp` key (M2 (usable shell): [several harts](../plan/m2-usable-shell.md#several-harts)).
- I16 (DMA pages reset before reuse): a live co-holder that still reaches a device reset at another holder's death: model only (`reset_at_one_death_does_not_cover_a_co_holder`).
- I1 (handles name live objects) and I10 (create-destroy leaves the parent unchanged): no mutation targets I1 alone; I10's `R10KeepCarvedLimits` is caught first by the per-step R6 recount, so `budget_lifecycle`'s own check may be doing no unique work. Not a gap in the kernel, a note on the model.

### model.md
- R15 (verified boot), R16, R17, R19 and R23: not in the model at all.
- `steward_noninterference` leaves out a server crash at an instant and one a vault's call causes (service-slot timing, a residual of R37 (vault non-interference)).
- Model replay on the real kernel: no case; every "attacked only in the model" on the kernel pages rests on it.
- The budget test's hand-copied model sequence: nothing checks it still matches the model.

## Why it matters

A status line that says "built" promises a test that would fail if the claim were false. Each
line here is a claim whose failure no bench case would see: it rests on the model, on reading the
code, or on nothing. The model's evidence about the kernel is only as good as its replay against
the kernel, which does not exist yet.

## Where

- [`tests/`](../../tests): the bench cases, one `.toml` each, and their programs in
  [`tests/programs`](../../tests/programs).
- [`model/`](../../model): the executable model, its properties and mutations.
- The kernel pages in [`docs/kernel`](../kernel/README.md), whose status lines name each gap.

## Done when

Each line has a case that attacks it, and the page's status line names that case; or the page
states the claim as argued from the code, outside any "built" status. The file is empty when
every kernel section's status line is `built · tested`.
