# The SSH loopback self-checks on this host

## What

The OpenSSH reference case, `bench-ssh-loopback-openssh`, cannot run on the development host while
the bench runs as part of a system service. The host is Fedora with SELinux enforcing, and the
agents' bench runs in `system_u:system_r:unconfined_service_t`. The loopback server is the host's
OpenSSH `sshd`, started by `ssh` itself in inetd mode; with SELinux enabled it always moves the
login shell into the user's default context (`unconfined_u:unconfined_r:unconfined_t`), which the
service's context may not enter, so the shell fails with `/bin/bash: Permission denied`. No
`sshd_config` option turns that off. The bench probes for this before the first OpenSSH loopback
case and fails each case with the reason ([SSH sessions](../testbench.md#ssh-sessions)), or skips it
under `--allow-skip`.

## Why it matters

The bench's self-checks of its SSH session runner run on Redoubt's own `sshd` on its host
platform, which needs no login context ([against Redoubt's sshd](../testbench.md#against-redoubts-sshd)).
The reference case is their independent witness: a bug the runner shared with Redoubt's server
would pass every self-check, and only a run against OpenSSH's server would show it. While it
cannot run, that witness is missing.

## Where

The host, not the tree: how the bench is started, or its SELinux policy.

## Done when

- The owner has chosen a host change (run the bench from a login session, or a local policy
  module that lets the service's context start the user's shell), and the reference case passes
  on the development host in a full bench run.
