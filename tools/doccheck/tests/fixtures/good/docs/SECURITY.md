# Security

| Property | Rule | Enforced in | Tested by | Status | Residual risks |
| --- | --- | --- | --- | --- | --- |
| Flow | [R1](kernel/ipc.md#r1-flow) (flow) | `demo/src/lib.rs` | `bench:smoke`, `host:demo::flows`, `host:demo::slowly` | built, partly tested | none known |
| Fair waiting | R2 (fair waiting) | `demo/src/lib.rs` | — | planned · M2 (usable shell) | starvation |
| Live handles | I1 (handles name live objects) | `demo/src/lib.rs` | `mutation:SkipCheck`, `fuzz:demo/parse` | built, partly tested | races |
| Flow in the model | [I2 (every flow obeys R1)](kernel/invariants.md#i2-every-flow-obeys-r1) | `demo/src/lib.rs` | `host:demo::flows` | built | none known |
