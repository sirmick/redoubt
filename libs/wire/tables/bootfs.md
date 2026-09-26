The `bootfs` protocol's message and error tables, included by its owning page, [servers/bootfsd.md](../../../docs/servers/bootfsd.md).

<!-- ANCHOR: tables -->
<!-- wire: bootfs ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `add` | `name: string`, `offset: u64`, `data: bytes` | - |
| 17 | `seal` | - | - |

<!-- wire-errors: bootfs -->
| Code | Error |
| --- | --- |
| 2 | `refused` |

<!-- ANCHOR_END: tables -->
