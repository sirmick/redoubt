# AGT4 ruling: the compile is a workload inside the session, not the session's size

Measured (AGT4 round 2, `.worktrees/AGT4/.tmp/AGT4/round2/`): stack peak 88,888 B rv64 /
81,056 B rv32 with miniz_oxide's 65,712-byte compressor as a stack local; heap 9,128 pages rv64 /
8,799 rv32 after a compile (from 5,492 / 5,308 at the prompt), ~3,600 pages of compiler modules
held for good.

## (a) beamlet-footprint stays compile-free; a new case states the compile

The sizing rule's own words bind the idle VM, not every workload (beamlet.md:189-191): "the
budget must be at least twice what the VM uses with no Erlang process running; then one flooding
process ... meets its limit while the VM still has pages." `beamlet-footprint` measures that
idle footprint at the prompt and sizes the common session from it; it stays as it is.

A new case, `beamlet-compile-footprint`, one scheduler, compiles the brief's module at the prompt
over the console and prints the VM's breakdown after it, and judges:

1. the compile fits the session: the scan's heap peak under the heap cap (11,092) with a stated
   margin of at least an eighth of the cap (today 9,128 of 11,092: 82 %, margin 1,964 pages); the
   margin is the case's number, quoted both widths on beamlet.md;
2. the stack: the first scheduler's peak with the compressor on the heap (lever (c)), quoted both
   widths; `SESSION_STACK_PAGES` and `SCHEDULER_STACK_PAGES` follow it by the existing rule
   (twice the peak, rounded up to pages, one constant stated once); if the pages move, the share's
   "outside the heap" count (44 today) and the 128-page step (beamlet.md:247-256) are recomputed
   on the page, and the share moves only if the arithmetic says so;
3. the flood guarantee after a compile, measured, not assumed: the sixteenth limits are static
   (696 pages each; a flooder peaks at about four times its limit, ~2,784 pages), so after a
   compile the VM holds 9,128 + 2,784 ≈ 11,912 > 11,136: a flooding process in a session that has
   compiled reaches the backstop and ends the VM (a logout), where before the compile it would
   have been killed alone. The case runs one flooding process after the compile and records which
   happened, both widths.

If (3) shows the VM ends: that is a stated residual on beamlet.md "Limits inside one VM" ("a
session that has compiled has spent its flood headroom on the compiler's code; one flooding
process then ends the VM, and init restarts it: a logout, within the session's own budget, no
other principal's"), and the fix is a follow-up node, not AGT4's: the VM derives its limits from
the pages not held by loaded code (recomputed after a load), which keeps the guarantee at any
footprint without resizing anything. The owner is not asked to resize sessions for this.

## (b) no decision_request now

Nothing must resize: the compile fits with margin. Sessions per principal (alice's 47,624 pages
hold 4 at 11,264; ~2 at ~18,400) is the owner's question only if a compiling session must keep
the flood guarantee by size; the follow-up above keeps it without size. If the owner wants the
agent principal's session larger regardless, that is a per-principal session size in `sizes`
(today one `session` for all), to be raised with AGT1's report, not here.

## (c) one scheduler

Agreed: `beamlet-compile-footprint` runs `schedulers=1`; the page says the compile runs on
whichever scheduler holds its process, so its peak is measured where the placement is fixed.

## Pages

- beamlet.md "What the VM holds at its prompt" / "Limits inside one VM": the compile's peaks and
  margin, the stack constants' new value and derivation, the residual (if measured), the
  one-scheduler note.
- testbench.md's memory-budget paragraph (the stack peak sentence at :1636) with the new peak.
- init.md "Stacks" if the stack constant is stated there; steward.rs's comment follows.

## For a later node (not AGT4's)

Helper scheduler threads have no stack guard page (libs/rt/src/thread.rs:25): an overflow
corrupts the heap silently (rv64 died later with an illegal instruction). A guard page per
spawned thread is a libs/rt change with a case that overflows a helper on purpose and expects the
fault, not a later crash; I will draft it when AGT4's report lands.
