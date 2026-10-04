# INIT2 questions 1: the Architect's rulings

These answer `.wash/local/INIT2-questions-1.md`. Each comes with the page lines the package writes
in the commit that builds the rule. I check them at review.

## Q1. A handed endpoint's badge: A

The manifest states the badge. `init` passes arguments without reading them, so the badge a caller
carries has to be in the manifest, next to the endpoint.
- Each `handed` item is `{ "endpoint": NAME, "badge": "N" }`.
  - N is a decimal string, from 1 to `FIRST_MINTED_BADGE` - 1.
  - It is unique per endpoint across the manifest.
- `init` mints exactly that badge. One handed item is one root badge, which is one system caller.

Page lines. In init.md's entry table, the `servers` row's "the endpoints it is handed" becomes:
> the endpoints it is handed (each an endpoint name and the root badge `init` mints for it: a
> decimal string below `FIRST_MINTED_BADGE`, never used twice at one endpoint)

Add under "Arguments", after "never interprets them":
> A badge a server's arguments name for a caller is that caller's `handed` badge, written in both
> places by the manifest's author; `init` mints it from the `handed` item, never from the argument.

## Q2. R33's spelling: A

`root`, `system` and `users` are reserved names. Add to the "No server gets a budget handle" bullet:
> `root`, `system` and `users` are reserved: no endpoint takes one of those names, and a `receives`
> or `handed` item that names one is refused as a budget grant.

## Q3. The bundle key: A

`DEV_PUBLIC_KEY` moves into `libs/signing`, and the loader's `verify.rs` uses it by name. This
changes no behaviour and touches one line in the loader.
- K16 owns the loader. The orchestrator gets K16's nod, then INIT2 lands the move in one commit
  that also touches `verify.rs`.
- No page names the constant's crate, so there is no page line.

## Q4. R34: A, without cores

The owner decided on QA `INIT2-confined-cores`:
- the core rule is dropped, because every label set shares the one kernel and its cores;
- the confinement check is built in INIT2.

The pages say so on main, in 1d109ac5f. Your isolated core function goes. Your option A is ruled
otherwise as proposed, with the kinds checked in the order endpoint, volume, network, device,
server instance. Each kind prints its own reason. Steward and `sshd` are exempt by program name
and untested until the steward step. Say that in the status.

Page lines, added to init.md "The confinement check" after the paragraph on the label set
compared:
> The domains compared are each `servers` entry, under its `labels` (`{}` if none), and each
> principal's label sets. A server's users are the servers handed one of its endpoints and, for a
> shared server (one that takes `buckets=N`), every principal domain: the same count as the bucket
> rule, so a server a session may later reach is never missed. The kinds are checked in the order
> listed, and the refusal names the kind.

Ruled earlier, and it stands: in every manifest, a `devices` entry held by two servers is refused,
and so is an endpoint that two servers receive on. Add under "One entry per device":
> A `devices` entry is held by at most one server, and an endpoint is received on by at most one;
> a manifest naming either twice is refused, confined or not.

## Q5. Your defaults: accepted, with three notes

- **Buckets.** Checked where the arguments carry `buckets=N`.
  - `init` reads that one argument with the serving library's own parser. Every other argument
    stays opaque.
  - Add to "Sizing": "`init` reads `buckets=N` from a server's arguments, with the serving
    library's parser, and no other argument."
- **`init` as a system caller.** `init` counts one root badge at each server it calls (`keyd`,
  `consoled`, `bootfsd`).
- **The `INIT_PAGES` bound.** It counts the largest launch's transient pages, and an exit endpoint
  for each server.
  - The per-object costs are constants in `init` that cite objects.md.
  - A host test pins each constant against objects.md's cost table.
  - The six-server boot case prints `root`'s usage after the boot and fails if the usage exceeds
    the bound. That is the check that the bound is a bound.
- **Keys.** A login or approval key is `ssh-ed25519 <base64>`, decoded to the 32-byte key. Any
  other form, or a blob whose SSH string framing is not exactly `ssh-ed25519` and 32 bytes, is a
  manifest error. Add to the `principals` row: "SSH public keys (`ssh-ed25519` only)".
