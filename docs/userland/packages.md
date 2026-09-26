# Packages

A package is a signed archive of programs and Elixir code with a manifest that requests
capabilities. A principal installs one with `pkg add`, chooses the version it runs with
`pkg use`, and removes unused versions with `pkg gc`. Installed packages live in a directory no
session can write; which versions a principal runs (its **profile**) and whose code it runs (its
**trust list**) are steward records, not files in its space. A signature says who vouches for the
code; the capabilities it runs with are what the principal grants, never more.

## Purpose

Code arrives on the box from people, from agents and from the system's own developers. Packages
answer two questions about it: who vouches for this code, and what may it do. The first is a
signature checked against the principal's trust list; the second is the principal's grant,
narrowed from its own capabilities. Keeping installs, profiles and trust lists out of the
principal's writable space means that write access to someone's home never becomes a launch with
their authority.

## How to use it

```text
iex(1)> pkg add logscan-1.3.xpkg
  signed by: alice (on your trust list)   requests: read /logs, write ~/reports
iex(2)> pkg use logscan 1.3               # pkg use logscan 1.2 rolls back
iex(3)> logscan --since yesterday
iex(4)> pkg gc                            # remove versions no profile uses
```

Adding a signer to the trust list is a high-stakes step, answered at `ssh approve@box`
([sessions](sessions.md#approve)).

## What it can and cannot do

### Installing a package

Status: planned · M5 (persist, install, share)

A package is a ustar archive with a manifest, signed as a whole, the way the boot bundle is: the
boot bundle is the system's first package, and boot, system updates and user packages share one
verifier ([pkg](../servers/pkg.md), [boot](../kernel/boot.md)). The signature covers the domain
string `redoubt.pkg.v1`, the archive's length and the archive, so a package signature is never a
bundle signature, and the reverse.
- **The manifest requests; it grants nothing.** It is strict JSON naming the contents and the
  capabilities the code asks for ("`/net` connect to 443", "read my config directory"). At install
  or run time the principal grants those requests, narrowed from its own capabilities, and the
  launcher builds the program's namespace from exactly those grants.
- **Verified before it is parsed.** The package server (`pkg`) checks the signature over the whole
  archive, with the same verifier boot and updates use, against the installing principal's trust
  list, before it reads a single tar header or manifest byte. An archive no trusted key signed never
  reaches the parser.
- **The parser is contained.** A trusted key can still sign a hostile archive. So the steward
  starts a `pkg` instance per principal for each install, holding the archive it was handed and a
  write handle to that principal's package directory, `/system/pkgs/<principal>/`, only; each
  package goes in `<name>-<version>-<hash>/` there. A parser bug reaches only that principal's
  own packages. Nothing a session holds writes there.
- **The steward keeps the authority.** `pkg` does the parsing, so the steward never parses an
  archive, as `init` and the steward never parse an ELF. Profiles, `use` records, trust lists and
  the grants a manifest requests are steward records; `pkg` asks the steward to record and never
  holds grant authority. The steward records which key signed each installed program.
- **Routine when trusted.** Installing a package signed by a key already on the principal's trust
  list needs no approval.
- **One front end.** `pkg add`, `pkg use` and `pkg gc` are a `pkg` command and a `Redoubt.Pkg`
  module over the package server ([pkg](../servers/pkg.md)).

**Open:** none.

### Profiles and upgrades

Status: planned · M5 (persist, install, share)

A principal's **profile** is the set of package versions it runs and the `/bin` its sessions see.
It is a steward record, not a file the principal can write.
- **Upgrading is atomic per package.** `pkg add` installs a new version beside the old; `pkg use`
  flips one steward record; flipping back is the rollback; `pkg gc` removes versions no profile
  uses.
- **Installed code comes only from installed packages.** `Platform::load_module` resolves a module
  name only from the system bundle and the profile's package directories, through read-only
  handles, and the steward launches only installed programs. Neither ever consults the session's
  writable namespace, so a file dropped in a person's home cannot pose as their installed code
  ([beamlet](beamlet.md#beamlet-on-redoubt)).
- **No shadowing.** The system bundle always resolves first, and a package may not define a module
  the bundle defines: `pkg` refuses it at install. Two packages in one profile may not define the
  same module: the steward refuses it at `use`.
- **One's own code is not installed code.** A session that compiles or loads its own code
  (`Code.compile_string`, `Code.require_file` on its own files) runs it within its own authority.
  The code-path rule is about what loads implicitly, not a wall against one's own code
  ([development](development.md#compiling-on-the-box)).
- **A project has its own package directory** and profile, so a shared toolchain is installed once
  for its members ([the steward](../servers/steward.md)).

**Open:** none.

### Trust lists

Status: planned · M5 (persist, install, share)

A principal's **trust list** is the signing keys whose code it runs. The steward launches code on
a principal's behalf only if the principal trusts the signer, and with at most what the principal
grants.
- **System code** (drivers, servers, the loader stub, beamlet) is signed by the system key.
- **A principal may trust its own key**, and others'. `.beam` code follows the same rule as
  native code.
- **Adding a key is a high-stakes approval**, at `approve@`, because it lets code launch with the
  principal's grants.
- **Removing a key** needs no approval, because it only narrows, and it is audited. New launches and
  loads of that key's packages are refused at once, and the steward stops the principal's running
  processes whose recorded signer is that key, by destroying their budgets. `use` records are
  never switched silently to another version or signer: a package left without a trusted signer is
  unusable until the principal chooses.
- **The system key is on no trust list** and cannot be removed: system code is trusted by the
  bundle's verification, not by a list.
- **Agents sign with their own keys.** Code an agent builds runs within its lease; running it
  outside needs the sponsor to trust the agent's key, which is a high-stakes approval
  ([agents](agents.md)).

**Open:** none.

### What signatures do not do

Status: planned · M5 (persist, install, share)

A hijacked agent can run code it wrote: any process can create a child and map pages into it
(the launcher needs exactly that), and IEx evaluates any Elixir. What holds is that such code
never runs with more authority than its author already holds. Signatures gate only what the
steward launches with **new** grants ([native programs](native.md#launching-from-a-session)).

What a signature does buy:
- **No launch with new authority without trust.** Code gets grants from the steward only if a
  trusted key signed it.
- **A compromised key is containable.** Removing it from trust lists stops its code launching, and
  every launched process's signer is on record.
- **Attribution.** Every launched process has a signer in the audit log.

**Open:** none.

## Why

**Signature and capability answer different questions.** A signature cannot say what code should
be allowed to do, and a capability grant cannot say whether the code is what its author meant to
ship. Asking both, separately, means a trusted author's buggy code still runs with only the
grants it was given, and an untrusted author's code gets no grants at all.

**Installs outside the principal's space.** If installed code lived in a principal's home, any
process that could write there could replace it, and the next launch would run the replacement
with the principal's grants. With installs, profiles and trust lists as steward records, write
access to a home is only write access to a home.

**Versions beside each other.** Installing next to the old version and flipping one record makes
upgrade and rollback one step each, with nothing half-installed in between.
