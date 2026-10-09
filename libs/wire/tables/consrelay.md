The console relay's control protocol's message and error tables, included by its owning page, [servers/consrelay.md](../../../docs/servers/consrelay.md).

`attach` and `detach` are accepted only on the steward's control badge, and the relay serves them
on the endpoint it serves the session's `/dev/cons` on; `hello` is the relay's one message to the
steward, sent once on the hello badge the steward handed it at launch.

<!-- ANCHOR: tables -->
<!-- wire: consrelay ninep -->
| Opcode | Kind | Message | Fields | Reply |
| --- | --- | --- | --- | --- |
| 24 | call | `attach` | `note: string`, `console: handle[0] endpoint` | - |
| 25 | call | `detach` | `note: string` | - |
| 26 | send | `hello` | `vm: handle[0] endpoint`, `control: handle[1] endpoint` | - |

<!-- wire-errors: consrelay -->
| Code | Error |
| --- | --- |
| 2 | `failed` |

<!-- ANCHOR_END: tables -->
