# The `consol` protocol

Owned by [servers/consoled.md](../../../docs/servers/consoled.md).

<!-- wire: consol ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `size` | - | `cols: u16`, `rows: u16` |
| 17 | `resize` | - | `cols: u16`, `rows: u16` |

<!-- wire-errors: consol -->
| Code | Error |
| --- | --- |
