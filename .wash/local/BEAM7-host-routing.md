# BEAM7 host-test routing draft (MEM1 owns implementation)

`HostTests` in `tools/testbench/src/case.rs`: add `#[serde(default)] workspace:
Option<PathBuf>` and `#[serde(default)] features: Vec<String>`. A missing workspace means
the repository root; a missing feature list means no `--features` argument. Preserve
`deny_unknown_fields`.

In `Builder::cargo_test`, resolve `host.workspace` before launching Cargo. For `Some(rel)`,
reject an absolute path, empty component, `.`/`..` traversal, a symlink that escapes the
root, a missing directory, or a directory without `Cargo.toml`. Canonicalize both root
and requested path and require the latter to be inside the former. Propagate this as a
bench error; never fall back to the root after a requested workspace fails. Pass the
validated current directory to `test_command`. For nonempty features, append
`--features` with comma-joined entries. Existing package and test arguments stay as
they are. The root default command should be byte-for-byte unchanged.

Command-building tests: default HostTests runs from root with `test -p p` and no
features; `workspace="userland/otp"` runs from that directory; `features=["beamlet-redoubt/fake"]`
adds that feature; missing and invalid workspaces return an error before Cargo. A
case should prove the real tests execute, rather than merely seeing a successful
Cargo exit.

New cases after routing:

```
# tests/beamlet-lookup-host.toml
description = "A refused verified module or application stays refused at the VM boundary; absent names can use an authorized code path"
kind = "host-tests"
workspace = "userland/otp"
packages = ["beamlet-vm", "beamlet-redoubt"]
features = ["beamlet-redoubt/fake"]

# tests/beamlet-lookup-cli-host.toml
description = "The host beamlet lookup keeps its existing search and root confinement"
kind = "host-tests"
workspace = "userland/otp"
packages = ["beamlet"]
```

The VM and Redoubt case should run all test targets so it includes the VM unit
regression and Redoubt's fake-kernel `lookup` integration test. The CLI runs
separately because its default `threads` feature enables VM `std`.
