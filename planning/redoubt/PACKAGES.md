# Packages, launching and code signing

Designed, not built. Owns: the package format, launching a process, per-principal packages and
profiles, signer trust, native user code, system updates and the system signing key.
Principals and the powerbox: CAPABILITIES.md.

## Format: one format, one verifier
A package is a ustar archive with a manifest, signed the way the boot bundle is (VERIFIED-BOOT.md
owns the signature container). The boot bundle is the system's first package. Boot, system updates
and user packages share one verifier.

The package manifest names the contents (ELFs, `.beam` archives, data) and **requests**
capabilities ("`/net` connect to 443", "read my config directory"). It can grant nothing.

## No dynamic linking
Native code is statically linked, signed as one binary, and fixed at build time. Code shared at run
time is a server, not a library. The dynamic part of the system is the BEAM: modules load at run time
through beamlet's `Platform::load_module`, where signatures are checked.

## Launching a process
One mechanism for every process after `init`, at boot or at run time:
1. The launcher (`init` at boot, the steward afterwards) checks the ELF's signer against the runner's
   trust list. Checking a signature needs no ELF parsing.
2. It creates an empty process in the target budget, writes its startup block (INIT.md), including
   a handle to the ELF file, and starts it.
3. The process begins as the **loader stub**: a few hundred lines, system-signed, the same for
   everyone. Running inside the new process's own budget, it reads its ELF, maps its segments and
   jumps to the entry point.

`init` and the steward never parse an ELF. A malicious ELF can at most compromise the process it was
going to become. The kernel primitive is small: create an empty process in a budget (naming its
exit endpoint), map pages into it, start it with handles. (seL4 and Fuchsia work this way.)

## Per-principal packages and profiles
- `pkg add` writes a package into **`/system/pkgs/<principal>/<name>-<version>-<hash>/`**, a directory
  only the steward writes. A principal's **profile** (the versions it uses, its `/bin`) and **trust
  list** (the signing keys whose code it runs) are steward records, not files in its space.
- **Installing** a package signed by a key already on your trust list is routine and needs no
  approval. **Adding a key** to a trust list is a high-stakes approval (CAPABILITIES.md).
- **Upgrading** is atomic per package: `pkg add` installs a new version beside the old, `pkg use`
  flips one steward record, flipping back is the rollback, and `pkg gc` removes unused versions.
- Neither launching nor beamlet's module loading ever consults anything in a session's writable
  namespace, so write access to someone's home does not become code execution as them.
- Two principals can use different versions with no conflict. A project (CAPABILITIES.md) has its
  own package directory, so a shared toolchain is installed once.

```
alice> pkg add logscan-1.3.xpkg
  signed by: alice (on your trust list)   requests: read /logs, write ~/reports
alice> pkg use logscan 1.3                # pkg use logscan 1.2 rolls back
```

## Running: signer trust and granted authority
**Code runs only if the runner trusts the signer, and runs with at most what the runner grants.**
Signing answers "who vouches for this code"; capabilities decide "what it can do".
- At install or run time the runner grants the manifest's requests, attenuated from its own
  capabilities; the launcher builds the namespace from exactly those grants.
- TCB code (drivers, servers, the loader stub, beamlet) must be signed by the system key. Users may
  run native code signed by keys on their trust list, including their own. `.beam` code follows the
  same rule.

What the signature buys:
- **No drive-by execution.** A compromised app or hijacked agent that writes a binary cannot run it
  without a trusted signature, and cannot add its own key without a high-stakes approval. Signing
  keys live in `keyd`, never in process memory.
- **Agents sign with their own keys.** Agent-built code runs within the agent's lease; running it
  outside needs the sponsor to trust the agent's key, a high-stakes approval.
- **Key compromise is containable.** Removing a key from a trust list stops its code launching; the
  steward records each process's signer, so running ones can be found and stopped.
- **Attribution:** every process has a signer in the audit log.

## Native user code: the bet and its backstops
Native user code is needed, so any user can reach the full syscall interface. The kernel's surface
must hold on its own; it is small (CAPABILITIES.md: a process without handles can only manage its
own memory and threads and receive with a timeout). Denial of service is bounded by budgets. The
bench runs hostile native programs as attack tests; the syscall interface gets a fuzz target.

## System updates and the system signing key
- **A/B bundle slots.** `sys update` writes the new signed bundle to the inactive slot; the loader
  verifies it on the next boot; if it fails to boot or to confirm health, the next boot falls back
  to the other slot. Atomic: the whole old system or the whole new one.
- **M-of-N signatures** on system bundles: one key owning every machine would be a single point of
  failure. Builds are **reproducible** and confirmed by independent builders.
- **Rollback protection:** a version counter the loader refuses to go below.
- Today the loader checks one development key (VERIFIED-BOOT.md).

## Later: a shared content-addressed store
One store shared by all principals, packages named by their hash, deduplicated and garbage-collected,
sharing read-only code pages between users. Deferred because a store shared by everyone is a covert
channel (add a blob, probe for it) that needs uniform charges and closure-only visibility to defend,
while its benefit (deduplication) is worth little with a handful of users.

## Developer flow (target)
```
cargo build --release --target riscv64gc-unknown-xous-elf
xpkg build && xpkg sign --key alice      # -> logscan-1.3.xpkg
pkg add logscan-1.3.xpkg                 # on the box, via the steward
```
