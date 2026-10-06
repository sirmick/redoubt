The `consol` protocol's message and error tables, included by its owning page, [servers/consoled.md](../../../docs/servers/consoled.md).

<!-- ANCHOR: tables -->
<!-- wire: consol ninep -->
| Opcode | Kind | Message | Fields | Reply |
| --- | --- | --- | --- | --- |
| 16 | call | `size` | - | `cols: u16`, `rows: u16` |
| 17 | call | `resize` | - | `cols: u16`, `rows: u16` |
| 18 | send | `ended` | - | - |

<!-- wire-errors: consol -->
| Code | Error |
| --- | --- |

<!-- ANCHOR_END: tables -->
