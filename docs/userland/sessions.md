# Sessions and namespaces

A **session** is a person's or agent's working environment on the box: one beamlet VM in one
budget, with a namespace and capabilities the steward built for it. A **namespace** is the
session's own map from path prefixes to capabilities; it is built before the session starts,
nothing in it is inherited, and there is no global file tree behind it. This page covers logging
in over SSH, what a session is, vault sessions, namespaces and binds, and `approve@`, the one
terminal where a person grants authority.

## Purpose

A session is the unit a person works in and the unit Redoubt keeps apart. Alice's session and
Bob's are different VMs in different budgets, and each reaches only what its namespace and
handles name. So the questions this page answers are the ones a person asks first: how do I get
in, what can my session reach and why, how do I work with data that must not leak, and where do I
say yes when something asks for more.

## How to use it

Log in with your own SSH key. The user name picks the session:

```text
$ ssh alice@box            # an ordinary session: Alice's unlabelled data
$ ssh alice+tax@box        # a vault session carrying Alice's `tax` label
$ ssh approve@box          # the approval terminal: only the steward talks here
```

At the prompt, the session's namespace is a table you can print:

```elixir
iex(1)> ns()
/home/alice  fsd:home     (Alice's home volume)
/dev/cons    sshd         (this SSH channel)
/boot        bootfsd      (the boot bundle, read-only)
/net         ipd          (the hosts and ports this session may reach)
iex(2)> File.ls!("/home/alice")
["notes.txt", "src"]
iex(3)> File.read("/home/bob/notes.txt")
{:error, :enoent}
```

`bind/2` makes a connection the session already holds appear at another prefix. It creates no
authority, only a name:

```elixir
iex(4)> {home, _rest} = ns_lookup("/home/alice")
iex(5)> bind("/h", home)
:ok
iex(6)> File.ls!("/h/src")                  # the same files as /home/alice/src
```

Leaving the session (`exit`, or closing the SSH connection) ends it: the steward destroys the
session's budget, and every process in it ends with it.

## What it can and cannot do

### Logging in

Status: planned · M1 (separation and containment)

`sshd` (the SSH server) accepts a connection, and `sshd` and the steward authenticate the person
with the principal's login key. In M1 (separation and containment) the principals and their keys
come from the boot manifest ([init](../servers/init.md)). A login key is the person's own: it stays
on their machine or security key and never lives in `keyd` (the key server). `sshd` refuses
authentication with any public key `keyd` holds, so a hijacked session that can sign with `keyd`
cannot log in as anyone ([sshd](../servers/sshd.md)).

The steward then starts the session: it carves a budget for it under the principal's budget,
builds its namespace from the principal's capabilities, and launches a beamlet VM in it
([the steward](../servers/steward.md)). Each principal has one fixed sub-budget per label set,
and sessions are carved from the one that matches their labels, so a flood of sessions in one
label set cannot starve another.

**Open:** none.

### A session is a VM in a budget

Status: planned · M1 (separation and containment)

One session is one beamlet VM, in one budget of class `user`, under the principal's budget
([budgets](../kernel/budgets.md#root-system-and-users)). What that gives:
- **Everything the session does is paid for there.** Every page, process and share of the CPU it
  uses is charged to the session's budget or to a budget carved from it
  ([R6 (charging)](../kernel/budgets.md#r6-charging)). A session cannot spend Bob's pages.
- **Every call it makes says who it is.** The kernel stamps each message with the budget's
  account (the principal it bills to) and label set; the session cannot choose either
  ([R14 (unforgeable sender)](../kernel/ipc.md#r14-unforgeable-sender)). Servers admit and check
  calls by that (account, label set).
- **Ending it ends everything in it.** Logout, a lost connection, or the steward ending the
  session destroys its budget. Every process in it ends, every handle stamped with it dies
  wherever copies went, and nothing outside it is touched
  ([R10 (destruction)](../kernel/budgets.md#r10-destruction)).
- **One VM is one trust domain.** Everything typed at the prompt, every script and every Erlang
  process in the VM runs with the session's full authority. Code that must have less (an agent,
  an untrusted parser) runs in a VM or native program of its own, in a budget of its own
  ([agents](agents.md), [native programs](native.md)).

There is no root and no `sudo`. "Admin" means holding specific capabilities over shared things,
and a session holds only what its principal was granted.

**Open:** none.

### Vault sessions

Status: planned · M1 (separation and containment)

A **vault session** carries one of its principal's labels: `ssh alice+tax@box` starts a session
whose budget has Alice's `tax` label. A budget's labels are fixed when it is created and only grow
downward (I6 (labels only grow downward)), so nothing started inside a vault session can shed the
label.

What the label changes, per the label rule of the servers
([labels](../servers/README.md#labels)):
- **It can read** unlabelled data and data carrying `tax`: reading needs the caller's labels to
  include the object's.
- **It can write** only data carrying exactly `tax`: writing needs the labels to be equal. So it
  can read Alice's home volume and cannot write a byte to it.
- **It cannot send** to an unlabelled process at all
  ([R1 (flow)](../kernel/ipc.md#r1-flow)), and servers that are sinks refuse it: `ipd` (the TCP/IP
  server) refuses every labelled caller, so a vault session has no network.

What it shows on the SSH channel reaches only Alice, who owns the label; `sshd` keeps each
channel's labels, and a labelled channel is its owner's terminal only, with no forwarding, no
subsystems (so no file transfer: [file transfer](transfer.md)) and no `exec`. Data leaves the label only by **declassification**: a request to the steward,
approved by the label's owner at `approve@` ([the steward](../servers/steward.md)).

**Open:** none.

### Namespaces

Status: planned · M1 (separation and containment)

A namespace is a table inside the process: path prefixes, each naming a capability (a 9P
connection) the process holds. The launcher writes it into the child's startup block before the
child starts ([processes](../kernel/processes.md#creating-and-starting)); for a session, the
launcher is the steward. There is no mount call and no kernel mount table: the kernel knows
handles, never paths.

```svgbob
  Alice's session: one beamlet VM                       servers
 +- - - - - - - - - - - - - - - - - - - - - - - -+
 : namespace: prefix -> connection                :
 :                                                :
 :  "/home/alice" o- - - - - - - - - - - - - - - - - - - - > fsd, Alice's home volume
 :  "/dev/cons" o- - - - - - - - - - - - - - - - - - - - - > sshd, this SSH channel
 :  "/boot"     o- - - - - - - - - - - - - - - - - - - - - > bootfsd, read-only
 :  "/net"      o- - - - - - - - - - - - - - - - - - - - - > ipd, a scope of hosts and ports
 :                                                :
 : named handles                                  :
 :  "steward"   o- - - - - - - - - - - - - - - - - - - - - > the steward, this session's grant
 :  "budget"    o  the session's own budget       :
 +- - - - - - - - - - - - - - - - - - - - - - - -+
      "/home/bob/notes.txt": no entry is a prefix of it, so it is enoent
```
*Figure: a session's namespace. Every part is planned (dashed). Each entry is a connection the
session holds; a path reaches only what an entry names.*

What follows from a table of capabilities:
- **A path resolves by its longest matching prefix**, and the rest of the path is walked on that
  connection. `/dev/cons/x` goes to the console's connection with `x` left over.
- **Nothing is inherited.** A path whose prefix names nothing the session holds is `:enoent`, not
  a permission error: there is nothing there to refuse.
- **`..` never climbs out.** Paths are cleaned lexically before lookup, in the client and again
  in the server, so `/../../etc` is `/etc` on the same connection.
- **Different prefixes are usually different servers.** `/home/alice` and a vault's volume are
  two connections, so a rename between them is a copy and a remove, never atomic
  ([files](files.md)).
- **A copy of a connection is the same connection.** All holders of a handle share one badge and
  one set of open files, so a launcher never passes its own connection to a child: it asks the
  server for a fresh one with `new_connection`, and disconnects it when the child's exit notice
  arrives ([the serving library](../servers/serving.md)).

**Binds.** `bind/2` puts a connection the session already holds at another prefix. It needs no
server and creates no authority. A namespace is the session's own: binding in it changes nothing
for any other process, and a child sees only the table its launcher wrote for it
([files and binds](files.md)).

**Open:** none.

### How a program reads its namespace

Status: built · tested: host:redoubt-rt::resolve_takes_the_longest_prefix, host:redoubt-rt::dot_dot_never_climbs_above_the_root, host:redoubt-rt::bad_names_are_refused, host:redoubt-rt::hostile_blocks_are_refused, host:redoubt-rt::handle_names_follow_the_manifest_rule, host:redoubt-rt::a_client_without_its_namespace_fails_cleanly

For native programs the namespace table exists in code: `redoubt-rt` (the native runtime) parses the
startup block and resolves paths against it
([`libs/rt/src/startup.rs`](../../libs/rt/src/startup.rs),
[`libs/rt/src/path.rs`](../../libs/rt/src/path.rs)).
- The block names the handles `process_start` installed (slots 1 to n, at most
  `MAX_START_HANDLES`, 64), namespace entries (a handle and a clean absolute path), named handles
  (a handle and a name) and the arguments. Paths are unique, names are unique, and a block that
  breaks any rule is refused whole: the parent may be hostile.
- `resolve` returns the entry with the longest matching prefix and what is left of the path; a
  prefix matches only at a `/` boundary, so `/dev/consx` does not match `/dev/cons`. A relative
  path resolves to nothing.
- `clean` resolves `.` and `..` lexically and never climbs above the root: `../../etc/passwd` is
  `etc/passwd`. A name longer than 255 bytes or holding a NUL, and a path deeper than 64
  components, are refused.
- A named handle's name is lower-case ASCII letters, digits and `_:+-`, starting with a letter,
  at most 64 bytes (`fsd:data`, `alice+secrets`).
- A program started with an empty namespace fails cleanly: the echo client exits with its error
  code, and the echo server with `NO_ENDPOINT`, instead of reaching anything.

### `approve@`

Status: planned · M1 (separation and containment)

`ssh approve@box` is the approval terminal. When an agent asks its sponsor for more authority, or
a session asks for a declassification, the request goes to the steward, and the requester's own
terminal only shows that an approval is waiting. The person answers at `approve@`, where only the
steward talks to the terminal, so the requester can never draw on the screen the person decides
from ([the steward](../servers/steward.md)).
- **The person's own key.** `approve@` authenticates with the person's own SSH key, which never
  lives in `keyd`. A session's network capabilities never include the box's own addresses, so a
  hijacked session cannot reach `approve@` over loopback.
- **What is shown** is rendered by the steward from the structured request: who asks (its kind,
  such as agent or session, and its steward-given name, `agent-7`), what, where, for how long, and
  the label consequences. Every field is printable ASCII, escaped and length-capped. A labelled
  requester's free text is never shown; an unlabelled requester's is quoted and marked untrusted.
- **What is approved** is bound: each request has a random 64-bit id and a hash of its exact
  content, and approving confirms both. An approval grants no more than the approver holds.

In M1 (separation and containment) `approve@` is served by the same `sshd` as every other
channel. That is a stated residual: a bug in the SSH code reached from any channel could control
the approval screen, and a network flood can delay approvals ([sshd](../servers/sshd.md)). The
design protects the approval channel from the requester; it does not protect the person's
judgment, and an approval that grants more than the person intended is a wall the design names,
not one it closes ([agents](agents.md)).

**Open:** none.

## Why

**Sessions are budgets because budgets are what the kernel enforces.** The kernel knows nothing
about people. It knows budgets, which pay for everything and whose destruction revokes
everything, and label sets, which it checks on every message. Making a session a budget with a
label set means the separation between Alice and Bob is the kernel's, and ending a session is
one call that cannot leave anything behind.

**Namespaces instead of a file tree.** A global file tree with permission bits asks every server
to decide, per path, who may do what, and a mistake anywhere is a leak everywhere. A namespace of
capabilities asks one question once, at launch: what does this process get. A path that is not in
the table does not exist for that process, so there is nothing to probe, and a child can be given
a narrower table than its parent by writing fewer entries. Plan 9 showed that per-process
namespaces make a usable system; Redoubt adds that every entry is a capability.

**Approval out of band.** If an approval could happen in the requester's own terminal, a
hijacked session could draw a fake prompt, or answer it. Putting approvals behind a separate
login, with the person's own key, rendered only by the steward, makes the channel something the
requester cannot touch. The human is still a wall that can be talked into things; the design
says so rather than hide it.
