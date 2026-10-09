The steward's `session` machine: a login's context, `ssh alice@box`, `ssh alice+secrets@box` or
a named one of either (`ssh alice.work@box`), or the console principal's session on the UART
(`Console`, the steward's own, with no key and no context), in the domain of its principal's
account and its label set. A context is `Running` while a channel is attached to it and
`Detached` while none is; a login naming a live context attaches to it (`Attach`, raised by
`take_over` once the login is authenticated), taking it over from its channel if it has one.
Included by [the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: session -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `Login` | `!login_key` | - | `refuse` |
| - | `Login` | `!owns_labels` | - | `refuse` |
| - | `Login` | `!not_locked` | - | `refuse` |
| - | `Login` | `!context_free` | - | `take_over` |
| - | `Login` | - | `Starting` | `carve_session`, `create_scope`, `launch_relay`, `attach_relay`, `connect`, `launch` |
| - | `Console` | `!not_locked` | - | `refuse` |
| - | `Console` | - | `Starting` | `carve_session`, `create_scope`, `connect`, `launch` |
| `Starting` | `Done` | `!not_locked` | `Ending` | `refuse`, `destroy_budget` |
| `Starting` | `Done` | - | `Running` | `route`, `audit_login`, `reply_login` |
| `Starting` | `Failed` | - | `Ending` | `refuse`, `destroy_partial` |
| `Starting` | `EndSession`, `ChannelClosed`, `Exited`, `Detach` | - | = | `unreachable` |
| `Starting` | `Attach` | - | = | `refuse_in_use` |
| `Starting` | `LockedOut` | - | = | - |
| `Running` | `Attach` | - | = | `detach_relay`, `attach_relay`, `audit_attached` |
| `Running` | `ChannelClosed`, `Detach` | - | `Detached` | `detach_relay` |
| `Running` | `EndSession` | - | `Ending` | `unroute`, `drop_requests`, `reply_ok`, `destroy_budget` |
| `Running` | `Exited`, `LockedOut` | - | `Ending` | `unroute`, `drop_requests`, `destroy_budget` |
| `Running` | `Done` | - | = | `reply_login` |
| `Running` | `Failed` | - | `Ending` | `refuse`, `unroute`, `drop_requests`, `destroy_budget` |
| `Detached` | `Attach` | - | `Running` | `attach_relay`, `audit_attached` |
| `Detached` | `ChannelClosed`, `Detach`, `Done` | - | = | - |
| `Detached` | `EndSession` | - | `Ending` | `unroute`, `drop_requests`, `reply_ok`, `destroy_budget` |
| `Detached` | `Exited`, `LockedOut`, `Failed` | - | `Ending` | `unroute`, `drop_requests`, `destroy_budget` |
| `Ending` | `Done`, `Failed` | - | `Ended` | `forget` |
| `Ending` | `EndSession`, `ChannelClosed`, `Exited`, `Attach`, `Detach` | - | = | `unreachable` |
| `Ending` | `LockedOut` | - | = | - |

<!-- ANCHOR_END: table -->
