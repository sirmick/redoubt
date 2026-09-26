# The `net_ctl` protocol

Owned by [servers/ipd.md](../../../docs/servers/ipd.md).

<!-- wire: net_ctl -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `connect` | `addr: bytes`, `port: u16` | - |
| 2 | `listen` | `port: u16`, `backlog: u8` | - |
| 3 | `close` | - | - |
| 4 | `abort` | - | - |

<!-- wire-errors: net_ctl -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `in_use` |
| 4 | `too_many` |
| 5 | `state` |
| 6 | `unreachable` |
| 7 | `refused` |
| 8 | `timeout` |
