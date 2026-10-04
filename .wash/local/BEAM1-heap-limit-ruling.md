# BEAM1: the heap and ETS limits, sized again (architect-11)

Supersedes part (1) and the "Limits" page sentence of BEAM1-heap-flood-ruling.md. Its cases
(2a) beamlet-heap-flood and (2b) beamlet-budget-flood stand.

## Why half failed

A flood needs about four pages per page of heap limit: the old heap, the collector's to-space and
the growth step. The measurements fit that factor (pages, budget 3,072):

| | VM's own use | free | 4 x limit at 1/8 | 4 x 250K words |
| --- | --- | --- | --- | --- |
| rv64 | 1,408-1,536 | 1,536-1,664 | 1,536: passed | 1,953: passed* |
| rv32 | 1,800-2,048 | 1,024-1,272 | 1,536: failed | 976: passed |

(*the factor is not exact; rv64 has room.) So a limit L is safe only if the VM's own use + 4L
fits the budget. The VM cannot see its own use (libs/rt's heap counts nothing, and adding a
count is a runtime contract change), so A (the use as a manifest argument, per width and build)
is a second number to keep true by hand, and D has no means.

## Ruling: B, a sixteenth

1. `max_heap_words` and `max_ets_words` are each a sixteenth of the budget, in words
   (budget bytes / 16 / word size). The same bytes at both widths. A flood then peaks at about
   a quarter of the budget.
2. The sizing rule the manifest keeps: the budget is at least twice the VM's own use (its pages
   with no Erlang process running). Then a quarter for the flood leaves a quarter of margin.
   rv32 does not meet it today (2,048 of 3,072): raise `beamlet`'s budget in the image's
   manifest and in the beamlet cases' manifests to 4,096 pages. If the machine's RAM cannot hold
   that with the rest of the image, stop and report the numbers.
3. `budget_pages=N` is required on Redoubt: `beamlet` exits `BAD_ARGS` before the VM starts
   without it, or with a malformed one. Every limit fails closed; a missing argument must not
   silently leave the 2^27 defaults. (The host embedding keeps no budget and the defaults.)
4. Keeping N equal to the budget: the manifest author writes it, for now. `init` neither
   writes nor checks it: init.md's settled rule is that arguments are opaque and `init` never
   interprets them. The right home for the number is the startup block, which `init` already
   writes and the runtime already parses; a `budget_pages` field there (0 for none) lets
   `beamlet` read its own budget and drops the argument. That is a `libs/rt` contract change with
   a consumer sweep, so it is a follow-up, not BEAM1's: BEAM1 adds
   `docs/todo/beamlet-budget-from-startup.md` (lines below).
5. C (a fixed limit) is rejected: it does not grow with the budget, so a session given more
   pages for a larger Erlang workload would still die at the same heap size.

Report: the VM's own use at both widths with the new budget, beamlet-heap-flood and
beamlet-budget-flood passing on both, and every beamlet case passing with the limits at a
sixteenth (a legitimate process meeting its limit is a finding: report the case, don't raise
the fraction).

## Page lines, beamlet.md "Limits inside one VM"

After "on Redoubt the session budget's page limit
([R6 (charging)](../kernel/budgets.md#r6-charging))." insert (replacing architect-10's
sentence):

> On Redoubt the platform lowers `max_heap_words` and `max_ets_words` to a sixteenth of the VM's
> budget each, which it takes from its required argument `budget_pages=N`, the budget's pages
> ([todo](../todo/beamlet-budget-from-startup.md)). A flooding process peaks at about four times its heap
> limit, the old heap, the collector's copy and its growth, so the budget must be at least twice
> what the VM uses with no Erlang process running; then one flooding process, or the tables,
> meets its limit while the VM still has pages. Several flooding at once, or a native's single
> large allocation, reach the backstop instead, which ends the VM, and `init` restarts it.

(Rewrap to the page's width.)

## docs/todo/beamlet-budget-from-startup.md (new, in BEAM1)

> # beamlet is told its budget by an argument the manifest keeps equal by hand
>
> ## What
>
> `beamlet` sizes its heap and ETS limits from `budget_pages=N`, a manifest argument
> ([limits](../userland/beamlet.md#limits-inside-one-vm)). Nothing checks that N is the budget
> `init` gives it: arguments are opaque to `init` ([init](../servers/init.md)).
>
> ## Why it matters
>
> An N above the budget makes the limits too high, and a flood reaches the backstop and ends the
> VM instead of the flooding process. An N below it makes them too low, and legitimate processes
> are killed.
>
> ## The fix
>
> The startup block gains a `budget_pages` field (0 for none), which `init` fills with the
> budget it created for the process; `beamlet` reads it and the argument goes. It changes the
> block's layout, so the runtime and every program linking it are built and tested together.

The status line and its two bench lines are as in BEAM1-heap-flood-ruling.md. The doc comment on
`beamlet_redoubt::limits` says a sixteenth and drops "Only a native's single large allocation
can still reach the budget" (several flooding processes can too).
