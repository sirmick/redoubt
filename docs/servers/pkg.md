# Packages

A **package** is a signed ustar archive with a manifest: the unit in which code arrives on the box,
from the boot bundle itself to one principal's tool. The steward installs packages per principal,
keeps each principal's profile and trust list, and launches a package's code only if its principal
trusts the signer, with at most what the principal grants. System updates are packages too, written
to one of two bundle slots and protected against rollback.

## Purpose

Signing answers "who vouches for this code"; capabilities answer "what can it do". Redoubt keeps the
two apart: a signature never grants authority, and authority is granted only to code a trusted key
signed. One container, one verifier and one domain rule cover the boot bundle, system updates and
every user package, so there is one parser of signed archives to trust.

## Interface

### What is signed

Status: planned · M5 (persist, install, share)

- **A package is signed as a whole**: a ustar archive with a manifest, in the boot bundle's
  signature container ([boot](../kernel/boot.md)). `.beam` archives and native programs are package
  contents.
- **Its own domain.** The signature covers `"redoubt.pkg.v1\0" || u64_le(len) || archive`, never the
  bare archive. The bundle's domain is `"redoubt.bundle.v1\0"`, and domains are prefix-free, so a
  package signature is never a bundle signature, and the reverse. `keyd` signs a 32-byte digest of the
  preimage that it computed itself ([keyd](keyd.md)).
- **The manifest** is strict JSON ([wire](wire.md#strict-json)). It names the contents and
  **requests** capabilities ("`/net` connect to 443", "read my config directory"); it can grant
  nothing.
- The steward records which key signed each installed program.

**Open:** the package manifest's schema; the maximum package size and how an archive is streamed to
the verifier.

### Signer trust and granted authority

Status: planned · M5 (persist, install, share)

- **The steward launches code for a principal only if the principal trusts the signer**, and with at
  most what the principal grants: at install or run time the principal grants the manifest's
  requests, attenuated from its own capabilities, and the launcher builds the namespace from exactly
  those grants ([R71 (no new authority without trust)](#r71-no-new-authority-without-trust)).
- **System code** (drivers, servers, the loader stub, the VM) is signed by the system key. A principal
  may run native code signed by keys on its trust list, its own included; `.beam` code follows the
  same rule.
- **Adding a key to a trust list** is a high-stakes approval
  ([steward](steward.md#the-powerbox-and-approvals)). Signing keys live in `keyd`, never in a
  process's memory.
- **Agents sign with their own keys.** Code an agent built runs within the agent's lease; running it
  outside needs the sponsor to trust the agent's key.
- **Removing a key** from a trust list stops its code launching; the steward records each process's
  signer, so running ones can be found and stopped. Every launched process has a signer in the audit
  log.

**Open:** none.

### Per-principal packages and profiles

Status: planned · M5 (persist, install, share)

- `pkg add` writes a package into `/system/pkgs/<principal>/<name>-<version>-<hash>/`, a directory only
  the steward writes. A principal's **profile** (the versions it uses, its `/bin`) and **trust list**
  are steward records, not files in its space.
- **Installing** a package signed by a key already on the trust list needs no approval.
- **Upgrading is atomic per package**: `pkg add` installs a new version beside the old, `pkg use` flips
  one steward record, flipping back is the rollback, and `pkg gc` removes unused versions.
- **Launching never consults a session's writable namespace**, and neither does the VM's module
  loading, so write access to someone's home never becomes a launch with their authority.
- A project has its own package directory, so a shared toolchain is installed once.

```
alice> pkg add logscan-1.3.xpkg
  signed by: alice (on your trust list)   requests: read /logs, write ~/reports
alice> pkg use logscan 1.3                # pkg use logscan 1.2 rolls back
```
*Installing and choosing a version.*

**Open:** whether `pkg` is a command talking to the steward or a server of its own; the protocol it
uses.

### System updates

Status: planned · M5 (persist, install, share)

- **A/B bundle slots.** `sys update` writes the new signed bundle to the inactive slot; the loader
  verifies it on the next boot. The new system is **healthy** once `init` reaches a steady state,
  every system server running, **before any user session starts**, so a user's crash loop cannot force
  a rollback. If it fails to boot or to become healthy, the next boot falls back to the other slot.
- **Rollback protection.** A version counter kept outside both slots, which the loader refuses to go
  below ([R72 (no rollback below the counter)](#r72-no-rollback-below-the-counter)).
- **M-of-N signatures** on system bundles, so that no one key owns every machine, and builds are
  reproducible and confirmed by independent builders.

**Open:** where the version counter lives on a virtio-only machine and what protects it; the M and
N, and how the loader holds the N keys.

## Authority

Status: planned · M5 (persist, install, share)

- The steward holds the package directories and the profile and trust records, and writes them; no
  principal writes its own.
- A package's code gets exactly the grants its principal made from the manifest's requests, never
  more than the principal holds.
- A signature grants nothing.

**Open:** none.

## Security properties

### R71 (no new authority without trust)

Status: planned · M5 (persist, install, share)

The steward launches code with grants a principal makes only if a key on that principal's trust list
signed it, and with at most those grants. Code a principal or its agent wrote runs, but never with
more authority than its author already holds.

**Open:** none.

### R72 (no rollback below the counter)

Status: planned · M5 (persist, install, share)

The loader refuses a system bundle whose version is below the counter kept outside both slots, so a
validly signed but older, vulnerable system cannot be booted in place of a newer one.

**Open:** none.

## Failure and restart

Status: planned · M5 (persist, install, share)

- **An update fails to boot or to become healthy:** the next boot uses the other slot.
- **An install is interrupted:** the new version's directory is incomplete and unused; the profile
  still names the old one, and `pkg gc` removes the remains.

**Open:** none.

## Residual risks

- **A hijacked agent runs code it wrote,** within its own authority: any process can create a child
  and map pages into it, and the shell evaluates any Elixir. Signatures gate only new grants.
- **Every principal reaches the whole system-call interface** through native code; it must hold on its
  own, and the bench attacks it with hostile native programs.
- **A trusted key that is stolen** launches code with whatever its trusters grant, until it is removed.

## Why

- **Signatures and capabilities apart.** A signature says who vouches for code, not what it may do; a
  system that granted authority by signature alone would make every signing key a master key.
- **One container and verifier.** The boot bundle, updates and packages share one parser of signed
  archives, the most attacked code in the system.
- **Healthy before any session.** Judging an update healthy only before users run keeps a user from
  forcing a rollback to an older system.
- **No shared content store.** A store shared by every principal is a covert channel (add a blob, probe
  for it); per-principal directories give up deduplication, which a handful of users hardly need.
