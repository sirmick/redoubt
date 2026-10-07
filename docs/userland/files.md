# Files and binds

A file on Redoubt is something a server serves over 9P, reached through a connection in the
session's namespace. Elixir's `File`, `IO` and `Path` work unchanged on top: beamlet's file
natives speak 9P to the file server, one instance per volume, which keeps the data in littlefs on
a block device. There are no permission bits, no owners, no symlinks and no hard links. Access is
by capability (holding the connection) and by label (the volume's labels against the caller's);
a **bind** puts a connection the session already holds at another path.

## Purpose

People and agents work with files more than with anything else, and the shape of the file system
is where Redoubt differs most visibly from Unix. This page says what `File` does over 9P, which
Unix habits carry over and which do not (`chmod`, `ln -s`, an atomic rename between two
directories), how labels decide who may read and write a volume, and how a directory is shared
with another principal.

## How to use it

Ordinary Elixir:

```elixir
File.write!("/home/alice/notes.txt", "buy milk\n")
File.read!("/home/alice/notes.txt")
File.ls!("/home/alice")
File.cp!("/home/alice/notes.txt", "/work/notes.txt")   # across volumes: a copy loop
File.rename!("/home/alice/a.txt", "/home/alice/b.txt")  # one volume: the server renames
```

The shell's commands are the same operations with short names: `cat`, `cp`, `mv`, `rm`, `mkdir`,
`ls`, `stat` ([the shell](shell.md)). A bind names a connection the session holds under another
prefix:

```elixir
{home, _rest} = ns_lookup("/home/alice")   # the connection behind the prefix
bind("/h", home)
File.ls!("/h/projects")                     # the same files as /home/alice/projects
```

Redoubt-only operations are in `Redoubt.File`: `copy_file` (a copy the server does), `rename`
(the file server's own, between two directories of one volume), and `set_attr`/`get_attr` for
per-file metadata.

## What it can and cannot do

### Files over 9P

<details><summary>Status: built · partly tested: the host tests run beamlet's platform against the real `littlefsd` on the fake kernel, and OTP's `prim_file` over the natives runs in a boot in bench:beamlet-files; Elixir's `File` over them runs in a session once the steward's sessions do · tested (19)</summary>

- host:beamlet-redoubt::files_are_written_read_listed_renamed_and_removed
- host:beamlet-redoubt::opening_to_write_replaces_and_appending_adds
- host:beamlet-redoubt::a_read_past_one_answer_comes_in_pieces
- host:beamlet-redoubt::the_position_lives_in_the_vm_and_moves_with_reads_and_seeks
- host:beamlet-redoubt::a_path_under_no_binding_is_enoent
- host:beamlet-redoubt::a_path_above_a_binding_is_a_directory_the_namespace_answers
- host:beamlet-redoubt::two_askers_wait_at_once_and_each_gets_its_own_answer
- host:beamlet-redoubt::the_vms_own_calls_wait_in_place
- host:beamlet-redoubt::closed_files_give_their_fids_back
- host:beamlet-redoubt::an_abandoned_operation_stops_at_its_next_answer
- host:beamlet-redoubt::a_file_operation_on_the_consoles_connection_is_answered
- host:beamlet-redoubt::a_stat_reports_only_what_9p_has
- host:beamlet-redoubt::what_has_no_9p_field_is_refused_visibly
- host:beamlet-redoubt::every_row_of_the_error_table_maps_to_its_posix_error
- host:beamlet-redoubt::a_bind_argument_puts_a_handed_volume_in_the_namespace
- host:beamlet-vm::a_completion_reaches_the_process_that_asked_and_no_other
- host:beamlet-vm::a_message_does_not_end_a_wait_for_a_file
- host:beamlet-vm::a_process_killed_while_it_waits_has_its_operation_dropped
- bench:beamlet-files

</details>

```mermaid
flowchart LR
    F["File, IO<br/>(Elixir)"] -.-> FM[":file, file_io_server<br/>(OTP, unchanged)"]
    FM -.-> PF["prim_file natives<br/>(beamlet)"]
    PF -.-> C["the 9P client<br/>(beamlet's Platform)"]
    C -.->|"9P over call and lend"| LFSD["littlefsd, one per volume"]
    LFSD -.-> LFS["littlefs"]
    LFS -.->|"typed calls"| B["blkd"]
    B -.-> D["virtio block device"]
```
*Figure: the file I/O path from `File` to the block device. The links are built; they are drawn
dashed until Elixir's `File` runs over them in a session. The servers are
[the file server](../servers/littlefsd.md)'s and [blkd](../servers/blkd.md)'s pages.*

`File.read!/1` becomes OTP's `file` module, whose `prim_file` calls beamlet implements over its
9P client ([beamlet](beamlet.md#the-platform-boundary);
[`userland/otp/redoubt/src/files.rs`](../../userland/otp/redoubt/src/files.rs)). The client
resolves the path in the session's namespace, walks the rest of it on that connection, and reads.
A path above a binding, such as `/`, is a directory the namespace answers itself; a path
neither inside nor above one is `:enoent` ([sessions](sessions.md#namespaces)).
- **A fid is the file descriptor**, and a directory fid is a capability. The file position lives
  in the VM, because 9P reads and writes carry explicit offsets; nothing else is kept between
  calls. A closed file's fid is clunked, and comes back when the server has let it go.
- **Everything moves in pieces**: a read asks at most what one answer carries (the 64 KiB `msize`
  less its header), a write at most a page, each one request on the VM's hub
  ([asynchronous underneath](beamlet.md#asynchronous-underneath-synchronous-on-top)), so only the
  Erlang process that asked waits for it. `littlefsd`'s `rename` is a typed call, which no hub
  carries: it is made on the VM's thread.
- **Plain 9P2000**, with no Unix extensions: a `stat` has a name, a length, a modification time
  and a qid (the server's identity and version for the file), and nothing else. Custom
  per-file metadata is the file server's typed `set_attr` and `get_attr`, kept in littlefs
  attributes.
- **An error is a Redoubt error first.** The file server refuses with its own reasons
  (`not_found`, `refused`, `exists`, `not_dir`, `removed` for a fid whose file was removed,
  `too_large` for an attribute over its limit), and labels and budgets add theirs, each a name of
  the one table ([wire](../servers/wire.md#error-names)). Everywhere but the `File` boundary a
  Redoubt error keeps its name; there, beamlet's platform maps each name to the POSIX error OTP's
  `file` expects, by [the table's last column](../servers/wire.md#error-names).

| Operation | What happens |
| --- | --- |
| `File.stat`, `:file.read_file_info` | what 9P and the file server have: the type (from the qid), the size and the modification time (0 until `littlefsd` keeps one: the residual below); `access`, `mode`, `uid`, `gid`, `links`, `inode` and `major_device` are `:undefined`, since no server says what a connection may do; attributes through `get_attr` |
| `File.ls` | a read of a directory fid; entries the caller may not read are left out |
| `File.rm` of an open file | succeeds: an "in use" refusal would tell one client about another |
| `File.chmod`, `File.chown` | `{:error, :enotsup}`: there are no mode or owner bits, and access is by capability |
| `File.ln_s`, `File.ln` | `{:error, :enotsup}`: 9P2000 has no links, and binds do their job |
| `File.write_stat` | applies the modification and access times only; `{:error, :enotsup}` if it carries a mode, a user or a group (times are refused for now: the residual below) |

**Refuse visibly; report only real fields.** The `File` API does not emulate POSIX: no program
should rely on a permission bit that no server enforces, because authority on Redoubt is the
capability, never a mode bit. `:undefined` is within OTP's own `file_info` type for exactly these
fields, so nothing is invented. This is the one statement of the rule; file transfer follows it
([file transfer](transfer.md#confined-to-the-sessions-files)). Standard-library code that does
arithmetic on a mode (the mode preservation in `File.cp` and `File.cp_r`, Mix's check that a file
is executable) is adjusted in beamlet's platform layer to skip the mode, never fed a fake one; the
M4 (self-hosted development) case that compiles a Mix project on the box catches any caller that
breaks. The host tests above hold the undefined fields and the four `:enotsup`s, and
bench:beamlet-files sees `mode` undefined in a boot.

Residuals, each a departure from the table, until the file server serves what it needs:
- **No times are set and none is stored.** The 9P skeleton refuses `Twstat`, so `File.write_stat`
  with times, and a cut at a position (`:file.truncate/1`) other than an open's, are
  `{:error, :enotsup}`; and `littlefsd` keeps no modification time, so it reads as 0 (1970).

### Copying, moving, removing and binds

Status: planned · M2 (usable shell)

Two paths on different prefixes are usually on different servers, and one server cannot act on
another's files. So what an operation costs depends on where its two ends are:

| Operation | Within one volume | Across volumes |
| --- | --- | --- |
| copy (`File.cp`, `cp_r`, `cp`) | the file server's `copy_file`: no bytes cross into the VM | a read and write loop in the VM |
| rename or move (`File.rename`, `mv`) | the file server's `rename`, atomic, within one directory or between two | `File.rename` returns `{:error, :exdev}`; a move is the caller's copy and remove (`mv` does both), not atomic |
| remove (`rm`, `rm_rf`) | a 9P `remove`, recursively for `rm_rf` | |
| make a directory (`mkdir`, `mkdir_p`) | a 9P `create` with the directory bit | |

**Binds.** `bind(prefix, conn)` adds an entry to the session's own namespace: the connection
`conn`, already held, appears at `prefix`. It changes nothing on any server and creates no
authority, and no other process sees it. It replaces what Unix does with symbolic links, hard
links and bind mounts: giving something a second name. A child gets only what its launcher writes
into its startup block, so a session's binds reach a child only if the session passes them on
([native programs](native.md)).

**Open:** none.

### Labels on files

Status: planned · M1 (separation and containment)

Labels are per volume: each volume has its own file server instance and its own label set, fixed
when the volume is set up ([the file server](../servers/littlefsd.md)). The file server checks every
request against the caller's label set, which the kernel stamps on the message
([R14 (unforgeable sender)](../kernel/ipc.md#r14-unforgeable-sender)), by the servers' label rule
([labels](../servers/README.md#labels)):
- **Read** needs the caller's labels to include the volume's. A session with `{tax}` reads an
  unlabelled volume and the `tax` volume; an unlabelled session reads only unlabelled volumes.
- **Write** needs the caller's labels to equal the volume's. A vault session cannot write its
  principal's unlabelled home, so it cannot copy labelled data down into it.
- **Metadata follows the data.** Names, sizes and listings of a labelled volume are refused to a
  caller that cannot read it, so an unlabelled caller can neither read labelled data nor learn
  that it exists.

**Open:** none.

### Sharing a directory

Status: planned · M5 (persist, install, share)

Alice shares a directory with Bob through the steward. The only delegation primitive is
`new_connection(root, quota)`: it mints a connection rooted at a subdirectory, with its own byte
quota, and returns it with a random id ([the serving library](../servers/serving.md)).

1. Alice asks the steward to share `/home/alice/shared` with Bob.
2. The steward mints the share into a **revocation scope**: a budget with no pages, processes or
   weight, made under Alice's budget only to be destroyed later. It passes the connection to Bob.
3. Bob can narrow it further: asking the file server for `shared/sub` gives a handle that keeps
   the share's stamp ([R9 (stamps)](../kernel/objects.md#r9-stamps)).
4. Alice un-shares: the steward destroys the scope, and every handle stamped with it dies,
   `sub` included, wherever the copies went ([R10 (destruction)](../kernel/budgets.md#r10-destruction)).

Labels still decide access: a share of a labelled volume reaches only a principal whose sessions
carry the label. A project, a principal sponsored by several members, shares its own volume the
same way, one revocation scope per member ([the steward](../servers/steward.md)).

**Open:** none.

## Why

**Capabilities instead of permission bits.** A mode and an owner are a question every server asks
about every file ("may this user do that?"), and the answer depends on a global table of users.
A connection is the answer given once: holding it is the permission, narrowed by the server when
it was minted (a subdirectory, read-only). There is nothing to `chmod` because there is no table
to consult.

**No symlinks or hard links.** A symbolic link is a path the server follows on the client's
behalf, which is how a program is tricked into writing where it should not (a link from `/tmp`
into someone's home). A hard link gives one file two parents, which breaks per-directory quotas
and makes "remove" mean two things. A bind gives something a second name in one process's
namespace, and cannot point anywhere that process could not already reach.

**Rename is not always atomic, and the docs say so.** A rename between two servers cannot be
atomic without a transaction protocol between them, and a transaction protocol between file
servers would be the largest shared code in the system. Redoubt keeps each server alone and says
which renames are copies.

**Removing an open file succeeds.** If a server refused to remove a file another client has open,
the refusal would tell one client something about another: a channel. So a remove always does
what it says.
