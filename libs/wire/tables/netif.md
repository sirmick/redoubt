# The `netif` protocol

Owned by [servers/netd.md](../../../docs/servers/netd.md).

<!-- wire: netif -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `info` | - | `mac: u64`, `mtu: u32` |
| 2 | `transmit` | `frame: bytes` | - |

<!-- wire-errors: netif -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `too_many` |
| 4 | `busy` |
| 5 | `failed` |
