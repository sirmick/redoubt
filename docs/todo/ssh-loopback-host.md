# The SSH loopback self-checks on this host

## What

Five of the six `bench-ssh-loopback*` cases fail on the development host: every session that logs
in ends with `/bin/bash: Permission denied` or `Broken pipe`, before its first step, so the cases
fail for a reason their `must_fail` pattern does not name. Only `bench-ssh-loopback-deadlock`,
which never gets a shell, passes. The loopback server is the host's OpenSSH `sshd`, started by
`ssh` itself in inetd mode for each session; on this host it cannot start the user's login shell.

## Why it matters

These cases are the self-checks of the bench's SSH session runner: that `expect`, `forbid`,
`wait`, exit statuses and host keys each fail when they should. While they fail for the wrong
reason, a broken session runner would go unnoticed, and every future SSH attack case rests on it
([the test bench](../testbench.md#ssh-sessions)).

## Where

- [`tools/testbench/src/ssh.rs`](../../tools/testbench/src/ssh.rs): the loopback server's
  configuration (`ForceCommand /bin/sh`) and how `sshd -i` is started.
- [`tests/bench-ssh-loopback.toml`](../../tests/bench-ssh-loopback.toml) and its siblings.

## Done when

- The six cases pass on the development host, or the bench reports them as skipped with the
  reason when the host cannot run a loopback `sshd`, and a host that can runs them in every full
  bench run.
