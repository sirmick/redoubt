# Packages, the store and code signing

Designed, not built. Owns: package format, the store, profiles, signer trust, native user code.
Principals and the powerbox: CAPABILITIES.md.

## Format: one format, one verifier
A package is a ustar archive with a manifest and an Ed25519 signature over its hash: the format the
boot bundle already uses and the loader already verifies (BOOT.md, VERIFIED-BOOT.md). The boot
bundle is the system's first package. Boot, system updates and user packages share one verifier.
System packages need M-of-N signatures (CONTAINMENT.md).

The manifest names the contents (ELFs, `.beam` archives, data) and **requests** capabilities
("`/net` connect to 443", "read my config directory"). It can grant nothing.

## No dynamic linking
Native code is statically linked, signed as one binary, and fixed at build time. Code shared at run
time is a server, not a library. Identical ELFs share read-only pages across processes. The dynamic
part of the system is the BEAM: modules load at run time through beamlet's `Platform::load_module`,
where signatures are checked.

## The store
- **Content-addressed**, shared, immutable once written: a package is named by its hash, so adding
  one can never overwrite anything. Anyone may add; it is charged to the adder's disk budget.
- **Uniform charges:** adding is charged in full and answered the same way whether or not the blob
  already exists, so quota and timing do not reveal what others installed. A principal sees only
  its own profile's closure.
- Garbage-collected when no profile references a package.

## Profiles and trust lists are steward state
- A principal's **profile** (its `/bin`: the packages it chose) and its **trust list** (the signing
  keys whose code it runs) are held by the steward, not stored as files in the principal's space.
  Two principals can use different versions with no conflict (Nix profiles; no union mounts).
- **Installing** a package signed by a key already on your trust list is routine and needs no
  approval. **Adding a key** to a trust list is a high-stakes approval (CAPABILITIES.md).
- Neither launching nor beamlet's module loading ever consults anything in a session's writable
  namespace. So write access to someone's home does not become code execution as them.
- System updates are atomic: a new system is a new set of hashes; rollback switches back.

## Running: signer trust and granted authority
**Code runs only if the runner trusts the signer, and runs with at most what the runner grants.**
Signing answers "who vouches for this code"; capabilities decide "what it can do".
- At install or run time the runner grants the manifest's requests, attenuated from its own
  capabilities. The steward's launcher builds the namespace from exactly those grants.
- TCB code (drivers, servers) must be signed by the system key. Users may run native code signed by
  keys on their trust list, including their own. `.beam` code follows the same rule.

What the signature buys:
- **No drive-by execution.** A compromised app or hijacked agent that writes a binary cannot run it
  without a trusted signature, and cannot add its own key without an out-of-band approval. Signing
  keys live in `keyd`, never in process memory.
- **Agents sign with their own keys.** Agent-built code runs within the agent's lease; running it
  outside needs the sponsor to trust the agent's key, a high-stakes approval.
- **Key compromise is containable.** Removing a key from a trust list stops its code launching; the
  launcher records each process's signer, so running ones can be found and stopped.
- **Attribution:** every process has a signer in the audit log.

## Native user code: the bet and its backstops
Native user code is needed, so any user can reach the full syscall interface. The kernel's surface
must hold on its own; we keep it small:
- Without handles, a process can only manage its own memory and threads, receive (with a timeout)
  and use handles it holds; everything else is a server checking badges.
- Denial of service is bounded by budgets (memory, handles, threads, CPU weight).
- The bench runs hostile native programs as attack tests; the syscall interface gets a fuzz target.

## Developer flow (target)
```
cargo build --release --target riscv64gc-unknown-xous-elf
xpkg build && xpkg sign --key alice      # -> logscan-<hash>.xpkg
pkg add logscan-<hash>.xpkg              # on the box: store and profile, via the steward
```
