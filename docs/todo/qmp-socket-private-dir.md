# A private directory for the QMP socket

## What

A boot case with `memory = true` starts QEMU with a QMP socket at
`<tmp>/redoubt-qmp-<pid>-<n>.sock`: the bench's process id and a count of its boots, in the
shared temporary directory ([the memory budget](../testbench.md#the-memory-budget)). The name is
predictable, and QEMU creates the socket under the bench's umask. With umask 002, any member of
the bench user's group can connect to it while the case runs.

## Why it matters

Whoever holds the QMP connection drives the guest: it can stop it, write its memory and save
its RAM to any file the bench's user can write, all as the bench's user. In a sticky `/tmp`
another user cannot replace the socket, but a file left at the predicted name makes QEMU fail to
start, so the case fails without a verdict.

## Where

`tools/testbench/src/qemu.rs`: `qmp_socket` names the path, and `run` passes it to QEMU's `-qmp`
and removes it when the run ends.

## Done when

- Each run makes a private directory with `mkdtemp` (mode 0700) in the temporary directory, and
  the socket is created inside it, so only the bench's user can reach it, whatever the umask.
- The directory and the socket go when the run ends, however it ends.
- A host test checks the directory's mode and that the socket path still fits a Unix socket's
  108 bytes.
- The memory budget's note on the umask leaves the page, and this page is deleted.
