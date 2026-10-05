# BENCHENV1 remote rootless VM checkpoint

Assignment `4c678dbb9c692bd995b68101fb2bd515`, QA
`BENCHENV1-container-permissions`. Review of the final VM feasibility section in
`BENCHENV1-preflight.md` and the existing QA ruling. No source edit, launch,
permission change or test; this checkpoint is the only new artifact.

## Ruling

Conditionally admissible as an environment for the unchanged reference runner;
not established as equivalent, usable or authorized to execute by this review.
Rootless Podman inside a VM does not inherently weaken the existing rule. Reference
container root must map to the **guest bench account**, while QEMU runs as the
outer bench account. These are separate namespaces: equal numeric UIDs alone do
not establish equivalent host-file authority. Verify the guest mapping and the
effective access to exported files through the unprivileged QEMU process.

Preserve the pinned reference recipe/image, exact four per-file mounts with three
read-only and only the log writable, one networkless `sshd -i` per session, concurrent
session/log behavior and real version/verdict checks. A guest control channel and
build network do not themselves put a reference container on the network; that
separation must be demonstrated. The Podman API remains a guest-user Unix socket
accessed over authenticated SSH, with host-key verification and a dedicated protected
identity. Bind the SSH forward only to the outer container's loopback. Do not expose
an unauthenticated or rootful API. The control identity grants guest-account
execution authority, so its reachable exports matter as much as the session mounts.

The proposed writable **case subtree** is broader authority than the four reference
container mounts. Bound it to disposable reference-case artifacts, not the entire
bench run directory, binaries, unrelated logs, worktree or credentials. If unchanged
dynamic path creation cannot fit that boundary, stop. Read-only recipe/context
export and writable case export must remain disjoint; symlinks must not escape the
approved export roots. API/SSH credentials must not enter the reference container.

## Smallest preparation and proof sequence

Recommend at most one disposable feasibility attempt after an explicit execution
assignment. This is not a recommendation to develop a remote-runner framework.

1. Prepare one reproducible outer-image/guest-image recipe and exact launch/cleanup
   description in local scratch. Name immutable image inputs, compatible Podman
   versions, fixed CPU/RAM/disk/time limits, the two export roots, socket/key paths
   and the guest bench identity. Include only x86 TCG/guest boot prerequisites and
   the rootless Podman/SSH setup. Keep default outer confinement, no KVM/device or
   Docker socket, no host configuration changes, no new repository backend or shim.
   Preparation does not itself authorize a launch; current assignment is read-only.
2. In the single assigned setup, prove boot under that confinement, guest helper
   UID/GID maps, unprivileged service identity, guest cgroup/storage configuration
   and SSH socket access. Do not mistake the remote client's cgroup flag for guest
   configuration. Stop at the first failed prerequisite.
3. Before a full reference build, establish the narrow shared-path contract with
   disposable files at the actual absolute paths: ownership, read-only boundaries,
   guest-to-outer log visibility and concurrent append behavior. Then exercise the
   unchanged `sg`/environment and exact remote `image exists`, pinned `build -f`
   and per-session `run` invocations. Require actual `:z` handling and per-file
   mount behavior; file copies or a hidden synchronization layer are not equivalent
   evidence. No image pin changes or wider export to make this pass.
4. Only after those checks, run the unchanged focused
   `cargo testbench bench-ssh-loopback-openssh`, including version and all case
   verdicts. Then document the proven invocation/prerequisites and obtain review
   before using it for the outer whole bench. Coordinate QEMU resources separately;
   guest feasibility is not product-test or timing evidence.

## Stops, complexity and owner choices

The preflight has neither the required x86 VM image nor an established sharing
configuration. This is materially more work than selecting a remote endpoint.
The highest-risk integration is unchanged absolute case paths plus ownership,
labels and concurrent writable logs across the filesystem export. Do not declare
this the supported runner before the focused gate passes.

Stop if default confinement blocks boot, rootless mappings fail, exact commands
are incompatible, sharing changes permissions/labels or loses log semantics,
reference networking differs, or the predetermined resource/time budget is exceeded.
Do not iterate through filesystem backends, add a file mirror, change runner code,
remove `:z`, widen mounts, run Podman as root, expose host services or relax security
options within this attempt. If it requires those adaptations, it has ceased to be
the small environment-only solution; report the exact blocker and close this attempt.

Passing empirical gates requires no weaker identity/mount/network rule. Adoption
does require a reviewed owning-page clarification of the remote guest account,
transport, exports and residual trust, rather than pretending the backend is local.
A changed authority boundary or weakened property requires a concrete new rule and
actual owner decision before implementation. Ordinary preparation/testing that
preserves the boundary needs a scoped assignment, not a invented security exception.
The earlier two human approvals concerned the old container and its stopped probes;
they do not authorize this VM or transfer their confinement exceptions to it.

No owner tradeoff is being requested here. The recommendation is the bounded proof
above, with the missing reference gate and QA remaining blocking until it succeeds.
