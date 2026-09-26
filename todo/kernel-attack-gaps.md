# Kernel attack gaps

Claims the kernel pages state as built that no bench case attacks, or that only the model
attacks. Each page says "partly tested" for these. The switch-over folds this list into
`docs/todo/` (one file per gap, or one file per page); until then it is working material.
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
