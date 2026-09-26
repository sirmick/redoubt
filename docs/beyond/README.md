# Beyond M5

Ideas past the plan. None is a goal: the hard goals are M1 (separation and containment) through
M5 (persist, install, share), and nothing here is built toward until the owner makes it one. Each
page says what the idea is, why it is not a goal, and what it would need, including the attack
cases it would have to pass. Some are kept so they are not redesigned from scratch; some are
recorded as ruled out, with the reason, so they are not proposed again without one.

| Page | The idea |
| --- | --- |
| [The FPGA platform](fpga-platform.md) | Redoubt on its own softcore cards, with DMA confined in hardware, local inference and an approval button |
| [SMP](smp.md) | the kernel on several harts |
| [rv32](rv32.md) | the full stack on 32-bit RISC-V, booted in every milestone |
| [Other runtimes](runtimes.md) | Python and Java, ported to Rust |
| [The web stack](web-stack.md) | serving HTTP through isolated TCP, TLS and HTTP servers, reached only through SSH forwarding |
| [Unattended operation](unattended.md) | backup, crash records, field updates, monitoring and a rescue console |
| [A Rust OS facade](rust-os-facade.md) | one blocking client library over every server |
| [Swap](swap.md) | a userspace swapper under per-budget limits |
| [Disk encryption](disk-encryption.md) | authenticated encryption of every block, for a disk outside the trust boundary |
| [Linux on reserved cores](linux-cores.md) | Linux driving messy hardware and serving virtio to Redoubt |
| [IOMMU](iommu.md) | hardware confinement of DMA through an IOMMU or IOPMP |
| [An OS debugger](os-debugger.md) | a debug capability and a GDB server, never issued in production |
| [A shared content store](content-store.md) | packages stored once for everyone, by hash (ruled out as a channel) |
| [A shared image cache](image-cache.md) | program images shared read-only between principals (ruled out as a channel) |
| [The link layer](link-layer.md) | VLANs, rate limits and routing, in `linkd` and `routerd` |
| [A browser GUI](browser-gui.md) | a desktop in the browser (conflicts with the no-GUI non-goal) |
| [Hardware approval](hardware-approval.md) | high-stakes approvals signed by a FIDO key or a button on the board |
| [ASLR](aslr.md) | randomised address-space layout |
| [Scheduling extensions](scheduling-extensions.md) | time donation and CPU quotas |
| [Label extensions](label-extensions.md) | taint-on-read and integrity labels |
