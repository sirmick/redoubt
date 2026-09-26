# Process names in verdicts and case descriptions

## What

Five bench cases carry the name of the package that built them in their own names:
`d3-net-attacks`, `d3-net-host-tests`, `d3-net-pinned`, `d3-net-self-unrefused` and
`d3-net-tcp`. Their verdict lines and descriptions no longer name a package, and neither does any
other case: the switch-over rewrote them, and the IPC completion checker's verdicts read
`ipc-outcomes ...`.

## Why it matters

A bench case's name is what the pages and [the security register](../SECURITY.md) cite in their
`tested:` lists, and people auditing a rule read it there. A package name means nothing to them,
and the book keeps process names out of everything but its process pages.

Belongs to no follow-up package: it is the last part of the documentation switch-over.

## Where

- [`tests/`](../../tests): the five `d3-net-*.toml` cases, and the network rig that runs them.
- Every page and register row that cites `bench:d3-net-...`.

## Done when

- The five cases are named for what they attack, with every page, register row and rig reference
  that cites them, and the checker and the bench pass.
