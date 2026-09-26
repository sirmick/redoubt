The `keyd` protocol's message and error tables, included by its owning page, [servers/keyd.md](../../../docs/servers/keyd.md).

<!-- ANCHOR: tables -->
<!-- wire: keyd -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `sign_ssh_exchange` | `v_c: bytes`, `v_s: bytes`, `i_c: bytes`, `i_s: bytes`, `q_c: bytes`, `q_s: bytes`, `k: bytes` | `signature: bytes` |
| 2 | `sign_record` | `record: bytes` | `signature: bytes` |
| 3 | `public_key` | - | `key: bytes` |
| 4 | `holds` | `key: bytes` | `held: u32` |
| 5 | `grant` | - | `id: u64`, `capability: handle[0] endpoint` |
| 6 | `release` | `id: u64` | - |

<!-- wire-errors: keyd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `too_many` |
| 4 | `failed` |

<!-- ANCHOR_END: tables -->
