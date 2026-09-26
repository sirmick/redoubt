# The `ipd` protocol

Owned by [servers/ipd.md](../../../docs/servers/ipd.md).

<!-- wire: ipd ninep -->
| Opcode | Kind | Message | Fields | Reply |
| --- | --- | --- | --- | --- |
| 16 | call | `grant` | `scope: bytes` | `conn: handle[0] endpoint`, `id: u64` |
| 17 | send | `frame` | `frame: bytes` | - |

<!-- wire-errors: ipd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `too_many` |
