# Packages

A **package** is a signed ustar archive with a manifest: the unit in which code arrives on the box,
from the boot bundle itself to one principal's tool. The **pkg server** installs packages: the
steward starts one per principal's install, and it checks the signature against the installing
principal's trust list before it parses a byte, then writes only that principal's package
directory. The steward keeps each principal's profile, trust list and grants, and launches a
package's code only if its principal trusts the signer, with at most what the principal grants.
System updates are packages too, written to one of two bundle slots and protected against rollback.

## Purpose

Signing answers "who vouches for this code"; capabilities answer "what can it do". Redoubt keeps the
two apart: a signature never grants authority, and authority is granted only to code a trusted key
signed. One container, one verifier and one domain rule cover the boot bundle, system updates and
every user package, so there is one parser of signed archives to trust, and it runs in a process
that can reach only one principal's packages.

## Interface

### What is signed

Status: planned · M5 (persist, install, share)

- **A package is signed as a whole**: a ustar archive with a manifest, in the boot bundle's
  signature container ([boot](../kernel/boot.md)). `.beam` archives and native programs are package
  contents.
- **Its own domain, the loader's form.** The Ed25519 signature is over the full preimage
  `"redoubt.pkg.v1\0" || u64_le(len) || archive`, never the bare archive, and is checked by the same
  verifier the loader uses for the bundle, whose domain is `"redoubt.bundle.v1\0"`. Domains are
  prefix-free, so a package signature is never a bundle signature, and the reverse.
- **Where signing keys live.** A person's signing key may live off the box, where they sign a package
  themselves, or in [`keyd`](keyd.md) under a `pkg` purpose, which signs only the package preimage it
  builds itself from the archive, with an approval per signature. An agent's key lives in `keyd`.
- **The manifest** is strict JSON ([wire](wire.md#strict-json)). It names the contents and
  **requests** capabilities ("`/net` connect to 443", "read my config directory"); it can grant
  nothing.
- The steward records which key signed each installed program.

**Open:** the package manifest's schema; the maximum package size and how an archive is streamed to
the verifier.

### The pkg server

Status: planned · M5 (persist, install, share)

`pkg` (the command, over the `Redoubt.Pkg` module) talks to the pkg server; the steward never
parses an archive, as `init` and the steward never parse an ELF.

- **Verify before parse.** The pkg server checks the signature over the whole archive against the
  **installing** principal's trust list before it reads a single tar header or manifest byte. An
  archive not signed by a trusted key never reaches the parser.
- **One principal's packages.** A key on one principal's trust list can still sign a hostile archive,
  so the steward starts the pkg server per principal, per install, holding the archive it was handed
  and a write handle to that principal's package directory, `/system/pkgs/<principal>/`, and
  nothing else. A parser bug then reaches only that principal's own packages
  ([R74 (a hostile package stays in its principal's packages)](#r74-a-hostile-package-stays-in-its-principals-packages)).
  Nothing a session holds can write there.
- **The steward keeps the authority.** The pkg server asks the steward to record the profile, `use`
  records, trust lists and the grants a manifest requests; it holds no grant authority itself.

The attack tests: an unsigned or untrusted archive is refused before any parse (a fuzz corpus
confirms the parser is never reached); a hostile archive signed by a trusted key cannot write
outside its principal's package directory.

**Open:** the pkg server's protocol table.

### Signer trust and granted authority

Status: planned · M5 (persist, install, share)

- **The steward launches code for a principal only if the principal trusts the signer**, and with at
  most what the principal grants: at install or run time the principal grants the manifest's
  requests, attenuated from its own capabilities, and the launcher builds the namespace from exactly
  those grants ([R71 (no new authority without trust)](#r71-no-new-authority-without-trust)).
- **System code** (drivers, servers, the loader stub, the VM) is signed by the system key. The system
  key is on no trust list and cannot be removed: system code is trusted by the bundle's verification,
  not by a list. A principal may run native code signed by keys on its trust list, its own included;
  `.beam` code follows the same rule.
- **Adding a key to a trust list** is a high-stakes approval
  ([steward](steward.md#the-powerbox-and-approvals)).
- **Agents sign with their own keys.** Code an agent built runs within the agent's lease; running it
  outside needs the sponsor to trust the agent's key.
- **Removing a key** from one's own trust list needs no approval, since it only narrows, and it is
  audited. New launches and loads of that key's packages are refused at once; the steward finds the
  principal's running processes whose recorded signer is that key and stops them by destroying their
  budgets; `use` records are never switched silently to another version or signer, so a package left
  without a trusted signer is unusable until the principal chooses.
- **Every process the steward launches with grants has a recorded signer** in the audit log. Code a
  principal runs itself without new grants needs none.

The attack test: removing a key stops that signer's running processes and leaves `use` records
unchanged.

**Open:** none.

### Per-principal packages and profiles

Status: planned · M5 (persist, install, share)

- `pkg add` has the pkg server write a package into
  `/system/pkgs/<principal>/<name>-<version>-<hash>/`. A principal's **profile** (the versions it
  uses, its `/bin`) and **trust list** are steward records, not files in its space.
- **Installing** a package signed by a key already on the trust list needs no approval.
- **Upgrading is atomic per package**: `pkg add` installs a new version beside the old, `pkg use` flips
  one steward record, flipping back is the rollback, and `pkg gc` removes unused versions.
- **Loading code.** Launching never consults a session's writable namespace, and the VM's
  `load_module` resolves module names only from the system bundle and the profile's package
  directories, through read-only handles, so write access to someone's home never becomes a launch
  or a load with their authority.
- **No shadowing.** A package may not define a module the system bundle defines (the pkg server
  refuses it at install), and two packages in one profile may not define the same module (the
  steward refuses it at `use`); the system bundle always resolves first.
- **Code one loads oneself** (compiling a string, requiring one's own file) runs within one's own
  authority; the code-path rule is not a wall against it.
- A project has its own package directory, so a shared toolchain is installed once.

```
alice> pkg add logscan-1.3.xpkg
  signed by: alice (on your trust list)   requests: read /logs, write ~/reports
alice> pkg use logscan 1.3                # pkg use logscan 1.2 rolls back
```
*Installing and choosing a version.*

The attack tests: a module named like a system module is refused; a `.beam` planted in the session's
home is never loaded by name.

**Open:** none.

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
- **Key rotation.** The system's signing keys can be replaced through a system update, so a
  compromised or retired key stops being one the loader trusts.

**Open:** where the version counter lives on a virtio-only machine and what protects it; the M and
N, and how the loader holds the N keys; how a rotation is signed and how the loader learns the new
keys.

## Authority

Status: planned · M5 (persist, install, share)

- The steward keeps the profile, trust and grant records; it does not write packages or parse them.
- Each pkg server instance holds one archive and a write handle to one principal's package
  directory, and nothing else.
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
validly signed but older, vulnerable system cannot be booted in place of a newer one, as long as the
counter's storage cannot be reset (Residual risks).

**Open:** none.

### R74 (a hostile package stays in its principal's packages)

Status: planned · M5 (persist, install, share)

No archive reaches the package parser unless a key on the installing principal's trust list signed
it, and the parser runs in a pkg server holding only that archive and that principal's package
directory. So a hostile archive, even one a trusted key signed, can write only its own principal's
packages.

**Open:** none.

## Failure and restart

Status: planned · M5 (persist, install, share)

- **An update fails to boot or to become healthy:** the next boot uses the other slot.
- **An install is interrupted:** the new version's directory is incomplete and unused; the profile
  still names the old one, and `pkg gc` removes the remains.
- **The pkg server crashes on an archive:** that install fails; the principal's other packages and
  records are unchanged.

**Open:** none.

## Residual risks

- **A hijacked agent runs code it wrote,** within its own authority: any process can create a child
  and map pages into it, and the shell evaluates any Elixir. Signatures gate only new grants.
- **Every principal reaches the whole system-call interface** through native code; it must hold on its
  own, and the bench attacks it with hostile native programs.
- **A trusted key that is stolen** launches code with whatever its trusters grant, until it is removed.
- **A resettable counter undoes rollback protection.** If whatever holds the version counter can be
  reset or rewritten (a virtio disk the host controls), an older signed system boots again.

## Why

- **Signatures and capabilities apart.** A signature says who vouches for code, not what it may do; a
  system that granted authority by signature alone would make every signing key a master key.
- **One container and verifier.** The boot bundle, updates and packages share one verifier of signed
  archives, the most attacked code in the system.
- **Parse in a per-principal server.** Keys a principal trusts can still sign a hostile archive; a
  parser that can write only that principal's packages keeps the damage there, and the steward,
  which holds everyone's records, never parses one.
- **Healthy before any session.** Judging an update healthy only before users run keeps a user from
  forcing a rollback to an older system.
- **No shared content store.** A store shared by every principal is a covert channel (add a blob, probe
  for it); per-principal directories give up deduplication, which a handful of users hardly need.
