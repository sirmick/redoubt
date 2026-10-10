SMP5 handoff — per-hart frame magazines (R84, Option X). Branch wp-SMP5 in /home/mcloonan/redoubt/.worktrees/SMP5.

## Branch state (frozen; kernel-red reviewed 1ecb4d3c8: correctness OK with notes; value held for owner choice: merge-now vs park-until-lock-split)
- Base 58326f13c (SMP6 merge as first built; NOT on main: main was rewritten without SMP6; SMP6's fixed version rides train 20 -> expected main 9e4150cac = 2afa4e38d + SMP6(one-drainer fix, B56) + K31).
- 783aae576 model: magazine (MAGAZINE=2 in the model), Event::Tick commit fills it (from in_flight zeroing, then free_frames), alloc draws it first; checker R84 clauses; mutations R84MagazineUnzeroed, R84MagazineStillFree; Mutation::ALL 172; MODELLED gains R84; Size budget: model 11,125->11,155.
- 1ecb4d3c8 kernel, testbench, docs (one commit: pages move with code). Size budget: kernel 10,595->10,871. No new unsafe.
- Nothing uncommitted. Never pushed (implementer). No WIP commits.
- Report: .wash/local/SMP5-report.md; design + idle-path assumptions §9: .wash/local/SMP5-design.md; orchestrator ruling thread SMP5-magazine-saving.

## Design as built (kernel/src/reclaim.rs, mem.rs, irq.rs)
- Owner MAGAZINE = Pid 0xfffc (mem.rs, beside IN_FLIGHT 0xfffd); bit clear in bitmap; every owner check refuses it.
- reclaim.rs `Magazine` per hart (static MAGAZINES[MAX_HARTS]): frames[MAG_MAX=16] + n (zero frames, under the lock only); fill[REFILL_MAX=2] + fill_n, fill_entry (section number that took them), fill_state TAKEN 0 / ZEROING 1 / ZEROED 2; entry counter bumped at every mem::entered(). Slot word = index+1 | FROM_PENDING (bit 31).
- Draw: mem::alloc_frame -> reclaim::draw() (pop magazine; `drawn` asserts MAGAZINE owner, debug samples zero) else bitmap/take/reclaim_wait; then `gather()`: if wants_refill() (fill_n<2, n+fill_n<16, same section), take own pending (pop_own; IN_FLIGHT->MAGAZINE, in_flight-=1, put_fill(from_pending)) else lowest free frame -> put_zero if clean, put_fill if dirty (boot-never-used).
- Zeroing: ONE line after KERNEL_LOCK.release() in arch/riscv/irq.rs return_registers: reclaim::refill(): only if fill_entry==entry (this section took them) and CAS TAKEN->ZEROING; kframe::zero each; store ZEROED; wake WANT. Billing = caller's user time (leave() already closed the kernel bill and set user_since).
- Next entry: mem::entered() -> refill_found() BEFORE commit: reclaim::entered() takes the fill out; ZEROED -> check_zero (inflight-trace audit) + put_zero, and sched::zeroed_here(me) per FROM_PENDING frame (freer's debt off, unbilled); not zeroed (section ended by fault/block/switch/idle) -> unfill: pending frame back IN_FLIGHT + in_flight+=1 + reclaim::pend; bitmap frame back free with its dirty bit set.
- Steal (the "return", under the lock): reclaim_wait after commit/find_free: reclaim::steal() pops any hart's magazine, then any refill via CAS from ZEROED or TAKEN to ZEROING (restore state, TAKEN if emptied); zero under the lock if not zeroed; zeroed_here(owner hart) if FROM_PENDING; trace '3' pass 2; set_owner None; return. NoFrame only if in_flight==0 && !refilling(). wait_done also stops halting when any fill_state==ZEROED.
- Audit (check_free_frames): count of MAGAZINE owners == reclaim::magazines() (Σ n + fill_n).
- Trace (inflight-trace): '1' fill (pass 1 pending / 0 bitmap), '2' into magazine, '3' out (0 bitmap, 1 pending, 2 stolen), 'o' pass 1 = drawn. Oracle sched_oracle::flights() (smp_inflight / smp_magazine); whitelist "...123".
- Negatives (kernel features): magazine-unzeroed (refill skips zero; fails mem.rs check_zero 'R84: ... entered a magazine not zero'), magazine-still-free (gather leaves bit set; fails I1 in unfill/drawn).
- Oracle hold-trace line gains "allocating, net of audits:" per-cause totals (ALLOCATING const).
- Case tests/smp-magazine-race.toml + tests/programs/src/bin/smp-magazine-race.rs (witness starts after fillers via START message; two threads single-page churn). smp-inflight-race forbids 'R84:' too.

## Measurements (rv64, 2 harts, containment, hold-trace config 288 MiB ring/640 MiB; logs .tmp/SMP5/{before,before2,after,after2}-smp2.log)
- endpoint_create 73,691: 249.53 M -> 229.44/229.96 M ticks; 3,386 -> 3,113/3,120 each (-8 %, ~27 us). map_anon 5,708->5,262; process_map 9,986->9,679.
- Lock waits 2,634 M -> 2,672/2,670 M (+1.4 %); behind sections flat; kernel-audits 2,549.7 -> 2,579.0 M (+1 %, 2 % more sections); charged +28.5 M (refill zeroing now caller time).
- R10 p99 26.43->26.36 ms; deadline_notice p99 34.93->35.14 ms (bound 40); lease end 30.46->29.93 ms.
- In-flight peak (inflight-trace config, .tmp/SMP5/{before,after}-if.log): 80,734 -> 24,396; 56,433 refilled from pending.
- 'time_now 270k ticks' longest section in after = a deadline destruction run at that entry (irq.rs:231 expires due deadlines at every entry); not new work.
- Before is deterministic (two runs identical); pre-SMP6 main run matched before within 0.03 %.

## What would show the real gain (~1 day, not built)
A 2/4-hart MTTCG case (like sched-lock-contention-4-mttcg, quiet class, host clock) of single-page map/unmap churn on each hart; numbers: pages/s per hart and lock-wait share, before vs after. Under icount the one virtual clock bills zeroing anywhere, so icount cases can't show parallel zeroing.

## Rebase onto train 20 tip (only on the orchestrator's go)
- Expected conflicts: kernel/src/reclaim.rs and mem.rs reclaim paths (SMP6's take_one / DRAINER one-drainer fix vs magazines), kernel size ceiling (main measured 10,642: re-measure, keep the Size budget line in the kernel commit), docs/kernel/README C13 unsafe cells (37/11: no new unsafe here, so unchanged unless the count moves with tests/unsafe-budget.toml), mutation count (175 + 2 = 177 in model.md and Mutation::ALL array size), testbench.md/memory.md text around R81 drains.
- kernel-red P2-2: after the rebase, take_one's 'more' and the idle wake mask must not count magazine/refill frames (they are MAGAZINE-owned, not on pending lists: keep it so), and the zeroing at return (refill) must not reintroduce a pending-timer pass (SMP6's fix for ~190 IRQ/s at rest): refill only runs when fill_entry==entry, i.e. only after a syscall section that drew frames; nothing runs at rest. Verify with launch-idle (interrupts/s per hart at rest) after rebase.
- Re-run after rebase: model-mutations (deadline), smp-magazine-race + smp-inflight-race both widths, the 44-case set at 1 and 2 harts (script .tmp/SMP5/sweep.sh <own|1|2>, needs make prebuilt first), docs, size/unsafe/formatting, launch-idle, containment hold-trace after.

## Traps
- Never edit the worktree during prebuilt/case runs ("tree changed"). make prebuilt after any change.
- cargo test -p redoubt-model runs the full mutations test (hours): use the bench cases model-host-tests / model-mutations.
- A model mutation that loops forever hangs model-host-tests (coverage runs all mutations).
- Format with `cargo +nightly fmt -p <crate>` (per-crate editions; kernel is 2018).
- jobs.mk runs kernel-containment at its own count (1 hart): use q run ... cargo testbench --exact --arch rv64 --smp 2 kernel-containment for 2 harts.
- Scratch worktrees under .tmp/SMP5/ (before, after, before-if, after-if, neg-magazine-*): git worktrees, remove with `git worktree remove --force` when done.
