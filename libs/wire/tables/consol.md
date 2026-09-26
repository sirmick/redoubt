The `consol` protocol's message and error tables, included by its owning page, [servers/consoled.md](../../../docs/servers/consoled.md).

<!-- ANCHOR: tables -->
<!-- wire: consol ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `size` | - | `cols: u16`, `rows: u16` |
| 17 | `resize` | - | `cols: u16`, `rows: u16` |

<!-- wire-errors: consol -->
| Code | Error |
| --- | --- |

<!-- ANCHOR_END: tables -->
