# Documentation

Read the governing [TENETS](TENETS.md) and the [swarm method](SWARM.md) first.
Then use [setup and running](https://github.com/sirmick/redoubt/blob/main/GETTING-STARTED.md),
[current implementation](STATUS.md), and [the plan](PLAN.md). The [visual tour](README.html)
shows the architecture. beamlet runs on the host today; its Redoubt platform and the native
server stack still need boot integration.

Each contract has one owner below. [TENETS](TENETS.md) has precedence. A specification describes
the accepted target; STATUS identifies implementation gaps. Approved changes and their short
rationale go in the owning specification, with provenance in [ANSWERS](ANSWERS.md).

| Topic | Owner |
| --- | --- |
| Purpose, adversary, guarantees and exclusions | [TENETS](TENETS.md) |
| Kernel objects, costs, syscalls, ABI, errors and invariants | [KERNEL-SPEC](KERNEL-SPEC.md) |
| Delegation, leases and owner approvals | [CAPABILITIES](CAPABILITIES.md) |
| Labels, confinement, mediation, admission and crash policy | [CONTAINMENT](CONTAINMENT.md) |
| Budgets, CPU shares and timer policy | [RESOURCES](RESOURCES.md) |
| Current boot flow and argument tags | [BOOT](BOOT.md) |
| Bundle authentication and trust boundary | [VERIFIED-BOOT](VERIFIED-BOOT.md) |
| Device handles and the retired grants; address-space layout | [DEVICE-GRANTS](DEVICE-GRANTS.md), [MEMORY-LAYOUT](MEMORY-LAYOUT.md) |
| Init, manifests, startup, restarts and keyd | [INIT](INIT.md) |
| Namespaces, 9P connections, console, bootfs and filesystem protocols | [NAMESPACES](NAMESPACES.md) |
| Encoding and generator inputs | [WIRE](WIRE.md) |
| Drivers, storage, networking and DMA trust | [IO-ARCHITECTURE](IO-ARCHITECTURE.md) |
| Native launching, signing, packages and updates | [PACKAGES](PACKAGES.md) |
| VM boundary and planned Elixir surface | [USERLAND](USERLAND.md), [USERLAND-API](USERLAND-API.md) |
| Proposed Rust client facade | [OS-API](OS-API.md) |
| Planned FPGA platform | [PLATFORM-FPGA](PLATFORM-FPGA.md) |
| Adversarial-agent scenarios and verdicts | [GAME](GAME.md) |

## Development

- [Testbench](testbench.md): commands, case schema and trusted verdicts; `./test --list` lists cases.
- [Debugging](DEBUGGING.md): build source debug information and attach GDB.
- [Contributing](CONTRIBUTING.md): scope, licensing, disclosure, formatting and checks.
- [Build plan](BUILD-PLAN.md): remaining deliverables and acceptance.
- [Package coordination](SWARM.md#claims): claims, dependencies and review debt.
- [Open decisions](QUESTIONS.md): unresolved questions, including the security/latency claims.

## Terms

| Term | Meaning |
| --- | --- |
| Capability / handle | Authority represented by an index in a process's kernel-held table. A badge identifies a server grant; a stamp gives its revocation budget. |
| Budget / account | Resource and revocation scope; account plus label set identifies admission and crash blame. |
| Label set / trust domain | The information-flow isolation unit. Capabilities bound authority; labels bound flow. |
| Lend / transfer | Call-scoped access to pages / permanent ownership transfer. KERNEL-SPEC defines abandonment and return. |
| 9P connection / fid | A server connection / file identifier within that connection. |
| Hart / XLEN / physmap | RISC-V hardware thread / register width / kernel RAM mapping. |
| TCB | Code whose failure can violate the security guarantees. |
| sys / layout | `libs/sys` is the system-call ABI; `libs/layout` is the kernel half of the address map, shared by the loader and the kernel. |

[Milestones](HISTORY.md) and [approval provenance](ANSWERS.md) are reference material.
Dated review and decision archives preserve evidence; they are not required reading for the contracts.
