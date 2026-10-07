# M3 (agents, approvals and the attack suite)

## Goal

Alice's agent runs contained under a lease. An approval counts only from `approve@box`, data
crosses a label only as one approved item, and crash blame ends only the blamed (account, label
set)'s sessions and leases. Every property is backed by an attack case, and the attack suite
below passes.

The milestone completes the slice [M1 (sessions over SSH, kept apart)](m1-separation.md) began: the
steward's leases, agents, approvals, declassification and push and crash blame; vault sessions
and `approve@box` over SSH; TCP for a session's VM; one scripted hostile agent and one scripted
hostile user; and the model's traces replayed on the real kernel.

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
| Flood the steward and `littlefsd` until Alice cannot open a file or end its lease | [R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) | not yet in a boot |
| Have `keyd` sign arbitrary bytes (an SSH user-auth blob relayed from its peer) | [R44 (one key, one purpose, keyd's own digest)](../servers/keyd.md#r44-one-key-one-purpose-keyds-own-digest) | not yet in a boot |

### A scripted hostile user

Bob, attacking Alice.

| Attack | Rules | Bench case |
| --- | --- | --- |
| Call fuzzing: any arguments to any call get an error, never a kernel panic | [I14 (no call panics the kernel)](../kernel/invariants.md#i14-no-call-panics-the-kernel) | `budget-syscall-attack`, `budget-forge-attack`, `syscall-attack`, `legacy-gone` |
| Endpoint flooding: 10,000 sender threads calling `littlefsd`, and Alice is still served in her turn | [R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting) | `redoubt-ipc` for the kernel's half (`WAIT_CAP` and turns per group); with `littlefsd` not yet |
| A vault session filling its `WAIT_CAP` on a shared server leaves its owner's unlabelled session's turn and cap unaffected | R2, [R37 (vault non-interference)](../servers/steward.md#r37-vault-non-interference) | not yet |
| A lender destroyed while `littlefsd` holds its lent pages, and `littlefsd` survives | [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls) | `uaf-lent-page`, `process-lifecycle` for the kernel; with `littlefsd` not yet |
| Crash blame: Bob crashes `littlefsd` three times while Alice is busy; every session and lease of Bob's with that label set ends and he cannot log straight back in; Alice is unaffected, also when `littlefsd` panics rather than faults and when the crashing thread holds her calls open too; a crash from a `send` while a bystander's call is parked blames nobody; a vault session's crashes do not end its owner's unlabelled session | [R21 (crash blame)](../kernel/processes.md#r21-crash-blame), [R40 (blame by label set)](../servers/steward.md#r40-blame-by-label-set) | `process`, `process-attack` for the kernel's blame; the steward's not yet |
| Pinned open calls: 64 lent calls parked at `ipd` with short timeouts, and `ipd` still takes `netd`'s frames and frees the abandoned calls; SSH sessions survive | [R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting), [R4a (open calls)](../kernel/ipc.md#r4a-open-calls) | `net-pinned`; with SSH not yet |
| System fairness: a busy `walfsd:data` does not fill `blkd`'s `WAIT_CAP` for `walfsd:alice-secrets` | R2 | not yet |
| Server CPU: expensive requests to a server delay other users only by that server's weight | R12 | `sched-server-busy`, `sched-large-weight` |
| Shared pools: filling the `data` volume does not fail Alice's saves; flooding `littlefsd` with handles does not grow its table | [R48 (a quota per attach root)](../servers/littlefsd.md#r48-a-quota-per-attach-root) | not yet |
| Server authority: no server's startup block holds its budget, a manifest granting one is refused, and no server can destroy a session | [R33 (no server holds a system budget)](../servers/init.md#r33-no-server-holds-a-system-budget) | `init-refuses-budget-handle`; that no server can destroy a session is the steward's, not yet |
| `process_create` with a badged exit endpoint, to aim exit notices and blame at a server | R21 | `process-attack` |
| A reused PID carries authority | [R20 (PID reuse)](../kernel/processes.md#r20-pid-reuse) | `pid-reuse-authority` |
| Loopback login: a session connects to the box's own `sshd` with a key `keyd` holds | [R59 (never the box's own addresses)](../servers/ipd.md#r59-never-the-boxs-own-addresses) | `net-attacks` for `ipd`'s refusal; the login not yet |
| No leaky state: while a vault session works, an unlabelled observer sees no change in usage, request and session ids, message ids, PIDs, file versions, qids, directory listings, audit records or approval notifications, and cannot write, truncate, create or remove anything in the vault's volume | R37, R25 | not yet |
| Hostile launch: a malformed ELF or startup block from a user parent hurts only the child | [R32 (a hostile image hurts only its process)](../servers/init.md#r32-a-hostile-image-hurts-only-its-process), [R31 (startup block checked whole)](../servers/init.md#r31-startup-block-checked-whole) | `stub-launch` for a malformed ELF, `process-attack` for malformed startup records at `process_start`; from a user parent, the steward's, not yet |
| Approval flood: requests hit the per-(account, label set) cap; the steward and Alice's approval screen are unaffected | R38, R26 | not yet |
| Admission: a crashed or killed client's fids and quota come back when its launcher disconnects it; a system daemon filling its admission slots does not lock out the steward | R26 | not yet in a boot |

## Remaining work

In this order, after [M2 (usable shell)](m2-usable-shell.md). Each part lands with the attack
cases for what it builds.

- **The steward's second half.** Leases and agents, the powerbox and approvals at `approve@box`,
   and crash blame; then declassification and push ([the steward](../servers/steward.md#leases),
   [the powerbox](../servers/steward.md#the-powerbox-and-approvals),
   [crash blame](../servers/steward.md#crash-blame),
   [declassification](../servers/steward.md#declassification-and-push)). The cost of destroying
   a budget follows the dying subtree, under its target
   ([budgets](../kernel/budgets.md#residual-risks)), and the steward's decision-wake target is set
   from a seed sweep ([responsiveness](../kernel/scheduling.md#responsiveness)). The scheduling
   latency bench, measured with stand-ins for the steward and the drivers, is rerun with the real
   ones, and its numbers must stay within the target.
- **`sshd`.** Vault sessions and `approve@box` ([sshd](../servers/sshd.md#approvebox),
  [sessions](../userland/sessions.md#vault-sessions)).
- **TCP in a session.** `gen_tcp` over `/net`, which the loopback login case needs
  ([beamlet](../userland/beamlet.md#beamlet-on-redoubt)).
- **The agent and the attack suite.** Alice's agent as its own principal under a lease, with
   delegation that only narrows ([agents](../userland/agents.md)), launching native programs from
   a session ([native programs](../userland/native.md#launching-from-a-session)), and every "not
   yet" above turned into a case.
- **The model on the real kernel.** Model traces replayed on the real kernel and compared step by
   step ([the model](../kernel/model.md#replaying-traces-on-the-real-kernel)), the model making
   the kernel's checks in the kernel's order ([ABI](../kernel/abi.md#errors-and-the-order-of-checks)).

## Progress

The steward's policy core decides leases, approvals, declassification, push and crash blame, and
is tested on the host; the running server's half for them remains planned
([the policy core](../servers/steward.md#the-policy-core)). The kernel's half of several attacks
above has its case, as the tables say, and the kernel containment gate holds hostile leases on the
kernel's primitives alone ([containment](../kernel/README.md#containment)). What the milestone
builds on: the steward's principals, sub-budgets and sessions, and `sshd` on the box, from
[M1 (sessions over SSH, kept apart)](m1-separation.md).
