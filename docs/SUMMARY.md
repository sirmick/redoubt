# Summary

- [Reading this book]()
- [Tenets]()
- [Security register]()
- [Glossary](GLOSSARY.md)

# The kernel

- [The kernel](kernel/README.md)
  - [Handles and objects](kernel/objects.md)
  - [IPC](kernel/ipc.md)
  - [Memory]()
  - [Budgets](kernel/budgets.md)
  - [Scheduling]()
  - [Time and timeouts]()
  - [Processes]()
  - [Devices and DMA]()
  - [Boot and verified boot]()
  - [Memory layout]()
  - [System call reference]()
  - [Invariants]()
  - [The executable model]()

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
