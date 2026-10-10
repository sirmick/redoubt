# SMP2 checkpoint 2: the four scenarios in the model (the brief's step 3)

The model's scheduler (model/src/sched.rs, uncommitted in .worktrees/SMP2) now:
- runs H harts. Each pick is one hart's: the lowest-ranked queued budget with a runnable thread
  no hart runs.
- computes the cap set by water-filling as a fixed point: cap every budget not yet capped with
  w x H' > k x W', take their threads out of H' and their weight out of W', and repeat until none
  is capped. This is the model's own way, with no ordering shortcut.
- takes the floor over queued budgets that are not capped.
- lifts a budget that is no longer capped to max(own, floor).

The old floor is the mutation R12CappedHoldsFloor.

The driver is model/src/check.rs `smp_scenario`. Harts run in lockstep whole slices. The 400
slices before the event are not measured; the shares are measured over the 400 slices from one
slice after it. Each is in thousandths of a hart, against water-filling, within 50 per thousand
of the machine.

## Results: (budget, got, want), in thousandths of a hart

- None: late join at 2: passes [(1, 1000, 1000), (2, 500, 500), (3, 500, 500)]
- None: second cap at 3: passes [(1, 1000, 1000), (2, 1000, 1000), (3, 332, 333), (4, 335, 333), (5, 332, 333)]
- None: uncap at 2: passes [(1, 1640, 1636), (2, 180, 182), (3, 180, 182)]
- None: spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- None: spread at 4: passes [(1, 3000, 3000), (2, 1000, 1000)]
- Some(R12CappedHoldsFloor): late join at 2: R12 across harts, late join at 2 harts: budget 2 got 57 of 1000 of a hart, its share is 500 ([(1, 1000, 1000), (2, 57, 500), (3, 942, 500)])
- Some(R12CappedHoldsFloor): second cap at 3: R12 across harts, second cap at 3 harts: budget 3 got 167 of 1000 of a hart, its share is 333 ([(1, 1000, 1000), (2, 1000, 1000), (3, 167, 333), (4, 170, 333), (5, 662, 333)])
- Some(R12CappedHoldsFloor): uncap at 2: R12 across harts, uncap at 2 harts: budget 1 got 2000 of 1000 of a hart, its share is 1636 ([(1, 2000, 1636), (2, 0, 182), (3, 0, 182)])
- Some(R12CappedHoldsFloor): spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- Some(R12CappedHoldsFloor): spread at 4: passes [(1, 3000, 3000), (2, 1000, 1000)]
- Some(R12CapOnce): late join at 2: passes [(1, 1000, 1000), (2, 500, 500), (3, 500, 500)]
- Some(R12CapOnce): second cap at 3: R12 across harts, second cap at 3 harts: budget 5 got 602 of 1000 of a hart, its share is 333 ([(1, 1000, 1000), (2, 1000, 1000), (3, 197, 333), (4, 200, 333), (5, 602, 333)])
- Some(R12CapOnce): uncap at 2: passes [(1, 1640, 1636), (2, 180, 182), (3, 180, 182)]
- Some(R12CapOnce): spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- Some(R12CapOnce): spread at 4: passes [(1, 3000, 3000), (2, 1000, 1000)]
- Some(R12UncapBanksCredit): late join at 2: passes [(1, 1000, 1000), (2, 500, 500), (3, 500, 500)]
- Some(R12UncapBanksCredit): second cap at 3: passes [(1, 1000, 1000), (2, 1000, 1000), (3, 332, 333), (4, 335, 333), (5, 332, 333)]
- Some(R12UncapBanksCredit): uncap at 2: R12 across harts, uncap at 2 harts: budget 1 got 2000 of 1000 of a hart, its share is 1636 ([(1, 2000, 1636), (2, 0, 182), (3, 0, 182)])
- Some(R12UncapBanksCredit): spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- Some(R12UncapBanksCredit): spread at 4: passes [(1, 3000, 3000), (2, 1000, 1000)]
- Some(R12OneRunnerPerBudget): late join at 2: passes [(1, 1000, 1000), (2, 500, 500), (3, 500, 500)]
- Some(R12OneRunnerPerBudget): second cap at 3: passes [(1, 1000, 1000), (2, 1000, 1000), (3, 335, 333), (4, 332, 333), (5, 332, 333)]
- Some(R12OneRunnerPerBudget): uncap at 2: R12 across harts, uncap at 2 harts: budget 1 got 1000 of 1000 of a hart, its share is 1636 ([(1, 1000, 1636), (2, 500, 182), (3, 500, 182)])
- Some(R12OneRunnerPerBudget): spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- Some(R12OneRunnerPerBudget): spread at 4: R12 across harts, spread at 4 harts: budget 1 got 1000 of 1000 of a hart, its share is 3000 ([(1, 1000, 3000), (2, 1000, 1000)])
- Some(R12SpreadChargesOnce): late join at 2: passes [(1, 1000, 1000), (2, 500, 500), (3, 500, 500)]
- Some(R12SpreadChargesOnce): second cap at 3: passes [(1, 1000, 1000), (2, 1000, 1000), (3, 332, 333), (4, 335, 333), (5, 332, 333)]
- Some(R12SpreadChargesOnce): uncap at 2: R12 across harts, uncap at 2 harts: budget 1 got 1800 of 1000 of a hart, its share is 1636 ([(1, 1800, 1636), (2, 100, 182), (3, 100, 182)])
- Some(R12SpreadChargesOnce): spread at 2: passes [(1, 1000, 1000), (2, 1000, 1000)]
- Some(R12SpreadChargesOnce): spread at 4: passes [(1, 3000, 3000), (2, 1000, 1000)]

## Summary

- Capped floor: all four pass (spread at both 2 and 4 harts).
- Old floor: late join fails (B gets 57 of a hart's 1000 against 500; C takes 942) and second cap
  fails (E takes 662, C and D 167 each, against 333 each), as the brief predicts. Uncap fails too:
  A's pass lagged while capped, so with a second thread it takes both harts (2000 against 1636)
  until B and C catch up.
- Each new mutation is caught by at least one scenario:
  - R12CapOnce: second cap.
  - R12UncapBanksCredit: uncap.
  - R12OneRunnerPerBudget: uncap and spread at 4.
  - R12SpreadChargesOnce: uncap (1800 against 1636).
  - R12CappedHoldsFloor: late join, second cap and uncap.
