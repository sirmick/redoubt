# File transfer

Files come in and go out over SSH: SFTP and SCP, inside the same SSH connection a person logs in
with, and nothing else. A transfer is confined to the capabilities of the session it opens (the
same namespace, the same label set), so it can reach exactly the files the person could reach at
the prompt, and every transfer is recorded in the audit log. There is no FTP, no HTTP
upload, no shared network drive: SSH is the only inbound service.

## Purpose

A box people and agents work on needs data in (source trees, datasets, programs built elsewhere)
and results out. The usual tools for that (a file-sharing daemon, a web upload) are inbound
services, each a parser exposed to the network and each a second way in. Redoubt keeps one way in
and puts file transfer inside it, with the same walls as a session.

## How to use it

From the person's own machine, with their own SSH key:

```text
$ sftp alice@box
sftp> put report.csv /home/alice/in/report.csv
sftp> get /home/alice/out/results.txt
$ scp build/logscan alice@box:/home/alice/bin/logscan
$ scp alice+tax@box:/tax/2026/summary.pdf .      # from a vault, to Alice, who owns the label
```

Paths are the session's namespace paths: `/home/alice/...` is Alice's home volume, and a path no
entry of her namespace names does not exist. `chmod`, `chown`, `ln -s` and `ln` do nothing that
means anything, because the files have no mode bits, no owners and no links
([files and binds](files.md)).

## What it can and cannot do

### Transfers inside SSH

Status: planned · M3 (files in and out)

SFTP and SCP run inside an SSH connection to `sshd` (the SSH server), authenticated like a login,
with the person's own key, which never lives in `keyd` ([sshd](../servers/sshd.md)). The user name
picks the session the transfer belongs to, exactly as for a login: `alice@box` is Alice's
ordinary session, `alice+tax@box` a vault session carrying her `tax` label
([sessions](sessions.md)). `approve@box` offers no transfer.

Inbound traffic other than SSH is refused, so SFTP and SCP are the only way files arrive from
outside; there is no other file service to attack.

**Open:** where the SFTP and SCP servers run. The recommendation: `sshd` only relays the channel's
bytes, and the file protocol is served by a process the steward starts for the connection inside
the session's budget, with exactly the session's namespace and label set, so a bug in the SFTP
parser reaches only what the session already could. The alternative serves SFTP from inside the
session's own Elixir VM with OTP's SFTP daemon over `File`: less code, with the SSH subsystem
parser in the session's VM.

### Confined to the session

Status: planned · M3 (files in and out)

A transfer can do what its session can, and nothing more.
- **The same namespace.** Paths resolve through the session's namespace, and `..` is cleaned
  lexically, so no path climbs out of a connection's root. A path whose prefix names nothing is
  "no such file", not "permission denied".
- **The same labels.** A transfer in a vault session carries the vault's label: it can read the
  labelled volume and the unlabelled home, and write only to volumes with exactly that label
  ([files](files.md#labels-on-files)). Bytes it sends to the person's machine leave on a channel
  `sshd` keeps labelled, to the principal who owns the label.
- **The same budget.** A transfer's pages, processes and CPU are charged to the session's
  budget; a large upload cannot spend anyone else's. A volume's byte quota is the file server's,
  and a full volume refuses the write.
- **The same file rules.** A rename between two volumes is a copy and a remove; removing an open
  file succeeds; there are no symbolic links to follow.

**Open:** none.

### Audited

Status: planned · M3 (files in and out)

Every transfer is recorded in the audit log with the principal, the session it belongs to and the
paths it touched. The audit log is the steward's, from M4 (self-hosted development)
([the steward](../servers/steward.md)).

**Open:** which operations are recorded. The recommendation: each file opened for writing, each
remove, each rename and each directory made, with each file opened for reading recorded as well in
a vault session, where what leaves matters most. And what records transfers in
M3 (files in and out), which comes before the audit log of M4 (self-hosted development).

## Why

**One way in.** Every inbound service is a parser the whole network can reach. SSH is needed for
logins anyway; putting file transfer inside it adds a subsystem behind authentication, not a new
listener in front of it.

**A transfer is a session.** If file transfer had its own authority (a service account, a
drop directory writable by everyone), it would be a way around the session's walls: upload to a
place you could not write from the prompt, or read what a vault forbids. Making each transfer run
with exactly a session's namespace and labels means there is one set of walls to reason about,
and the attack cases for sessions cover it.
