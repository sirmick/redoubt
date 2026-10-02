The steward's `crossing` machine: one item moved between a labelled domain and the unlabelled
domain of the same account, its kind fixed when it opens. A declassification's read (at
submission) and a push's write (on approval) go through a budget carrying exactly the labelled
side's label set and a deadline; a declassification's copy out (on approval) is the steward's own
write of the snapshot to the unlabelled volume, with no budget. Its records are stamped with the
labelled side's domain. Included by [the steward's page](../../../docs/servers/steward.md#machines);
the notation is in [README.md](README.md).

<!-- ANCHOR: table -->
<!-- steward: crossing -->
| From | Event | Guard | To | Effects |
| --- | --- | --- | --- | --- |
| - | `Open` | `reading` | `Open` | `carve_crossing`, `read_item`, `destroy_crossing` |
| - | `Open` | `copying` | `Open` | `copy_out` |
| - | `Open` | - | `Open` | `carve_crossing`, `write_item`, `destroy_crossing` |
| `Open` | `Done` | `reading` | `Closed` | `pass_snapshot` |
| `Open` | `Done` | `copying` | `Closed` | `audit_declassified` |
| `Open` | `Done` | - | `Closed` | `audit_pushed` |
| `Open` | `Failed` | `reading` | `Closing` | `pass_failure`, `destroy_partial` |
| `Open` | `Failed` | `copying` | `Closed` | `audit_copy_failed` |
| `Open` | `Failed` | - | `Closing` | `audit_push_failed`, `destroy_partial` |
| `Open` | `Exited` | - | = | `unreachable` |
| `Closing` | `Done`, `Failed` | - | `Closed` | - |
| `Closing` | `Exited` | - | = | `unreachable` |

<!-- ANCHOR_END: table -->
