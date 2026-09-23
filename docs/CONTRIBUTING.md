# Contributing

Contributions use the project's [licenses](https://github.com/sirmick/redoubt/tree/main/LICENSES)
(inbound = outbound). Sign off commits under the [Developer Certificate of Origin 1.1](https://developercertificate.org/)
with `Signed-off-by: Your Name <email>`. Participation follows the [Code of Conduct](CODE_OF_CONDUCT.md).

## Scope and review

Prefer one concern per PR; keep bug fixes, refactors and documentation changes separately reviewable.
Discuss large or cross-cutting changes first. Split mechanical sweeps by target and open no more
than two such PRs per week. Include the problem, changed behavior, rationale and test evidence.
You are responsible for correctness, licensing and the whole contribution, including AI-assisted work.

Run relevant tests and add regressions for defects. Kernel/loader changes require the full
[testbench](testbench.md); package acceptance and review follow [SWARM](SWARM.md).
Update the owning contract when approved behavior changes, and [STATUS](STATUS.md) when
implementation changes. Keep documentation focused on what, why and remaining work.

## Formatting

Use the repository's `rustfmt.toml` with nightly and remove trailing whitespace:

```sh
cargo +nightly fmt -p <crate>
# For an individual file:
rustfmt +nightly --config skip_children=true path/to/file.rs
git diff --check
```

Commit subjects use the imperative, start with a capital, omit a final period and stay within
50 characters. Separate the body with a blank line, wrap it at 72 characters, and explain what
and why. GPG signing is optional; DCO sign-off is required.

## AI disclosure

Disclose substantial AI-generated content retained unchanged in a commit trailer, for example
`Assisted-by: <tool/model>`. Also disclose assistance when useful to reviewers; spelling and
grammar correction need no disclosure. Use the `AI` PR tag for work primarily generated or
guided by AI. Keep all contributions modular, reviewable and explainable.
