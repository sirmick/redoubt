# K20 handoff (k20-implementer → k20-implementer-2)

## State: done and reported, awaiting review

There is no WIP. Branch `wp-k20` in `.worktrees/k20` holds one commit, 2de60cc58, on base
31dfad3f0: "kernel: a timer interrupt is billed to what it found, never to the budget it
interrupted". The tree is clean apart from the untracked `.k20/`, which holds logs, the scratch
notes and `report.md`. The full report is `.worktrees/k20/.k20/report.md`. The final report was
sent to the orchestrator as an answer, because the assignment had already resolved at the
checkpoint.

## Status of each item: all DONE in 2de60cc58

### The fixes

- **The billing fix.**
  - In `irq.rs`, after expiry, `sched::bill_from_now(expired.last)` makes the rest of the entry
    the budget billed last.
  - A timer entry calls `begin_billing` only when nothing was found and `slice_over` (read once,
    before `on_interrupt`). Non-timer entries keep `begin_billing`.
  - In `sched::leave`, the payer is re-opened after the first close and billed through
    runnable(), reconcile and rearm, then closed at `back`. `user_since = back`.
  - `kmain`'s pick after a deschedule stays the descheduled budget's, as the page line says.
- **The arming change.** In `message.rs`, `mark` only records the wait and the deadline.
  - `settle`'s block branch lowers `earliest_timeout` and calls `note_timeout`.
  - That branch is the only `activate_process_thread(.., false)`, so every way to block passes
    it. A call taken by its server is already blocked from there.
- **The stale-wait payer.** `next_timeout` returns `Timeouts { due, next, stale }`.
  - `stale` is the last account whose cache had come with no thread due.
  - In `time::expire_due`, `stale` is used only if nothing expired, because an item expired
    leaves its own account's cache behind.
  - That budget then becomes `last`, and the final walk plus the re-arm are billed to `last`.
- **The deadline payer.** It is `mm.destruction_payer(frame)` after `mark_dying`, so budget.rs
  is untouched.

### The direct check, the negative and the case

- **Trace records** (sched-trace, recorded only inside a timer entry from user mode):
  - `I`: the interrupted budget's id (0 for none) and the time in µs.
  - `B`: the payer and the ticks, from `close_billing` and `sched::bill`.
  - `E`: the last budget, with pass 0 for none, 1 for an item, 2 for a stale wait.
  - `O`: pass 1 for a return to user, 0 for a return to kmain.
- **Oracle rule** (`check_timer_entry`):
  - After an item, every charge after `E` goes to the last budget.
  - After a stale wait, every charge goes to that wait's budget.
  - Otherwise, only the interrupted budget is charged, and only when the entry returns to kmain
    (a slice end).
  - The oracle counts the entries that were nobody's and ended no slice in each SHARE window
    (`timer_empty`).
  - Unit test: `a_timer_interrupt_bills_the_budgets_whose_items_it_expired`.
- **The negative feature.** `timer-tail-billed` implies sched-trace and keeps the old billing in
  irq.rs and in leave. It is listed in scheduling.md's R23 lines. It FAILS the check on both
  widths (`.k20/neg-rv*.log`).
- **The third case, `cancelled-waits`.** Role TimerFlood with p3=2 runs `flood_waiter` x2, each
  on its own endpoint, and one `flood_answerer` that sends with timeout 0.
  - Timeouts are 15 ms plus 1.3 ms per waiter. The program checks that the count of waits
    answered is above 0.
  - **Departure from the ruling's "few hundred µs":** at short timeouts, or with 29 waiters,
    every wait timed out for real, because a block hands the CPU on.
- **Page lines.**
  - scheduling.md Charging bullet and timer.md R12 paragraph: from K20-empty-timer-ruling.md,
    which supersedes the rerule's lines.
  - timer.md residual "Expiry walks threads": changed.
  - Also changed: timer.md's hart-timer hint lines and figure, the Responsiveness figures, a
    Charging paragraph on the check and the negative, the R23 trace-site list (time.rs and
    irq.rs added), and the boot-case list.
- **Size budget.** The kernel ceiling went from 7752 to 7856, with a `Size budget:` line in the
  commit.

## Numbers

| Measure | Before | After |
| --- | --- | --- |
| Empty timer walks | 5,236 | 0 |
| Timer entries, flood case 1 (rv64) | 5,697 | 461 |

- **The ruled fix without the arming change:** the flood shares fell to 418/419 (rv64) and
  395/397 (rv32).
- **Final net shares**, sleepers / deadlines / cancelled-waits:
  - rv64: 494 / 494 / 493
  - rv32: 492 / 492 / 492
  - Main was 469/472 and 468/469.
- **Whole bench:** 280 PASS and 1 SKIP. The one FAIL was size-budget, which now passes after the
  raise.
- **Stale-wait entries:** 69 in the flood case and 111–133 in sched-latency.
- **sched-latency** at seed 3 passes on both widths.
- **Non-reproduction:** 445/446 on ca5a6437b (K16's figure); 469/472 and 468/469 on 31dfad3f0.
  Padding raises the share, +10 at 8000 nops.

## Traps

- **K16 overlap.** K16 commit 1 changed only `next_timeout`'s two loop headers, to
  `for pid in pids()` and `for tid in mm.live_tids(pid)`. My rewrite of its return and of the
  per-account logic overlaps that one hunk. Rebase mechanically: keep my logic over their
  iteration.
- **K16 also edits `sched::leave`.** K16 owns the walks, and K20 owns the billing order.
  Whichever merges second rebases.
- **No `as u8` for a PID or an index.** K16's commit 3 widens Pid to 16 bits. I added none.
- **Run every command through `/home/mcloonan/redoubt/.wash/local/in-dev`.** Format with nightly
  `rustfmt +nightly --config skip_children=true <files>`; stable `cargo fmt` gives false diffs.
- **Child programs cannot print to the console.** Debug them through the report value.
- **A program rebuild moves the shares by a few thousandths.** Re-measure before quoting figures
  in a page.

## What consumed my context

- **The two re-rulings**, and diagnosing the stale-hint cause with scratch prints.
- **Debugging the third case's design.** It went through four variants: calls through a badge-0
  handle, too-short timeouts, timeouts aligned to slice boundaries, then waiters with a sender.
- **The "read in full" rule:** message.rs (1780 lines), sched_oracle.rs, the test program's
  sched.rs and scheduling.md.

## Next step

Wait for the review. On findings, fix them in 2de60cc58 as a clean rewrite, since the history is
not yet merged, then re-run the flood case on both widths, size-budget and doccheck.
