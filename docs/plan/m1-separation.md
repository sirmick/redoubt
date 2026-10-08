# M1 (sessions over SSH, kept apart)

## Goal

Alice and Bob log in over SSH, on QEMU, and on the console, into Elixir sessions, and are kept
apart: each session runs in a fixed sub-budget for its principal's label set, with files (the
system read-only, `/home` writable). A dead steward restarts without a reboot, and the
scheduler's targets hold. The attack suite is the kernel cases below and the cases for what this
milestone builds.

The milestone is one thin vertical slice through the whole system: the kernel, `init` and the
boot manifest, the drivers, the file servers, the steward, `keyd` and `sshd`, and beamlet running
the shell in each session. A session in this milestone needs only the console, files and
launching native programs. The rest of the shell is [M2 (usable shell)](m2-usable-shell.md);
agents, approvals and the scripted hostile agent and user are
[M3 (agents, approvals and the attack suite)](m3-agents.md).

## Attack suite

Each attack is a deterministic program in the test bench. Its outcome is judged by a party the
attacker cannot impersonate: the kernel, a victim, or a clean power-off, never the attacker's own
output ([rule F](../testbench.md#rule-f-trusted-verdicts)). A covert channel an attack finds is
recorded as an observation against [the side-channel wall](../TENETS.md#side-channels), not as a
breach.

### Kernel cases

| Attack | Rules | Bench case |
| --- | --- | --- |
| Revocation by budget: mint into a revocation scope and destroy it; the handles are dead everywhere, messages already sent through them fail and carry no reply's handles, and handles inside queued messages arrive as 0 | [R9 (stamps)](../kernel/objects.md#r9-stamps), [R10 (destruction)](../kernel/budgets.md#r10-destruction) | `redoubt-revoke`, `budget-destroy-attack` |
| Budgets: carving, charging and exhaustion | [R6 (charging)](../kernel/budgets.md#r6-charging), [R7 (carving)](../kernel/budgets.md#r7-carving) | `budget`, `budget-carve-attack`, `budget-table-attack`, `budget-mem-churn`, `touch-beyond-ram` |
| The scheduler: churn, carving, sleeping and ties gain nothing | [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) | `sched-budget-churn`, `sched-carve-inflation`, `sched-debt-lift`, `sched-destroy-billing`, `sched-exit-churn`, `sched-idle-gap`, `sched-lift-delay`, `sched-ties`, `sched-wake-no-preempt` |
| The timer: every blocking call returns by its timeout | [I13 (every blocking call returns by its timeout)](../kernel/invariants.md#i13-every-blocking-call-returns-by-its-timeout) | `timeouts`, `budget-deadline` |
| Memory: W^X, zeroed pages, no replacement by `map_fixed` | [R11 (memory)](../kernel/memory.md#r11-memory) | `wx`, `kernel-wx`, `mem-attack`, `write-only-attack`, `map-fixed-attack` |
| Devices: no device authority without a device object; DMA pages reset before reuse | [R18 (device authority)](../kernel/devices.md#r18-device-authority), [I16 (DMA pages reset before reuse)](../kernel/invariants.md#i16-dma-pages-reset-before-reuse) | `irq-attack`, `dma-rules`, `dma-reset-reuse`, `dma-reset-quarantine` |
| Containment: hostile agents in leases are preempted, then ended at their deadlines or revoked by hand with their messages and lends in flight, while the driver, the steward and a victim server stay responsive | R10, R12, [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls), [I2 (revocation is complete)](../kernel/invariants.md#i2-revocation-is-complete), [I10 (create-destroy leaves the parent unchanged)](../kernel/invariants.md#i10-create-destroy-leaves-the-parent-unchanged) | `kernel-containment` |
| Boot: a tampered or confused bundle does not run | [R15 (verified boot)](../kernel/boot.md#r15-verified-boot), [R16 (image confinement)](../kernel/boot.md#r16-image-confinement) | `verified-boot-rejects-tamper`, `verified-boot-rejects-bare-archive`, `loader-rejects-kernel-address`, `loader-rejects-kernel-entry`, `loader-rejects-truncated-elf` |

The gaps the kernel's own cases leave (R1 (flow) across label sets on the real kernel,
[R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting)'s turns between groups, completion races
between harts) are listed in [kernel attack gaps](../todo/kernel-attack-gaps.md).

## Remaining work

Each part lands with the attack cases for what it builds. The order the parts run in, and which
are under way, is kept with the project's work plan ([AGENTS.md](../../AGENTS.md)), not on this
page.

- **The follow-up packages.** The fixes found while writing this book, before anything is built
   on top of them.
   - **beamlet:** [bounded operands for modular exponentiation](../todo/beamlet-bignum-bounds.md).
- **The client library.** `redoubt-client`: the namespace, files over 9P, the file server's
  typed operations, the console, launching, a launcher's grants and one typed call, the API every
  userland binds to, tested on the host against real servers
  ([native programs](../userland/native.md#the-client-library)). `init` and beamlet's platform are
  built on it, and an `Rerror` keeps its name by one table
  ([wire](../servers/wire.md#error-names)); a session binds each typed server through a
  generated Elixir client ([wire](../servers/wire.md#generated-clients)). Still planned: dropped
  files' fids that come back, calls by path, and whole reads and writes
  ([native programs](../userland/native.md#dropped-files-calls-by-path-and-generated-calls)).
- **The VM's remaining platform work.** The VM and shell boot on the UART console, and its I/O
   is asynchronous through the client library's hub, files over 9P included
   ([beamlet](../userland/beamlet.md#asynchronous-underneath-synchronous-on-top),
   [files](../userland/files.md#files-over-9p)), and its system natives (the namespace, typed
   calls, serving, budgets, labels and launching) run on the machine in a session a tester starts
   in the steward's place ([beamlet](../userland/beamlet.md#natives)); Elixir's `File` in a
   steward's session waits for the session's namespace to reach its VM.
- **The steward.** Principals from the manifest, fixed sub-budgets per label set, and sessions
   on the console and over SSH run ([the steward](../servers/steward.md#principals),
   [sub-budgets](../servers/steward.md#fixed-sub-budgets-per-label-set),
   [sessions](../servers/steward.md#authentication-and-sessions)), on the server graph, trust
   tiers and capability holdings ([the servers](../servers/README.md)). Still to come: a dead
   steward restarts without a reboot: `init` empties the `users` budget, which ends every session,
   and relaunches the steward ([the steward](../servers/steward.md#failure-and-restart),
   [init](../servers/init.md#restarts-and-reboots)).
- **Files.** The system volume read-only, and `/home` writable, served by `walfsd`
   ([walfsd](../servers/walfsd.md#serving)).

Every follow-up page is placed above.

## Progress

Built and attack-tested today:
- **The kernel**, except SUM and MXR clearing: handles and objects, IPC, memory, budgets,
  scheduling, the timer, processes, devices and DMA, verified boot
  ([the kernel](../kernel/README.md)), and the executable model with its mutations
  ([the model](../kernel/model.md)).
- **The kernel containment gate:** hostile leases preempted, ended at their deadlines and revoked
  by hand with their calls and lends in flight, while the victims stay responsive, on both widths
  ([containment](../kernel/README.md#containment)).
- **The serving library** and the wire formats: admission, the label check, minted connections,
  parked calls, typed dispatch, the 9P skeleton ([serving](../servers/serving.md),
  [wire](../servers/wire.md)).
- **The drivers and the network:** `blkd` and `netd` attacked by hostile devices in host tests,
  `blkd`, `netd` and `ipd` on the real kernel under `init` ([blkd](../servers/blkd.md),
  [netd](../servers/netd.md), [ipd](../servers/ipd.md)).
- **The file system's core:** littlefs against a hostile medium and power loss
  ([littlefsd](../servers/littlefsd.md#littlefs)).
- **The flash file server:** `littlefsd` over `blkd`, placed by `init`, with one volume per instance, quotas
  and typed operations ([littlefsd](../servers/littlefsd.md)).
- **walfs**, the format for the SSD's writable volumes: its library against a model, a power cut
  at every write and hostile volumes, the bench's packer, and its server, `walfsd`, serving the
  image's data volume under `init`, with a power cut on the machine before or after
  ([walfsd](../servers/walfsd.md)).
- **`bootfsd`, `consoled` and `keyd`**, attacked in host tests and booted under `init`
  ([bootfsd](../servers/bootfsd.md), [consoled](../servers/consoled.md), [keyd](../servers/keyd.md)).
- **Launching:** the startup block and the loader stub ([init](../servers/init.md#the-startup-block)).
- **`init` and the boot manifest:** the loader loads only the kernel and `init`; `init` checks
  the manifest, builds the budget tree, hands each server its devices, and starts every server
  through the loader stub with fresh connections. It restarts a server that ends, a driver on its
  reset device, and reboots when one cannot stay up. Every server's case boots `init`
  ([init](../servers/init.md),
  [starting a case's programs](../testbench.md#starting-a-cases-programs)).
- **The client library**, with launching exercised by `init-boot` on the machine and its other
  tested operations exercised on the host against real servers
  ([native programs](../userland/native.md#the-client-library)).
- **beamlet** on the host and under `init` on Redoubt, with console, clock and randomness;
  its modules come from the userland disk, a volume `verityd` checks block by block against a
  root the signed manifest pins, those the shell's prompt loads from one boot pack read whole
  before the VM starts ([beamlet](../userland/beamlet.md#beamlet-on-redoubt),
  [verityd](../servers/verityd.md)).
  Its I/O is asynchronous through the client library's hub, and its files are 9P files in its
  namespace ([files](../userland/files.md#files-over-9p)).
- **The shell** on beamlet on the host: the loop, the commands, the file and text commands and
  help, with hostile text drawn visibly and the cell protocol held to its vectors
  ([the shell](../userland/shell.md#the-loop),
  [the cell protocol](../userland/shell.md#the-cell-protocol)).
  The shell also boots on Redoubt's UART console, and in a session its `exec` launches a native
  program from `/boot` ([native programs](../userland/native.md#launching-from-a-session)); its
  files wait for a session's namespace
  ([the shell on the machine](../userland/shell.md#the-shell-in-a-session)).
- **The steward**: its policy core, tested on the host, and the server, which carves the
  principals from the manifest, logs in `sshd`'s users and starts each session as a beamlet VM in
  its label set's sub-budget, and the console principal's on the UART
  ([the steward](../servers/steward.md#authentication-and-sessions)). A steward that dies is
  restarted with every session logged out, `users` emptied by the kernel's `budget_reap`, and the
  console session starts again; the machine does not reboot
  ([failure and restart](../servers/steward.md#failure-and-restart)).
- **`sshd`'s core** on its host platform, which the bench's SSH sessions run against, with
  OpenSSH's server as the reference ([sshd](../servers/sshd.md#the-core-and-its-platforms),
  [SSH sessions](../testbench.md#sessions-and-the-loopback-server)), and on the box's, logins
  through the steward ([sessions over SSH](../servers/sshd.md#sessions-over-ssh)).

Not built: a session's files, and a steward restart without a reboot.
