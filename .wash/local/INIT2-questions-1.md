# INIT2: questions before deliverable 1 (init2-implementer)

Each names the page, the rule, the options and my recommendation. Q1-Q4 block the checks they
name; I build the rest of deliverable 1 meanwhile, with the recommended option behind one
function each so an answer changes one place.

## Q1. Which badge does a `handed` endpoint carry? (init.md "The boot manifest", `servers`; "Starting the servers" step 2)

The table gives `handed` as endpoint names only (`"handed": ["blkd"]`). Step 2 says init "mints the
badged handles the server's arguments name for its callers", and the servers do name them: `netd`
takes `client=5` and must be handed `ipd` with badge 3 (`ingress=3` in ipd's args); `ipd` takes
`scope=4:...`, `limits=4:...`. init never interprets arguments, so it cannot learn the badge from
them. Ruling 2 also counts "one root badge per system caller", which needs one badge per handed
entry.
- A (recommended): each `handed` item is `{ "endpoint": "ipd", "badge": "3" }`, the badge a decimal
  string, 1 to `FIRST_MINTED_BADGE` - 1, unique per endpoint across the manifest; init mints
  exactly that. One handed item = one root badge = one system caller.
- B: init numbers badges itself (e.g. by the holder's place in `servers`), and the manifest writer
  must write arguments to match. Fragile: a reorder silently changes every argument's meaning.

## Q2. How does a manifest "grant a server a budget handle"? (init.md R33; "No server gets a budget handle")

Unknown members are refused by the parser, so a `budgets` member is already an unknown-member
error, not R33. The attack case needs a spelling that R33's own check refuses.
- A (recommended): `root`, `system` and `users` are reserved: a `receives` or `handed` item naming
  one is refused with R33's reason (and no endpoint may be named so).
- B: accept an explicit `budgets` member in the schema only to refuse it with R33's reason.

## Q3. The bundle key's source constant (brief ruling 5; boot.md; loader/src/verify.rs)

`DEV_PUBLIC_KEY` is in `loader/src/verify.rs`, a module of the loader *binary* (`main.rs`), not of
its lib. init cannot name it without a copy. The loader is K16's and verify.rs is read-only for me.
- A (recommended): move the constant into `libs/signing` (`redoubt-signing`, already the loader's
  and the bench's source for the bundle preamble), the loader's verify.rs using it by name: a
  one-line loader change, coordinated with K16 (who lands it, or I do with their nod).
- B: export it from the loader lib (`loader/src/lib.rs`), and init depends on the loader lib,
  pulling fdt-rs etc. into init's dependency tree on the host only. Worse for init's size budget.

## Q4. R34: who does a server "serve", and in what order are the sharing kinds checked? (init.md "The confinement check")

The page lists kinds but the manifest does not say which domains reach a server (as ruling 2 found
for buckets). I need a rule that gives each kind a manifest that trips it and only it, since each
attack case must show its own reason. Proposal, consistent with ruling 2:
- Domains: each `servers` entry (label set = its `labels`, default {}) and each principal label
  set (its {} and each set it works under). The steward and `sshd` (the control plane) are
  exempt as the page says; in INIT2 neither is a `servers` program, so the exemption is by
  program name `steward`/`sshd` and untested until the steward step.
- A server's users: the servers handed one of its endpoints, plus every principal domain if it is
  shared (its args carry `buckets=`).
- Order, each its own reason: (1) endpoint: two domains with differing sets hold one endpoint
  (receive or handed); (2) volume: two attachers (a server's `volume`, a principal's `home`)
  with differing sets, or a labelled domain attaching an unlabelled volume another domain
  attaches; (3) network: a server whose program is `ipd` or `netd` used by differing sets, or a
  labelled principal set with `net` (the page: "a labelled domain gets no /net"); (4) device: a
  driver holding a `devices` entry is used by differing sets; (5) server instance: any other
  server used by differing sets; (6) cores (the isolated function, pending the owner).
- Always refused, confined or not: a `devices` entry held by two servers (one holder per device,
  "One entry per device"); an endpoint received by two servers.
Options: A (recommended) the above; B the Architect writes the rule.

## Q5. Smaller, with my default (tell me only if wrong)

- Buckets apply to a server whose args carry `buckets=N` (the shared ones); `blkd`/`netd` take
  none. N is checked with the serving library's own `buckets()` parser.
- init itself is a system caller at the servers it calls (`keyd` for `holds`, `consoled`'s root,
  `bootfsd`'s founding handle), so each counts one more root badge there.
- The `INIT_PAGES` bound also counts the largest single launch's transient copies (stub, image,
  stack, block: init's pages until `process_map` moves them), and an exit endpoint per server;
  per-object costs (endpoint 1, process object 1, handle-table page per 64 handles) are init
  constants citing objects.md, since the kernel exports none.
- Principals' login/approval keys are OpenSSH `ssh-ed25519 <base64>` strings; init decodes the
  blob to the 32-byte key for `holds` (base64 and the SSH string framing, no cryptography).
