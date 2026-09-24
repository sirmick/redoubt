# Packages, launching and code signing

Designed, not built. Owns: launching a process, the package format and what is signed, signer trust,
native user code, per-principal packages, system updates and the system signing key.
Principals and the powerbox: CAPABILITIES.md.

**Milestone 1 builds only launching** (below); every program comes from the system-signed boot
bundle. Everything else in this note is milestone 2.

## Launching a process
One mechanism for every process after `init`, at boot or at run time:
1. The launcher (`init` at boot, the steward afterwards) checks who signed the program (below).
2. It creates an empty process in the target budget, naming its exit endpoint (KERNEL-SPEC.md,
   `process_create`).
3. It maps the **loader stub** into the process: a flat binary, one code region at a fixed address,
   system-signed and the same for everyone. Mapping it needs no parsing.
4. It copies the program's ELF bytes into pages and maps them into the process, read-write, as data
   (`process_map`), then writes the startup block (INIT.md) into a page mapped read-only, and starts
   the process at the stub with that page's address as `process_start`'s `arg`. The startup block
   names the image (`image_addr`, `image_len`: INIT.md, Startup block), so the stub finds it
   without a fixed address.
5. The stub, running inside the new process's own budget, parses the ELF from memory and maps its
   segments at their link addresses with `map_fixed` (KERNEL-SPEC.md, answer 172), code executable
   and never writable, before it maps anything else; a segment overlapping the stub, the startup
   page or the image makes it exit. It then frees the image pages and jumps to the entry point,
   passing the startup page's address on.

`init` and the steward copy bytes; they never parse an ELF. A malicious ELF can at most compromise the
process it was going to become. The stub needs no file access, so `init` starts `bootfsd` and
everything else straight from the bundle. (seL4 and Fuchsia work this way.)

## No dynamic linking
Native code is statically linked and fixed at build time. Code shared at run time is a server, not a
library. The dynamic part of the system is the BEAM: modules load at run time through beamlet's
`Platform::load_module`.

## What is signed
**A package is signed as a whole**: a ustar archive with a manifest, signed the way the boot bundle
is (VERIFIED-BOOT.md owns the signature container). The boot bundle is the system's first package;
boot, system updates and user packages share one verifier. `.beam` archives and native programs are
covered as package contents. The steward records which key signed each installed program.

**The package container has its own domain** (milestone 2, when package signing lands): the
signature covers `"redoubt.pkg.v1\0" || u64_le(len) || tar`, never the bare archive, and `keyd`
signs a 32-byte digest of that preimage which it computed itself, never bytes a caller handed it.
The bundle's domain is `"redoubt.bundle.v1\0"` (VERIFIED-BOOT.md, which owns the rule that every
domain is prefix-free); a package signature is therefore never a bundle signature, and the reverse
(question 120).

The package manifest (strict JSON, WIRE.md) names the contents and **requests** capabilities
("`/net` connect to 443", "read my config directory"). It can grant nothing.

## Signer trust and granted authority (milestone 2)
**The steward launches code on a principal's behalf only if the principal trusts the signer, and
with at most what the principal grants.** Signing answers "who vouches for this code"; capabilities
decide "what it can do".
- At install or run time the principal grants the manifest's requests, attenuated from its own
  capabilities; the launcher builds the namespace from exactly those grants.
- System code (drivers, servers, the loader stub, beamlet) is signed by the system key. Users may run
  native code signed by keys on their trust list, including their own. `.beam` code follows the same
  rule.

**What signatures do not do.** A hijacked agent **can** run code it wrote: any process can create a
child and map pages into it (the launcher needs exactly that), and IEx evaluates any Elixir. The
property that holds is that **such code never runs with more authority than its author already
holds.** Signatures gate only what the steward launches with *new* grants.

What the signature buys:
- **No launch with new authority without trust.** Code gets grants from the steward only if a trusted
  key signed it; adding a key to a trust list is a high-stakes approval. Signing keys live in `keyd`,
  never in process memory.
- **Agents sign with their own keys.** Agent-built code runs within the agent's lease; running it
  outside needs the sponsor to trust the agent's key, a high-stakes approval.
- **Key compromise is containable.** Removing a key from a trust list stops its code launching; the
  steward records each process's signer, so running ones can be found and stopped.
- **Attribution:** every launched process has a signer in the audit log.

## Native user code: the bet and its backstops
Users may run native code, so every user can reach the full system-call interface; it must hold on
its own. It is small (KERNEL-SPEC.md), denial of service is bounded by budgets, the bench runs hostile
native programs as attack tests, and the interface gets a fuzz target.

## Per-principal packages and profiles (milestone 2)
- `pkg add` writes a package into **`/system/pkgs/<principal>/<name>-<version>-<hash>/`**, a directory
  only the steward writes. A principal's **profile** (the versions it uses, its `/bin`) and **trust
  list** (the signing keys whose code it runs) are steward records, not files in its space.
- **Installing** a package signed by a key already on your trust list is routine and needs no
  approval. **Adding a key** to a trust list is a high-stakes approval (CAPABILITIES.md).
- **Upgrading** is atomic per package: `pkg add` installs a new version beside the old, `pkg use`
  flips one steward record, flipping back is the rollback, and `pkg gc` removes unused versions.
- Neither launching nor beamlet's module loading ever consults anything in a session's writable
  namespace, so write access to someone's home does not become a launch with their authority.
- A project (CAPABILITIES.md) has its own package directory, so a shared toolchain is installed once.

```
alice> pkg add logscan-1.3.xpkg
  signed by: alice (on your trust list)   requests: read /logs, write ~/reports
alice> pkg use logscan 1.3                # pkg use logscan 1.2 rolls back
```

## System updates and the system signing key (milestone 2)
- **A/B bundle slots.** `sys update` writes the new signed bundle to the inactive slot; the loader
  verifies it on the next boot. The new system is **healthy** once `init` reaches a steady state
  (every system server running) **before any user session starts**, so a user's crash loop cannot
  force a rollback. If it fails to boot or to become healthy, the next boot falls back to the other
  slot.
- **Rollback protection:** a version counter, kept **outside both slots**, that the loader refuses
  to go below.
- **M-of-N signatures** on system bundles: one key owning every machine would be a single point of
  failure. Builds are **reproducible** and confirmed by independent builders.
- Today the loader checks one development key (VERIFIED-BOOT.md).

## Later: a shared content-addressed store
One store shared by all principals, packages named by their hash, deduplicated and garbage-collected,
sharing read-only code pages between users. Deferred because a store shared by everyone is a covert
channel (add a blob, probe for it) that needs uniform charges and restricted visibility to defend,
while its benefit (deduplication) is worth little with a handful of users.

## Developer flow (milestone 2)
```
cargo build --release --target riscv64gc-unknown-redoubt-elf
xpkg build && xpkg sign --key alice      # -> logscan-1.3.xpkg
pkg add logscan-1.3.xpkg                 # on the box, via the steward
```
