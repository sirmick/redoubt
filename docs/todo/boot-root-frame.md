# `root`'s own page is charged to no one

## What

At boot the kernel makes `root` with a page limit of every RAM frame it did not keep for
itself. `root` carves `system`'s and `users`' pages and pays for their two budget pages, which
adds up to that whole limit. But `root`'s own budget page is taken from the same frames and
charged to no budget. So the charges the tree promises total one page more than the free frames.
If every budget fills to its limit, the last allocation finds no frame, and the kernel panics
(the frame allocator's `expect`) instead of refusing the call with `OutOfMemory`.

R6 (charging) gains, after "a budget's own page to its parent": "and `root`'s own page to
`root`: `root`'s limit is the RAM frames the kernel did not keep, less `root`'s own page, so the
sum of all charges never exceeds the free frames." The last clause is the invariant to state and
test.

## Why it matters

A kernel stop under memory exhaustion breaks I14 (no call panics the kernel) and the ledger's
promise that every charged page has a real frame behind it. It takes every budget full at once,
so it is a denial of service, not an escape.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/budget.rs`](../../kernel/src/budget.rs): `boot_budgets` (`pages`, `users_pages`).
- The page: [budgets](../kernel/budgets.md#r6-charging) and its residual risks.

## Done when

- `root`'s limit is the free frames less `BUDGET_PAGES`, and `users`' pages are computed from
  that.
- A boot-time assertion checks that the carved limits, `root`'s own page and the kernel's frames
  add up to at most the RAM frames.
- A model property checks the same sum at boot and after every carve.
- An exhaustion attack case fills every budget to its limit; the last allocation gets
  `OutOfMemory` and the kernel keeps running.
- A planted mutation that leaves `root`'s own page out of its limit (the boot split as it is
  today) fails the model property and the exhaustion case.
