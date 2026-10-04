# BEAM1: the heap flood (architect-10)

## (1) The limits come from the budget: wanted in BEAM1

beamlet's defaults (`max_heap_words` 2^27, `max_ets_words` 2^27) sit above any session budget,
so on Redoubt an Erlang process's own limit never fires. One flooding process then takes the
whole VM down, against beamlet.md's "a buggy or hostile Erlang process must not take the rest of
the VM down".

- In `run()`, set `max_heap_words` and `max_ets_words` each to half the VM's budget, in words.
- Take the budget's pages from what the platform can see; if it can see nothing, from an
  argument `init` writes. Report which.
- A host test on the fake checks that `run()` sets both.

## (2a) `beamlet-heap-flood`: the VM survives a flooding process

The module spawns a process that floods its heap, and monitors it.

Expect, in order:
- the module's DOWN line (the process killed by its heap limit);
- the module's prompt, then the echo of a typed `[[input]]` line, which shows the console is
  still served by the same VM;
- `init`'s line that `beamlet` exited, code 0. That exit code is the system's verdict that the
  flood did not end the VM.

Forbid: `exited, code 101`, any other server's end, and `init: rebooting`.

## (2b) `beamlet-budget-flood`: the backstop

The module prints a start line, then makes one allocation larger than the budget inside a
native (for example `binary:copy/2` of a large binary), which no slice-end check sees.

Expect, in order:
- the start line;
- `init`'s line that `beamlet` ended (code 101, or a fault);
- `init: restarted beamlet, console M`;
- the start line again, under the new console's tag.

Forbid: any other server's end, and `init: rebooting`.

If the case runs on past the restarted VM's line until the restart limit reboots the machine,
report it and I'll reshape the case. No sleeping programs (options B and C).

## Page lines, beamlet.md "Limits inside one VM"

- The status becomes:
  > <details><summary>Status: built · partly tested: in a boot, only the process heap limit and
  > the budget's backstop are attacked · tested (14)</summary>

  (on one line, as the page writes it), with bench:beamlet-heap-flood and
  bench:beamlet-budget-flood added to its list.
- After "on Redoubt the session budget's page limit
  ([R6 (charging)](../kernel/budgets.md#r6-charging))." insert:
  > On Redoubt the platform lowers `max_heap_words` and `max_ets_words` to half the VM's budget
  > each, so a flooding process or table meets its limit while the VM still has pages; only a
  > native's single large allocation reaches the backstop, which ends the VM, and `init`
  > restarts it.
- BEAM1's status line for "beamlet on Redoubt" lists both case names.
