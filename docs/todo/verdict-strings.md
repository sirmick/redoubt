# Process names in verdicts and case descriptions

## What

The IPC completion checker, `tests/programs/src/bin/ipc-outcomes.rs`, begins each verdict line
with the name of the package that built it (`IPC` and a number), and `tests/ipc-outcomes.toml`
expects those lines and names the package in its description. The name belongs to the process
that built the checker, not to anything in the system.

The same holds for most case files: about fifty `tests/*.toml` descriptions and comments carry a
work-package name, the number of an answered design question, or an owner-decision tag, among
them `legacy-gone`, the `d3-net-*` cases and most kernel and scheduler cases; and the `d3-net-*`
cases carry a package name in their own names.

## Why it matters

A bench case's verdicts and description are read by people auditing the rules it attacks
([R13 (one outcome per call)](../kernel/ipc.md#r13-one-outcome-per-call), and every row of
[the security register](../SECURITY.md)). A package name or an answer number there means nothing
to them, and the book keeps process names out of everything but its process pages.

Belongs to no follow-up package: it is part of the documentation switch-over, which rewrites
process references in code and case descriptions, and whose docs-checker code rule (`--code`)
finds them ([the docs checker](../testbench.md#what-it-checks)).

## Where

- [`tests/programs/src/bin/ipc-outcomes.rs`](../../tests/programs/src/bin/ipc-outcomes.rs): the
  verdict lines.
- [`tests/ipc-outcomes.toml`](../../tests/ipc-outcomes.toml): its `expect` patterns and
  description.
- [`tests/`](../../tests): every case whose description or comments cite a package, an answer or a
  decision tag, and the `d3-net-*` case names.

## Done when

- The verdict lines read `ipc-outcomes ...`, the case expects them, and the description names the
  rule it attacks instead of the package.
- Every case description names the rules it attacks and no package, answer or decision tag, the
  `d3-net-*` cases are renamed for what they attack (with every page that cites them), and
  `cargo run -q -p redoubt-doccheck -- --code` finds nothing in `tests/`.
