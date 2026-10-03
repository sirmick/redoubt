# beamlet is told its budget by an argument the manifest keeps equal by hand

## What

`beamlet` sizes its heap and ETS limits from `budget_pages=N`, a manifest argument
([limits](../userland/beamlet.md#limits-inside-one-vm)). Nothing checks that N is the budget
`init` gives it: arguments are opaque to `init` ([init](../servers/init.md)).

## Why it matters

An N above the budget makes the limits too high, and a flood reaches the backstop and ends the
VM instead of the flooding process. An N below it makes them too low, and legitimate processes
are killed.

## Where

`userland/otp/redoubt/src/bin/beamlet.rs` reads the argument; `libs/rt/src/startup.rs` parses the
startup block that `init` writes (`servers/init/src/bin/init.rs`).

Until it is done, the stopgap is a host test of the bench's,
host:testbench::every_beamlet_is_told_its_own_budget: every manifest in the tree that starts
`beamlet` gives it N equal to its budget's pages. A manifest written anywhere else is not checked.

## Done when

The startup block gains a `budget_pages` field (0 for none), which `init` fills with the
budget it created for the process; `beamlet` reads it and the argument goes. It changes the
block's layout, so the runtime and every program linking it are built and tested together.
