# Contributing

Contributions use the project's [licences](LICENSES/) (inbound = outbound). Sign off every commit
under the [Developer Certificate of Origin 1.1](https://developercertificate.org/) with
`Signed-off-by: Your Name <email>`. Participation follows the
[Code of Conduct](CODE_OF_CONDUCT.md).

## Scope and review

- One concern per pull request: keep bug fixes, refactors and documentation changes separately
  reviewable. Discuss a large or cross-cutting change first. Split mechanical sweeps by target, and
  open no more than two of them a week.
- Say what the problem was, what behaviour changes, why, and the test evidence.
- You are responsible for the whole contribution, its correctness and its licensing, AI-assisted
  work included.
- A change must keep [the tenets](docs/TENETS.md). A change that conflicts with one amends the tenet
  first, with the reason.

## Tests

- Run the tests the change touches, and add a regression test for every defect fixed.
- A change to the kernel or the loader runs the whole [test bench](docs/testbench.md), and rv32 must
  still compile.
- A new security property lands with an attack case whose verdict comes from the system (the
  kernel, a victim or a clean power-off), never from the attacker's own output.
- No `unsafe` without a comment stating the invariant it relies on; the
  [unsafe budget](docs/testbench.md#the-unsafe-budget) only goes down without a stated reason.

## Documentation

The book in [`docs/`](docs/README.md) is written from the code. A change that alters behaviour
updates the page that describes it in the same change: the text, and the section's status line,
which names the tests that attack it. A new security property takes the next free rule ID on its
owning page. Pages describe the system, not its history: no dates, package names or review
references (git keeps those). Run the docs checker and render the book before sending:

```sh
cargo run -q -p redoubt-doccheck
mdbook build docs
```

Code comments cite pages and rule IDs (the page `kernel/memory.md` and its R11 (memory), say),
never issue numbers or review threads.

## Formatting

Rust is formatted with the repository's [`rustfmt.toml`](rustfmt.toml), under nightly, since it
uses nightly-only options; no trailing whitespace anywhere.

```sh
cargo +nightly fmt -p <crate>
rustfmt +nightly --config skip_children=true path/to/file.rs   # one file
git diff --check
```

The tree does not yet pass this everywhere ([the follow-up](docs/todo/rustfmt-nightly-drift.md)):
format what you change, not the files around it.

## Commits

- The subject is in the imperative, starts with a capital letter, has no final full stop and
  stays within 50 characters.
- A blank line, then a body wrapped at 72 characters that says what and why.
- The DCO sign-off is required; GPG signing is optional.

## AI disclosure

Disclose substantial AI-generated content kept unchanged in a commit trailer, for example
`Assisted-by: <tool/model>`, and disclose assistance whenever it helps reviewers; spelling and
grammar corrections need none. Tag a pull request `AI` when the work was primarily generated or
guided by AI. Keep every contribution modular, reviewable and explainable.

## Reporting a vulnerability

Do not open a public issue for a security problem. Report it privately through the repository's
security advisories on GitHub ("Report a vulnerability"), with the rule or page it breaks, if you
know it, and the steps or attack case that show it. A confirmed report gets an attack case before
its fix, and the fix lands with it.
