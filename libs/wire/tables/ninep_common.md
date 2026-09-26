The `ninep_common` protocol's message and error tables, included by its owning page, [servers/wire.md](../../../docs/servers/wire.md).

<!-- ANCHOR: tables -->
<!-- wire: ninep_common -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 2 | `new_connection` | `root: string`, `quota: u64` | `conn: handle[0] endpoint`, `id: u64` |
| 3 | `disconnect` | `id: u64` | - |

<!-- wire-errors: ninep_common -->
| Code | Error |
| --- | --- |
| 2 | `not_yours` |
| 3 | `refused` |

<!-- ANCHOR_END: tables -->
