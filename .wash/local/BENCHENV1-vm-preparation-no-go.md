# BENCHENV1 VM preparation: stop at dynamic export boundary

2026-10-04. Assignment `54d143f435b9d1475a921a4d1e3b03f0` (BENCHENV1).
Preparation only: no image, container or VM build/launch/download/install; no test or
runtime retry; no source, host configuration, security setting, stopped preflight
container, or QEMU allocation changed. The real OpenSSH reference gate remains unrun.

## Decision

The mandatory first check fails for a **prelaunched** default-confined TCG VM with
an unchanged runner and a fixed, narrow writable QEMU filesystem export. Stop here
under the remote VM ruling. No outer/guest recipes, boot image selection, credential
setup or launch command is offered as a feasible attempt because the writable source
path needed by the guest does not exist or have a knowable name at VM launch. The
2-vCPU, 2-GiB RAM, 8-GiB disk, 20-minute ceiling is therefore not assessed.

## Source-derived path trace

1. `tools/testbench/src/main.rs` canonicalizes the compiled Cargo workspace and
   calls `Run::start(&workspace.join("target/testbench"))` before case execution
   (lines 175-193). In the existing outer dev-style container this workspace is
   an absolute `/work/...` path. It may be a worktree, so do not substitute the
   shared checkout's absolute path.
2. `tools/testbench/src/run.rs` creates
   `target/testbench/run-<process-id>-<current-time-nanoseconds>/` inside
   `Run::start` (lines 22-48). The name is generated at runtime and the bench
   exposes no flag or environment override for this root. `target/testbench/last`
   is a symlink for readers; `run.dir` retains the newly generated direct path.
3. The reference preflight calls `ssh::loopback_usable(&workspace,
   &logs.join("ssh"))`; it creates
   `<workspace>/target/testbench/run-.../ssh/loopback-probe/`.
   The case later calls `ssh::loopback` with the same `logs.join("ssh")` and the
   case name `bench-ssh-loopback-openssh`, creating
   `<workspace>/target/testbench/run-.../ssh/bench-ssh-loopback-openssh/`
   (`main.rs` lines 340-361, 885-900; `ssh.rs` lines 280-303, 335-366).
4. Each reference container's unchanged `ProxyCommand` bind-mounts exactly
   `sshd_config`, `authorized_keys`, `loopback-host` (`ro,z`) and `sshd.log`
   (`z`) from one such directory, at that **direct absolute path** (`ssh.rs`
   lines 255-270, 322-325). `sshd.log` must be a host-visible file shared by
   concurrent sessions. The host also writes per-run locks, transcripts, client
   keys and known-hosts files elsewhere in the run, which are outside the
   reference container's four mounts (`run.rs`, `ssh.rs` lines 400-475).

## Why the proposed export cannot be narrowed here

A fixed QEMU share of `<workspace>/target/testbench` would contain the entire
current and prior bench runs: locks, unrelated case logs, client credentials and
other work product. It is the broader writable run-tree authority that the
Architect's `.wash/local/BENCHENV1-remote-vm-ruling.md` forbids. A fixed share of
`<workspace>/target/testbench/last` does not help: the runner passes the direct
`run-...` pathname to Podman, not `last`, and the symlink resolves into the broader
run tree. A narrow share rooted at `.../run-.../ssh/loopback-probe` or at the case
directory requires the unpredictable name and directories to exist first. They do
not exist until `cargo testbench` is already running. Exporting the worktree or
whole run, redirecting paths with an added mirror/synchronizer, or modifying the
runner would cross the stated scope and ruling. No launch ordering or file-share
backend can be claimed as a verified workaround from the present source evidence.

The independent read-only build context is stable:
`<workspace>/tests/ssh-reference/Containerfile` in
`<workspace>/tests/ssh-reference/`. Its Docker base digest and OpenSSH package pin
remain those in the unchanged Containerfile. This does not solve the dynamic
writable case path. Guest Podman version, maps, SSH transport, `:z` behavior,
ownership and log semantics therefore remain unverified; the earlier container's
UID-map EPERM does not authorize another probe.

## Checked summaries and state

- `docs/testbench.md`, “Sessions and the loopback server”: accurately describes
  the current image, per-file mounts, rootless Podman and log semantics. No change:
  no new environment or gate result exists.
- `GETTING-STARTED.md`, “Prerequisites” and “Test”: the prior preflight already
  records that its dev-container prerequisite claim is overbroad for this gate.
  This no-go preparation establishes no replacement setup; update only with a
  reviewed environment change, not an unsupported invocation.
- `README.md`, “Today” and M1, and `docs/plan/m1-separation.md`, M1 progress:
  product integration claims are unchanged by this blocked host environment.
- `docs/README.md`, book/testbench pointers: unchanged and still accurate.

Read for this checkpoint: `.wash/README.md`; `.wash/SWARM.md` implementer,
two-tiers, pages, staging/commits/handoffs sections;
`.wash/local/BENCHENV1-handoff.md`, `BENCHENV1-preflight.md`,
`BENCHENV1-mapping-continuation.md`, `BENCHENV1-remote-vm-ruling.md`;
the named source/case/summary passages above. No tests were run, as assigned.
The stopped `redoubt-bench-env-preflight` container must remain stopped. The only
new file is this ignored local report; no commit, push, stage or stash was made.
