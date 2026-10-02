The steward's `lease` machine: an agent on a lease, started by an unlabelled session of its
sponsor or by an approved request, in the domain of the sponsor's account and the lease's label
set. A granted lease has no caller to answer, so its start failing is audited in the lease's
domain. Every row leaving `Running` tells the sponsor's unlabelled sessions that the lease ended,
through the lease-supervision edge. Included by
[the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: lease -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `StartAgent` | `!caller_unlabelled` | - | `refuse` |
| - | `StartAgent` | `!lease_bounded` | - | `refuse` |
| - | `StartAgent` | `!not_locked` | - | `refuse` |
| - | `StartAgent` | - | `Starting` | `carve_lease`, `create_scope`, `connect`, `launch` |
| - | `Granted` | - | `Starting` | `carve_lease`, `create_scope`, `connect`, `launch` |
| `Starting` | `Done` | `granted`, `!not_locked` | `Ending` | `audit_start_failed`, `destroy_budget` |
| `Starting` | `Done` | `!not_locked` | `Ending` | `refuse`, `destroy_budget` |
| `Starting` | `Done` | `granted` | `Running` | `route`, `audit_agent_started` |
| `Starting` | `Done` | - | `Running` | `route`, `audit_agent_started`, `reply_agent` |
| `Starting` | `Failed` | `granted` | `Ending` | `audit_start_failed`, `destroy_partial` |
| `Starting` | `Failed` | - | `Ending` | `refuse`, `destroy_partial` |
| `Starting` | `EndLease` (ahead), `Exited` | - | = | `unreachable` |
| `Starting` | `LockedOut` | - | = | - |
| `Running` | `EndLease` (ahead) | `!sponsor_session` | = | `refuse` |
| `Running` | `EndLease` (ahead) | - | `Ending` | `unroute`, `drop_requests`, `audit_lease_ended`, `notify_sponsor`, `reply_ok`, `destroy_budget` |
| `Running` | `Exited`, `LockedOut` | - | `Ending` | `unroute`, `drop_requests`, `audit_lease_ended`, `notify_sponsor`, `destroy_budget` |
| `Running` | `Done`, `Failed` | - | = | `unreachable` |
| `Ending` | `Done`, `Failed` | - | `Ended` | `forget` |
| `Ending` | `EndLease` (ahead), `Exited` | - | = | `unreachable` |
| `Ending` | `LockedOut` | - | = | - |

<!-- ANCHOR_END: table -->
