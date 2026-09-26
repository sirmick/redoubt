The `fsd` protocol's message and error tables, included by its owning page, [servers/fsd.md](../../../docs/servers/fsd.md).

<!-- ANCHOR: tables -->
<!-- wire: fsd ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `rename` | `old_dir: u32`, `old_name: string`, `new_dir: u32`, `new_name: string` | - |
| 17 | `copy_file` | `src_fid: u32`, `dst_dir: u32`, `dst_name: string` | `count: u64` |
| 18 | `set_attr` | `fid: u32`, `attr: u8`, `value: bytes` | - |
| 19 | `get_attr` | `fid: u32`, `attr: u8` | `value: bytes` |

<!-- wire-errors: fsd -->
| Code | Error |
| --- | --- |
| 2 | `not_found` |
| 3 | `refused` |
| 4 | `exists` |
| 5 | `not_dir` |
| 6 | `removed` |
| 7 | `too_large` |

<!-- ANCHOR_END: tables -->
