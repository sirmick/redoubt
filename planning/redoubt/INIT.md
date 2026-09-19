# Init, the steward, restarts and the startup block

Designed, not built. Owns: what happens after the kernel starts, the system servers' roles, restart
semantics, the startup block, and the worked example. Server names: README.md. Launching a process:
PACKAGES.md.

## Decisions
1. **Two stages.** A tiny Rust `init` holds all authority at boot, starts and wires the system
   class, then hands the user side to the **steward** (a Rust system server) and keeps only what it
   needs to restart things.
2. **`init` restarts every OS process, with one rule:** restart, rate-limited. Before a crash counts,
   it is attributed (CONTAINMENT.md): a principal present in 3 consecutive crashes is logged out. If
   crashes continue with no principal consistently present, reboot. OTP supervisors only restart
   Erlang processes inside a VM. Session VMs are not restarted: a dead session is a logout.
3. **The BEAM is not in the TCB.** Everything whose compromise could cross principals is Rust: the
   steward, `keyd`, `sshd`. Elixir is userland: shells, applications, agents. A compromised VM holds
   exactly its principal's capabilities, like a native binary.
4. **Endpoints outlive servers.** An endpoint is its own kernel object, held by `init`. Clients hold
   handles to the endpoint; a restarted server receives on the same endpoint.
5. **Only the steward persists authority.** What a badge means lives in server memory and is lost on
   restart; a server that saved it could be made to write itself a root badge. The steward's records
   (principals, shares, leases, trust lists, profiles) live on a steward-only system volume; after a
   restart the steward re-mints cross-principal grants from them, and `init` re-delivers the
   manifest's grants. Their integrity rests on `blkd`, `fsd` and, until disk encryption returns
   (IO-ARCHITECTURE.md, Later), the trusted host.

## Boot
```
firmware -> loader (verifies bundle; loads kernel and init) -> kernel -> init (root budget, all device objects)
init:     system budget; consoled, bootfsd, blkd, fsd, netd, ipd, keyd
init:     steward (system class; holds the users budget), sshd
steward:  principals, sessions and agents (beamlet VMs)
```
- `init` starts every other process the same way the steward does (PACKAGES.md, Launching): from
  `bootfsd` until a disk is up. `init` parses only the verified boot manifest (server graph, device
  objects, budgets). It has no network and no user data.
- Device authority: today loader-emitted grants (DEVICE-GRANTS.md). Designed: `init` holds every
  device object and places each driver's handles in its startup block.
- **First boot:** the first owner is created on the physical console (a trusted path) and enrolls
  the approver credential (README.md glossary). The steward then holds the root user capabilities
  on the owners' behalf.

## The system servers above the drivers
- **steward:** principals, authentication decisions, sessions, the powerbox, leases (as budgets),
  packages, trust lists and profiles (PACKAGES.md), launching. It appends the audit log to a
  system-only file until there is a second writer. It parses the most untrusted input in the system
  (every agent's requests), so it holds no keys and never parses an ELF.
- **keyd:** holds every private key (host keys, principals' signing keys); signs on request, never
  exports. Separate from the steward because a leaked key cannot be revoked; authority can.
- **sshd:** the SSH front door (`sunset`: `no_std`, no allocation, by dropbear's author). It asks the
  steward to authenticate users and start sessions, and asks `keyd` to sign with the host key. It
  serves the approval sessions (`ssh approve@box`, `ssh approve-hs@box`), in which only the steward
  talks, and applies the terminal rule (CONTAINMENT.md).
- Users' own outbound TLS and SSH (OTP `:ssl`, `:ssh`) run inside their VMs, in userland.

## The shell
A session's shell is **IEx** (Elixir's interactive shell) on beamlet, with a small Redoubt helpers
module: `ls`, `cd` and `cat` over the namespace, `pkg`, `ps`, `budget`, and a notice when an approval
is waiting. IEx evaluates any Elixir, with exactly the session's capabilities.

## Restart semantics
- Calls in flight to a dead server, and senders blocked on it, get an error; the client retries.
- Server-side state (open 9P fids) is lost; the namespace library knows each handle's path and
  re-walks from the root, so most programs see a hiccup, not an error.

## Startup block
- The process's handles, installed in its handle table before it runs.
- An ordinary page the parent writes and maps into the child, holding tagged entries (the kernel
  argument block's tag format): the namespace table (`"/"` -> handle 3, `"/dev/cons"` -> handle 4,
  ...), named service handles (`"keys"`, `"powerbox"`), device handles for drivers, the handle of its
  own ELF (PACKAGES.md), arguments, its budget handle.
- No environment variables, nothing inherited. Configuration is files in the namespace.

## Worked example: Alice, Bob and Alice's agent
```
kernel
└── init (Rust)                                       root
    ├── consoled bootfsd blkd fsd:data fsd:alice-secrets system [reserved]
    │   netd ipd:lan keyd steward sshd
    ├── session VM alice-1  (IEx)                     users/alice/session-1
    │   └── logscan (native)                          same budget
    ├── agent VM alice/researcher [lease 2 h]         users/alice/researcher
    └── session VM bob-1    (IEx)                     users/bob/session-1
```
CPU weights: alice 100, bob 100; the agent 20, carved from Alice's.

**Login:** `ipd:lan` delivers port 22 only to `sshd` (sole holder of "listen TCP 22"); `keyd` signs
with the host key (never in `sshd`'s memory); `sshd` asks the steward whose key it is (only the
steward holds the principal records); the steward carves `users/alice/session-1`, builds her
namespace from her root set, and launches beamlet with it. IEx's `.beam` files load from her profile,
checked against her trust list (both steward state).

| Name | Alice's session | Bob's session | Enforced by |
| --- | --- | --- | --- |
| `/` | `fsd:data` at `/home/alice`, rw | `fsd:data` at `/home/bob`, rw | `fsd` (badge) |
| `/bin` | her packages, ro | his, ro | `fsd` |
| `/dev/cons` | her SSH channel | his | `sshd` (badge, terminal rule) |
| `/net` | `ipd:lan`, connect out to ports 22 and 443 | `ipd:lan`, connect out to 443 | `ipd` (badge) |
| `keys` | sign with Alice's keys | Bob's | `keyd` |
| `powerbox`, `budget` | hers | his | steward, kernel |

Neither can name the other's home, `/system`, `fsd:alice-secrets`, the host key or block devices, or
listen on the network.

**Scenarios:**
- `cat notes.txt`: 9P on her `/` handle; `fsd` limits her by her principal id.
- `logscan /logs`: the steward checks the package's signer against her trust list; the new process's
  loader stub maps its own ELF in her budget, with only what she granted.
- Vault: `ssh alice+secrets@box` gives a session labelled `alice-secrets` that can read
  `fsd:alice-secrets`, has no `/net`, and prints only to Alice's own terminal.
- Sharing: the steward creates a sub-budget of Alice's for the share; `fsd` mints a read-only
  capability at `/home/alice/shared` into it; Bob accepts in his approval session; it is bound at
  `/shared/alice`. Alice un-shares: the sub-budget is destroyed, and everything derived from the
  share dies with it.
- Agent: own principal and VM, a 2-hour lease, `/work` only, no `/net`. The bench's scripted hostile
  agent tries to read outside `/work`, reach the network, outlive its lease, message Bob and spoof
  the approval screen; each attempt is refused. Its escalations wait for Alice in `ssh approve@box`;
  lease expiry destroys its budget and everything it passed on.
- Bob spins: he gets his share only. Bob allocates too much: `OutOfMemory` in his budget.
- Bob's VM crashes: the steward destroys his session budget; `sshd` closes the channel; Alice is
  unaffected.
- Bob fully compromises his VM (a beamlet bug): he holds Bob's capabilities, nothing more. Going
  further needs a bug in a server he talks to (`fsd`, `ipd`, `keyd`, the steward) or the kernel.
- `fsd:data` crashes: it restarts on the same endpoint; Alice's reads are retried and re-walked. If
  Bob was in flight in 3 consecutive crashes, he is logged out.

**Weak spot:** users are separated everywhere except inside shared servers, where a server bug reaches
every client's data. Where it matters, give each user their own `fsd` instance (own partition) or
`ipd` instance: memory, not redesign.
