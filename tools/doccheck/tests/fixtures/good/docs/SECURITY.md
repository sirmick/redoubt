# Security

| Property | Rule | Enforced in | Tested by | Status | Residual risks |
| --- | --- | --- | --- | --- | --- |
| Flow | [R1](kernel/ipc.md#r1-flow) (flow) | `demo/src/lib.rs` | `bench:smoke`, `host:demo::flows` | built | none known |
| Fair waiting | R2 (fair waiting) | `demo/src/lib.rs` | — | planned · M2 (usable shell) | starvation |
| Live handles | I1 (handles name live objects) | `demo/src/lib.rs` | `mutation:SkipCheck`, `fuzz:demo/parse` | built, partly tested | races |
