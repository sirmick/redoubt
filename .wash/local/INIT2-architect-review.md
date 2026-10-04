# INIT2 pages: the Architect's review (wp-init2 at 84dda1274)

Every ruled line is on the pages as ruled:
- init.md: the confinement bullets in the ruled order, the domains paragraph, step 2's minting
  line, the handed row, the reserved names, `ssh-ed25519` only, Sizing's `buckets=N` sentence,
  the Arguments badge sentence, and R33-R35 marked built with the steward's half left partly;
- devices.md: the `system_reset` kind 3 sentence;
- consoled.md: `[con N]` in 16 hex digits, and the root badge held by `init` alone by the check;
- native.md: `Registers::unmap` and `first_entry!`;
- testbench.md: the servers' cases under `init`, rule F's consoled bullet, and the data entries,
  partly;
- image/boot.toml and image/README.md: documented. The cited "The boot bundle" heading exists.

What INIT2 leaves to INIT3 and INIT4 matches their nodes. I added two items to INIT3's body (see
the end).

Three content changes and two reflows. Each is an exact line, on wp-init2.

## 1. init.md "Starting the servers", step 1: the watcher clause

Replace "or if it names more servers than `init` has threads to watch, one each beside its own
(`MAX_THREADS`)." with:
> or if it names more servers than `init` can watch, one thread each beside its own: at most
> `MAX_THREADS` - 1.

## 2. init.md step 3: the no-`keyd` refusal has no page sentence

The check refuses it (`Why::NoKeyd`, host test
`a_manifest_without_keyd_is_refused_and_init_calls_each_server_at_an_endpoint`), but no page
says so. Replace "3. starts `keyd` and runs the [key-separation check](#the-key-separation-check)
against it;" with:
> 3. starts `keyd` and runs the [key-separation check](#the-key-separation-check) against it. A
>    manifest with no `keyd` entry is refused at step 1, since the bundle's key always needs
>    asking about;

## 3. init.md, the boot figure: `fsd` is drawn as built

The solid launch edge names `fsd`, which step 5's status says is not built. Replace the line
`I->>S: launch through the stub:<br/>consoled, then bootfsd, blkd, fsd, netd, ipd` with these two
lines:
> `    I->>S: launch through the stub:<br/>consoled, then bootfsd, blkd, netd, ipd`
> `    I-->>S: launch fsd, one per volume`

The caption becomes:
> *Figure: the boot from the loader to the first session. Dashed: planned (`fsd`, the steward and `sshd`).*

## 4. budgets.md "The tree from the boot manifest": the `INIT_PAGES` list misses the watchers

The bound paragraph counts a watching thread per server. The list of what `init` uses does not,
and "its heap" is now an arena.
- Replace "  - its heap, for the manifest and the startup blocks it builds;" with:
  > - its heap, one fixed arena mapped once, for the manifest and the startup blocks it builds;
- After "  - a page for each server endpoint it owns;", add:
  > - a thread for each server, watching its exit endpoint: the thread's stack and IPC page;
- Replace "That is about 500 pages, and 1,024 doubles it." with:
  > For the image's six servers the bound is under 500 pages (`init-boot` prints it), and 1,024
  > doubles that.

## 5. Reflow only (the editor's, listed so it is not missed)

- init.md "Sizing": "server's arguments, with the serving library's parser, and no other argument.
  So a server's bucket count never binds ..." runs past the wrap width on one line.
- devices.md `system_reset`: the line ending "...(boot.md#r17-fail-closed)). On success it does not
  return. Errors:" is too long.

## Plan: two items added to INIT3's body (it has not started)

- A case that watches a child's console connection disconnected when the child exits. This is
  the partly-tested half of init.md "Fresh connections per child".
- A server that exits while the boot is starting, consoled refusing a `buckets=N` its budget
  cannot hold among them, falls under the reboot rule. Today the next console mint fails and the
  boot is refused. If consoled is the last server started, nothing catches it.
