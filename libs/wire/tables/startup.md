The `startup` protocol's message and error tables, included by its owning page, [servers/init.md](../../../docs/servers/init.md).

<!-- ANCHOR: tables -->
<!-- wire: startup -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `startup` | `version: u32`, `handle_count: u32`, `namespace: bytes`, `handles: bytes`, `argv: bytes`, `image_addr: u64`, `image_len: u64` | - |

<!-- wire-errors: startup -->
| Code | Error |
| --- | --- |

<!-- ANCHOR_END: tables -->
