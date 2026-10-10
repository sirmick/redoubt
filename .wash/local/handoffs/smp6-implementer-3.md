SMP6 handoff (smp6-implementer-3, 2026-10-10). SMP6 is merged into train 20 (1fcdd656d, K31 on top at 9e4150cac); the whole bench on 9e4150cac was running at handoff time.

# Branch state and traps first
- wp-SMP6 is at f590f70c8 (kernel, testbench) on 4dce2abe5 (model), on main 2afa4e38d (04b7fa4d5 + CTX3). The worktree /home/mcloonan/redoubt/.worktrees/SMP6 is clean and frozen until train 20 is pushed.
- Earlier heads, all reachable by sha:
  - 8fb11dc37: the original, which had B56.
  - 833331cdb: the first fix, which kernel-red reviewed.
  - 4c1dd6a91: round 2, on base 98b866854.
  - c3eaec9d7: rebased onto 04b7fa4d5.
  - f590f70c8: rebased onto 2afa4e38d.
- TRAP, README rule C13. docs/kernel/README.md's TCB `unsafe` cells are held to tests/unsafe-budget.toml by doccheck C13 (K32). SMP6 sets the kernel/src row to 37 and the "kernel: Sv39, SBI and PLIC backends" row to 11 (11 + 10 + 16). Any rebase must keep those cells in step with the ratchet.
- TRAP, mutation counts. They live in three places:
  - model/src/mutation.rs `ALL: [Mutation; N]` (model commit);
  - docs/kernel/model.md "lists all N variants" (kernel commit);
  - docs/testbench.md's `model-mutations` cost row "N jobs" (kernel commit; SMP6 had missed it until the train-19 rebase).

  Main with CTX3 = 173; SMP6 adds 2 (R81InFlightAllocatable, R81CommitUnzeroed), so 175. The model size ceiling is CTX3's 11354 + 33 = 11387, and the model commit's "Size budget: model: 11354 to 11387" line must match. Kernel ceiling 10,608 (main 10,076 + 532). K31 was meant to move it to 10,106; check what train 20 resolved.
- TRAP, crossed messages. Orchestrator messages arrive out of creation order. I rebased onto 04b7fa4d5, then got an older "hold at 4c1dd6a91", reset, then a newer "go back to c3eaec9d7". Read each message's created_at before acting on a base change, and confirm the branch sha after any reset.
- TRAP, launch-idle docs. docs/testbench.md "The machine at rest" and the kernel commit message state measured ranges: "five windows each, 3.1 to 4.2 (rv64) and 2.7 to 3.7 (rv32), at 0.005 to 0.008 cores". Each new window outside the range made them false; I amended them three times. B57 should replace window-count prose with the case's own printed drain numbers.
- No edits in a worktree while prebuilt cases run ("tree changed"). `pgrep -f` matches your own shell; check /proc/<pid>/cwd.

# R81 as built (kernel/src/reclaim.rs, arch/riscv/mod.rs idle(), mem.rs)
- **In flight.** A freed frame's owner becomes IN_FLIGHT and its bitmap bit stays clear. There are per-hart lists in `reclaim::LISTS`:
  - **staged:** by number, until the call's shootdown; STAGE_MAX 64 shoots early;
  - **pending:** linked through LINK = DEFER_WORD*8 inside the frame. Changed only under the lock;
  - **zeroing:** the one frame this hart zeroes outside the lock;
  - **done:** a lock-free push by its own hart; the lock holder takes it whole with a swap.

  `mem::entered()` at every lock acquisition gives back up to 64 done frames (COMMIT_MAX) to the bitmap, clearing LINK.
- **A1.** An idle hart takes ONE frame per pass under the lock (`take_one`), releases the lock, zeroes it (`zero_taken`), and with more left does not halt. It goes back through kmain's SIE window, so an interrupt waits at most one frame.
- **A3.** It zeroes only while every started hart is idle (`hart::all_idle()`), because of icount's shared clock.
- **No zeroing at an exit to user mode** (ruling A). SMP5's Option X amends this narrowly; see below.
- **On demand.** An allocation that finds the bitmap empty commits the done lists, then `pop_pending` (any hart's list, zeroed under the lock, the payer decremented by count). If only zeroing frames remain, it waits in `wait_done` (WANT + wake_halted).
- **Billing.** Idle zeroing is billed at the next lock take (`sched::zeroed_idle`) to up to four payers per hart. A destruction lifts the debt owed by a budget it ends (`p` trace record); the `0` record traces zeroing.
- **Checked build.** `reclaim::counted()` counts staged + pending + zeroing + done against the ownership table at each destruction's audit.
- **One drainer at a time (B56 fix).** `reclaim::DRAINER` holds the boot index plus one; it is changed only under the lock. `take_one(all_idle) -> (frame, more, wake)`:
  - `free = all_idle && (DRAINER == 0 || mine)`.
  - Takes its own pending frame only if free.
  - If more are left: claim DRAINER and return.
  - Otherwise: clear the claim if mine. If free, return the first OTHER hart with pending frames as `wake`, a mask.

  `arch::idle` releases the lock, then `hart::wake_halted(wake)`, after the release so the woken hart finds the lock free.

  This covers three cases:
  - **The hand-on:** the finisher wakes the next hart.
  - **The last-idler wake:** a hart idling with nothing of its own, when every hart is idle and no one drains, wakes a hart that halted with frames while another worked. This was kernel-red's P2-1.
  - **A drainer woken to work:** it keeps the claim while busy (no one else can drain anyway, since not every hart is idle). At its next idle pass it continues if every hart is idle, or else clears the claim.

  A wake before the target's wfi is not lost: halt() skips the wfi when sip&sie != 0, and SSIE is enabled.
- A hart drains only its OWN list idle; billing is per hart.
- Untested path, said so to kernel-red: the stranding and last-idler wake is not expressible in the model (no harts), smp-inflight-race (harts never idle) or the trace (printed only at a reset). It would need a new trace record or a checked assertion at the idle pass.

# B56 measurement and method
- **Cause.** launch-idle (tests/launch-idle.toml, quiet class, 4 harts) failed at 166.6/s (rv64) and 88.3/s (rv32) per hart on 8fb11dc37. On the bisect run at train 18 it read 191/s.
  - Method: count QEMU's -d int log by hart, cause and epc. Symbolise against the release kernel: `nm -C <kernel> | grep -E ' [tT] ' | grep -v '\.L\|\$x' | sort` and take the greatest address ≤ epc. Kernel: target/testbench/last/cargo-qemu-virt/riscv64imac-unknown-none-elf/release/redoubt-kernel, or target/prebuilt/rv64/cargo-qemu-virt/.../release/redoubt-kernel. The prebuilt dir also has checked and hold-trace kernels; use release.
  - m_software epc = the instruction after wfi in TicketLock::acquire_ticket, paired 1:1 with supervisor_ecall in hart::wake_halted: lock hand-offs.
  - The int log has no timestamps. Use s_timer (~1/s per hart, in clusters of four) as the clock: ~33k (rv64) / ~16.6k (rv32) IPIs fell in one interval at the window's start.
  - It was the login drain: ~35k frames, four harts draining at once and contending per frame.
- **Ruled out:** a timer storm (s_timer 0.7-0.9/s throughout), WANT, shootdowns, user ecalls.
- **Per cause, busiest hart:**

  | | rv64 | rv32 |
  | --- | --- | --- |
  | Before the fix | 166.6 (m_software 165.5), 0.012 cores | 88.3 (m_software 87.3), 0.010 cores |
  | After the fix, five windows | 3.1-4.2 (m_software 2.1-3.1, s_timer ~0.8, s_software ≤ 0.2), 0.005-0.006 cores | 2.7-3.7, 0.007-0.008 cores |
  | IDLE1, before SMP6 | 3.1, 0.002 cores | 2.8, 0.002 cores |

  After the fix, acquire_ticket waits over a whole window = 520 on all harts, against IDLE1's 559: the drain adds none.
- **Drain length.** Measured with a temporary print in take_one (patch at /home/mcloonan/redoubt/.tmp/SMP6/b56/measure-drain.patch, not committed):
  - rv64: 34,691 frames in 0.25 s (~7.2 µs a frame), starting just after `log int`;
  - rv32: 35,440 frames in 0.32 s (~9 µs a frame).
  - At rest: 1-3 frame drains every few seconds. After `log none`: ~3.8k frames.
  - Host cost: ~0.004-0.005 cores of a 60 s window.
- Logs and int logs: /home/mcloonan/redoubt/.tmp/SMP6/b56/ (idle-*.log, *.int.log, measure-*-smp4.log, containment logs, nm*.txt).

# What B57 needs (launch-idle settle: open the window at frames in flight = 0, print the drain)
- **The kernel has no runtime count visible to the bench.** `counted()` exists only in debug_assertions, and launch is a release kernel. Options:
  - a release-cheap counter of frames in flight: increment in `retire`/pend; decrement in commit and in `pop_pending`'s give-back;
  - or "all lists empty" (every LISTS[h].pending, staged_n, zeroing and done is 0), printed on the console when it becomes true. The debug console is UART; `crate::println!` works and the bench reads console lines.
- My measure patch shows where the drain begins and ends (take_one; frame != 0 && !more ends a hart's run). The bench could read a kernel line such as "frames in flight 0 after N in T us".
- Drain on launch: ~35k frames in 0.25-0.32 s. Background frees continue at rest (1-3 frames every few seconds), so "0" is momentary. Wait for 0 once after login; don't require it to stay 0.

# What SMP5 needs (magazines, R84, Option X)
- Option X, as ruled (architect-questions.md, thread SMP5-magazine-saving): after a syscall releases the lock, the hart zeroes at most the 2 frames its caller drew (its pending first, then the bitmap's), billed to that caller. They enter the magazine zero at the next entry.
- Interaction with my fix: Option X zeroes at an exit, outside the idle path, so DRAINER is untouched. But if a magazine refill takes this hart's PENDING frames, `take_one`'s list may empty mid-drain. That is fine: !more clears the claim and hands on.
- Keep pending lists changed only under the lock, and count magazine frames in `counted()`, or the checked audit fails.
- Its zeroing-at-exit must be added to `end_section` / irq.rs return paths with care: `reclaim::end_section()` runs before every release.
- launch-idle should be unaffected (no exits at rest beyond ~4 user ecalls/s). Rerun it at 4 harts alone, both widths.

# Paths
- Report: /home/mcloonan/redoubt/.wash/local/SMP6-report.md. Sections: B56; B56 follow-up (drain and per-cause tables); B56 round 2; Rebase onto main 04b7fa4d5; kernel-red P2-1; Rebase onto train 19's tip.
- Design: .wash/local/SMP6-design.md. QA thread: .wash/qa/SMP6-zeroing-billed-to-nobody.md (grep '^## ', never tail).
- Code:
  - kernel/src/reclaim.rs (DRAINER, take_one);
  - kernel/src/arch/riscv/mod.rs idle();
  - kernel/src/arch/riscv/hart.rs (all_idle, wake_halted doc);
  - kernel/src/mem.rs (retire/commit/pop_pending).
- Docs:
  - docs/kernel/memory.md "Frames in flight" (Idle bullet), R81, residuals;
  - docs/testbench.md "The machine at rest";
  - docs/kernel/README.md TCB cells;
  - docs/kernel/model.md.
- Env: .wash/local/launch-env-2026-10-09.md. Gates: `make -f scripts/jobs.mk set CASES=...`, `make -f scripts/jobs.mk prebuilt`. launch-idle: `scripts/q run --cores 4 --quiet -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> launch-idle`, one width after the other, alone. Containment at 2 harts: `q run --cores 2 -- target/prebuilt/testbench --prebuilt target/prebuilt --exact --arch <w> --smp 2 kernel-containment` (~10 min rv64, ~15-17 min rv32; notice p99 35-37 ms against 40).
