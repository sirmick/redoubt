# Packages, the store and code signing

Status: agreed direction, 2026-09-18. Nothing here is built yet. Principals and capabilities:
CAPABILITIES.md.

## Format: one format, one verifier
A package is a ustar archive with a manifest and an Ed25519 signature over its hash: the format the
boot bundle already uses and `loader64` already verifies. The boot bundle is the system's first
package, signed by the system key. Boot, system updates and user packages share one verifier.

The manifest names the contents (ELFs, `.beam` archives, data) and **requests** capabilities
("`/net` connect to 443", "read my config directory"). It can grant nothing.

## The store
- **Content-addressed**, shared, immutable once written: a package is named by its hash, so adding
  one can never overwrite anything. Anyone may add; it is charged to the adder's disk quota.
- **Deduplicated**; garbage-collected when no profile references a package.
- The launcher shares read-only pages of identical ELFs across processes (no dynamic linking).

## Profiles: installing is binding into your own namespace
A principal's `/bin` is a directory in its own space listing the packages it chose (Nix profiles;
no union mounts). Two principals can use different versions with no conflict. **Installing for
yourself needs no approval.** System updates are atomic: a new system is a new set of hashes;
rollback switches back.

## Running: signer trust and granted authority
**Code runs only if the runner trusts the signer, and runs with at most what the runner grants.**
Signing answers "who vouches for this code"; capabilities decide "what it can do".
- Each principal keeps a trusted-keys file in its own space: its own key and the system key by default.
- At install or run time the runner grants the manifest's requests, attenuated from its own
  capabilities. The launcher builds the namespace from exactly those grants.
- TCB code (drivers, servers, the launcher) must be signed by the system key. Users may run native
  code signed by keys they trust, including their own. `.beam` code follows the same rule through
  beamlet's `Platform::load_module`.

What the signature buys:
- **No drive-by execution.** A compromised app or hijacked agent that writes a binary cannot run it
  natively without the principal's signature. Signing keys live in the key server, never in process
  memory; signing needs the principal (e.g. a hardware-key touch).
- **Agents sign with their own keys.** Agent-built code runs within the agent's lease; running it
  outside needs an explicit, revocable trust decision by the sponsor.
- **Key compromise is containable.** Removing a key from a trust list stops its code launching; the
  launcher records each process's signer, so running ones can be found and stopped.
- **Attribution:** every process has a signer in the audit log.

## Native user code: the bet and its backstops
Allowing it exposes the full syscall interface to any user; we assume a really good kernel and keep
the bet small:
- Without handles, a process can only manage its own memory and threads, yield, read the clock and
  use handles it holds; everything else is a server checking badges.
- Denial of service is bounded by per-principal quotas (memory, handles, threads, CPU share).
- The bench runs hostile native programs as attack tests; the syscall interface gets a fuzz target.

## Developer flow (target)
```
cargo build --release --target riscv64gc-unknown-xous-elf
xpkg build && xpkg sign --key alice      # -> logscan-<hash>.xpkg
pkg add logscan-<hash>.xpkg              # on the box: store, profile, grants
```
