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
| Write outside its `/project`, or reach the network: the case judges the writes by alice's canaries; that its reads, listings and network lookup are refused rests on the lease's namespace holding only `/project`, `/boot` and the system volume, host-tested | [R25 (the label check)](../servers/serving.md#r25-the-label-check), [R41 (narrowing by revocation scope)](../servers/steward.md#r41-narrowing-by-revocation-scope) | `steward-agent-escape` |
| A labelled agent reaches a sink not cleared for its labels | [R60 (a sink refuses labels)](../servers/ipd.md#r60-a-sink-refuses-labels) | `net-attacks` for `ipd`; the agent's case not yet |
| Outlive its lease: expiry destroys everything, the handles it passed on and its sub-agents included | [R39 (leases end)](../servers/steward.md#r39-leases-end), [R10 (destruction)](../kernel/budgets.md#r10-destruction) | `budget-deadline`, `budget-destroy-attack` for the kernel; the lease case `steward-lease-cut` |
| Ask for a lease longer than `MAX_LEASE` | R39 | `steward-agent-escape` |
| Spoof the approval screen (control, bidi and format characters, swapped requests); send free text from a vault | [R38 (out-of-band approval)](../servers/steward.md#r38-out-of-band-approval) | `steward-screen-inert`, `steward-approve-box` (a stale hash); free text from a vault is the server's host test `notes_are_rendered_by_their_requesters_labels_and_a_dead_sessions_requests_are_gone`, the boot case `steward-vault-note` |
| Use its sponsor's 9P connection instead of the fresh one it was given | R41 | `steward-agent-escape` |
| Slow Bob beyond its weight | [R12 (scheduling)](../kernel/scheduling.md#r12-scheduling) | `sched-share`, `sched-sleep-gaming`, `sched-timer-flood` |
| Flood the steward and `walfsd` until Alice cannot open a file or end its lease | [R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) | `steward-agent-flood` |
| Have `keyd` sign arbitrary bytes (an SSH user-auth blob relayed from its peer) | [R44 (one key, one purpose, keyd's own digest)](../servers/keyd.md#r44-one-key-one-purpose-keyds-own-digest) | not yet in a boot |

### A scripted hostile user

Bob, attacking Alice.

| Attack | Rules | Bench case |
| --- | --- | --- |
| Call fuzzing: any arguments to any call get an error, never a kernel panic | [I14 (no call panics the kernel)](../kernel/invariants.md#i14-no-call-panics-the-kernel) | `budget-syscall-attack`, `budget-forge-attack`, `syscall-attack`, `legacy-gone` |
| Endpoint flooding: a hostile principal's every hub slot calling `walfsd` without end, and Alice is still served in her turn | [R2 (fair waiting)](../kernel/ipc.md#r2-fair-waiting) | `redoubt-ipc` for the kernel's half (`WAIT_CAP` and turns per group); `walfsd-flood-turn` for a real `walfsd` flooded by a second principal while Alice's own reads and writes return within the bound |
| A vault session filling its `WAIT_CAP` on a shared server leaves its owner's unlabelled session's turn and cap unaffected | R2, [R37 (vault non-interference)](../servers/steward.md#r37-vault-non-interference) | `vault-flood-turn` |
| A lender destroyed while `walfsd` holds its lent pages, and `walfsd` survives | [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls) | `uaf-lent-page`, `process-lifecycle` for the kernel; with `walfsd` not yet |
| Crash blame: Bob crashes `walfsd` three times while Alice is busy; every session and lease of Bob's with that label set ends and he cannot log straight back in; Alice is unaffected, also when `walfsd` panics rather than faults and when the crashing thread holds her calls open too; a crash from a `send` while a bystander's call is parked blames nobody; a vault session's crashes do not end its owner's unlabelled session | [R21 (crash blame)](../kernel/processes.md#r21-crash-blame), [R40 (blame by label set)](../servers/steward.md#r40-blame-by-label-set) | `process`, `process-attack` for the kernel's blame; `steward-blame-lockout` for one blame counted on the machine; `steward-server-restart` for the lockout at three and "Alice is unaffected" (her shell re-binds her home after each of Bob's crashes and reads back what she wrote); the vault arm host only |
| Pinned open calls: 64 lent calls parked at `ipd` with short timeouts, and `ipd` still takes `netd`'s frames and frees the abandoned calls; SSH sessions survive | [R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting), [R4a (open calls)](../kernel/ipc.md#r4a-open-calls) | `net-pinned`; `net-pinned-ssh` for the same from a session's `gen_tcp` over `/net`, Bob's SSH login reaching its prompt and Alice's echo still working |
| System fairness: a busy `walfsd:data` does not fill `blkd`'s `WAIT_CAP` for `walfsd:alice-secrets` | R2 | `blkd-fairness` |
| Server CPU: expensive requests to a server delay other users only by that server's weight | R12 | `sched-server-busy`, `sched-large-weight` |
| Shared pools: filling the `data` volume does not fail Alice's saves; flooding `walfsd` with handles does not grow its table | [R48 (a quota per attach root)](../servers/littlefsd.md#r48-a-quota-per-attach-root), [R26 (admission fairness)](../servers/serving.md#r26-admission-fairness) | `walfsd-pools`: Bob fills his home on the `data` volume to `enospc`, then holds handles on one file until `walfsd` refuses the sixteenth by name, his bucket's share; Alice's 64 KiB saves to her home succeed while he fills and while he holds them, each under 5 s, and the bench's memory scan finds `walfsd:data`'s heap at 37 of its 64 pages with the flood held |
| Server authority: no server's startup block holds its budget, a manifest granting one is refused, and no server can destroy a session | [R33 (no server holds a system budget)](../servers/init.md#r33-no-server-holds-a-system-budget) | `init-refuses-budget-handle`; that no server can destroy a session is the steward's, not yet |
| `process_create` with a badged exit endpoint, to aim exit notices and blame at a server | R21 | `process-attack` |
| A reused PID carries authority | [R20 (PID reuse)](../kernel/processes.md#r20-pid-reuse) | `pid-reuse-authority` |
| Loopback login: a session connects to the box's own `sshd` with a key `keyd` holds | [R59 (never the box's own addresses)](../servers/ipd.md#r59-never-the-boxs-own-addresses) | `session-tcp-loopback`: a session's `gen_tcp` connect to the box's loopback and listed address on port 22 is refused by `ipd`, no SYN leaves the box (the capture) and the peer counts only the control; `net-attacks` for the whole refusal set |
| No leaky state: while a vault session works, an unlabelled observer sees no change in usage, request and session ids, message ids, PIDs, file versions, qids, directory listings, audit records or approval notifications, and cannot write, truncate, create or remove anything in the vault's volume | R37, R25, [R36 (unpredictable ids)](../servers/steward.md#r36-unpredictable-ids) | `vault-no-leak`, for every observable a session has ([the list](../servers/steward.md#r37-vault-non-interference)): Alice's unlabelled session records its budget's limits and processes, a probe carve, its contexts, its home's listing with each entry's kind, size and mtime, and a launch's ending and use, before and after her vault session writes, launches and submits, and prints them equal; no notice but its own requests' reaches it; its two request ids and the vault's are three distinct random words, the second no successor of the first; its writes, creates, removes and truncates in the vault's volume are `enoent`, the namespace giving it no connection there. Message ids, PIDs, qids, file versions and audit records are not reachable from a session today, so they are not on the list; exposing a PID or a qid would be its own node |
| Hostile launch: a malformed ELF or startup block from a user parent hurts only the child | [R32 (a hostile image hurts only its process)](../servers/init.md#r32-a-hostile-image-hurts-only-its-process), [R31 (startup block checked whole)](../servers/init.md#r31-startup-block-checked-whole) | `stub-launch` for a malformed ELF, `process-attack` for malformed startup records at `process_start`; `session-hostile-launch` from a user parent: Alice's session launches, from her home, an image cut to 40 bytes, one cut in half, one of zeros, an empty one and a whole one with a handle named outside the rule, through the launch native; the stub refuses the three malformed images as the child's exit 111 with its budget holding nothing, the client refuses the empty image and the bad name before any kernel call, and her session's processes are the same after, a well-formed `exec` running |
| Approval flood: requests hit the per-(account, label set) cap; the steward and Alice's approval screen are unaffected | R38, R26 | `steward-pending-cap` |
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
- **The agent and the attack suite.** Alice's agent as its own principal under a lease, with
   delegation that only narrows ([agents](../userland/agents.md)), launching native programs from
   a session ([native programs](../userland/native.md#launching-from-a-session)), and every "not
   yet" above turned into a case.
- **The model on the real kernel.** Model traces replayed on the real kernel and compared step by
   step ([the model](../kernel/model.md#replaying-traces-on-the-real-kernel)), the model making
   the kernel's checks in the kernel's order ([ABI](../kernel/abi.md#errors-and-the-order-of-checks)).

## Progress

The steward's policy core decides leases, approvals, declassification, push and crash blame, and
is tested on the host; the running server's half is built for leases, approvals at
`approve@box`, declassification and push (one item through a crossing budget with exactly its
labels, landing in `~/out`, or pushed into the vault; a confined boot's push lands on the
machine, the labelled session's home left unbound) and crash blame (the lockout at three on
the machine, the sessions a crashed server served re-bound through the steward, an ended
session's servers told at the steward's turn so no blame is lost behind them)
([the policy core](../servers/steward.md#the-policy-core),
[declassification and push](../servers/steward.md#declassification-and-push)). The kernel's half of several attacks
above has its case, as the tables say, and the kernel containment gate holds hostile leases on the
kernel's primitives alone ([containment](../kernel/README.md#containment)). The image's
`agent` principal, for a sponsor-run agent whose loop runs outside the box, is sponsored by alice:
its login is a lease of hers, which her sessions can end and are told the end of, with no labels
and no network scope, logged in by `agent-login`; in that session the agent compiles a module
from a string, keeps its `.beam` in its home and loads it again from the bytes, by
`shell-compile` ([development](../userland/development.md#the-compilers-on-beamlet)); and
`start_agent` and `end_lease` run on the machine ([the image](../../image/README.md);
[the steward](../servers/steward.md#leases)). A session's VM has TCP: `gen_tcp` over the
session's `/net` at `ipd`, every socket `ipd`'s, so the loopback login above is `ipd`'s refusal
and a lease, with no `/net`, has no network (`session-tcp-echo`, `session-tcp-loopback`;
[beamlet](../userland/beamlet.md#beamlet-on-redoubt)). What the milestone
builds on: the steward's principals, sub-budgets and sessions, and `sshd` on the box, from
[M1 (sessions over SSH, kept apart)](m1-separation.md).
