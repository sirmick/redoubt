# Owner choice: the OpenSSH reference runner

Prepared for assignment `8d3a624180f194a6028b7609fdc9a591`, QA
`BENCHENV1-container-permissions`. Proposal only: no implementation, installation,
launch, test or host/security change. Earlier stop-on-failure approvals are exhausted.

## Decision to present

**Recommendation: retain rootless isolation and supply a compatible native runner.**
The local rootless mapping attempts failed, and the bounded remote-VM proposal
stopped at its dynamic export-path incompatibility. No more environment variants
are proposed. The alternative below is technically concrete but grants substantially
more host authority than a narrowly confined reference container.

Owner question, ready for `decision_request` on the QA thread:

> Which reference-runner path should we implement? Retain rootless Podman and supply
> a compatible native runner (recommended), or approve an explicit Docker exception
> for the pinned OpenSSH reference test, including mounting the existing rootful
> Docker socket into a dedicated dev-based bench container? The latter grants the
> entire bench invocation root-equivalent host authority; the socket cannot enforce
> “this pinned test only.” The implementation, invocation and gates below define the
> authorized operational scope. Neither option waives the reference gate.

Options:

- **Retain rootless runner (recommended):** supply a named native environment where
  the bench user's Podman mappings and reference case work. Preserve the current
  authority rule; BEAM7 acceptance waits for that runner and required evidence.
- **Approve explicit Docker exception:** authorize the bounded source/docs work,
  supplemental CLI image preparation and reviewed dedicated invocations below,
  accepting the loss of the bench container's host-isolation boundary while its
  socket is mounted. This offers a local route to the missing gate, without a VM,
  but it is not permission for unrelated daemon operations or host changes.

## Option 1: retain the current rule

Supply a concrete native runner with rootless Podman/uidmap/subids, working
cgroup/storage configuration, the pinned image build prerequisites and the project
toolchain. The current hardcoded Podman runner can remain. First pass the unchanged
focused reference case, then obtain the required whole-bench evidence on the exact
reviewed content, with the orchestrator coordinating machine resources. A focused
result alone does not silently replace the currently required whole-bench gate.
No compatible native runner is identified yet, so this option has no promised date.

## Option 2: exact implementation and owning-rule delta

- `tools/testbench/src/ssh.rs`: introduce one explicit backend selector,
  `TESTBENCH_SSH_REFERENCE_BACKEND=podman|docker-rootful`, defaulting to `podman`.
  Reject unknown values; never auto-fallback after a Podman failure. Preserve the
  Podman `sg`/cgroupfs path. Docker uses the fixed local Unix endpoint
  `/var/run/docker.sock`, without `sg` or Podman's cgroup flag. Distinguish missing
  CLI/daemon access, absent image and broken build; daemon failure must not masquerade
  as a missing image. Keep the recipe-hash tag; record and use the resolved image ID
  for sessions so a tag change during the run cannot change the server. Log backend,
  image identity and the rootful authority warning in the run evidence.
- The Docker backend implements only the existing reference image check/build and
  session proxy, not a general command adapter. Preserve existing shell-argument
  validation and add focused host checks for selection, exact command/mount rendering,
  failures and quoting through `cargo testbench`. Keep readiness/version/session
  verdict behavior unchanged. Add a narrow per-invocation label to reference
  containers for cleanup after timeout/interruption; no broad daemon cleanup.
- `docs/testbench.md`, “Sessions and the loopback server”: keep rootless as default
  and add the explicitly selected trusted-host exception. Proposed rule wording:
  “The default reference backend is rootless Podman: container root maps to the
  bench user. An explicitly selected Docker-rootful backend may run this pinned
  OpenSSH reference and its version probe on a trusted development host. It does
  not provide that UID mapping or confine the bench launcher to user authority.
  Access to its rootful daemon grants host-root-equivalent authority to the bench
  invocation. All four mounts, networkless sessions, pins and verdicts still apply.”
  This changes a host harness rule, not Redoubt's guest capability/label guarantees;
  do not claim the tenets make the host daemon safe.
- `GETTING-STARTED.md`: state the reference prerequisite and document the exceptional
  invocation separately. Add `scripts/testbench-reference-docker.sh` and the
  supplemental `tests/ssh-reference/Dockerfile.bench-client` (the existing
  `Containerfile` stays byte-identical). The image derives from the current dev image
  with version-recorded Docker CLI/build support only, no nested daemon. Keep
  `dev.sh` and its no-socket default unchanged. No interactive/agent session receives
  the socket through this workflow.

## Concrete topology for the existing dev whole bench

The host's existing rootful daemon creates the reference containers as **siblings**
of a fresh dedicated dev-based bench container. No daemon runs inside that container.
Use its existing nonroot UID/GID `1447391350:1447391350`, adding only the numeric
group owning the daemon socket. Do not chmod/chown the socket or change host groups.

Mount the project at the **same canonical absolute path on host and client**:
`/home/mcloonan/redoubt:/home/mcloonan/redoubt:z`; set the working directory to
`/home/mcloonan/redoubt/.worktrees/BEAM7`. Set CARGO_HOME, RUSTUP_HOME and the two
firmware variables to their corresponding absolute paths beneath that project root.
This replaces `/work` for this dedicated invocation, avoiding daemon-side path
translation for the runtime-generated `target/testbench/run-.../ssh/...` files.
Fail if source paths are not ordinary existing files under the canonical case tree.

The only additional authority-bearing mount is
`/var/run/docker.sock:/var/run/docker.sock`. Do not relabel that host socket; if
existing confinement prevents its use, stop rather than disable confinement.
Use a fixed `--host unix:///var/run/docker.sock` client endpoint. Do not expose an
API over TCP or inherit an arbitrary remote Docker context. Do not mount `/config`,
agent state, host SSH keys or the rest of the home directory. Retain default outer
seccomp/AppArmor, no privileged flag, added capabilities or devices. The outer bench
can use its existing offline network mode; the daemon's pinned image build separately
needs registry/package network access. Offline outer networking does not restrict
what the rootful daemon can do.

For every reference session, retain precisely:

| Host case file | Container destination | Mode |
| --- | --- | --- |
| `sshd_config` | `/case/sshd_config` | `ro,z` |
| `authorized_keys` | `/case/authorized_keys` | `ro,z` |
| `loopback-host` | `/case/loopback-host` | `ro,z` |
| `sshd.log` | `/case/sshd.log` | `z` (writable) |

The backend runs `docker ... run -i --rm --network=none --pull=never`, these four
mounts and the resolved reference image, with
`/usr/sbin/sshd -i -f /case/sshd_config -E /case/sshd.log`.
The reference containers receive neither the socket nor the project tree. Keep
default container security options, no extra privileges/devices, no published port.
Keep root login, forced `/bin/sh`, forbidden forwarding and all existing restrictions.
Keep the current base digest, `openssh-server=1:10.0p1-7+deb13u4`, recipe-hash naming,
real OpenSSH version probe, concurrent sessions, PTY, refused key and exit checks.

## Authority and practical limitations

**The whole bench container becomes a trusted host administrator for this run.**
Direct Docker API access is not restricted to those generated commands: any process
in the container able to use the socket can ask for host mounts or privileged
containers. This includes testbench/build code and a compromised host-side test tool
or emulator. Guest test payloads have no direct socket, but that is not a defense
against compromise of their host tools. A read-only socket mount would not make the
API read-only. Existing Docker-group membership authorizes none of this by itself.

The four session mounts and `--network=none` constrain each reference container,
not its launcher. With this rootful daemon and no remapping, reference UID 0 is
host UID 0, subject to the container's remaining namespaces/capability/LSM controls.
This changes the precise identity guarantee; it is not merely a CLI substitution.
If the owner requires a technically enforced “pinned test only” daemon capability,
option 2 as specified is unsuitable. An authorization broker would be a different
project, and is not proposed here.

Docker documents the daemon authority and daemon-side bind-source semantics:
[daemon attack surface](https://docs.docker.com/engine/security/#docker-daemon-attack-surface),
[bind mounts](https://docs.docker.com/engine/storage/bind-mounts/).
Local read-only inspection finds `/usr/bin/docker` dynamically linked to host libc;
do not assume bind-mounting that executable into the dev image provides a supported
client. The supplemental CLI image is an implementation prerequisite, not built or
validated here. Exact default-confinement socket access, SELinux label compatibility,
image build, file ownership and cancellation cleanup remain empirical gates.

## Review, execution and cleanup gates

1. Actual owner selection is recorded on QA before the owning-rule change or backend
   implementation. Only approval of option 2 authorizes its socket-bearing workflow.
2. Implementer prepares source, documentation, image/launcher recipe and exact
   invocation. Red-team review covers full socket authority, fail-closed backend
   selection, canonical path identity, four mounts, pins, logs and cleanup. Relevant
   host checks run through `cargo testbench`; no permission-bearing runtime smoke
   precedes that review. Do not bundle unrelated reference or guest policy changes.
3. Prepare the supplemental CLI image and run one bounded focused
   `cargo testbench bench-ssh-loopback-openssh` in the dedicated container on the
   reviewed snapshot. Record actual UID/daemon/image/mount/network/security settings,
   file ownership, version and all session verdicts. Stop on first failure; no
   security relaxation, chown of the worktree, pin update or alternative backend.
4. On focused success and orchestrator resource release, run the normal
   `cargo testbench` whole bench for BEAM7 on its final reviewed content, with the
   same explicit backend. Product QEMU remains in the outer dev environment. Required
   failures and final review still gate acceptance; no `--allow-skip` substitution.
5. `--rm` removes completed session and outer containers. On interruption, stop/remove
   only reference containers recorded for this invocation and its dedicated outer
   container. Verify none remain before declaring cleanup complete. Preserve logs
   and image hashes; retain reusable images unless their removal is separately
   intended. Never daemon-wide prune, remove others' containers, or modify the
   previously stopped preflight container under this authorization.

Both paths keep BEAM7 acceptance blocked until actual reference and required final
gate evidence exists. Option 1 needs a supplied runner; option 2 needs a reviewed
backend and knowingly trusted host-admin bench invocation. This proposal supplies
no PASS, schedule guarantee, or authority beyond the eventual explicit owner choice.
