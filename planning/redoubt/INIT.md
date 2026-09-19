# Init, the steward, restarts and the boot manifest

Designed, not built. Owns: what happens after the kernel starts, the boot manifest, the system
servers' roles, restart and reboot rules, the startup block, and the worked example. Server names:
README.md. Launching a process: PACKAGES.md.

## Decisions
1. **Two stages.** A tiny Rust `init` holds all authority at boot, starts and wires the system
   class from the boot manifest, then hands the user side to the **steward** (a Rust system server)
   and keeps only what it needs to restart things.
2. **`init` restarts every OS process.** OTP supervisors only restart Erlang processes inside a VM.
   Session VMs are not restarted: a dead session is a logout.
3. **The BEAM is not in the TCB.** Everything whose compromise could cross principals is Rust: the
   steward, `keyd`, `sshd`. Elixir is userland: shells, applications, agents. A compromised VM holds
   exactly its principal's capabilities, like a native binary.
4. **Endpoints outlive servers.** `init` creates each server's endpoint and keeps it. Clients hold
   handles to the endpoint; a restarted server receives on the same endpoint.
5. **Servers persist no authority.** What a badge means lives in server memory and is lost on
   restart; a server that saved it could be made to write itself a root badge. In milestone 1 the
   steward is **stateless**: principals and their keys come from the boot manifest. From milestone 2
   the steward keeps its records (principals, shares, leases, trust lists, profiles) on a
   steward-only system volume and re-mints cross-principal grants from them after a restart.

## Boot
```
firmware -> loader (verifies bundle; loads kernel and init) -> kernel -> init
init:     system budget; consoled, bootfsd, blkd, fsd:*, netd, ipd:lan, keyd
init:     steward (system class; holds the users budget), sshd
steward:  principals, sessions and agents (beamlet VMs)
```
- The kernel gives `init` the root, system and users budgets, every device object, the Reset device,
  and the bundle's pages. `init` parses only the verified boot manifest. It has no network and no user
  data.
- `init` starts every process by the one launch mechanism (PACKAGES.md), straight from the bundle's
  pages: no file server is needed to start `bootfsd` or anything else.
- Device authority today: loader-emitted grants (DEVICE-GRANTS.md). Designed: `init` holds every
  device object and places each driver's handles in its startup block.
- The physical console is labelled with no labels. From milestone 2 the first owner is enrolled on
  it at first boot (a trusted path) and uses it for approvals.

## The boot manifest
One strict JSON file (WIRE.md) in the signed bundle; `init`'s only input. Entries:

| Entry | Holds |
| --- | --- |
| `system` | the system budget's pages, processes and weight (default 25% of RAM) |
| `devices` | each device object's name, its device-tree node path, and whether it may do DMA |
| `labels` | each label's name, owner principal and 64-bit id |
| `volumes` | each volume's name, `blkd` partition and label set |
| `servers` | each server's name, program (a bundle entry), budget (pages, processes, weight), device names, volume, the endpoints it receives on, the endpoints it is handed, and arguments |
| `principals` | milestone 1 only: each principal's name, SSH public keys for login and approval, budget, account, owned labels, home (volume and path), and network scope (IP prefixes and ports) |

Each field has one JSON type (WIRE.md): 64-bit quantities (label ids, accounts, page and byte sizes,
deadlines) are decimal strings; small counts (processes, weights, depths, restart limits) and ports
are numbers. A value of the wrong JSON type is an error.

**Names.** Every name in the manifest (devices, labels, volumes, servers, endpoints, principals) is
1-64 bytes of `[a-z0-9_:+-]`, starting with a letter (`fsd:data`, `alice+secrets`), and the
manifest decoder refuses any other. Names become endpoint names, volume names and 9P paths, so an
empty name, a NUL, U+FEFF or a C1 control must never reach them.

Example fragment:
```json
{ "servers": [ { "name": "fsd:data", "program": "fsd", "volume": "data",
                 "budget": { "pages": "4096", "processes": 1, "weight": 100 },
                 "receives": ["fsd:data"], "handed": ["blkd"] } ],
  "principals": [ { "name": "alice", "account": "1001", "labels": ["alice-secrets"],
                    "ssh_keys": ["ssh-ed25519 AAAA..."], "home": "data:/home/alice",
                    "net": [ { "prefix": "0.0.0.0/0", "ports": [22, 443] } ] } ] }
```

## Restarts and reboots
- **Restart:** a server that exits is restarted on the same endpoint. Calls it had taken get `Dead`
  (in milestone 1 clients see the error and retry); senders still blocked on the endpoint wait and
  are served by the restarted server (KERNEL-SPEC.md, R4b). (Milestone 2: the namespace
  library re-walks from the root, so most programs see only a hiccup.)
- **Blame:** each exit notice for a fault names an account and label set; `init` passes them to the
  steward, whose logout rule is in CONTAINMENT.md (Crash blame).
- **Reboot:** more than 5 restarts of one server within 60 seconds, not stopped by blame, reboots the
  machine (fail closed).
- **The steward:** if it dies in milestone 1, `init` destroys and recreates the users budget: every
  session is logged out. The steward is TCB; its crash is our bug.

## The system servers above the drivers
- **steward:** principals, authentication decisions, sessions, the powerbox, leases (as budgets),
  launching (PACKAGES.md), and, from milestone 2, packages, trust lists and profiles. It appends the
  audit log to a file only it can write (a separate audit server is deferred). It parses the most
  untrusted input in the system (every agent's requests), so it holds no keys and never parses an ELF.
  It filters requests by labels (CONTAINMENT.md).
- **keyd:** holds the keys the box uses on your behalf (host keys, principals' signing keys); signs
  on request, never exports. It never holds keys that authenticate a person to the box
  (CAPABILITIES.md, approvals). Separate from the steward because a leaked key cannot be revoked;
  authority can.
- **sshd:** the SSH front door (`sunset`: `no_std`, no allocation, by dropbear's author). It asks the
  steward to authenticate users and start sessions, and asks `keyd` to sign with the host key. It
  rejects any login key that `keyd` holds. It serves `ssh approve@box`, in which only the steward
  talks. Its state is per channel, and each channel carries its session's labels (CONTAINMENT.md).
- Users' own outbound TLS and SSH (OTP `:ssl`, `:ssh`) run inside their VMs, in userland.

## The shell
A session's shell is **IEx** (Elixir's interactive shell) on beamlet, with a small Redoubt helpers
module: `ls`, `cd` and `cat` over the namespace, `ps`, `budget`, and a notice when an approval is
waiting (`pkg` from milestone 2). IEx evaluates any Elixir, with exactly the session's capabilities.
It runs on the UART console before SSH exists. In milestone 1 the physical console's IEx exists only
in the bench build; the system manifest starts no shell on the UART.

## Startup block
Before a process runs, its parent installs its handles in its table (`process_start` copies them
into slots 1..n, at most `MAX_START_HANDLES`; handle 0 is never a handle) and maps one ordinary page
into it, read-only (`process_map`), holding the block below. **`process_start`'s `arg` is that
page's address** (page-aligned; 0 = no block), which the child's first thread receives
(KERNEL-SPEC.md); there is no fixed address. The program image travels in its own pages, which the
block names (PACKAGES.md, launching; its tag is defined with the loader stub). No environment
variables, nothing inherited. Configuration is files in the namespace.

**Format.** The kernel argument block's framing: little-endian `u32` words. Each entry is a 4-byte
ASCII tag, one word holding a CRC-16/X-25 of the entry's data in its low half and the data length
in words in its high half, then the data. A string is a `u32` byte length followed by UTF-8,
zero-padded to a whole word; an entry's length is exactly what its fields need.

| Tag | Data | Meaning |
| --- | --- | --- |
| `SBlk` | version (1), block length in words (this entry included), handle count n | the header: first, exactly once |
| `NmSp` | handle, string | a namespace entry: a clean absolute path (`/`, `/dev/cons`: no `.`, `..`, empty component or trailing `/`) and the connection it resolves to |
| `Hndl` | handle, string | a named handle, its name following the manifest's name rule (Names, above): services (`keys`, `powerbox`), device handles for drivers, the process's budget as `budget` |
| `Argv` | string | one argument (may be empty), in block order |

Rules: the block is at most one page; handles are 1..=n, n ≤ `MAX_START_HANDLES`; paths are unique
among `NmSp` entries and names among `Hndl` entries; nothing follows the last entry within the
block's length (the rest of the page is not read). A block breaking any rule is refused whole. The
parent may be hostile, so the child parses defensively; the reference implementation is
`redoubt/rt/src/startup.rs` (`redoubt-rt`), which also writes blocks for launchers.

**Launching gives fresh connections.** A launcher never places its own connection to a server in a
child's block; it asks the server for a fresh connection for the child and passes that one
(CAPABILITIES.md, one badge, one client). This is a rule for `init`, the steward and every shell.

## Worked example: Alice, Bob and Alice's agent (milestone 1)
```
kernel
└── init (Rust)                                       root
    ├── consoled bootfsd blkd fsd:data fsd:alice-secrets system [reserved]
    │   netd ipd:lan keyd steward sshd
    ├── session VM alice-1  (IEx)                     users/alice/session-1
    ├── agent VM alice/researcher [lease 2 h]         users/alice/researcher
    └── session VM bob-1    (IEx)                     users/bob/session-1
```
CPU weights: alice 100, bob 100; the agent 20, carved from Alice's. The agent shares Alice's account.

**Login:** `ipd:lan` delivers port 22 only to `sshd` (sole holder of "listen TCP 22"); `keyd` signs
with the host key (never in `sshd`'s memory); `sshd` asks the steward whose key it is (the steward
knows the principals from the boot manifest); the steward carves `users/alice/session-1`, builds her
namespace, and launches beamlet with it; IEx's `.beam` files come from the system bundle.

| Name | Alice's session | Bob's session | Enforced by |
| --- | --- | --- | --- |
| `/` | `fsd:data` at `/home/alice`, rw | `fsd:data` at `/home/bob`, rw | `fsd` (badge) |
| `/dev/cons` | her SSH channel | his | `sshd` (badge, channel labels) |
| `/net` | `ipd:lan`, connect out to ports 22 and 443, not the box's own addresses | `ipd:lan`, connect out to 443 | `ipd` (badge) |
| `keys` | sign with Alice's keys | Bob's | `keyd` |
| `powerbox`, `budget` | hers | his | steward, kernel |

Neither can name the other's home, `/system`, `fsd:alice-secrets`, the host key or block devices, or
listen on the network.

**Scenarios:**
- `cat notes.txt`: 9P on her `/` handle; `fsd` admits her by her account and label set.
- Vault: `ssh alice+secrets@box` gives a session labelled `{alice-secrets}` that reads and writes
  `fsd:alice-secrets`, reads (never writes) her home on the unlabelled `fsd:data`, which is how
  data enters the vault (`check`: a read needs the volume's labels ⊆ the session's), has no `/net`,
  and prints only to its own channel.
- Agent: own principal and VM, a 2-hour lease, `/work` only, no `/net`. An agent's namespace never
  includes its sponsor's `/dev/cons`. The bench's scripted hostile agent tries to escape (PLAN.md,
  milestone 1 attack suite); each attempt is refused. Its escalations
  wait for Alice in `ssh approve@box`; lease expiry destroys its budget and everything it passed on.
- Bob spins: he gets his share only. Bob allocates too much: `OutOfMemory` in his budget.
- Bob's VM crashes: the steward destroys his session budget; `sshd` closes the channel; Alice is
  unaffected.
- Bob fully compromises his VM (a beamlet bug): he holds Bob's capabilities, nothing more. Going
  further needs a bug in a server he talks to (`fsd`, `ipd`, `keyd`, the steward) or the kernel.
- Bob crashes `fsd:data` three times: each exit notice blames his account (the call the failing
  thread took most recently), so he is logged out; Alice, busy throughout, is not.

**Weak spot:** users are separated everywhere except inside shared servers, where a server bug reaches
every client's data. Where it matters, give each user their own `fsd` instance (own partition) or
`ipd` instance: memory, not redesign.
