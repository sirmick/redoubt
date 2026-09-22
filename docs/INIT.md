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
- **Only `init` and the steward ever hold a handle to a system-class budget.** A server's startup
  block carries no `budget` handle, and a manifest that grants a server one is refused: a
  compromised `ipd` holding its budget could create system-class children with any labels and any
  account. Every budget shares one stride queue (RESOURCES.md): `init`, the steward and the drivers
  run at the large weights the manifest gives them, every other server at its ordinary manifest
  weight. Nothing runs ahead of the queue.
- **`init` refuses a manifest that hands `keyd` a key the box is authenticated by.** Two cases, each
  a boot failure: a key listed both as a principal's login or approval key and as a `keyd` key
  (CAPABILITIES.md, approvals), and **the key the loader verifies the boot bundle with**
  (VERIFIED-BOOT.md), which `init` carries as the same compiled-in constant. `keyd` cannot see
  either itself: it is given seeds and purposes, and not what the rest of the system does with the
  matching public keys. `init` holds no crypto and never derives a public key from a seed, so it
  asks `keyd` instead, once `keyd` is started and before anything else runs: `holds(public key)`,
  answered yes or no (WP-S1 writes the operation into `keyd`'s table), and a yes stops the boot.
  Domain separation already keeps a `keyd` signature from being a valid bundle signature
  (VERIFIED-BOOT.md); this check keeps the bundle key out of `keyd` at all, so the boot root and a
  key some badge may use are never one key. Closed from both sides, not from one.
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
| `servers` | each server's name, program (a bundle entry), budget (pages, processes, weight), device names, volume, the endpoints it receives on, the endpoints it is handed, and arguments (never its own budget) |
| `public` | the bundle entries `bootfsd` serves at `/boot`, by exact name: programs and module archives, and nothing else in the bundle |
| `principals` | milestone 1 only: each principal's name, SSH public keys for login and approval, budget, account, owned labels, the label sets it works under (each gets a fixed sub-budget of the principal's budget: pages, processes, weight), home (volume and path), and network scope (IP prefixes and ports) |
| `confined` | optional deployment flag (a boolean, at the top level); set, `init` refuses any placement of differing label sets (Confinement, below) |

Each field has one JSON type (WIRE.md): 64-bit quantities (label ids, accounts, page and byte sizes,
deadlines) are decimal strings; small counts (processes, weights, depths, restart limits) and ports
are numbers. A value of the wrong JSON type is an error.

**Confinement.** A manifest may carry a `confined` flag (a deployment profile; GAME.md, TENETS.md's
high/low pair). **It is a boot-wide property, not a per-domain one:** `init` parses it once and
applies it to the whole manifest. If it is set, `init` **refuses the boot** whenever two manifest
entries with **differing label sets** share any of these:

- a **server instance** — one `servers` entry serving them;
- a **volume** — one `volumes` entry they both attach;
- an **endpoint** — one name in a `servers` entry's `receives` or `handed` list they both hold;
- a **network instance** — one `ipd:*` (or `netd`) instance they both use (and a labelled domain is
given no `/net` at all: a sink refuses labelled callers);
- a **device object** — one `devices` entry they both hold. A shared disk, NIC or GPU is a shared
  scheduler, shared caches and a shared timing surface (CONTAINMENT.md, the channel table's
  disk/NIC/GPU row), so it is refused like any other sharing; a device the manifest clears for a
  label in a confined deployment needs its own instance per domain like the rest;
- a **core** — a hardware core their budgets both run on. Milestone 1 is one budget per core already
  (PLATFORM-FPGA.md, RESOURCES.md); a confined manifest that names more cores than budget groups is
  refused rather than silently time-sharing a core between two label sets.

What `init` compares is the **label set**: the labels each budget carries (`labels` in the manifest,
plus each principal's `label sets`), and for a volume the `label set` field of its `volumes` entry.
Two entries differ when their sets are not equal (`{a}` differs from `{}`, `{a}` and `{b}` differ, and
`{a}` and `{a,b}` differ; a system server such as the steward carries no labels and is therefore a
domain of its own). `init` also refuses a confined manifest in which a labelled domain reads a shared
unlabelled volume (input arrives by an audited push from the steward; CONTAINMENT.md, Push). The
refusal is a **boot failure, not a warning**. The default (no flag) is ordinary multi-tenancy, where a
shared server is acceptable and CONTAINMENT.md's residuals apply. **WP-R3 implements and tests this**;
the confinement attack verdict is the boot failing, not the manifest's claim (BUILD-PLAN.md).

It is a check on the **manifest**, not a run-time invariant: a budget, volume, endpoint or device
handed over after boot (by `mint`, by `keyd`'s `grant`, by a system server) is outside `init`'s
static comparison, and a system server that hands one across label sets is buggy, not the kernel
(CONTAINMENT.md, Labels). The confined flag is what makes the channel table's software-closed rows a
configuration rather than a default; it is not a second enforcer.

**Weights.** One stride queue serves everyone (RESOURCES.md), so the manifest's weights are the
whole scheduling policy. `init`, the steward and the drivers (`consoled`, `blkd`, `netd`) get
weights an order of magnitude above a session's — 1000 against a user's 100 — so that they are
served promptly without running ahead of the queue; the servers that work for users (`bootfsd`,
`fsd:*`, `ipd:*`, `keyd`, `sshd`) get ordinary weights and bound the work of one request. The
weights carve the system budget, like every other limit (R7).

**Names.** Every name in the manifest (devices, labels, volumes, servers, endpoints, principals) is
1-64 bytes of `[a-z0-9_:+-]`, starting with a letter (`fsd:data`, `alice+secrets`), and the
manifest decoder refuses any other. Names become endpoint names, volume names and 9P paths, so an
empty name, a NUL, U+FEFF or a C1 control must never reach them.

**Arguments.** A `servers` entry's arguments are **opaque strings**. `init` passes them through
unchanged and in order as the startup block's `argv` (below) and never interprets them: what they
mean belongs to the server, and **each server's note defines its own** (`keyd`'s
`name,purpose,seed`, for instance, written with WP-S1). `init` validates only their **count**,
their **length** and their **encoding**: each is well-formed UTF-8 with no NUL, and the count and
the lengths together must leave the startup block inside its one page, which is what bounds them.
A manifest breaking any of that is refused. Arguments are not names in the sense above, so the
name rule does not apply to them; a server that wants one validates it itself.

**What `/boot` shows.** `bootfsd` serves exactly the bundle entries the `public` list names,
matched byte for byte (no globbing, no prefixes), as one flat read-only directory; a walk to any
other name is "does not exist", so nothing there tells a caller what else the bundle holds
(NAMESPACES.md). `init` passes the list to `bootfsd` as its arguments, and refuses a manifest whose
`public` list names an entry the bundle does not hold, or names the manifest itself. **The manifest
is never public:** it carries `keyd`'s seeds (below) and every principal's account and keys, while
every session reaches `/boot` for its modules.

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
  steward in one typed message (its table is written with the steward, BUILD-PLAN.md WP-S2), and
  the steward's rule is in CONTAINMENT.md (Crash blame).
- **Reboot:** more than 5 restarts of one server within 60 seconds, not stopped by blame, reboots the
  machine (fail closed).
- **The steward:** if it dies in milestone 1, `init` destroys and recreates the users budget: every
  session is logged out. The steward is TCB; its crash is our bug.

## The system servers above the drivers
- **steward:** principals, authentication decisions, sessions, the powerbox, leases (as budgets),
  launching (PACKAGES.md), and, from milestone 2, packages, trust lists and profiles. It appends the
  audit log to a file only it can write (a separate audit server is deferred), signing each record
  through `keyd`'s `audit` purpose (CONTAINMENT.md): it holds a grant, never a key. It parses the most
  untrusted input in the system (every agent's requests), so it holds no keys and never parses an ELF.
  It filters requests by labels (CONTAINMENT.md). Its manifest weight is large, which is what keeps
  logout and ending a lease responsive; it bounds the work any one request can cause and relies on
  its caps. At
  boot it splits each principal's budget into the fixed sub-budgets the manifest names, one per
  label set, and carves sessions and leases from them. It passes a server a narrowing budget only
  as a revocation scope created for that purpose, never a budget that holds processes
  (CAPABILITIES.md).
- **keyd:** holds the keys the box uses on your behalf (in milestone 1 the SSH host key and the
  steward's audit key; principals' signing keys from milestone 2); signs
  on request, never exports. Each badge names one key and one purpose (for SSH, a signature over the
  session identifier `keyd` computed itself), never arbitrary bytes. It never holds keys that
  authenticate a person to the box (CAPABILITIES.md, approvals). Separate from the steward because a
  leaked key cannot be revoked; authority can. In milestone 1 its keys arrive as manifest
  arguments (`name,purpose,seed`, defined in `keyd`'s own note below, with its purposes and its
  typed messages), so **the private seeds live in `init`'s memory and in the bundle image**, at
  the same trust as the bundle: whoever can read the bundle image holds the box's private keys,
  and what keeps them off `/boot` is that the manifest is not public (above), not encryption —
  the bundle is signed, never encrypted (VERIFIED-BOOT.md). That is the stated residual for
  milestone 1. Milestone 2 seals them to the machine and generates them at first boot instead of
  shipping them in an image.
- **sshd:** the SSH front door (`sunset`: `no_std`, no allocation, by dropbear's author). It asks the
  steward to authenticate users and start sessions, and asks `keyd` to sign with the host key,
  through a root badge the **manifest** hands it (the steward never holds the host key's badge,
  and could not pass one on). It rejects any login key that `keyd` holds, by asking `keyd`
  `holds`. It serves `ssh approve@box`, in which only the steward
  talks. Its state is per channel, and each channel carries its session's labels; it is the one sink
  cleared for a label, on the channel the label's owner authenticated (CONTAINMENT.md), and in
  milestone 1 `approve@` shares it with every other channel (a stated residual; milestone 2 gives
  `approve@` its own instance or the console).
- Users' own outbound TLS and SSH (OTP `:ssl`, `:ssh`) run inside their VMs, in userland.

## keyd: keys, purposes and messages

**Its keys come from the manifest, one per argument** (`servers`, `arguments`). Milestone 1
generates no key on the box: `keyd` holds exactly what the signed bundle gave it, and there is no
enrolment, import or export operation for it to hold anything else. An argument is
`name,purpose,seed`, separated by commas (a comma is outside the name rule, so no field can swallow
another): `name` under the manifest's name rule, `purpose` from the table below, and `seed` the
Ed25519 secret seed (RFC 8032) as exactly 64 lower-case hex digits. `keyd` refuses to start on an
argument it cannot parse, an unknown purpose, a repeated name, two keys with the same public key,
or an all-zero seed — fail closed and loudly, since a key it cannot read is a key it cannot sign
with. It receives on the endpoint its startup block names `keyd`.

**A badge names one key and one purpose.** The **root badge of the key in argument *i* is *i***
(from 1), so `init` mints each root capability without asking `keyd` anything, and a restarted
`keyd` gives the same badges the same meaning from the same arguments, holding no state across the
restart (decision 5). Badges at or above 2^63 are minted by `grant` at run time and are never
reused. What a restart does to them needs saying, because "gone" is not something `keyd` can make
true: the endpoint outlives the server (decision 4) and a handle granted before the restart is
still a live handle afterwards, stamped with its requester. What protects it is that **each
incarnation draws its first granted badge at random above 2^63** (answer 126), so the badges the
restarted `keyd` gives out are not the ones stale handles carry, and a stale handle names no key —
`not_permitted`, like any badge `keyd` does not know. A counter that started in the same place
every time would instead have handed a stale handle whatever the first new client asked for.

**Only a root badge may grant.** A granted capability cannot grant another, so grants never chain.
Admission keys account 0 by badge (CONTAINMENT.md, because the budget a system caller shares does
not travel), and a chained grant would open a fresh bucket per link, so one system server could
spend every bucket `keyd` has and lock out the steward. Milestone 1 needs no chain: only the
steward and `sshd` hold `keyd` capabilities, both through root badges (answer 124).

**`release(0)` frees everything the caller granted.** A grant is never given the id 0, so it names
nothing else. It is what a holder asks for when its ids are gone — a server `init` restarted on the
same root badge knows none of them, and only the holder of an id can name a capability, so without
it that holder's share would stay full for the life of `keyd`.

| Purpose | Key | The one thing its badge may sign |
| --- | --- | --- |
| `ssh_host` | the box's SSH host key | `sign_ssh_exchange`: the exchange hash `keyd` computes, which is the session identifier |
| `audit` | the steward's audit key | `sign_record`: the digest of an audit record under the audit domain string |

A purpose bounds what a badge can get signed, not what that is worth: an `ssh_host` badge speaks
as the box in a key exchange, which is what it is for, so the steward grants one only to `sshd`.

`keyd`'s keys carry no labels in milestone 1, so `check` lets anyone read a public key and only an
unlabelled caller sign: a labelled (vault) session that needs to sign needs a labelled key, which
is milestone 2.

**It never holds a key that authenticates a person to the box** (CAPABILITIES.md, approvals). No
purpose signs an SSH user-authentication request, and `keyd` itself refuses any purpose outside the
table. That a manifest does not hand `keyd` a key it also lists as a principal's login or approval
key is checked in `init` (BUILD-PLAN.md, WP-R3), by **one mechanism and no other**: `init` asks
`keyd`, through a root badge, `holds` for each public key the manifest lists for a principal, and
refuses the manifest if any is held. `init` never derives a public key from a seed itself — it
would need the signature scheme's arithmetic to do it, and a second place that turns seeds into
keys is a second place that can be wrong about which key is which. The same call is how `init`
refuses a manifest that gives `keyd` the key the loader verifies the bundle with (answer 120), and
how `sshd` refuses a login with a key `keyd` holds.

**Messages.** A typed protocol, not 9P: `keyd` serves six fixed operations and no namespace, and a
file server's read and write would be the export this protocol must not have. Every request is
label-checked and resolved to one key and one purpose before `keyd` does any work for it; a badge
that names no key, names another key's purpose, or fails the label check gets `not_permitted`,
which says no more than that. **Admission counts grants, and only grants** — they are the one
thing a client can make `keyd` hold. `keyd` parks no call and keeps no other per-client state, so
a flood of signing requests makes it grow by nothing; what bounds that flood is the kernel's fair
waiting per (account, label set) (R2, CONTAINMENT.md) and the bound on one request's work below.

**Bounds.** At most 16 keys; a transcript part at most 16 KiB and an audit record at most 8 KiB,
so the work of one request is bounded by a number stated here rather than by the buffer that
carried it; at most 8 live grants per (account, label set), across at most 16 of those at once,
which is what `keyd`'s budget covers with every bucket at its cap (CONTAINMENT.md, answer 85).

**What each error answers.** `malformed` (code 1, as in every protocol): the request did not
decode, or its lengths are ones no sender could mean — a transcript with an empty part, or an
ephemeral key that is not a 32-byte Curve25519 point. `not_permitted`: the badge names no key,
names a key whose purpose does not allow this operation, fails the label check, is a granted badge
asking to grant, or named an id it did not receive — one answer for all of them, so a refusal says
only "not you". `too_many`: a cap is reached — a record or transcript part over its bound, or a
bucket or share with no room for another grant. `failed`: `keyd` could not do the work — no memory
for a record, no randomness for an id, or the kernel refused to mint. None of them says which.

<!-- wire: keyd -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `sign_ssh_exchange` | `v_c: bytes`, `v_s: bytes`, `i_c: bytes`, `i_s: bytes`, `q_c: bytes`, `q_s: bytes`, `k: bytes` | `signature: bytes` |
| 2 | `sign_record` | `record: bytes` | `signature: bytes` |
| 3 | `public_key` | - | `key: bytes` |
| 4 | `holds` | `key: bytes` | `held: u32` |
| 5 | `grant` | - | `id: u64`, `capability: handle[0] endpoint` |
| 6 | `release` | `id: u64` | - |

<!-- wire-errors: keyd -->
| Code | Error |
| --- | --- |
| 2 | `not_permitted` |
| 3 | `too_many` |
| 4 | `failed` |

- `sign_ssh_exchange` (purpose `ssh_host`) is the SSH exchange hash of RFC 4253 §8: `keyd` hashes
  `string V_C`, `string V_S`, `string I_C`, `string I_S`, `string K_S`, `string Q_C`, `string Q_S`,
  `mpint K` with SHA-256 and signs the 32-byte result. The caller passes the seven fields it knows;
  `K_S` is `string "ssh-ed25519" || string <public key>`, which **`keyd` builds from its own key**,
  so no caller can have it sign a transcript naming another host key. `k` is the shared secret
  already in `mpint` body form (the caller has it and computes the same hash for its own key
  derivation; encoding it here would make `keyd`'s work depend on the secret's leading bytes).
  The reply is the raw 64-byte Ed25519 signature; SSH's `string "ssh-ed25519" || string <sig>`
  framing is the caller's.
- `sign_record` (purpose `audit`) signs the SHA-256 of `"redoubt.audit.v1\0"`, the record's length
  as a little-endian `u64`, and the record. **Every signature `keyd` makes is over exactly 32
  bytes, and those bytes are always a digest `keyd` computed itself** — this one or the exchange
  hash. A domain string in front of a caller's bytes would not be enough on its own: it separates
  `keyd`'s purposes from each other, but not from a container that signs raw bytes, and the boot
  bundle's is `signature || tar` with no domain (VERIFIED-BOOT.md), whose first 100 bytes are a
  file name the attacker picks. Signing a digest closes that: no container whose messages are
  longer than 32 bytes can be what a `keyd` signature covers. Package signing (PACKAGES.md) gets
  its own domain here when it lands, and until then `init` should also refuse a manifest that
  gives `keyd` the key the loader verifies the bundle with (WP-R3, with the login-key check).
- `public_key` returns the 32 raw public-key bytes of the key the badge names. Neither it nor
  `holds` carries an algorithm name: the box has one signature scheme (VERIFIED-BOOT.md), and
  `sshd` frames `ssh-ed25519` itself, which it must do anyway. A second scheme would be a new
  message, not a string to branch on. `holds` answers 1 if `keyd` holds that public key and 0 if
  not, for a key the asker already has; public keys are published (the host key goes to every
  client that connects), so this reveals nothing, and it is how `init` and `sshd` ask whether a
  key is one of `keyd`'s. **Residual:** it answers about every key, not only the badge's, because
  that is the question its askers have; when keys carry labels (milestone 2) it needs a `check`
  per key rather than the badge's alone.
- `grant` mints a fresh capability with the caller's own key and purpose, the way `new_connection`
  does for 9P, because **a launcher never passes its own connection to a child**: the steward asks
  for one per session or lease rather than copying its own. It is stamped like the handle the
  request came through, so it dies with what the caller holds; the reply's `id` is random, and only
  the caller that received it may `release` it, which frees it and everything granted under it.
  Nothing granted is ever wider than the badge it came through, so there is no attenuation
  argument to get wrong.
- There is **no operation that returns a private key, or any function of one but a signature**, and
  none that adds, replaces or removes a key.

**Stated residuals.**
- `sign_ssh_exchange` hands `keyd` the session's shared secret `K`, because the exchange hash is
  computed over it and `keyd` computes that hash itself. So a compromised `keyd` does not only
  speak as the box: it can derive any session's keys and read the traffic. Having `sshd` pass the
  finished hash instead would avoid it, and would also turn the host key into an oracle that signs
  any 32 bytes, which is the property this design exists to keep; so the secret reaching `keyd`
  stands, and `keyd` is written to be small enough to read.
- The seeds live in `init`'s memory and in the bundle image, at the same trust as the bundle
  itself, which verified boot authenticates. `bootfsd` serves only the entries the manifest marks
  public, never the manifest (answer 123), so no session can read them. Milestone 2 generates the
  keys on the box at first boot and seals them to the machine, and then no seed is in a manifest
  at all.
- `holds` answers about every key, not only the badge's. When keys carry labels (milestone 2) it
  needs a `check` per key rather than the badge's alone.

## The shell
A session's shell is **IEx** (Elixir's interactive shell) on beamlet, with a small Redoubt helpers
module: `ls`, `cd` and `cat` over the namespace, `ps` and `budget` (showing only the session's own
(account, label set)), and a notice when an approval is waiting (`pkg` from milestone 2). IEx
evaluates any Elixir, with exactly the session's capabilities. It runs on the UART console before
SSH exists. In milestone 1 the physical console's IEx exists only in the bench build; the system
manifest starts no shell on the UART.

## Startup block
Before a process runs, its parent installs its handles in its table (`process_start` copies them
into slots 1..n, at most `MAX_START_HANDLES`; handle 0 is never a handle) and maps one ordinary page
into it, read-only (`process_map`), holding the block below. **`process_start`'s `arg` is that
page's address** (page-aligned; 0 = no block), which the child's first thread receives
(KERNEL-SPEC.md); there is no fixed address. The program image travels in its own pages, which the
block names (PACKAGES.md, launching; its fields are defined with the loader stub). No environment
variables, nothing inherited. Configuration is files in the namespace.

**Format.** The page starts with a `u32` byte length, then the block: one typed message (WIRE.md),
`startup`, laid out as a typed operation written into a file is (the opcode as a `u32`, then the
buffer-shape encoding of its fields). The length counts the message's bytes, not itself, and the
decoder reads exactly that many: a typed message carries no overall length of its own, and the
decoder refuses trailing bytes, so without the length the block could not be read out of a page
(question 112). `redoubt-wire` decodes it; there is no second framing format and no checksum (the
parent writes the block and could write any checksum too).

<!-- wire: startup -->
| Opcode | Message | Fields | Reply |
| --- | --- | --- | --- |
| 1 | `startup` | `version: u32`, `handle_count: u32`, `namespace: bytes`, `handles: bytes`, `argv: bytes` | - |

<!-- wire-errors: startup -->
| Code | Error |
| --- | --- |

- `version` is 1; `handle_count` is n, the number of handles `process_start` installed.
- `namespace` is a sequence of entries, each `handle: u32`, `path: string`: a clean absolute path
  (`/`, `/dev/cons`: no `.`, `..`, empty component or trailing `/`) and the connection it resolves
  to.
- `handles` is a sequence of entries, each `handle: u32`, `name: string`: a named handle, its name
  following the manifest's name rule (Names, above): services (`keys`, `powerbox`), device handles
  for drivers, and, for a session or agent only (never a server), its own budget as `budget`.
- `argv` is a sequence of `string`s, the arguments in order (each may be empty).

Rules: the length and the message it counts fit in one page; handles are 1..=n, n ≤
`MAX_START_HANDLES`; paths are unique
among `namespace` entries and names among `handles` entries; each `bytes` field holds whole entries
and nothing else; the rest of the page after the message is not read. A block breaking any rule is
refused whole. The parent may be hostile, so the child decodes defensively; `redoubt-rt` also
writes blocks for launchers.

**Launching gives fresh connections.** A launcher never places its own connection to a server in a
child's block; it asks the server for a fresh connection for the child (`new_connection`,
NAMESPACES.md) and passes that one (CAPABILITIES.md, one badge, one client). It keeps each
connection's id and disconnects it when it receives the child's exit notice; on the same notice it
**releases** every grant it took for the child from a typed server, by the ids that server returned
(WIRE.md, granting and releasing). This is a rule for `init`, the steward and every shell.

## Worked example: Alice, Bob and Alice's agent (milestone 1)
```
kernel
└── init (Rust)                                       root
    ├── consoled bootfsd blkd fsd:data fsd:alice-secrets system [reserved]
    │   netd ipd:lan keyd steward sshd
    ├── session VM alice-1  (IEx)                     users/alice/{}/session-1
    ├── agent VM alice/researcher [lease 2 h]         users/alice/{}/researcher
    ├── vault VM alice+secrets-1 (IEx)                users/alice/{alice-secrets}/session-1
    └── session VM bob-1    (IEx)                     users/bob/{}/session-1
```
`{}` and `{alice-secrets}` are the fixed sub-budgets the steward splits each principal's budget
into at boot, one per label set (CONTAINMENT.md); sessions and leases of one (principal, label set)
sit together under one, and are ended together.
CPU weights: alice 100, bob 100; the agent 20, carved from Alice's. The agent shares Alice's
account. `init`, the steward and the drivers are 1000 each in one queue with them (RESOURCES.md).

**Login:** `ipd:lan` delivers port 22 only to `sshd` (sole holder of "listen TCP 22"); `keyd` signs
with the host key (never in `sshd`'s memory); `sshd` asks the steward whose key it is (the steward
knows the principals from the boot manifest); the steward carves `users/alice/{}/session-1`, builds
her namespace, and launches beamlet with it; IEx's `.beam` files come from the system bundle.

| Name | Alice's session | Bob's session | Enforced by |
| --- | --- | --- | --- |
| `/` | `fsd:data` at `/home/alice`, rw | `fsd:data` at `/home/bob`, rw | `fsd` (badge) |
| `/dev/cons` | her SSH channel | his | `sshd` (badge, channel labels) |
| `/net` | `ipd:lan`, connect out to ports 22 and 443, not the box's own addresses | `ipd:lan`, connect out to 443 | `ipd` (badge) |
| `powerbox`, `budget` | hers | his | steward, kernel |

No session or lease holds `keys` in milestone 1: `keyd`'s purposes are the host key and audit
signing, and a `grant` mints only the granter's own key and purpose, so there is nothing a session
could be given (CAPABILITIES.md, agents 7). Principals' signing keys are milestone 2.

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
- Bob crashes `fsd:data` three times: each exit notice blames his account and empty label set (the
  failing thread's current call), so every session and lease under `users/bob/{}` is destroyed and
  he cannot log in again for 10 minutes; Alice, busy throughout, is not affected.

**Weak spot:** users are separated everywhere except inside shared servers, where a server bug reaches
every client's data. Where it matters, give each user their own `fsd` instance (own partition) or
`ipd` instance: memory, not redesign.
