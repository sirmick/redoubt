# The SSH loopback self-checks on this host

## What

The six `bench-ssh-loopback*` cases cannot run on the development host while the bench runs as
part of a system service. The host is Fedora with SELinux enforcing, and the agents' bench runs
in `system_u:system_r:unconfined_service_t`. The loopback server is the host's OpenSSH `sshd`,
started by `ssh` itself in inetd mode; with SELinux enabled it always moves the login shell into
the user's default context (`unconfined_u:unconfined_r:unconfined_t`), which the service's context
may not enter, so the shell fails with `/bin/bash: Permission denied`. No `sshd_config` option
turns that off. The bench probes for this before the first loopback case and fails each case with
the reason ([SSH sessions](../testbench.md#ssh-sessions)), or skips it under `--allow-skip`.

## Why it matters

These cases are the self-checks of the bench's SSH session runner: that `expect`, `forbid`,
`wait`, exit statuses and host keys each fail when they should. While they cannot run, a broken
session runner would go unnoticed, and every future SSH attack case rests on it.

The self-checks are planned to move to Redoubt's own `sshd` on its host platform, which needs no
login context ([against Redoubt's sshd](../testbench.md#against-redoubts-sshd)). One reference
case stays on OpenSSH's `sshd` and keeps needing this.

## Where

The host, not the tree: how the bench is started, or its SELinux policy.

## Done when

- The owner has chosen a host change (run the bench from a login session, or a local policy
  module that lets the service's context start the user's shell), and the six cases pass on the
  development host in a full bench run.
