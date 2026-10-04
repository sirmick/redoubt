# Agents on Redoubt

Redoubt is built by AI agents under one human owner. If you are one:

- **What the system is:** [the book](docs/README.md). Read the pages your task names, not the
  whole book.
- **How the work is run, your role and its rules:** [.wash/README.md](.wash/README.md). Start
  there.
- **Build and test:** [GETTING-STARTED.md](GETTING-STARTED.md); every test runs through
  `cargo testbench` ([the test bench](docs/testbench.md)).

Only the orchestrator may push, under [SWARM's publishing rules](.wash/SWARM.md#publishing).
A session instruction forbidding pushes overrides that standing permission. Implementers and
reviewers never push. Never `git add -A`, never `git stash`; stage by path.
