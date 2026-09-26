# Verdict lines named after a package

## What

The IPC completion checker, `tests/programs/src/bin/ipc-outcomes.rs`, begins each verdict line
with the name of the package that built it (`IPC` and a number), and `tests/ipc-outcomes.toml`
expects those lines and names the package in its description. The name belongs to the process
that built the checker, not to anything in the system.

## Why it matters

A bench case's verdicts are read by people auditing the rules they attack
([R13 (one outcome per call)](../kernel/ipc.md#r13-one-outcome-per-call)). A package name there
means nothing to them, and the book keeps process names out of everything but its process pages.

Belongs to no follow-up package: it is part of the documentation switch-over, which rewrites
process references in code and case descriptions.

## Where

- [`tests/programs/src/bin/ipc-outcomes.rs`](../../tests/programs/src/bin/ipc-outcomes.rs): the
  verdict lines.
- [`tests/ipc-outcomes.toml`](../../tests/ipc-outcomes.toml): its `expect` patterns and
  description.

## Done when

- The verdict lines read `ipc-outcomes ...`, the case expects them, and the description names the
  rule it attacks instead of the package.
