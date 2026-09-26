# Swap

## Idea

A userspace swapper server moves a budget's cold pages to disk, with a per-budget swap limit
beside the page limit, so swapping never lets a budget exceed its total. Swapped pages are
encrypted and authenticated, and the `system` budget is never swapped.

## Why it is not a goal

No swap is a non-goal ([the tenets](../TENETS.md#non-goals)). Every page a budget uses is a page of
RAM it was carved, so a limit means what it says and nothing another budget does can page it out.
Swap would add a timing channel (a fault shows that a page was evicted, which depends on everyone's
use) and a server that holds every swapped budget's memory.

## What it would need

- The non-goal amended first, with the reason.
- The swapper holding no key: pages encrypted under keys kept in `keyd`.
- Eviction decided per budget only, from its own use, so one budget's pressure never evicts
  another's pages.
- Labelled budgets swapped only to storage of their own label set.

**Attack cases:** a budget's swap never exceeds its limit; one budget's allocation pattern changes
nothing observable in another's; a swapped page altered on disk is refused on the way back.
