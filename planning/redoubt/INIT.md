# Init, the steward, restarts and the startup block

Designed, not built. Owns: what happens after the kernel starts, the system servers' roles, restart
semantics, the startup block, and the worked example. Server names: README.md.

## Decisions
1. **Two stages.** A tiny Rust `init` holds all authority at boot, starts and wires the system
   class, then hands the user side to the **steward** (a Rust server) and keeps only what it needs to
   restart things.
2. **Every restart of an OS process is done by `init`**, with one rule: restart, rate-limited; if a
   server keeps crashing, reboot. Before a crash counts, the caller being served is quarantined
   (CONTAINMENT.md). OTP supervisors only restart Erlang processes inside a VM. Session VMs are not
   restarted: a dead session is a logout.
3. **The BEAM is not in the TCB.** Everything whose compromise could cross principals is Rust: the
   steward, `keyd`, `sshd`. Elixir is userland: shells, applications, agents. A compromised VM holds
   exactly its principal's capabilities, like a native binary.
4. **Endpoints outlive servers.** An endpoint is its own kernel object, held by `init`. Clients hold
   capabilities to the endpoint; a restarted server receives on the same endpoint.
5. **Servers persist nothing that grants authority.** What a badge means lives in server memory and
   is lost on restart; a server that saved it could be made to write itself a root badge. After a
   restart, the steward re-mints cross-principal grants (shares, leases) and `init` re-delivers the
   manifest's grants. Messages queued in the endpoint when the server died are failed, not replayed.

## Boot
```
firmware -> loader (verifies bundle) -> kernel -> init (root budget, all device authority)
init:     system budget; consoled, bootfsd, blkd, blockd, fsd, netd, ipd, keyd, gatewayd
init:     steward (users budget, powerbox, console); sshd
steward:  principals, sessions and agents (beamlet VMs)
```
- The loader creates the boot processes from the verified bundle (ELF parsing stays there until
  runtime launching is needed); they start **blocked until init delivers their startup block**.
- `init` parses only the verified manifest: server graph, device handles, budgets. It has no network
  and no user data. Device authority today comes from loader-emitted grants (DEVICE-GRANTS.md);
  with `init`, `init` holds the device handles and delegates them.
- **First boot:** the first owner is created on the physical console (a trusted path) and enrolls a
  hardware key. The steward then holds the root user capabilities on the owners' behalf.

## The system servers above the drivers
- **steward:** principals, authentication decisions, sessions, the powerbox, leases (as budgets),
  trust lists and profiles (PACKAGES.md). Launching is a library inside it (ELF parsing runs only on
  signature-verified input). It appends the audit log to a system-only file until there is a second
  writer. It parses the most untrusted input in the system (every agent's requests), so it holds no
  keys.
- **keyd:** holds every private key (host keys, principals' signing keys, the disk key); signs and
  decrypts on request, never exports. Separate from the steward because a leaked key cannot be
  revoked; authority can.
- **sshd:** the SSH front door (`sunset`: `no_std`, no allocation, by dropbear's author). It asks the
  steward to authenticate users and start sessions, and asks `keyd` to sign with the host key.
  It also serves the approval session (`ssh approve@box`), in which only the steward talks.
- **The browser GUI (`webd`)** is deferred (IO-ARCHITECTURE.md, Later).
- Users' own outbound TLS and SSH (OTP `:ssl`, `:ssh`) run inside their VMs, in userland.

## Restart semantics
- Calls in flight to a dead server fail with an error; the client retries.
- Server-side state (open 9P fids) is lost; the namespace library knows each handle's path and
  re-walks from the root, so most programs see a hiccup, not an error.

## Startup block
- The process's handles, installed in its handle table before it runs.
- A read-only page at a fixed address holding tagged entries (the kernel argument block's tag
  format): the namespace table (`"/"` -> handle 3, `"/dev/cons"` -> handle 4, ...), named service
  handles (`"keys"`, `"powerbox"`), device handles for drivers, arguments, its budget handle.
- No environment variables, nothing inherited. Configuration is files in the namespace.

## Worked example: Alice and Bob logged in over SSH
```
kernel
└── init (Rust)                                       root
    ├── consoled bootfsd blkd blockd fsd:data         system [reserved]
    │   netd ipd:lan keyd gatewayd
    ├── steward  (restarted by init)                  system
    ├── sshd     (restarted by init)                  system
    ├── session VM alice-1                            users/alice/session-1
    │   └── logscan (native, launched by the steward) same budget
    ├── agent VM alice/researcher [lease 2 h]         users/alice/researcher
    └── session VM bob-1                              users/bob/session-1
```
CPU weights: alice 100, bob 100; the agent 20 inside Alice's share.

**Login:** `ipd:lan` delivers port 22 only to `sshd` (sole holder of "listen TCP 22"); `keyd` signs
with the host key (never in `sshd`'s memory); `sshd` asks the steward whose key it is (only the
steward holds `/system/principals`); the steward carves `users/alice/session-1`, builds her namespace
from her root set, and launches beamlet with it. Her shell's `.beam` files load from her profile,
checked against her trust list (both steward state).

| Name | Alice's session | Bob's session | Enforced by |
| --- | --- | --- | --- |
| `/` | `fsd:data` at `/home/alice`, rw | `fsd:data` at `/home/bob`, rw | `fsd` (badge) |
| `/bin`, `/store` | her profile, the store, ro | his, the store, ro | `fsd` |
| `/dev/cons` | her SSH channel | his | `sshd` (badge) |
| `/net` | `ipd:lan`, connect out to ports 22 and 443 | `ipd:lan`, connect out to 443 | `ipd` (badge) |
| `keys` | sign with Alice's keys | Bob's | `keyd` |
| `powerbox`, `budget` | hers | his | steward, kernel |

Neither can name the other's home, `/system`, the host key or block devices, or listen on the network.

**Scenarios:**
- `cat notes.txt`: 9P on her `/` handle; `fsd` limits her by her budget id.
- `logscan /logs`: the steward checks the package's signer against her trust list and starts a
  native process in her budget with only what she granted.
- Sharing: the steward creates a sub-budget for the share; `fsd` mints a read-only capability at
  `/home/alice/shared` stamped with it; Bob accepts in his approval session; it is bound at
  `/shared/alice`. Alice un-shares: the sub-budget is destroyed, and everything derived from the
  share dies with it.
- Agent: own principal and VM, `/work` and a `gatewayd` handle, no `/net`; its escalations wait for
  Alice in `ssh approve@box`; lease expiry destroys its budget.
- Bob spins: he gets his share only. Bob allocates too much: `OutOfMemory` in his budget.
- Bob's VM crashes: the steward destroys his session budget; `sshd` closes the channel; Alice is
  unaffected.
- Bob fully compromises his VM (a beamlet bug): he holds Bob's capabilities, nothing more. Going
  further needs a bug in a server he talks to (`fsd`, `ipd`, `keyd`, the steward) or the kernel.
- `fsd:data` crashes while serving Bob: Bob's budget is quarantined from it and it restarts on the
  same endpoint; Alice's reads are retried and re-walked. If it keeps crashing, `init` reboots.

**Weak spot:** users are separated everywhere except inside shared servers, where a server bug reaches
every client's data. Where it matters, give each user their own `fsd` instance (own partition) or
`ipd` instance: memory, not redesign.
