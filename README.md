# Redoubt

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

| Start here | |
| --- | --- |
| [The book](docs/README.md) | the whole system, what is built and what is planned |
| [The tenets](docs/TENETS.md) | what Redoubt must achieve, against whom, and its limits |
| [The security register](docs/SECURITY.md) | every security property, its code and its test |
| [Getting started](GETTING-STARTED.md) | build, run, test and debug |
| [Contributing](CONTRIBUTING.md) | how to send a change |

Licensed under [LICENSE](LICENSE) and [LICENSES](LICENSES/).
