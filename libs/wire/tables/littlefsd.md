The `littlefsd` protocol's message and error tables, included by its owning page, [servers/littlefsd.md](../../../docs/servers/littlefsd.md).

<!-- ANCHOR: tables -->
<!-- wire: littlefsd ninep -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 16 | `rename` | `old_dir: u32`, `old_name: string`, `new_dir: u32`, `new_name: string` | - |
| 17 | `copy_file` | `src_fid: u32`, `dst_dir: u32`, `dst_name: string` | `count: u64` |
| 18 | `set_attr` | `fid: u32`, `attr: u8`, `value: bytes` | - |
| 19 | `get_attr` | `fid: u32`, `attr: u8` | `value: bytes` |

<!-- wire-errors: littlefsd -->
| Code | Error |
| --- | --- |
| 2 | `not_found` |
| 4 | `exists` |
| 5 | `not_dir` |
| 6 | `removed` |
| 7 | `too_large` |
| 8 | `corrupt` |
| 9 | `no_space` |
| 10 | `not_permitted` |
| 11 | `not_supported` |
| 12 | `bad_name` |
| 13 | `read_only` |
| 14 | `no_memory` |
| 15 | `is_dir` |
| 16 | `not_empty` |

<!-- ANCHOR_END: tables -->
