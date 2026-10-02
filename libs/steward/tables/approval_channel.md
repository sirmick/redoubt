The steward's `approval_channel` machine: an `ssh approve@box` connection that `sshd`
authenticated with one of a principal's approval keys. It belongs to the principal across its
label sets; `Pending`, `Approve` and `Deny` on it are the request machine's. Included by
[the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: approval_channel -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `ApprovalOpened` | `!approval_key` | - | `refuse` |
| - | `ApprovalOpened` | - | `Open` | `reply_ok` |
| `Open` | `ApprovalOpened` | - | = | `refuse` |
| `Open` | `ApprovalClosed` | - | `Closed` | - |

<!-- ANCHOR_END: table -->
