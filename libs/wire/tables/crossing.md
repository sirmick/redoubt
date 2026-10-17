The crossing program's message and error tables, included by its owning page, [servers/steward.md](../../../docs/servers/steward.md).

The steward calls `crossing` once, on the endpoint it made and handed the program the receive
right of at launch, so the labelled program never starts a flow (kernel/ipc.md R1): `read` asks
for the item at `path`, below the root of the program's one connection, answered with its first
`READ_MAX` bytes at most (one past `DECLASSIFY_MAX`: the steward's core judges the size), and
`write` hands it the bytes to write as the item at `path` there (its directory must exist).
A second call, whichever, is `served`; the program exits once its one call is answered
(servers/steward.md, "Declassification and push").

<!-- ANCHOR: tables -->
<!-- wire: crossing -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `read` | `path: string` | `bytes: bytes` |
| 2 | `write` | `path: string`, `bytes: bytes` | - |

<!-- wire-errors: crossing -->
| Code | Error |
| --- | --- |
| 2 | `failed` |
| 3 | `served` |

<!-- ANCHOR_END: tables -->
