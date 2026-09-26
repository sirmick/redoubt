# `map_anon`'s address search is quadratic in its area

## What

`map_anon` looks for a free run of pages by trying each start in its 256 MiB placement area and
testing the run page by page until it meets a taken page. The next start is the next page, so
the run is tested again from the beginning. The cost is the area's pages times the request's
pages: one page mapped in the middle of the area makes a request for half of it test about
5 × 10^8 pages before it fails. The receiver's message area (1024 pages) uses the same search.

The search runs before any budget check. Its kernel time is billed to the caller afterwards, so
the caller pays in its own pass (R12 (scheduling)); the harm is latency, not billing. The kernel
is not preemptible and interrupts are off during a call, so every wake, deadline and interrupt
on the machine waits out the search, and billing it after the fact gives nobody that time back.

R12 gains a sentence: "A system call's kernel time is bounded by a constant plus a term linear
in the pages it maps or the objects it names. It never depends on the extent of an address area
or on what other processes hold. Billing it to the caller does not excuse it, because every
wake waits for it." R10 (destruction)'s whole-system scan is the one stated exception
([budget destruction cost](budget-destroy-cost.md)).

## Why it matters

Any process can stall the whole machine for the search's length with one call. It needs no
authority beyond its own address space.

Fixed in the kernel follow-up package after the documentation rewrite, before the work on
`init` and the manifest.

## Where

- [`kernel/src/mem.rs`](../../kernel/src/mem.rs): the search loops in the placement code that
  `map_anon` and message delivery call (the two `for potential_start` loops).
- The pages: [memory](../kernel/memory.md#residual-risks) and
  [scheduling](../kernel/scheduling.md#r12-scheduling).

## Done when

- The search skips ahead: when page `p` is taken, the next candidate start is `p` plus one
  page, so the search is one pass over the area (at most 65536 probes for the default area,
  1024 for the message area).
- A bench case, `map-anon-search-bound`, maps one page mid-area, asks for half the area, and
  asserts the refusal's time and that a timer wake meanwhile stays within the latency target.
- A planted mutation that restores the retesting fails that case.
- R12 on the scheduling page carries the sentence above.
