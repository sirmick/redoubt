The steward's `session` machine: a login session, `ssh alice@box` or `ssh alice+secrets@box`, or
the console principal's on the UART (`Console`, the steward's own, with no key), in the domain of
its principal's account and its label set. Included by
[the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: session -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `Login` | `!login_key` | - | `refuse` |
| - | `Login` | `!owns_labels` | - | `refuse` |
| - | `Login` | `!not_locked` | - | `refuse` |
| - | `Login` | - | `Starting` | `carve_session`, `create_scope`, `connect`, `launch` |
| - | `Console` | `!not_locked` | - | `refuse` |
| - | `Console` | - | `Starting` | `carve_session`, `create_scope`, `connect`, `launch` |
| `Starting` | `Done` | `!not_locked` | `Ending` | `refuse`, `destroy_budget` |
| `Starting` | `Done` | - | `Running` | `route`, `audit_login`, `reply_login` |
| `Starting` | `Failed` | - | `Ending` | `refuse`, `destroy_partial` |
| `Starting` | `EndSession`, `ChannelClosed`, `Exited` | - | = | `unreachable` |
| `Starting` | `LockedOut` | - | = | - |
| `Running` | `EndSession` | - | `Ending` | `unroute`, `drop_requests`, `reply_ok`, `destroy_budget` |
| `Running` | `ChannelClosed`, `Exited`, `LockedOut` | - | `Ending` | `unroute`, `drop_requests`, `destroy_budget` |
| `Running` | `Done`, `Failed` | - | = | `unreachable` |
| `Ending` | `Done`, `Failed` | - | `Ended` | `forget` |
| `Ending` | `EndSession`, `ChannelClosed`, `Exited` | - | = | `unreachable` |
| `Ending` | `LockedOut` | - | = | - |

<!-- ANCHOR_END: table -->
