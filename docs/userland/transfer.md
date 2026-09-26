# File transfer

Files come in and go out over SSH: SFTP, and SCP served as SFTP, inside the same SSH connection a
person logs in with, and nothing else. A transfer runs in a small transfer server the steward starts
for the channel, holding only the session's file binds and its audit connection, so it can reach no
file the person could not reach at the prompt, and every operation it performs is recorded in the
audit log. Vault sessions have no file transfer at all. There is no FTP, no HTTP upload and no
shared network drive: SSH is the only inbound service.

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
$ scp build/logscan alice@box:/home/alice/bin/logscan     # OpenSSH 9 and later: SFTP underneath
$ scp -s build/logscan alice@box:/home/alice/bin/         # older clients: ask for SFTP explicitly
```

Paths are the session's namespace paths: `/home/alice/...` is Alice's home volume, and a path no
entry of her namespace names does not exist. `ln -s`, `chmod` and `chown` are refused, because the
files have no links, mode bits or owners ([files and binds](files.md)). `scp -O` (the old SCP
protocol) and `sftp alice+tax@box` (a vault session) are refused.

## What it can and cannot do

### Transfers inside SSH

Status: planned · M3 (files in and out)

SFTP runs inside an SSH connection to `sshd` (the SSH server), authenticated like a login, with the
person's own key, which never lives in `keyd` ([sshd](../servers/sshd.md)).
- **`sshd` relays bytes only.** On a `subsystem sftp` request it asks the steward to start the
  transfer server for that session and pipes the channel to it, as it pipes `/dev/cons` to a
  session. `sshd` never parses SFTP.
- **One transfer server per channel.** It is a small native Rust program, started by the steward
  in a budget carved from the session's. A connection that only transfers files gets a session
  budget as a login does, with the transfer server in it instead of beamlet.
- **SCP is served only as SFTP.** OpenSSH 9 and later run `scp` over the SFTP protocol. The old
  SCP protocol (`scp -O`, which runs `scp -t` or `scp -f` on the server) is refused: Redoubt has no
  `exec` and no shell to run it, and one protocol parser is less than two. Older clients use `sftp`
  or `scp -s`.
- **Inbound traffic other than SSH is refused**, so SSH, with SFTP inside it, is the only inbound
  service. (Outbound fetches, such as `git` through a gateway, are a session's own requests, not a
  service.)

**Open:** none.

### Confined to the session's files

Status: planned · M3 (files in and out)

The transfer server holds the session's file binds and one connection to the steward's audit path
([Audited](#audited)), and nothing else: no `/net`, no `/dev/cons`, no powerbox connection, no
budget or process handles. That is narrower than the session, as a launch always may be.
- **Confinement is by capability, not by path strings.** The server reaches files only through the
  namespace connections it holds, so no path, however it is written, reaches anything else. `..`
  at a connection's root stays at the root, by 9P's walk; cleaning names lexically makes them
  predictable, and is not the wall.
- **Unsupported operations fail visibly.** `SYMLINK`, `READLINK` and `LINK` get
  `SSH_FX_OP_UNSUPPORTED`. `SETSTAT` honours a size (truncation), and the modification and access
  times where the file server stores them; a mode, a user or a group gets `SSH_FX_OP_UNSUPPORTED`
  rather than a silent success. This is the same rule the `File` API follows on the box
  ([files](files.md#files-over-9p)).
- **The same budget and file rules.** Its pages and CPU are charged to the session's budget, a
  full volume refuses the write, a rename between volumes is a copy and a remove, and removing an
  open file succeeds ([files](files.md)).

**Open:** none.

### No transfers in a vault session

Status: planned · M3 (files in and out)

A vault session gets no SFTP and no SCP: `sshd` refuses a subsystem request on a labelled channel.
`sshd` is a sink cleared for a label only on its owner's terminal channel, with no forwarding, no
subsystems and no `exec` ([sessions](sessions.md#vault-sessions)). Labelled data enters a vault
through an audited push by the steward, and leaves only by declassification
([the steward](../servers/steward.md)). A file-transfer channel for vaults would be a new way for
labelled data to leave, and it is not part of the design.

**Open:** none.

### Audited

Status: planned · M3 (files in and out)

Every operation a transfer performs is one record in the audit log: each open (for reading and for
writing, since files go out as well as in), each close with its byte count, and each remove,
rename, mkdir, rmdir and setstat. The log that holds them comes with transfers: written by the
steward, append-only, recording transfers, with each record signed through `keyd`'s audit purpose
over `redoubt.audit.v1`, the record's length and the record ([keyd](../servers/keyd.md)). The same
log grows to every steward action in M4 (self-hosted development), and chaining (which catches
dropped or reordered records) and the offline verifier come with log retention in
M5 (persist, install, share) ([the steward](../servers/steward.md)). The principal
and labels in a record come from the badge the steward minted for the transfer server, never from
the server's own claim, so the server cannot forge whose transfer it was.

The audit is accountability, not a wall. A principal who compromises their own transfer server
with crafted input holds only their session's file capabilities, which they already had; they
could suppress their own records, and they could move data out unaudited through their terminal
anyway.

**Open:** whether the file server audits transfer handles itself, which matters only if the audit
must survive a compromised transfer server.

## Why

**One way in.** Every inbound service is a parser the whole network can reach. SSH is needed for
logins anyway; putting file transfer inside it adds a subsystem behind authentication, not a new
listener in front of it.

**A transfer is narrower than a session.** If file transfer had its own authority (a service
account, a drop directory writable by everyone), it would be a way around the session's walls.
A transfer server holding only the session's file binds has less than the session, so the attack
cases for sessions cover it, and a bug in its SFTP parser reaches only files the person could
already reach.

**Not in the session's VM.** OTP ships an SFTP daemon, and serving SFTP from the session's own VM
would be less code. But the principal controls their own VM, so audit records written there could
be skipped at will; it would need a whole VM for a connection that only moves files; and the
daemon is built for OTP's SSH server, not a relayed byte stream.

**Refuse, do not pretend.** A client told that `chmod` succeeded believes the file is protected.
Refusing an operation that means nothing here is more honest, and a tool that needs it finds out
at once.
