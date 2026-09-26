# M3 (files in and out)

## Goal

Files move in and out of the box by SFTP and SCP inside SSH, confined to the session's
capabilities and audited. There is no other inbound service and no other way in.

- A transfer runs in a transfer server the steward starts for one unlabelled session's channel, in
  a budget carved from the session's, with exactly the session's file binds.
- A vault session gets no transfers: what leaves a label leaves by declassification.
- Every operation is one signed record in the audit log, which begins here.

## Attack suite

- **Confined to the session's binds.** A path-escape suite (`..`, absolute paths, long and odd
  names) reaches only the session's binds, and a second principal's files are unreachable
  ([sshd](../servers/sshd.md#files-in-and-out),
  [transfer](../userland/transfer.md#confined-to-the-sessions-files)).
- **Only what 9P can say.** Symlink, chmod and chown requests are refused, and so is the old SCP
  protocol (`scp -O`), which runs a command on the server.
- **No transfers from a vault.** A subsystem request on a vault channel is refused
  ([R67 (a channel keeps its labels)](../servers/sshd.md#r67-a-channel-keeps-its-labels),
  [transfer](../userland/transfer.md#no-transfers-in-a-vault-session)).
- **Every operation is audited once.** Each open, close (with its byte count), remove, rename,
  mkdir, rmdir and setstat yields exactly one record, signed through `keyd`'s audit purpose, naming
  the principal from the badge the steward minted, never the transfer server's claim
  ([the steward](../servers/steward.md#the-transfer-audit-log)).
- **The audit key signs nothing else.** The steward's `keyd` grant signs audit records and no
  caller-chosen bytes
  ([R44 (one key, one purpose, keyd's own digest)](../servers/keyd.md#r44-one-key-one-purpose-keyds-own-digest)).

## Remaining work

In this order, after [M2 (usable shell)](m2-usable-shell.md):

1. **The transfer audit log**: the steward's append-only file of signed records
   ([the steward](../servers/steward.md#the-transfer-audit-log)).
2. **The transfer server**: SFTP over the session's binds, started by the steward on `sshd`'s
   request ([sshd](../servers/sshd.md#files-in-and-out)).
3. **SCP** served as SFTP, which current `scp` clients speak; the old protocol and every transfer
   on a vault channel refused ([transfer](../userland/transfer.md)).

## Progress

Nothing of this milestone is built. What it builds on: `sshd`'s sessions and channels and the
steward, from [M1 (separation and containment)](m1-separation.md), and `keyd`'s `audit` purpose,
which is built and tested in host tests ([keyd](../servers/keyd.md#keys-and-purposes)).
