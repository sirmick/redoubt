# Redoubt

<img src="logo.svg" width="96" alt="Redoubt">

[Read the book](https://sirmick.github.io/redoubt/) · [the tour](docs/TOUR.md)

Redoubt is a prison: a headless, multi-user operating system for RISC-V, built so that the most
capable and most malicious agents can run on it, do real work, and not get out. Authority is
capabilities, with no ambient authority, no root and no network by default; information-flow
labels keep principals' data apart, and only an out-of-band human approval moves it across. The
kernel is a small microkernel, and everything above it is an unprivileged server. Everything that
runs on the machine is Rust, with no C, so the trusted computing base can be read like a textbook.
Every security claim names the rule that makes it, the code that enforces it and the attack case
that tests it.

**Today** the kernel runs and is attack-tested on QEMU, on rv64 and rv32, with the network driver
and the TCP/IP server above it; the boot file, key and console servers are built and host-tested;
the steward, SSH sessions and agents come next ([the plan](docs/plan/m1-separation.md)).

## Goals

- [M1 (separation and containment)](docs/plan/m1-separation.md), **in progress**: Alice and Bob log
  in over SSH into Elixir sessions and are kept apart; Alice's agent runs contained under a lease;
  the attack suite passes.
  How far along: the kernel, the serving library, the drivers and the network server, the file
  system's core, `bootfsd`, `consoled`, `keyd`, the loader stub and beamlet are built and
  attack-tested; `init`'s manifest handling, the `fsd` server, the steward, `sshd`, sessions and
  the agent are not built yet.
- [M2 (usable shell)](docs/plan/m2-usable-shell.md), **planned**: the Elixir shell is a working
  environment, with a command mode, file operations, native programs and pipes, jobs, line editing
  and the editor.
- [M3 (files in and out)](docs/plan/m3-files.md), **planned**: SFTP and SCP inside SSH, confined
  to the session's capabilities and audited.
- [M4 (self-hosted development)](docs/plan/m4-self-hosted.md), **planned**: Redoubt is developed
  on Redoubt, with compilers, `git` and a model provider through gateways, and agents doing part of
  the work. How far along: the Elixir and Erlang compilers run on beamlet, on the host.
- [M5 (persist, install, share)](docs/plan/m5-persist.md), **planned**: the steward's state
  survives reboots; signed packages, trust lists and shared projects; A/B updates.

| Start here | |
| --- | --- |
| [The book](docs/README.md) | the whole system, what is built and what is planned |
| [The tenets](docs/TENETS.md) | what Redoubt must achieve, against whom, and its limits |
| [The security register](docs/SECURITY.md) | every security property, its code and its test |
| [Getting started](GETTING-STARTED.md) | build, run, test and debug |
| [Contributing](CONTRIBUTING.md) | how to send a change |

Licensed under [LICENSE](LICENSE) and [LICENSES](LICENSES/).
