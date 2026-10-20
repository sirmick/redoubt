# READ1 gate: a lease killed by a neighbouring lease's end

Logs: `/home/mcloonan/redoubt/.tmp/READ1/evidence/gate10/` (copies of the worktree's
`.tmp/READ1/gate10/`), `gates10.log` beside them; the full set's `gates9.log`.

## The set

`gates9.log`: 88 cases both widths (formatting, docs, size-budget, unsafe-budget, host-tests, the 14
build cases, every steward-* boot case but latency, erofs*, walfsd*, littlefsd-boot/host-tests,
verity-signed/flipped-tree, beamlet-launch, agent-login, pack-bad-*, userland-bad-start,
lease-bad-start): 162 PASS, no FAIL. `lease-code-load` was rerun alone (`gates10.log`).

## lease-code-load, four boots

| boot | launches | exits 0, in order | then |
| --- | --- | --- | --- |
| rv64 one hart | 16 in 2.3–3.3 s each | none reached: the SSH connection was closed by the guest right after the sixteenth launch, the shell idle at its prompt (`sshd: a connection's input ended`; no `[!] Terminating process` line, so no process died); the other three boots idled the same way without it | rerun alone to tell flake from finding |
| rv64 two harts | 16 | agent-1 … agent-10, agent-14 | `[!] Terminating process with PID 60` → `steward: lease alice/agent-11 … was killed 0` |
| rv32 one hart | 16 | agent-1 … agent-13, agent-16, agent-14 | `[!] Terminating process with PID 126` → `agent-15 … was killed 0` |
| rv32 two harts | 16 | agent-1 … agent-13, agent-16, agent-15 | `[!] Terminating process with PID 419` → `agent-14 … was killed 0` |

In each the victim is killed immediately after the preceding lease's `exited 0` → `LeaseEnded`
audit → its budget's destroy, with **no `LeaseEnded` audit of the victim first**. `killed` is the
kernel's R10 word (kernel/budget.rs ~1383: a budget destroyed with the process in it, "unless its
process object is charged to a budget in the same doomed subtree"), so one lease's clean end
destroyed a neighbouring lease's VM: the steward's `destroy_budget` reaching another lease's
budget (a token or handle to the wrong one; cf. the recycled-scope-index tests under steward.md's
"Failure and restart"), or the kernel charging one VM's process object to another's budget.
Fifteen of sixteen then, every time; which one varies (11, 15, 14).

Sixteen lease VMs exiting by themselves close together is a shape no case had: Echo's other
orders spin or write and wait; the latency leases die by the kernel's deadline. So the gate's
verdict channel (the exit) is what surfaced it. It is not the read path (every agent that exited
loaded all 27 modules; none was refused), and not READ1's to fix.

## The decision asked

(a) READ1 waits for the kill's node and reruns the gate; or (b) READ1 lands now with
`lease-code-load` kept, `whole_run = false` until the node lands, and erofsd.md's and
scheduling.md's rows stating fifteen of sixteen and the finding by name.
