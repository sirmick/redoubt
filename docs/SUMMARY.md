# Summary

- [Reading this book]()
- [Tenets]()
- [Security register]()
- [Glossary](GLOSSARY.md)

# The kernel

- [The kernel](kernel/README.md)
  - [Handles and objects](kernel/objects.md)
  - [IPC](kernel/ipc.md)
  - [Memory](kernel/memory.md)
  - [Budgets](kernel/budgets.md)
  - [Scheduling](kernel/scheduling.md)
  - [Time and timeouts](kernel/timer.md)
  - [Processes](kernel/processes.md)
  - [Devices and DMA](kernel/devices.md)
  - [Boot and verified boot](kernel/boot.md)
  - [Memory layout](kernel/memory-layout.md)
  - [System call reference](kernel/abi.md)
  - [Invariants](kernel/invariants.md)
  - [The executable model](kernel/model.md)

# The servers

- [The servers]()
  - [The serving library]()
  - [The wire protocol]()
  - [init and the boot manifest]()
  - [The steward]()
  - [keyd]()
  - [bootfsd]()
  - [The file server]()
  - [blkd]()
  - [netd]()
  - [ipd]()
  - [The resolver]()
  - [gatewayd]()
  - [sshd]()
  - [consoled]()
  - [pkg]()
  - [The supervisor]()

# Userland

- [Userland]()
  - [Sessions and namespaces]()
  - [beamlet, the Elixir VM]()
  - [The shell]()
  - [Files and binds]()
  - [Native programs]()
  - [Agents, leases and labels]()
  - [File transfer]()
  - [Development on Redoubt]()
  - [Packages]()

# The plan

- [M1 (separation and containment)]()
- [M2 (usable shell)]()
- [M3 (files in and out)]()
- [M4 (self-hosted development)]()
- [M5 (persist, install, share)]()
- [Follow-ups]()
- [Beyond M5]()

# Working on Redoubt

- [The test bench]()
- [How packages are built]()
- [The project and its orchestrator]()
