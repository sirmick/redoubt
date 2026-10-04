# K16: how far the kernel's fixed limits rise, and what each costs

The owner asks (2026-10-02) to raise `MAX_PROCESS_COUNT` (64, `kernel/src/arch/riscv/process.rs:36`,
a bare constant) and `MAX_THREADS` (31, `libs/sys/src/lib.rs`), and to consider raising a few of the
other caps generously now, to be screwed down later. Design K16 (plan node, todo) and write its
brief at `/home/mcloonan/redoubt/.wash/local/K16-implementer.md`.

## What the design must settle

1. **The new values and why.**
   - Processes: a deployment's count is init, about ten servers, the steward and sshd, then a VM
     per session and a process per agent and per sub-agent. 256?
   - Threads per process: 31 looks like a 32-bit mask with TID 0 unused. 63? 127? What beamlet's
     platform will want.
   - Your recommendation for `WAIT_CAP` 16, `MAX_OPEN_CALLS` 64, `MAX_START_HANDLES` 64,
     `MAX_DEPTH` 8 (the lease chain root → system → sessions → session → lease → agent → sub-agent
     is already 6 deep), `MAX_LABELS` 8.
2. **What each costs.**
   - The static tables: `[Process; N]` in `ptable.rs`, `[Account; N]` and the process list in
     `budget.rs`, `sched.rs`'s runnable array on the stack, `PROCESS_IMPL_PAGES` per process,
     against the kernel half on rv32 (`memory-layout.md`) and the per-process kernel data region.
   - Every walk bounded by N × T: timer expiry, R2's sender scan, R10's sweeps that K9, K10 and
     K12 just brought under 30 ms, device close. What the gate's and `sched-latency`'s targets
     become, and whether they must be remeasured in K16 itself.
   - Any bitmap or field width that silently assumes 64 or 31: `u64` masks, the 28-bit frame
     index in a handle slot, `Account::dma_mapped`'s `u16`, the PID's width in a slot or a notice.
3. **Static or charged.** Whether the tables stay static at the new size, or any of them becomes
   frames charged to budgets. Tenet 1 prefers the obvious static table; say so if that is the
   ruling, and say what the kernel's static footprint becomes on each width.
4. **The pages and the cases.** Every page that states a limit: `processes.md` (PIDs 2 to 64),
   `boot.md` (63 programs; INIT1 removes the loader's count), `objects.md` (what objects cost, the
   frame index), `budgets.md` (R10's budget, residual risks), `ipc.md` (R2, R4a), `abi.md` (the
   constants), `memory-layout.md`. The attack cases that pin them: the gate fills 4,091 handles;
   `endpoint-destroy-full`; `pages-exhaustion`; anything that counts threads.

Anything that is a genuine owner choice (kernel memory on rv32 against the process count, say) goes
to the owner with your recommendation; the rest you rule.

## Hotspots

INIT1 holds `budget.rs`'s `boot_*` functions and `setup_loader_process` until it merges; K13 holds
`message.rs`. Say whether K16 waits for either, and set K16's `needs`.
