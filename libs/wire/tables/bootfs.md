# The `bootfs` protocol

Owned by [servers/bootfsd.md](../../../docs/servers/bootfsd.md).

<!-- wire: bootfs ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `add` | `name: string`, `offset: u64`, `data: bytes` | - |
| 17 | `seal` | - | - |

<!-- wire-errors: bootfs -->
| Code | Error |
| --- | --- |
| 2 | `refused` |
