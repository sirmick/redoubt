# Redoubt

A RISC-V microkernel in pure Rust, forked from [Xous](https://github.com/betrusted-io/xous-core).
It boots on QEMU under vendored RustSBI on rv64 and rv32. Capabilities, budgets and labels bound
authority, resources and information flow.

The planned Elixir userland uses **beamlet**, a safe-Rust BEAM VM. beamlet runs on the host today;
its Redoubt platform and the native server stack are not integrated end to end.

The [seven tenets](docs/TENETS.md) govern every change: auditability, security by construction,
Rust, open standards, accountable dependencies, attack testing and virtio devices.
The [swarm method](docs/SWARM.md) turns them into isolated work packages, acceptance tests and
three review angles, with one current claims ledger.

| Start here | Purpose |
| --- | --- |
| [Getting started](GETTING-STARTED.md) | Toolchain, build, launch and tests |
| [Implementation status](docs/STATUS.md) | What works and what remains |
| [Documentation map](docs/README.md) | Contracts and development references |
| [Visual tour](https://sirmick.github.io/redoubt/README.html) | Architecture diagrams |
| [Plan](docs/PLAN.md) | Milestone outcomes and acceptance |

`bios/`, `loader/` and `kernel/` form the boot chain; `libs/` holds shared components;
`servers/` and `userland/otp/` hold native services and beamlet. Build/test tools live in
`tools/`, with boot cases in `tests/`.

Licensed under [LICENSE](LICENSE) / [LICENSES](LICENSES/).
