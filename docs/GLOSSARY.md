# Glossary

One vocabulary, used the same way on every page. Each entry says what the term means here,
the nearest Unix idea where there is one (and how it differs), and the page that defines it.
When a page and this list disagree, the page that owns the term is right and this list is
fixed.

### A/B slots

Two places for the signed bundle. An update writes the new bundle to the inactive slot, and the
loader verifies it on the next boot; a system that does not boot, or does not reach a healthy
state before any session starts, falls back to the other slot. A version counter kept outside
both slots stops a rollback below it. Unix: A/B partitions on a phone. Defined in
[packages](servers/pkg.md#system-updates).

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
(sudo's prompt is in-band). Defined in [the steward](servers/steward.md#the-powerbox-and-approvals).

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
which devices, labels, weights, volumes and arguments, and which principals exist. Unix:
`/etc/fstab`, `/etc/inittab`, `/etc/passwd` and the service files, as one signed document.
Defined in [init](servers/init.md#the-boot-manifest).

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
only way to reach anything. Defined in [handles and objects](kernel/objects.md).

### carve

Giving part of a budget's limits to a new child budget. The children's limits never add up to
more than the parent's. Unix: none. Defined in [budgets](kernel/budgets.md#r7-carving).

### class

`system` or `user`, fixed for a budget and inherited by its children. It decides trust (a flow
into a system budget is not label-checked by the kernel) and never scheduling order. Defined in
[budgets](kernel/budgets.md).

### command mode

The shell's short form for everyday work: bare words are quoted, `|>` joins Elixir stages, `|`
joins native stages, and `>` and `>>` write and append. It expands to ordinary Elixir calls to
the shell's helpers. Unix: the shell's own syntax, but only a preprocessor in front of Elixir.
Defined in [the shell](userland/shell.md#command-mode).

### confined deployment

A boot in which `init` refuses any manifest that lets two different label sets share a server
instance, volume, endpoint, network instance, device or core, except the two named control-plane
mediators. Defined in [init](servers/init.md#the-confinement-check).

### connection

One client's session with a server: an endpoint handle with its own badge, and the server state
keyed to it (for 9P, its fid table). Copies of the handle share the connection; a launcher gives
each child a fresh one. Unix: an open socket or file description. Defined in
[the servers](servers/README.md#connections).

### crash blame

When a server crashes while working on a call, the exit notice names the account and labels of
that call's sender. The steward counts blame and ends sessions that keep crashing shared
servers. Defined in [processes](kernel/processes.md).

### current call

The open call a server thread is working on: the call its last `receive` took, or the one it
named with `serve`; none once it replies to it or receives anything else. A crash blames its
sender. Defined in [IPC](kernel/ipc.md).

### declassification

Moving one item of data out of a label to an unlabelled volume, done only by the steward after
the label's owner approves that exact snapshot out of band. Unix: none. Defined in
[the steward](servers/steward.md#declassification-and-push).

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

### escape room

The standing game in which real agents, told to get out, attack a confined deployment while a
referee decides from the record, never from an agent's own output. It tests what nobody thought
to write a bench case for. Unix: none; a capture-the-flag run continuously. Defined in
[agents](userland/agents.md#the-escape-room).

### exit notice

The message the kernel sends to a process's exit endpoint when it exits, faults or is killed:
its PID, cause, code, and who is blamed. Unix: `SIGCHLD` and `wait`, as a message. Defined in
[processes](kernel/processes.md).

### fid

A number a 9P client uses to name a file it has walked to, within one connection. Unix: a file
descriptor, scoped to the connection. Defined in [the wire protocol](servers/wire.md).

### fixed sub-budget

One of the budgets the steward splits a principal's top budget into at boot, one per label set
the manifest names for that principal. Every session and lease of one (principal, label set) is
carved from its own sub-budget, so a vault's activity never shows in what the unlabelled side
can carve. Unix: none. Defined in
[the steward](servers/steward.md#fixed-sub-budgets-per-label-set).

### founding handle

The root badge of a server that is filled once at setup and then only served: `bootfsd`'s is
the one `init` uses to add the public entries and seal them. No connection minted for a client
can fill it. Unix: none. Defined in [bootfsd](servers/bootfsd.md#filling-it).

### gateway

A server that does a network job for an agent so the agent never holds a socket or a key: TLS,
credentials, request checks and logging. `gatewayd` is the first. Defined in
[gatewayd](servers/gatewayd.md).

### gateway capability

A connection to `gatewayd` whose badge names one grant (the `model` gateway, a `git` remote),
optionally narrowed to some of its hosts, with a meter. It is what an agent holds instead of a
socket. Unix: none; closest to a scoped API token the holder can use but never read. Defined in
[gatewayd](servers/gatewayd.md#gateway-capabilities).

### grant and release

The two operations of a typed protocol that mints a narrower capability for a client: a grant
replies with the new endpoint handle and a random id; a release frees it by that id, and only
for the client that received it. A 9P server's pair is `new_connection` and `disconnect`. Unix:
`open` and `close`, where only the opener may close. Defined in
[the wire protocol](servers/wire.md#granting-and-releasing).

### handle

An index into a process's handle table, which the kernel holds. It names an object, a badge and
a stamp. It cannot be forged; it can be copied and closed. Unix: a file descriptor. Defined in
[handles and objects](kernel/objects.md).

### hart

A RISC-V hardware thread: one core, or one thread of a multithreaded core. Unix: a CPU.

### job

The native stages one shell command started, each running in a budget of its own carved from
the session's. Killing a job destroys those budgets; there is no signal. Unix: a shell job,
killed by budget destruction rather than `SIGKILL`. Defined in
[native programs](userland/native.md#killing-a-job).

### label

A name owned by a principal, marking data that must not leave a set of budgets. The kernel sees
it as a 64-bit number. Defined in [the servers](servers/README.md#labels).

### label set

The labels on a budget, fixed when it is created. It is the isolation unit: two budgets with
different label sets have no path between them that the OS carries, except through the steward's
approved moves. Defined in [IPC](kernel/ipc.md#r1-flow).

### lease

A budget with a deadline, made by the steward for an agent or session. When the deadline passes,
the kernel destroys the budget and everything in it; the sponsor can end it sooner. Unix: none.
Defined in [the steward](servers/steward.md#leases).

### lend

Pages a caller hands to a server for the length of one call. They leave the caller's address
space until the reply brings them back. Defined in [IPC](kernel/ipc.md).

### loader stub

A small program mapped into a new process that reads the program's ELF image and maps its
segments, so the launcher never parses an ELF. Unix: the ELF loader in `execve`, moved out of
the kernel and the parent. Defined in [init](servers/init.md#launching-through-the-loader-stub).

### meter

The part of a gateway capability that says how much the holder may spend: tokens and money,
carved from its principal's own. Unix: a quota. Defined in
[gatewayd](servers/gatewayd.md#gateway-capabilities).

### mint

Making a new endpoint handle with a chosen badge, and optionally a narrower stamp. Only the
holder of the receive right can. Defined in [handles and objects](kernel/objects.md).

### model

The executable model: the Rust crate `redoubt-model`, which states the kernel's objects, calls,
rules and invariants as code, drives them with random call sequences and checks every invariant
after each step. Its traces replay on the real kernel. Unix: none. Defined in
[the model](kernel/model.md).

### mutation

A deliberate break of one rule planted in the executable model, to show the model's checks
catch it. Defined in [the model](kernel/model.md).

### name rules

What a resolver connection may have answered: an allowlist of domain names and suffixes, and a
blocklist that subtracts from it and always wins. A suffix matches only at a label boundary. A
grant only narrows them. Unix: none; closest to a filtering DNS proxy per client. Defined in
[the resolver](servers/resolver.md#resolving-a-name).

### namespace

A process's own map from path prefixes to capabilities, built by its launcher. Nothing is
inherited and there is no global file tree. Unix: a mount namespace with no root to fall back
on. Defined in [sessions and namespaces](userland/sessions.md#namespaces).

### native program

A Rust program built for Redoubt and started from a session in a budget of its own, through the
loader stub, with the namespace its launcher binds. Unix: an executable, but with no inherited
descriptors or environment. Defined in [native programs](userland/native.md).

### 9P

The Plan 9 file protocol (9P2000), used for every service a person or program sees as files.
Defined in [the wire protocol](servers/wire.md).

### open call

A call a server has taken with `receive` and not yet replied to. Each costs the server a page
and counts against its limit of open calls. Defined in [IPC](kernel/ipc.md#r4a-open-calls).

### pass

A budget's position in the stride scheduler: the lowest pass runs next, and running raises it
in inverse proportion to the budget's weight. Defined in [scheduling](kernel/scheduling.md).

### person

A principal who is a human: logs in with a key over SSH and can approve. Only a person gets
name-scoped TCP, and every chain of sponsors ends at one. Unix: a user. Defined in
[the steward](servers/steward.md#principals).

### physmap

The kernel's mapping of all RAM at a fixed offset, so it can reach any frame. Supervisor-only
and never executable. Defined in [memory layout](kernel/memory-layout.md).

### PID

A process's number, 2 to 64 (1 is the kernel), which is also its hardware address-space id. It is
drawn at random from the free ones and held until the process's exit notice is taken or dropped.
No authority is keyed by it. Unix: a PID, but it names; it grants nothing. Defined in
[processes](kernel/processes.md).

### pipe

A file that somebody serves over 9P, bound into two programs' namespaces as one's standard output
and the other's standard input. There is no pipe object in the kernel and no inherited
descriptor. Unix: `pipe(2)`, as a served file. Defined in
[native programs](userland/native.md#standard-input-and-output-and-pipes).

### powerbox

The steward's service that grants authority a principal lacks, or confirms a principal's own
high-stakes step, through an out-of-band approval, and if approved hands back a narrow
capability. Unix: none (`sudo` grants everything and asks in-band). Defined in
[the steward](servers/steward.md#the-powerbox-and-approvals).

### principal

A named, accountable identity the steward knows: a person, an agent or a project. It has an
authentication method, a set of capabilities and an audit identity, and its top budget carries
its account. Unix: a user account, without root and without ambient rights. Defined in
[the steward](servers/steward.md#principals).

### profile

The package versions a principal runs, and so the `/bin` its sessions see. It is a steward
record, not a file in the principal's space. Unix: a Nix profile. Defined in
[packages](userland/packages.md#profiles-and-upgrades).

### project

A principal sponsored by several members, with its own budget, volume, package directory and
profile, and optionally a label. Membership is capabilities minted into a revocation scope per
member. Unix: a group, but with its own budget and no ambient rights. Defined in
[the steward](servers/steward.md#projects-and-sharing).

### purpose

The one message shape a `keyd` badge may have its key sign: `ssh_host` signs only an SSH key
exchange, `audit` only an audit record. No purpose signs a caller's own bytes. Unix: none;
closest to a key usage extension. Defined in [keyd](servers/keyd.md#keys-and-purposes).

### push

Moving one item from an unlabelled volume into a labelled domain's volume, triggered by the
label's owner out of band. The mirror of declassification, and the only way input enters a
confined labelled domain. Unix: none. Defined in
[the steward](servers/steward.md#declassification-and-push).

### quarantine

What happens to DMA pages and devices when a device in the reset set does not confirm its reset:
the pages are mapped nowhere and never reused until the machine resets, still charged to their
budget, and the device's object is destroyed and none names it again until reboot. Unix: none. Defined in
[devices](kernel/devices.md#quarantine).

### reader budget

A short-lived budget the steward creates carrying exactly an item's labels, with a deadline, to
read that item for it: the steward stays unlabelled and calls it. There is no standing reader.
Defined in [the steward](servers/steward.md#declassification-and-push).

### receive right

An endpoint handle with badge 0. Only it can `receive`, and only its holder can mint.
Defined in [IPC](kernel/ipc.md#authority).

### revocation scope

A budget with zero limits, made only to be a stamp and later destroyed, so a grant can be
revoked without killing any process. Defined in [budgets](kernel/budgets.md).

### root badge

A badge below the minted range that a server's setter-up gives meaning to (the manifest, through
`init`), rather than one the server minted for a client. It is how system callers get separate
shares and how setup-only operations are reserved. Defined in
[serving](servers/serving.md#minted-connections).

### scope

The IP prefixes and ports an `ipd` connection may reach. A grant only narrows it, and no scope
reaches the box's own addresses. Unix: none; closest to a per-socket firewall rule. Defined in
[ipd](servers/ipd.md).

### seal

The message that ends a server's setup: before it clients see nothing; after it the contents are
fixed for good. `bootfsd` is sealed once its public entries are added. Defined in
[bootfsd](servers/bootfsd.md#filling-it).

### self set

The addresses that are the box itself: its own address, its network and broadcast addresses,
loopback, "this host", multicast, class E and the prefixes named as the host's. `ipd` refuses
them before any scope is looked at. Defined in [ipd](servers/ipd.md#the-boxs-own-addresses).

### send

A message that does not wait for a reply. It may transfer pages to the receiver for good.
Defined in [IPC](kernel/ipc.md).

### service record

The steward's record of one installed service: the package's program, its owner, the grants the
owner made for it, a budget carved from the owner's and a restart policy. The supervisor starts
services only from these. Unix: a systemd unit, holding grants rather than a user name. Defined
in [the supervisor](servers/supervisor.md#services).

### session

A person's or agent's working environment on the box: an Elixir VM in its own budget, with a
namespace and capabilities the steward built for it. Unix: a login session. Defined in
[sessions and namespaces](userland/sessions.md).

### sink

A server whose output leaves a principal or the machine (`ipd`, `gatewayd`, `sshd`). A sink is
cleared for no label and refuses every labelled caller, with one exception: `sshd`, on the
channel whose owner authenticated it. Defined in
[the servers](servers/README.md#who-checks-and-sinks).

### sponsor

The principal accountable for an agent: a person, or an agent with a person at the top of the
chain. The agent's budget sits under the sponsor's, and the sponsor can always end its lease.
Defined in [agents](userland/agents.md#an-agent-is-a-principal-with-a-sponsor).

### stamp

The budget recorded in a handle: destroying that budget, or any budget above it, closes the
handle everywhere. Defined in [handles and objects](kernel/objects.md#r9-stamps).

### startup block

The read-only page a launcher gives a new process: its namespace, named handles, arguments and
the location of its image. Unix: `argv`, `envp` and the inherited descriptors, in one checked
record. Defined in [init](servers/init.md#the-startup-block).

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
steward moves data, one approved item at a time. Defined in [the tenets](TENETS.md#guarantees).

### trust list

The signing keys whose code a principal runs. The steward launches code on a principal's behalf
only if a key on its trust list signed it. Unix: an apt keyring, per principal. Defined in
[packages](userland/packages.md#trust-lists).

### typed message

A message whose word 0 names an operation in a table, encoded by generated code, as opposed to
a 9P message. Defined in [the wire protocol](servers/wire.md).

### vault session

A session carrying one of its principal's labels, so it can read that label's data and cannot
send it anywhere unlabelled. Opened with `ssh alice+X@box`. Defined in
[sessions and namespaces](userland/sessions.md#vault-sessions).

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

### writer budget

A short-lived budget the steward creates carrying exactly a target label set, to write one pushed
item into that domain, since a write needs equal labels. Defined in
[the steward](servers/steward.md#declassification-and-push).

### XLEN

The register width: 32 bits (rv32) or 64 bits (rv64). Every milestone boots rv64 and compiles
rv32.
