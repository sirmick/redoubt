# Init, supervision and the startup block

Status: agreed direction, 2026-09-18. Nothing here is built yet. Builds on CAPABILITIES.md,
RESOURCES.md, NAMESPACES.md.

## Decisions
1. **Two stages.** A tiny Rust `init` holds all authority at boot, starts and wires the system
   class, then hands the user side to the **steward** (the first Elixir VM) and keeps only what it
   needs to restart things.
2. **Every restart of an OS process is done by Rust init**: system servers, the steward, the SSH VM.
   OTP supervisors only restart Erlang processes inside a VM. Session VMs are not restarted: a dead
   session is a logout.
3. **Endpoints outlive servers.** An endpoint is its own kernel object, held by init. Clients hold
   capabilities to the endpoint; a restarted server receives on the same endpoint, so client handles
   and badges stay valid.
4. **Critical or optional**, per system server, in the manifest: "restart up to N times in T", then
   either stay down (optional: the network) or fail closed and reboot (critical: the block server).

## Boot
```
firmware -> loader (verifies bundle) -> kernel -> init (root budget, all device grants)
init: system budget; console, bootfs, blk, blockd, fs, net, linkd, ip, keyd, launcherd, auditd
init: steward VM (users budget, launcher, powerbox, console); sshd VM
steward: principals, sessions, agents
```
- The loader still creates the boot processes from the verified bundle (ELF parsing stays there
  until runtime launching is needed); they start **blocked until init delivers their startup block**.
- init parses only the verified manifest: server graph, grants, budgets, restart policies. It has no
  network and no user data.
- **First boot:** the first owner is created on the physical console (a trusted path) and enrolls a
  hardware key. The steward then holds the root user capabilities on the owners' behalf.
- **Network-facing components get their own VM** (sshd), holding only what they need: sshd can ask
  the steward to authenticate and start sessions, and asks keyd to sign with the host key.

## Restart semantics
- Calls in flight to a dead server fail with an error; the client retries.
- Server-side state (open 9P fids) is lost; the namespace library knows each handle's path and
  re-walks from the root, so most programs see a hiccup, not an error.

## Startup block
- The process's handles, installed in its handle table before it runs.
- A read-only page at a fixed address holding tagged entries (the kernel argument block's tag format):
  the namespace table (`"/"` -> handle 3, `"/dev/cons"` -> handle 4, ...), named service handles
  (`"keys"`, `"launcher"`, `"powerbox"`), arguments, its budget handle.
- No environment variables, nothing inherited. Configuration is files in the namespace.

## Worked example: Alice and Bob logged in over SSH
```
kernel
└── init (Rust)                                   root
    ├── console-drv bootfs blk-drv blockd fs:data     system [reserved]
    │   net-drv linkd ip:lan keyd launcherd auditd
    ├── steward VM          (restarted by init)       users
    ├── sshd VM             (restarted by init)       users/sshd
    ├── session VM alice-1                            users/alice/session-1
    │   └── logscan (native, via launcherd)           same budget
    ├── agent VM alice/researcher [lease 2 h]         users/alice/researcher
    └── session VM bob-1                              users/bob/session-1
```
CPU weights: alice 100, bob 100; the agent 20 inside Alice's share.

**Login:** ip:lan delivers port 22 only to sshd (sole holder of "listen TCP 22"); keyd signs with the
host key (never in sshd's memory); sshd asks the steward whose key it is (only the steward holds
`/system/principals`); the steward carves `users/alice/session-1`, builds her namespace from her root
set, and asks launcherd to start beamlet with it. Her shell's `.beam` files load through her `/bin`,
checked against her trusted keys. sshd reserves a status line on her terminal for powerbox prompts.

| Name | Alice's session | Bob's session | Enforced by |
| --- | --- | --- | --- |
| `/` | fs:data at `/home/alice`, rw | fs:data at `/home/bob`, rw | fs:data (badge) |
| `/bin`, `/store` | her profile, the store, ro | his, the store, ro | fs:data |
| `/dev/cons` | her SSH channel | his | sshd (badge) |
| `/net` | ip:lan, connect out 22, 443 | ip:lan, connect out 443 | ip:lan (badge) |
| `keys` | sign with Alice's keys (touch) | Bob's | keyd |
| `powerbox`, `launcher`, `budget` | hers | his | steward, launcherd, kernel |

Neither can name the other's home, `/system`, the host key, block devices, or listen on the network.

**Scenarios:**
- `cat notes.txt`: 9P on her `/` handle; fs:data's CPU time is charged to her (donation).
- `logscan /logs`: launcherd checks her signature, starts a native process in her budget with only
  what she granted.
- Sharing: fs:data mints a read-only capability at `/home/alice/shared`; the steward delivers it; Bob
  accepts on his status line; it is bound at `/shared/alice`; Alice's revoker kills it.
- Agent: own principal and VM, `/work` and an LLM-gateway handle, no `/net`; escalations prompt on
  Alice's status line; lease expiry destroys its budget.
- Bob spins: gets his share only. Bob allocates too much: `OutOfMemory` in his budget.
- Bob's VM crashes: the steward destroys his session budget; sshd closes the channel; Alice unaffected.
- Bob fully compromises his VM: he holds Bob's capabilities. Going further needs a bug in a server he
  talks to (fs:data, ip:lan, keyd, steward) or the kernel.
- fs:data crashes: restarted on the same endpoint; reads retried and re-walked; if it keeps crashing,
  it is critical, so init reboots.

**Weak spot:** users are separated everywhere except inside shared servers. Where it matters, give
each user their own fs instance (own partition) or IP stack (own VLAN): memory, not redesign.
