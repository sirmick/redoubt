# BENCHENV1: proposed continuation after the mapping smoke failure

The owner approved one disposable container with seccomp and AppArmor unconfined,
the project mount and bridge network, with no privileged mode, host devices,
Docker socket or host configuration changes. The first run stopped as required
when direct `unshare -Ur` could not write its UID map. The container remains stopped.

Architect ruling `43708c6746e5d6fcc8b0f42f8b799611` establishes that this probe does
not exercise Podman's configured subordinate-ID mapping helpers. It does not
establish that Podman's mapping path fails. No additional permissions are proposed.

## Exact continuation scope

Verify the existing stopped `redoubt-bench-env-preflight` container still uses image
`sha256:fd04016ea9206bf839f6bdf28f3f30ab88a8ac1f6f262b2eb177a63d820cffc4`
and the settings recorded in `BENCHENV1-preflight.md`. Restart that same container
only after owner approval. As its existing dev UID/GID 1447391350, with its existing
HOME, runtime directory, vfs setting and primary group, run:

```sh
sg dev -c 'podman --log-level=debug --cgroup-manager=cgroupfs unshare cat /proc/self/uid_map /proc/self/gid_map'
```

Preserve full output and exit status. Require both maps to contain the intended
mapping of container root to the invoking user and the configured subordinate range:

```text
0 1447391350 1
1 3713496320 65536
```

A successful mapping probe proves only that mapping path. If successful, resume
the already documented vfs/cgroup checks, unchanged pinned OpenSSH image build and
`cargo testbench bench-ssh-loopback-openssh` from BENCHENV1-preflight.md. Reference
sessions remain networkless; build network use is unchanged. No QEMU or full suite
is included in this continuation.

On the first failed step or unexpected mapping, stop, preserve evidence and stop the
container. Do not add permissions, change host settings, substitute rootful Docker,
alter the runner or pin, skip gates, or retry. Podman may initialize container-local
state; this proposal is execution, not a read-only inspection. The completed image
and original dev environment remain unchanged.

This continuation is pending owner approval; writing this proposal authorizes no run.
