# A lead accrued while carved down is never rescaled

## What

A budget that runs while most of its weight is carved away to children accrues its pass lead at
the small weight it has left. When the children end and the weight returns, the lead is not
rescaled, so the budget is charged as if it still ran at the small weight. This only ever
over-charges the carving budget, so it is not a way to gain time (R12 (scheduling)).

## Why it matters

An honest shell or steward that carves heavily while it runs can be held off the CPU well past
its restored share. The steward carves for every lease, so this can show as lease-handling
latency.

The fix must work in both directions. Rescaling only when the weight returns would be a gain: a
budget could carve just before a burst and return just after, and its lead would shrink at the
return without having grown at the carve.

## Where

- [`kernel/src/sched.rs`](../../kernel/src/sched.rs): where a carve and its return change a
  budget's weight.
- [`libs/stride/src/lib.rs`](../../libs/stride/src/lib.rs): `rescale`, which the rescale would
  use.
- The page: [scheduling](../kernel/scheduling.md#the-lead-follows-the-weight).

## Done when

- The kernel follows
  [the lead follows the weight](../kernel/scheduling.md#the-lead-follows-the-weight): every
  weight change, carve and return alike, converts the lead and the remainder to the new weight,
  and `rescale` in `libs/stride` becomes that conversion.
- The model and the oracle convert the same way, and
  `host:redoubt-stride::the_crate_and_the_model_agree` covers it.
- A `host:redoubt-stride` test checks that a carve and its return with no run between leave pass
  and remainder unchanged, and a mutation that rescales only on a return fails a check.
- A bench case has a budget carve most of its weight away while it runs, return it, and get its
  share back against an equal victim.
- The pages move with it: the section's status goes to built with those tests, and the breach
  on "Running while carved down" is removed.
