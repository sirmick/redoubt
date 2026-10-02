The steward's `blame` machine: one per domain, made at boot, counting the crashes blamed on it
(R40). `lock_out` sends `LockedOut` to every session and lease of the domain. Included by
[the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: blame -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `Boot` | - | `Open` | - |
| `Open` | `Blame` | `blame_window` | `LockedOut` | `count_blame`, `audit_blamed`, `lock_out`, `audit_locked_out` |
| `Open` | `Blame` | - | = | `count_blame`, `audit_blamed` |
| `Open` | `Login` | - | = | - |
| `LockedOut` | `Blame` | `blame_window` | = | `count_blame`, `audit_blamed`, `lock_out`, `audit_locked_out` |
| `LockedOut` | `Blame` | - | = | `count_blame`, `audit_blamed` |
| `LockedOut` | `Login` | `not_locked` | `Open` | - |
| `LockedOut` | `Login` | - | = | - |

<!-- ANCHOR_END: table -->
