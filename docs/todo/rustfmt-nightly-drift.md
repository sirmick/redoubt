# Nightly rustfmt drift

## What

The contribution rules format Rust with the repository's `rustfmt.toml` under nightly
(`cargo +nightly fmt`), but the tree is not formatted that way: `cargo +nightly fmt --all --check`
lists 98 files in the workspace, and the crates outside it (`model/`, `userland/otp/`) add more.
Either the tree is reformatted once and a bench case keeps it so, or the rule changes. The owner
decides which.

## Why it matters

A formatting rule the tree does not follow is noise in every change: a contributor who runs the
formatter as told rewrites files they did not mean to touch, and a reviewer cannot tell their
change from the reformatting. A rule the bench does not check drifts again.

## Where

- [`rustfmt.toml`](../../rustfmt.toml): the configuration, with its unstable options
  (`wrap_comments`, `group_imports`, `fn_single_line` and others) that need nightly.
- [`CONTRIBUTING.md`](../../CONTRIBUTING.md#formatting): the rule.
- [The test bench](../testbench.md): no case runs the formatter.

## Done when

- `cargo +nightly fmt --all --check` passes over the workspace and the crates outside it, and a
  bench case runs it; or the rule in `CONTRIBUTING.md` names the formatter the tree follows.
