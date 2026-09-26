The `blkd` protocol's message and error tables, included by its owning page, [servers/blkd.md](../../../docs/servers/blkd.md).

<!-- ANCHOR: tables -->
<!-- wire: blkd -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `info` | - | `sectors: u64`, `sector_size: u32`, `read_only: u32` |
| 2 | `read` | `sector: u64`, `count: u32` | `data: bytes` |
| 3 | `write` | `sector: u64`, `data: bytes` | - |
| 4 | `flush` | - | - |

<!-- wire-errors: blkd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `out_of_range` |
| 4 | `too_many` |
| 5 | `failed` |

<!-- ANCHOR_END: tables -->
