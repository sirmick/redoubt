# Glossary

One vocabulary, used the same way on every page. Each entry says what the term means here,
the nearest Unix idea where there is one (and how it differs), and the page that defines it.
When a page and this list disagree, the page that owns the term is right and this list is
fixed.

### abandoned call

An open call whose caller has gone: it died, timed out, or was failed by revocation after the
server took the call. The server keeps the call, and any lend, until it replies; the reply
reaches nobody. Unix: none. Defined in [IPC](kernel/ipc.md#r3-lends-and-abandoned-calls).

### account

A 64-bit number carried by a budget that says which principal the budget bills to; 0 means
none. A new budget inherits its parent's account unless the parent has none. Servers see the
caller's account on every message. Unix: a user ID, but it grants nothing by itself. Defined in
[budgets](kernel/budgets.md).

### agent

A program that acts for a principal and is assumed hostile: an AI agent or any other automated
worker. It is its own principal with a sponsor, runs under a lease, and holds only capabilities
narrowed from its launcher's. Unix: none. Defined in [agents](userland/agents.md).

### approval

A human's out-of-band yes to a request that would add authority or move data across a label
boundary. The requester cannot draw, type or listen on the channel where it happens. Unix: none
(sudo's prompt is in-band). Defined in [the steward](servers/steward.md).

### attach root

The directory a 9P connection is rooted at. Nothing above it can be named through that
connection. Unix: a chroot, per connection. Defined in [the wire protocol](servers/wire.md).

### attack case

A bench case that tries to break a security property and passes only on a verdict the attacker
cannot produce: the kernel's own output, a victim's, or a clean power-off. Defined in
[the test bench](testbench.md).

### badge

A 64-bit number attached to an endpoint handle, chosen by the server that minted it. The kernel
delivers it with every message sent through that handle, so the server knows which grant is in
use. Badge 0 is the receive right. Unix: none; closest to a file descriptor's open-file
description as the server sees it. Defined in [handles and objects](kernel/objects.md).

### bench case

One test the bench runs: a file `tests/<name>.toml` that boots the real kernel under QEMU with
chosen programs, or runs host tests or a source check. Defined in [the test bench](testbench.md).

### bind

Making a capability the process already holds appear at another path in its namespace. It
creates no authority. Unix: `mount --bind`, but per process and with no global mount table.
Defined in [files and binds](userland/files.md).

### boot manifest

The one strict-JSON file in the signed bundle that tells `init` which servers to start, with
which devices, labels, weights, volumes and arguments. Unix: `/etc/fstab`, `/etc/inittab` and
the service files, as one signed document. Defined in [init](servers/init.md).

### budget

The kernel object that holds resources: page, process and CPU-weight limits, a class, a label
set, an account and an optional deadline. Every process runs in one; every kernel object is
charged to one; destroying one revokes everything under it. Budgets form a tree. Unix: a cgroup,
a revocation list and a security label in one object. Defined in [budgets](kernel/budgets.md).

### bundle

The signed archive the loader boots: the kernel, the first programs and the boot manifest.
Defined in [boot](kernel/boot.md).

### call

A message that waits for a reply. It may lend pages to the server for the length of the call.
Defined in [IPC](kernel/ipc.md).

### capability

The right to use something, held as a handle. There is no other authority: no user IDs, no
paths that grant access by name, no root. Unix: a file descriptor, if file descriptors were the
only way to reach anything.

### carve

Giving part of a budget's limits to a new child budget. The children's limits never add up to
more than the parent's. Unix: none. Defined in [budgets](kernel/budgets.md#r7-carving).

### class

`system` or `user`, fixed for a budget and inherited by its children. It decides trust (a flow
into a system budget is not label-checked by the kernel) and never scheduling order. Defined in
[budgets](kernel/budgets.md).

### confined deployment

A boot in which `init` refuses any manifest that lets two different label sets share a server
instance, volume, endpoint, network instance, device or core. Defined in
[init](servers/init.md).

### connection

One client's session with a server: an endpoint handle with its own badge, and the server state
keyed to it (for 9P, its fid table). Copies of the handle share the connection; a launcher gives
each child a fresh one. Unix: an open socket or file description. Defined in
[the servers](servers/README.md).

### crash blame

When a server crashes while working on a call, the exit notice names the account and labels of
that call's sender. The steward counts blame and ends sessions that keep crashing shared
servers. Defined in [processes](kernel/processes.md).

### current call

The open call a server thread is working on: the call its last `receive` took, or the one it
named with `serve`; none once it replies to it or receives anything else. A crash blames its
sender. Defined in [IPC](kernel/ipc.md).

### declassification

Moving one item of data out of a label set, done only by the steward after the label's owner
approves it out of band. Unix: none. Defined in [the steward](servers/steward.md).

### device object

The kernel's record of one device resource: an MMIO range (possibly able to do DMA), an
interrupt line, or the right to power off. A driver reaches a device only through a handle to
one. Unix: a device node, but held as a capability rather than found by path. Defined in
[devices](kernel/devices.md).

### DMA

Direct memory access: a device reading and writing RAM itself. A driver that programs DMA is
trusted unless hardware confines it. Defined in [devices](kernel/devices.md).

### endpoint

The kernel object clients call and servers receive on. It holds no queue: its waiting messages
are its blocked senders. Unix: a listening socket, with the badge in place of the peer address.
Defined in [IPC](kernel/ipc.md).

### exit notice

The message the kernel sends to a process's exit endpoint when it exits, faults or is killed:
its PID, cause, code, and who is blamed. Unix: `SIGCHLD` and `wait`, as a message. Defined in
[processes](kernel/processes.md).

### fid

A number a 9P client uses to name a file it has walked to, within one connection. Unix: a file
descriptor, scoped to the connection. Defined in [the wire protocol](servers/wire.md).

### gateway

A server that does a network job for an agent so the agent never holds a socket or a key: TLS,
credentials, request checks and logging. `gatewayd` is the first. Defined in
[gatewayd](servers/gatewayd.md).

### handle

An index into a process's handle table, which the kernel holds. It names an object, a badge and
a stamp. It cannot be forged; it can be copied and closed. Unix: a file descriptor. Defined in
[handles and objects](kernel/objects.md).

### hart

A RISC-V hardware thread: one core, or one thread of a multithreaded core. Unix: a CPU.

### label

A name owned by a principal, marking data that must not leave a set of budgets. The kernel sees
it as a 64-bit number. Defined in [the servers](servers/README.md#labels).

### label set

The labels on a budget, fixed when it is created. It is the isolation unit: two budgets with
different label sets have no path between them that the OS carries. Defined in
[IPC](kernel/ipc.md#r1-flow).

### lease

A budget with a deadline, made by the steward for an agent or session. When the deadline passes,
the kernel destroys the budget and everything in it; the sponsor can end it sooner. Unix: none.
Defined in [the steward](servers/steward.md).

### lend

Pages a caller hands to a server for the length of one call. They leave the caller's address
space until the reply brings them back. Defined in [IPC](kernel/ipc.md).

### loader stub

A small program mapped into a new process that reads the program's ELF image and maps its
segments, so the launcher never parses an ELF. Unix: the ELF loader in `execve`, moved out of
the kernel and the parent. Defined in [init](servers/init.md).

### mint

Making a new endpoint handle with a chosen badge, and optionally a narrower stamp. Only the
holder of the receive right can. Defined in [handles and objects](kernel/objects.md).

### mutation

A deliberate break of one rule planted in the executable model, to show the model's checks
catch it. Defined in [the model](kernel/model.md).

### namespace

A process's own map from path prefixes to capabilities, built by its launcher. Nothing is
inherited and there is no global file tree. Unix: a mount namespace with no root to fall back
on. Defined in [sessions and namespaces](userland/sessions.md).

### 9P

The Plan 9 file protocol (9P2000), used for every service a person or program sees as files.
Defined in [the wire protocol](servers/wire.md).

### open call

A call a server has taken with `receive` and not yet replied to. Each costs the server a page
and counts against its limit of open calls. Defined in [IPC](kernel/ipc.md#r4a-open-calls).

### pass

A budget's position in the stride scheduler: the lowest pass runs next, and running raises it
in inverse proportion to the budget's weight. Defined in [scheduling](kernel/scheduling.md).

### physmap

The kernel's mapping of all RAM at a fixed offset, so it can reach any frame. Supervisor-only
and never executable. Defined in [memory layout](kernel/memory-layout.md).

### powerbox

The steward's service that turns a request for more authority into an out-of-band approval and,
if approved, a narrow capability. Defined in [the steward](servers/steward.md).

### PID

A process's number, 2 to 64 (1 is the kernel), which is also its hardware address-space id. It is
drawn at random from the free ones and held until the process's exit notice is taken or dropped.
No authority is keyed by it. Unix: a PID, but it names; it grants nothing. Defined in
[processes](kernel/processes.md).

### principal

Someone the steward knows and bills: a person, an agent or a project. It has an authentication
method, a set of capabilities and an audit identity. Unix: a user account, without root and
without ambient rights.

### push

Moving one item into a labelled domain, triggered by the label's owner out of band. The mirror
of declassification. Defined in [the steward](servers/steward.md).

### receive right

An endpoint handle with badge 0. Only it can `receive`, and only its holder can mint.
Defined in [IPC](kernel/ipc.md#authority).

### revocation scope

A budget with zero limits, made only to be a stamp and later destroyed, so a grant can be
revoked without killing any process. Defined in [budgets](kernel/budgets.md).

### send

A message that does not wait for a reply. It may transfer pages to the receiver for good.
Defined in [IPC](kernel/ipc.md).

### session

A person's or agent's working environment on the box: an Elixir VM in its own budget, with a
namespace and capabilities the steward built for it. Unix: a login session. Defined in
[sessions and namespaces](userland/sessions.md).

### sink

A server that sends data off the box or to a person (`ipd`, `gatewayd`, `sshd`). A labelled
caller reaches a sink only if the sink is cleared for its labels. Defined in
[the servers](servers/README.md#labels).

### sponsor

The principal accountable for an agent. The sponsor can always end the agent's lease.
Defined in [agents](userland/agents.md).

### stamp

The budget recorded in a handle: destroying that budget, or any budget above it, closes the
handle everywhere. Defined in [handles and objects](kernel/objects.md#r9-stamps).

### startup block

The read-only page a launcher gives a new process: its namespace, named handles, arguments and
the location of its image. Unix: `argv`, `envp` and the inherited descriptors, in one checked
record. Defined in [init](servers/init.md).

### steward

The trusted server that knows principals, authenticates them, builds sessions, grants leases,
runs approvals and keeps the audit log. Defined in [the steward](servers/steward.md).

### TCB

The trusted computing base: the code whose failure can break the security guarantees. For
Redoubt: the firmware interface, the loader and the kernel, plus any server that programs DMA
without hardware confinement. Defined in [the kernel](kernel/README.md).

### TID

A thread's number within its process, 1 to 31; the first thread is 1. Defined in
[processes](kernel/processes.md).

### transfer

Pages given away with a `send`: they leave the sender and become the receiver's. Defined in
[IPC](kernel/ipc.md).

### trust domain

The budgets that share one label set. Within it, delegation is permitted; across it, only the
steward moves data. Defined in [TENETS](TENETS.md).

### typed message

A message whose word 0 names an operation in a table, encoded by generated code, as opposed to
a 9P message. Defined in [the wire protocol](servers/wire.md).

### vault session

A session carrying one of its principal's labels, so it can read that label's data and cannot
send it anywhere unlabelled. Opened with `ssh alice+X@box`. Defined in
[sessions and namespaces](userland/sessions.md).

### verdict

The line a bench case passes or fails on. It must come from a party the attacker cannot
impersonate. Defined in [the test bench](testbench.md#rule-f-trusted-verdicts).

### W^X

No RAM page is ever both writable and executable, under any mapping; the kernel's own mappings
keep the same rule. Device memory is a stated gap. Defined in
[memory](kernel/memory.md#r11-memory).

### weight

A budget's CPU share. The scheduler uses its free weight: its limit less what its children
carved. Defined in [scheduling](kernel/scheduling.md).

### wire table

A Markdown table in `libs/wire/tables/` that defines one typed protocol; the Rust and Elixir
codecs are generated from it. Defined in [the wire protocol](servers/wire.md).

### XLEN

The register width: 32 bits (rv32) or 64 bits (rv64). Every milestone boots rv64 and compiles
rv32.
