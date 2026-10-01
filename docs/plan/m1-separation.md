# M1 (separation and containment)

## Goal

Alice and Bob log in over SSH, on QEMU, into Elixir sessions, and are kept apart. Alice's agent
runs contained under a lease. Every property is backed by an attack case, and the attack suite
below passes.

The milestone is one thin vertical slice through the whole system: the kernel, `init` and the
boot manifest, the drivers, the file server, the steward, `keyd` and `sshd`, beamlet running the
shell in each session, and one scripted hostile agent. A session in this milestone needs only what the
attack suite uses: the console, files and launching native programs. The rest of the shell is
[M2 (usable shell)](m2-usable-shell.md).

## Attack suite

Each attack is a deterministic program in the test bench. Its outcome is judged by a party the
attacker cannot impersonate: the kernel, a victim, or a clean power-off, never the attacker's own
output ([rule F](../testbench.md#rule-f-trusted-verdicts)). A covert channel an attack finds is
recorded as an observation against [the side-channel wall](../TENETS.md#side-channels), not as a
breach. "Not yet" means the case waits for the
work below; a case named for part of an attack covers the part the table says.

### A scripted hostile agent

Alice's leased agent, trying to get out.

| Attack | Rules | Bench case |
| --- | --- | --- |
| Read outside its `/work`, or reach the network | [R25 (the label check)](../servers/serving.md#r25-the-label-check), [R41 (narrowing by revocation scope)](../servers/steward.md#r41-narrowing-by-revocation-scope) | not yet |
| A labelled agent reaches a sink not cleared for its labels | [R60 (a sink refuses labels)](../servers/ipd.md#r60-a-sink-refuses-labels) | `net-attacks` for `ipd`; the agent's case not yet |
| Outlive its lease: expiry destroys everything, the handles it passed on and its sub-agents included | [R39 (leases end)](../servers/steward.md#r39-leases-end), [R10 (destruction)](../kernel/budgets.md#r10-destruction) | `budget-deadline`, `budget-destroy-attack` for the kernel; the lease case not yet |
| Ask for a lease longer than `MAX_LEASE` | R39 | not yet |
| Spoof the approval screen (control, bidi and format characters, swapped requests); send free text from a vault | [R38 (out-of-band approval)](../servers/steward.md#r38-out-of-band-approval) | not yet |
| Use its sponsor's 9P connection instead of the fresh one it was given | R41 | not yet |
| Slow Bob beyond its weight | [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) | `sched-share`, `sched-sleep-gaming`, `sched-timer-flood` |
| Flood the steward and `fsd` until Alice cannot open a file or end its lease | [R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) | not yet in a boot |
| Have `keyd` sign arbitrary bytes (an SSH user-auth blob relayed from its peer) | [R44 (one key, one purpose, keyd's own digest)](../servers/keyd.md#r44-one-key-one-purpose-keyds-own-digest) | not yet in a boot |

### A scripted hostile user

Bob, attacking Alice.

| Attack | Rules | Bench case |
| --- | --- | --- |
| Call fuzzing: any arguments to any call get an error, never a kernel panic | [I14 (no call panics the kernel)](../kernel/invariants.md#i14-no-call-panics-the-kernel) | `budget-syscall-attack`, `budget-forge-attack`, `syscall-attack`, `legacy-gone` |
| Endpoint flooding: 10,000 sender threads calling `fsd`, and Alice is still served in her turn | [R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting) | `redoubt-ipc` for the kernel's half (`WAIT_CAP` and turns per group); with `fsd` not yet |
| A vault session filling its `WAIT_CAP` on a shared server leaves its owner's unlabelled session's turn and cap unaffected | R2, [R37 (vault non-interference)](../servers/steward.md#r37-vault-non-interference) | not yet |
| A lender destroyed while `fsd` holds its lent pages, and `fsd` survives | [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls) | `uaf-lent-page`, `process-lifecycle` for the kernel; with `fsd` not yet |
| Crash blame: Bob crashes `fsd` three times while Alice is busy; every session and lease of Bob's with that label set ends and he cannot log straight back in; Alice is unaffected, also when `fsd` panics rather than faults and when the crashing thread holds her calls open too; a crash from a `send` while a bystander's call is parked blames nobody; a vault session's crashes do not end its owner's unlabelled session | [R21 (crash blame)](../kernel/processes.md#r21-crash-blame), [R40 (blame by label set)](../servers/steward.md#r40-blame-by-label-set) | `process`, `process-attack` for the kernel's blame; the steward's not yet |
| Pinned open calls: 64 lent calls parked at `ipd` with short timeouts, and `ipd` still takes `netd`'s frames and frees the abandoned calls; SSH sessions survive | [R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting), [R4a (open calls)](../kernel/ipc.md#r4a-open-calls) | `net-pinned`; with SSH not yet |
| System fairness: a busy `fsd:data` does not fill `blkd`'s `WAIT_CAP` for `fsd:alice-secrets` | R2 | not yet |
| Server CPU: expensive requests to a server delay other users only by that server's weight | R12 | `sched-server-busy`, `sched-large-weight` |
| Shared pools: filling the `data` volume does not fail Alice's saves; flooding `fsd` with handles does not grow its table | [R48 (a quota per attach root)](../servers/fsd.md#r48-a-quota-per-attach-root) | not yet |
| Server authority: no server's startup block holds its budget, a manifest granting one is refused, and no server can destroy a session | [R33 (no server holds a system budget)](../servers/init.md#r33-no-server-holds-a-system-budget) | not yet |
| `process_create` with a badged exit endpoint, to aim exit notices and blame at a server | R21 | `process-attack` |
| A reused PID carries authority | [R20 (PID reuse)](../kernel/processes.md#r20-pid-reuse) | `pid-reuse-authority` |
| Loopback login: a session connects to the box's own `sshd` with a key `keyd` holds | [R59 (never the box's own addresses)](../servers/ipd.md#r59-never-the-boxs-own-addresses) | `net-attacks` for `ipd`'s refusal; the login not yet |
| No leaky state: while a vault session works, an unlabelled observer sees no change in usage, request and session ids, message ids, PIDs, file versions, qids, directory listings, audit records or approval notifications, and cannot write, truncate, create or remove anything in the vault's volume | R37, R25 | not yet |
| Hostile launch: a malformed ELF or startup block from a user parent hurts only the child | [R32 (a hostile image hurts only its process)](../servers/init.md#r32-a-hostile-image-hurts-only-its-process), [R31 (startup block checked whole)](../servers/init.md#r31-startup-block-checked-whole) | `stub-launch` for a malformed ELF, `process-attack` for malformed startup records at `process_start`; from a user parent not yet |
| Approval flood: requests hit the per-(account, label set) cap; the steward and Alice's approval screen are unaffected | R38, R26 | not yet |
| Admission: a crashed or killed client's fids and quota come back when its launcher disconnects it; a system daemon filling its admission slots does not lock out the steward | R26 | not yet in a boot |

### Kernel cases

| Attack | Rules | Bench case |
| --- | --- | --- |
| Revocation by budget: mint into a revocation scope and destroy it; the handles are dead everywhere, messages already sent through them fail and carry no reply's handles, and handles inside queued messages arrive as 0 | [R9 (stamps)](../kernel/objects.md#r9-stamps), R10 | `redoubt-revoke`, `budget-destroy-attack` |
| Budgets: carving, charging and exhaustion | [R6 (charging)](../kernel/budgets.md#r6-charging), [R7 (carving)](../kernel/budgets.md#r7-carving) | `budget`, `budget-carve-attack`, `budget-table-attack`, `budget-mem-churn`, `touch-beyond-ram` |
| The scheduler: churn, carving, sleeping and ties gain nothing | R12 | `sched-budget-churn`, `sched-carve-inflation`, `sched-debt-lift`, `sched-destroy-billing`, `sched-exit-churn`, `sched-idle-gap`, `sched-ties`, `sched-wake-no-preempt` |
| The timer: every blocking call returns by its timeout | [I13 (every blocking call returns by its timeout)](../kernel/invariants.md#i13-every-blocking-call-returns-by-its-timeout) | `timeouts`, `budget-deadline` |
| Memory: W^X, zeroed pages, no replacement by `map_fixed` | [R11 (memory)](../kernel/memory.md#r11-memory) | `wx`, `kernel-wx`, `mem-attack`, `write-only-attack`, `map-fixed-attack` |
| Devices: no device authority without a device object; DMA pages reset before reuse | [R18 (device authority)](../kernel/devices.md#r18-device-authority), [I16 (DMA pages reset before reuse)](../kernel/invariants.md#i16-dma-pages-reset-before-reuse) | `irq-attack`, `dma-rules`, `dma-reset-reuse`, `dma-reset-quarantine` |
| Containment: hostile agents in leases are preempted, then ended at their deadlines or revoked by hand with their messages and lends in flight, while the driver, the steward and a victim server stay responsive | R10, R12, R3, [I2 (revocation is complete)](../kernel/invariants.md#i2-revocation-is-complete), [I10 (create-destroy leaves the parent unchanged)](../kernel/invariants.md#i10-create-destroy-leaves-the-parent-unchanged) | not yet ([containment](../kernel/README.md#containment)) |
| Boot: a tampered or confused bundle does not run | [R15 (verified boot)](../kernel/boot.md#r15-verified-boot), [R16 (image confinement)](../kernel/boot.md#r16-image-confinement) | `verified-boot-rejects-tamper`, `verified-boot-rejects-bare-archive`, `loader-rejects-kernel-address`, `loader-rejects-kernel-entry`, `loader-rejects-truncated-elf` |

The gaps the kernel's own cases leave (R1 (flow) across label sets on the real kernel, R2's turns
between groups, completion races between harts) are listed in
[kernel attack gaps](../todo/kernel-attack-gaps.md).

## Remaining work

Each part lands with the attack cases for what it builds. The order the parts run in, and which
are under way, is kept with the project's work plan ([AGENTS.md](../../AGENTS.md)), not on this
page.

- **The follow-up packages.** The fixes found while writing this book, before anything is built
   on top of them.
   - **The kernel:** [the boot stack reservation](../todo/boot-stack-reservation.md),
     [`process_map` backs before refusing](../todo/process-map-backs-before-refusing.md),
     [an ending process pumps each endpoint once](../todo/process-ending-pumps-once.md).
   - **beamlet:** [bounded operands for modular exponentiation](../todo/beamlet-bignum-bounds.md).
- **A kernel containment gate.** One boot on QEMU that proves the kernel's primitives alone,
   before the whole milestone is layered on them: hostile code preempted and ended at its
   deadline, a budget subtree revoked with messages and lends in flight, and a victim that stays
   responsive throughout. It proves kernel primitives only; it is not evidence for the steward,
   approvals, the network or a session ([containment](../kernel/README.md#containment)).
- **The client library.** `redoubt-client`: the namespace, files over 9P, the file server's
  typed operations, the console, launching, a launcher's grants and one typed call, the API every
  userland binds to, tested on the host against real servers
  ([native programs](../userland/native.md#the-client-library)). `init` and beamlet's platform are
  built on it. Before beamlet's files run on it: dropped files' fids that come back, `Rerror`s by
  name, whole reads and writes, and generated Elixir clients
  ([native programs](../userland/native.md#dropped-files-error-names-and-generated-calls),
  [wire](../servers/wire.md#error-names)).
- **`init` and the boot manifest.** The loader loads only the kernel and `init`
   ([boot](../kernel/boot.md#the-loader-loads-only-the-kernel-and-init)); `init` reads the
   manifest, builds the budget tree from it
   ([budgets](../kernel/budgets.md#the-tree-from-the-boot-manifest)), hands each server its
   devices ([devices](../kernel/devices.md#which-process-gets-which-device)), runs the
   confinement and key-separation checks
   ([bucket counts among them](../todo/server-bucket-counts.md)), and starts every server through the loader stub with
   fresh connections ([init](../servers/init.md)). `blkd`, `netd`, `ipd`, `bootfsd`, `consoled`
   and `keyd` move from the bench's rigs to `init`. A launcher releases its children's grants
   ([wire](../servers/wire.md#a-launcher-releases-its-childs-grants)). A shared server's
   terminal fallback and rollback are exercised in a boot, not only in host tests
   ([serving](../servers/serving.md#replies-and-rollback)), and restarting a driver is a case of
   its own ([netd](../servers/netd.md#started-by-init)). Until `init` exists the loader's bundle
   programs run at `system`'s free weight, so one that busy-yields dominates the machine; the
   measured cases keep them blocked, and a budget per program ends this. The kernel's cases then
   start their programs from a tester in `init`'s place, with the budgets each case names, so the
   log server's interim `TAKE_GIFTS` fixture goes; the servers' cases boot `init` itself with a
   manifest of their own, and `consoled` says who wrote each line
   ([starting a case's programs](../testbench.md#starting-a-cases-programs)).
- **beamlet on Redoubt, and the shell on the console.** The VM runs on the kernel with its natives and
   asynchronous platform ([beamlet](../userland/beamlet.md#beamlet-on-redoubt)); an interactive
   Elixir shell on the UART console, before SSH exists
   ([the shell](../userland/shell.md#the-shell-in-a-session)).
- **The file server.** `fsd` over `blkd`: one volume per instance, labelled volumes, quotas per
   attach root, typed operations ([fsd](../servers/fsd.md)); files over 9P from a session
   ([files](../userland/files.md#files-over-9p)).
- **The steward.** Principals from the manifest, fixed sub-budgets per label set, sessions,
   leases, the powerbox and approvals, declassification and push, crash blame
   ([the steward](../servers/steward.md)); the server graph, trust tiers and capability holdings
   it runs on ([the servers](../servers/README.md)). The cost of destroying a budget follows the
   dying subtree, under its target ([budgets](../kernel/budgets.md#residual-risks)), and the
   steward's decision-wake target is set from a seed sweep
   ([responsiveness](../kernel/scheduling.md#responsiveness)).
   The scheduling latency bench, measured with stand-ins for the steward and the drivers, is
   rerun with the real ones, and its numbers must stay within the target.
- **`sshd`.** Sessions over SSH as beamlet VMs running the shell, vault sessions, and `approve@box`
   ([sshd](../servers/sshd.md), [sessions](../userland/sessions.md)).
- **The agent and the attack suite.** Alice's agent as its own principal under a lease, with
   delegation that only narrows ([agents](../userland/agents.md)), launching native programs from
   a session ([native programs](../userland/native.md#launching-from-a-session)), and every "not
   yet" above turned into a case.
- **The model on the real kernel.** Model traces replayed on the real kernel and compared step by
   step ([the model](../kernel/model.md#replaying-traces-on-the-real-kernel)), after the model and
   the kernel agree on their order of checks ([the model's order of checks](../todo/abi-model-disagreements.md)).

Every follow-up page is placed above.

## Progress

Built and attack-tested today:
- **The kernel**, except SUM and MXR clearing and the three places `init` takes over from the
  bench (what the loader loads, the budget tree, which process gets which device): handles and
  objects, IPC, memory, budgets, scheduling, the timer, processes, devices and DMA, verified boot
  ([the kernel](../kernel/README.md)), and the executable model with its mutations
  ([the model](../kernel/model.md)).
- **The serving library** and the wire formats: admission, the label check, minted connections,
  parked calls, typed dispatch, the 9P skeleton ([serving](../servers/serving.md),
  [wire](../servers/wire.md)).
- **The drivers and the network:** `blkd` and `netd` attacked by hostile devices in host tests,
  `netd` and `ipd` on the real kernel through a test rig ([blkd](../servers/blkd.md),
  [netd](../servers/netd.md), [ipd](../servers/ipd.md)).
- **The file system's core:** littlefs against a hostile medium and power loss
  ([fsd](../servers/fsd.md#littlefs)).
- **`bootfsd`, `consoled` and `keyd`**, served and attacked in host tests
  ([bootfsd](../servers/bootfsd.md), [consoled](../servers/consoled.md), [keyd](../servers/keyd.md)).
- **Launching:** the startup block and the loader stub ([init](../servers/init.md#the-startup-block)).
- **beamlet** on the host, loading hostile code with limits inside one VM
  ([beamlet](../userland/beamlet.md)); and its platform on Redoubt's console, clock and
  randomness, on the fake kernel ([beamlet](../userland/beamlet.md#the-console-the-clock-and-randomness)).

Not built: `init`'s manifest handling, the `fsd` server, the steward, `sshd`, sessions and the agent.
