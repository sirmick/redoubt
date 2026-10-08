The steward's protocol's message and error tables, included by its owning page, [servers/steward.md](../../../docs/servers/steward.md).

A label set travels as `bytes`: each label id as eight bytes, little-endian, in any order. Each
message is accepted only on its badge class: `sshd`'s root badge (`login`, `channel_closed`,
`approval_opened`, `approval_closed`, `watch`), the approval channel's (`pending`, `approve`, `deny`),
`init`'s (`blame`), and a session's or an agent's minted badge (`submit`, `start_agent`,
`end_lease`, `end_session`); on any other it is malformed, as an unknown opcode is.

<!-- ANCHOR: tables -->
<!-- wire: steward -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `login` | `principal: string`, `label: string`, `key: bytes`, `console: handle[0] endpoint` | `session: u64`, `name: string`, `labels: bytes` |
| 2 | `channel_closed` | `session: u64` | - |
| 3 | `approval_opened` | `principal: string`, `key: bytes` | `channel: u64` |
| 4 | `approval_closed` | `channel: u64` | - |
| 5 | `pending` | `channel: u64` | `request: u64`, `hash: bytes`, `labels: bytes`, `text: string` |
| 6 | `approve` | `channel: u64`, `request: u64`, `hash: bytes` | - |
| 7 | `deny` | `channel: u64`, `request: u64` | - |
| 8 | `blame` | `account: u64`, `labels: bytes`, `server: string` | - |
| 9 | `submit` | `kind: u32`, `labels: bytes`, `lease: u64`, `item: u64`, `source: u64`, `what: string`, `reason: string` | `request: u64` |
| 10 | `start_agent` | `lease: u64` | `lease: u64`, `name: string` |
| 11 | `end_lease` | `lease: u64` | - |
| 12 | `end_session` | - | - |
| 13 | `watch` | - | - |

<!-- wire-errors: steward -->
| Code | Error |
| --- | --- |
| 2 | `unknown` |
| 3 | `bad_key` |
| 4 | `not_owner` |
| 5 | `labelled` |
| 6 | `cap` |
| 7 | `too_big` |
| 8 | `not_printable` |
| 9 | `not_rendered` |
| 10 | `hash_mismatch` |
| 11 | `bad_lease` |
| 12 | `locked_out` |
| 13 | `not_sponsor` |
| 14 | `failed` |

<!-- ANCHOR_END: tables -->
