The steward's `request` machine: a request a session or an agent submits to the powerbox, in
the requester's domain, frozen until answered. It asks for a labelled agent, a declassification,
a push or nothing the steward starts (a note), fixed at submission. Its records are stamped with
its audit domain, also fixed at submission: the target's for a labelled agent or a push, the
requester's otherwise. Its `Approve` rows are the request and approval path: their guards read
the approver and, for a grant, the domain the grant starts in (`not_locked` holds for a request
that starts no lease), and a refusal answers only the approval channel. What an approval starts
(a lease, a crossing) belongs to the machine it starts, so `Approved` is final. Included by
[the steward's page](../../../docs/servers/steward.md#machines); the notation is in
[README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: request -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `Submit` | `!owns_labels` | - | `refuse` |
| - | `Submit` | `!exact_labels` | - | `refuse` |
| - | `Submit` | `!lease_bounded` | - | `refuse` |
| - | `Submit` | `!pending_cap` | - | `refuse` |
| - | `Submit` | `!fair_share` | - | `refuse` |
| - | `Submit` | `declassifies` | `Snapshotting` | `open_read` |
| - | `Submit` | `pushes` | `Snapshotting` | `read_source` |
| - | `Submit` | - | `Frozen` | `freeze`, `audit_submitted`, `reply_request`, `notify` |
| `Snapshotting` | `Done` | `declassifies`, `!item_fits` | `Dropped` | `refuse` |
| `Snapshotting` | `Done` | - | `Frozen` | `freeze`, `audit_submitted`, `reply_request`, `notify` |
| `Snapshotting` | `Failed` | - | `Dropped` | `refuse` |
| `Snapshotting` | `Pending` | - | = | - |
| `Snapshotting` | `Approve`, `Deny` | - | = | `refuse` |
| `Snapshotting` | `SessionEnded` | - | `Dropped` | - |
| `Frozen` | `Pending` | `!may_see` | = | - |
| `Frozen` | `Pending` | - | `Rendered` | `render` |
| `Frozen` | `Approve`, `Deny` | - | = | `refuse` |
| `Frozen`, `Rendered` | `SessionEnded` | - | `Dropped` | - |
| `Frozen`, `Rendered` | `Done`, `Failed` | - | = | `unreachable` |
| `Rendered` | `Pending` | `!may_see` | = | - |
| `Rendered` | `Pending` | - | = | `render` |
| `Rendered` | `Approve` | `!rendered_here` | = | `refuse` |
| `Rendered` | `Approve` | `!hash_matches` | = | `refuse` |
| `Rendered` | `Approve` | `!approver_holds` | = | `refuse` |
| `Rendered` | `Approve` | `!not_locked` | = | `refuse` |
| `Rendered` | `Approve` | `grants_lease` | `Approved` | `audit_approved`, `reply_ok`, `grant_lease` |
| `Rendered` | `Approve` | `declassifies` | `Approved` | `audit_approved`, `reply_ok`, `open_copy_out` |
| `Rendered` | `Approve` | `pushes` | `Approved` | `audit_approved`, `reply_ok`, `open_write` |
| `Rendered` | `Approve` | - | `Approved` | `audit_approved`, `reply_ok` |
| `Rendered` | `Deny` | `!rendered_here` | = | `refuse` |
| `Rendered` | `Deny` | - | `Denied` | `audit_denied`, `reply_ok` |

<!-- ANCHOR_END: table -->
