# STEWARD2: the steward server; principals, sub-budgets and sessions, on the console and over SSH

Tier A (a trusted system server). Size L. Needs STEWARD1, FSD3, BEAM2, SSH1 and INIT3 (all
merged) and MEM2 (the startup block's heap cap, which a session's VM is launched with). Don't
start until the node's needs are met.

**The owner's goal:** alice and bob as separate principals in separate SSH sessions, each a
beamlet VM in its own budget, each seeing only its own home. This package gets there; leases,
approvals and blame are STEWARD3's, declassification STEWARD4's.

**The policy is built.** `libs/steward` (STEWARD0) is one pure state machine, `decide(store,
event) -> Effects`, keyed by domain, and its Elixir reference (STEWARD1) is a differential oracle
in the bench. You write the **server** that embeds it: transport, admission, the batches its
effects name, and nothing the core already decides. steward.md "Two embedders and a reference"
is the contract: the server binds effects to the client library and the kernel, never reads the
core through `inspect`, and changes no guard, row or constant.

Run every cargo and bench command natively on this host: it has the toolchains, and the job pool
(`.wash/local/jobs.mk`) runs cases side by side under docs/testbench.md "On a shared host". No
`in-dev`.

## Context rules (read these first)

- **Don't read whole files.** `grep -n`, then Read a range. steward.md is 900 lines: read only
  the sections the reading list names. `libs/steward/src/*.rs`: read `effect.rs`, `event.rs`
  and `manifest.rs` whole (small), the rest by symbol.
- **Don't open `.wash/qa/*.md` or other packages' reports.** This brief holds the rulings.
- **Pipe bench output.** `cargo testbench --list | awk '{print $1}'`; read boot logs through
  `grep` or `tail` only.
- **Host-clock cases run alone** (testbench.md "On a shared host"): your `host-tests` case and
  any `[net]` or `ssh-loopback` case; the boot cases share the pool.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/STEWARD2-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/servers/steward.md`: "Principals", "Fixed sub-budgets per label set", "Authentication
  and sessions", "The steward's protocol", "Two embedders and a reference", "The trace encoding",
  "Authority", R36 and R37.
- `docs/userland/sessions.md`: "Logging in", "A session is a VM in a budget", "Vault sessions",
  "Namespaces".
- `docs/servers/sshd.md`: "Sessions over SSH".
- `docs/servers/init.md`: "The boot manifest" (the `principals` table), "Fresh connections per
  child", the boot's step 6, R33.
- `docs/servers/serving.md`: "`serve`", "`admit`", "Typed dispatch", R26.
- `docs/servers/README.md`: the trust tiers table and the capability holdings table (the
  steward's and `sshd`'s rows).
- `docs/userland/native.md`: "The loader stub", "The client library".
- `libs/steward/src/{lib,effect,event,manifest}.rs`; `libs/steward/trace/src/record.rs` (the
  manifest lines' grammar); `libs/steward/tables/session.md`.
- `servers/keyd/src/server.rs`: the typed-server pattern (roles by badge class, `serve`).
- `servers/sshd/src/lib.rs`: `Login`, the `Platform` trait, and the host platform's `login`.
- `image/manifest.json` and `image/boot.toml`: how beamlet is started today.

## The design

### What the server is

`servers/steward`, a Rust program on `redoubt_rt` (the `serve` loop and typed dispatch), started
by `init` at step 6 with the `users` budget, a `system`-class budget of its own, connections to
each `fsd` and `ipd`, and later a `keyd` grant for `audit` (not in M1: no audit file yet). It
serves one typed protocol, `libs/wire/tables/steward.md`, which you write from the core's event
set (below). It holds the core's `Store`, a clock (`time_now`), and the runtime's `random`
(`redoubt_rt::handle::random`) for the random words each event carries; the core holds no
generator (STEWARD0's choice).

1. **The manifest reaches the steward through its arguments.** The manifest is never public, so
   `/boot` cannot carry it. `init` hands the steward, in its startup `argv`, the trace crate's
   manifest lines, one per argument: `principal "NAME" account=N login=[..] approval=[..]
   owned=[..] sets=[[..],..] top=P,N,W`, `keyd [..]`, `servers N`, `sizes session=P,N,W
   agent=.. sub_agent=.. crossing=.. cost=N`. Move that grammar's parser out of the host-only
   trace crate into `libs/steward/src/manifest.rs` (no_std, strict: a malformed line is a
   steward start failure, printed once), and have the trace crate call it, so the two cannot
   drift. The manifest gains one object, `"steward": { "sizes": {...} }`, which `init` checks
   (every limit nonzero; sizes under every principal's smallest sub-budget) and turns into the
   `sizes` line; `init`'s `principals` checks already cover the rest.
   - **Keys.** The core knows a key as a `u64`. The id of an `ssh-ed25519` key is the first
     eight bytes, little-endian, of SHA-256 over the key's 32 raw bytes, computed by `init` for
     the lines and by the steward for a `login`, in one function in `libs/steward` (`hash.rs`
     beside the binding hash). `sshd` sends the raw key; the steward derives the id. The core
     never sees a key.
   - A principal's `home` and `net` scope stay `init`'s to check and become the session's
     namespace entries (point 4).
2. **Boot.** The core's `boot` fixes the principals and carves: for each principal, a top budget
   under `users` with its account and limits, and under it one sub-budget per label set, with
   that set's labels (`Carve`; the `carve` and `carve-sub` lines of the trace). The server runs
   those as kernel calls (`budget_create` with account, labels, limits) and keeps the token-to-
   handle map. A failed carve at boot is a steward start failure: the box has no users.
3. **Login, from `sshd` only.** `login(principal: string, label: string, key: bytes)` on the
   `sshd` root badge class; any other badge gets the unknown-operation answer (steward.md,
   "Each operation is accepted only through the badge class it belongs to"). The server
   derives the key id, draws the random words, and runs `Login`; the core refuses a key that is
   not the principal's, a label the principal does not own, or a `keyd` key (its P2), and
   otherwise answers `Session { id, name }` with a batch.
4. **The session batch** (the core's steps, bound in order; the batch stops at the first
   failure and returns one `Done`):
   - `CreateBudget` under the (principal, label set) sub-budget, the session's limits from
     `sizes`, the label set's labels, no deadline.
   - `Connect` to each server the namespace needs, each a `new_connection` the server makes for
     this child, never the steward's own (init.md "Fresh connections per child"), rooted and
     badged as the step says: the home volume's `fsd` at `home` (an unlabelled session: its
     unlabelled home; a vault session: the labelled volume, and the unlabelled home read-only,
     which the label check enforces at `fsd`, not the steward); `bootfsd` at `/boot`; `ipd` at
     `/net` for an unlabelled session only, with the principal's `net` scope; and `sshd`'s
     channel at `/dev/cons` (point 5).
   - `Launch`: beamlet through the loader stub, as `init` launches servers: the program bytes
     read from `/boot` (add the beamlet program to the image's `public` list; it is not secret),
     the startup block's namespace from the connections above, the named handles `steward` (a
     minted badge on the steward's own endpoint for `submit`, STEWARD3's) and `budget` (the
     session's own budget handle), the arguments beamlet takes today (`budget_pages`, the shell
     module), and MEM2's heap cap from `sizes`. The exit notice comes to the steward's endpoint:
     a dead VM is `Exited`, and the core ends the session and its batch destroys the budget.
   - The reply to `sshd`: the session id and the channel's labels.
5. **The console at `/dev/cons`.** beamlet already opens `/dev/cons` from its namespace with the
   client library's `Console` (consoled's protocol), so a session's console is whatever
   connection the steward puts there:
   - **Over SSH:** `sshd` serves the consol protocol on each channel (sshd.md "A pty session");
     the `Connect` step for `/dev/cons` is a `new_connection` from `sshd`, badged with the
     session id the reply will carry, so `sshd` binds the channel to it when the reply arrives.
     The channel's close is `ChannelClosed` to the steward, which ends the session; the VM's
     death ends the channel (sshd.md "Ending").
   - **On the UART:** the manifest's `console` names one principal (the owner's decision below);
     at boot the steward opens that principal's unlabelled session with `/dev/cons` a
     `new_connection` from `consoled`, and reopens it when it ends. Its event is a new core row,
     `Console { principal }` in `session.md` (no key, unlabelled domain, otherwise `Login`'s
     effects), with the reference's clause and one trace; nothing else in the core changes. The
     manifest's `beamlet` server entry goes: `init` no longer starts a shell, the steward does.
6. **Admission.** `serve` with `buckets` = the manifest's (account, label set) count, as
   `fsd` is sized (R26); `login` and `blame` come on root badges outside the buckets. Ending a
   lease ahead of admission is STEWARD3's.
7. **Ids are the core's random words** (R36): the server never numbers anything.
8. **What is not here:** `submit`, `start_agent`, `end_lease`, `approve`, `deny`, `pending`
   (the approval channel) and `blame` are in the table, accepted only on their badge classes,
   and answered with the core's refusal for an operation the server does not yet bind: the core
   decides them, the server returns `Refused(Unknown)` until STEWARD3 binds their batches. Say
   so on the page's status line.

### The rules it keeps

R33 (only `init` and the steward hold `system`-class budgets), R35 (login keys never in `keyd`;
the steward holds no key), R36 (ids from the keyed random words), R37 (every session carved
from its label set's sub-budget: the core's P1, and your case 4), R14 (the kernel stamps the
session's account and labels; the steward sets them once, at the carve), R25 at `fsd` and `ipd`
(the steward grants connections, the servers apply the label check), init's fresh-connections
rule.

## The cases (both widths; system verdicts)

The image boots `init`, `consoled`, `bootfsd`, `blkd`, `fsd:data` (homes), `fsd:system`,
`fsd:alice-secrets` (a labelled volume), `ipd`, `keyd`, the steward and `sshd`; the manifest
names alice (owns `alice-secrets`, sets `{}` and `{alice-secrets}`) and bob (set `{}`), and
`console = "alice"`. Every case uses the job pool's rules: boot cases share, `ssh-loopback` and
`[net]` cases run alone.

1. **`userland-boot`** (BEAM2's, re-aimed): the steward starts alice's console session; the
   prompt; `55`. The verdict is the shell's output on the UART through the steward's session,
   and the kernel's view: the session's budget is under `users/alice/{}` (the checked build's
   audit, or `init`'s budget listing).
2. **`steward-ssh-two-principals`** (`[[session]]`, alone): alice and bob log in over SSH with
   their test keys; each gets a prompt; alice writes a file under `/home/alice`, bob's
   `/home/alice/...` is `:enoent` (no entry), bob's own home works; bob's `ls /home/alice`
   names nothing. Forbid any line showing one session's output on the other's channel.
3. **`steward-vault-session`**: `alice+alice-secrets@box` gets a prompt; reads `/home/alice`
   and cannot write there (`:eacces` from `fsd`, the label check); writes its labelled volume;
   has no `/net` entry (`:enoent`); a bare `alice@box` session cannot read the labelled volume
   (no entry). The verdict is `fsd`'s refusal lines and the sessions' outputs.
4. **`steward-sub-budget-flood`**: a vault session fills its sub-budget's pages (a loop
   allocating binaries until `system_limit`); bob's and alice's unlabelled sessions still get a
   prompt and run `Enum.sum(1..10)`; the flood's session is refused more pages, not ended.
   Verdict: the kernel's refusal to the flooding budget and the other sessions' output.
5. **`steward-login-refused`**: a key the manifest lists for bob used as alice is refused by the
   steward (sshd's `Refused` line); a login naming a label the principal does not own is refused;
   a `login` sent on a session's badge (a test client in a session's budget) is answered as an
   unknown operation. Verdict: the steward's typed refusals, read by `sshd`'s core and the
   test client.
6. **`steward-session-ends`**: closing alice's SSH channel destroys her session's budget (the
   checked build's audit shows the subtree gone; a second login gets a fresh id); killing the
   VM from inside (`:erlang.halt`) closes the channel. Forbid `init: rebooting` and any
   `exited` line for the steward.
7. **Host, `init`:** a `console` naming no `principals` entry refuses the manifest (beside the
   `principals` checks). **Host, `steward`:** the manifest-line parser refuses malformed lines (fuzz target, tenet
   "fuzz what parses"); the key id function against a vector; the badge-class table (every
   operation on every other class is unknown); the batch runner stops at the first failed step
   and reports `StepFailed`; a `Console` row trace added to `libs/steward/trace/traces/` and
   taken by the reference (the `elixir-oracles` case must stay green).

## Page lines (exact text in the report)

- **steward.md:** the status lines of "Principals", "Fixed sub-budgets per label set",
  "Authentication and sessions" and "The steward's protocol" become `built · tested` with the
  cases above; "The steward's protocol" gains the table's include and the `console` manifest
  field; "Two embedders and a reference": the server is no longer "(planned, with the mechanism
  sections below)". A new `####` under "The policy core", "The manifest lines", moves the
  trace-encoding's manifest-line paragraph there and says `init` hands them as arguments.
  "Authority" status to built. Residual: "Audit records are console lines until M3".
- **init.md:** "The boot manifest" table gains `steward.sizes` and `console`; step 6 says what
  is handed (the lines, `users`, the connections); the status "the steward's half ... not built"
  lines on the confinement check and the budget tree become built with your cases.
- **sshd.md:** "Sessions over SSH" status to built with cases 2, 3, 5, 6; the login paragraph
  names the typed call.
- **sessions.md:** "Logging in", "A session is a VM in a budget", "Vault sessions",
  "Namespaces" statuses to built; the namespace figure loses its dashes.
- **servers/README.md:** the steward's row in the holdings table and the server graph's dashed
  steward edges become solid.
- **SECURITY.md:** R36 and R37 rows: code and tests columns, status built.
- **testbench.md** "SSH sessions": the box's platform case replaces the host platform's note.
- **image/README.md:** the shell is started by the steward as the console principal's session.

## Owned paths

- `servers/steward/**` (new); `libs/wire/tables/steward.md` and the codecs `libs/wire` generates
  from it; `libs/steward/src/manifest.rs` (the parser), `hash.rs` (the key id), `tables/session.md`
  (the `Console` row), `trace/**` (the parser's move, one trace), `elixir/` (the row's clause).
- `servers/init`: the `steward` and `console` manifest fields, the argument lines, step 6's
  handoff, the `public` beamlet entry.
- `servers/sshd`: the box platform (`login` over the typed protocol, the channel's consol
  connection); not the SSH core.
- `image/manifest.json`, `image/boot.toml`, `tests/keys/`: the principals, the labelled volume.
- The cases above, and the pages above.

**Not yours:** `libs/steward`'s guards, rows (other than the `Console` row), constants and
mutations; `fsd`, `ipd`, `consoled`, `keyd`; beamlet (BEAM3 and BEAM4 edit it: a session needs
nothing new from it; if it does, stop and ask). **Hotspots:** MEM2 edits the startup block (its
version carries the heap cap): rebase onto it; BEAM3/BEAM4 own beamlet's surface.

## Gates

- The whole bench on both widths under the job pool's rules; `elixir-oracles` green.
- The steward's host tests, and `init`'s and `sshd`'s.
- `cargo fmt --check`; the unsafe ratchet (the steward should add none); the size budget (a new
  program: state its size and reason in the report); doccheck.

Report each command with its exit code.

## Not here

Leases, agents, `approve@box`, `end_lease`, `blame` (STEWARD3); declassification and push
(STEWARD4); the audit file and its signing (M3, M4); run-time principals (M5). Do not change the
core's constants or any guard to make a case pass: that is a design question for the thread.

## Checkpoint

After point 1 (the manifest lines parsed by the core, `init` producing them, the steward
started by `init` and carving the principals: `init-servers` extended with the budget tree),
send one progress line with the branch, before any session work.

## 2026-10-06: rulings from the early checkpoint (architect-15, QA `STEWARD2-manifest-lines`)

- **Q1, sub-budget sizes: (a).** The core's `Fixed::carves` (an equal share of `top` per label
  set, less `budget_cost`) is the rule; `init` drops `label_sets[].budget` from the schema and its
  check. Page lines: init.md's `principals` row, "(each with a fixed sub-budget: pages,
  processes, weight)" becomes "(each a fixed, equal share of the principal's budget)";
  steward.md "Fixed sub-budgets per label set", "each with its own pages, processes and weight"
  becomes "each an equal share of the principal's top budget, less a budget's own cost". Per-set
  sizes return, if evidence asks, with M5's run-time principals: one sentence in that section's
  text, not an `**Open:**`.
- **Q2, the `keyd` line: (a).** `init` writes `keyd []`. The box's R35 is `init`'s key-separation
  check (every login and approval key asked about, the boot refused on a yes) and `sshd`'s
  `holds` refusal before any login reaches the steward; the core's guard stays, exercised by the
  model and the traces, and is vacuous on the box. Residual on steward.md: "The core's `keyd`-key
  guard is vacuous on the box: `init`'s and `sshd`'s `holds` checks are the live ones."
- **Q3, the connections: a binding table in the server, over a fixed slot order.** The core
  emits one `Connect` per slot `Shared(i)`, `i < servers`, the same for every session; `servers`
  is the slot count, one per kind, in this fixed order: 0 `bootfsd` at `/boot`; 1 the home
  volume's `fsd` rooted at `home` (every session: a vault session's writes there are refused by
  `fsd`'s label check, R25, not by the steward); 2 the labelled volume's `fsd` at `/vault`; 3
  `ipd` at `/net` with the principal's scope; 4 the console (`sshd`'s channel, or `consoled` for
  the console session) at `/dev/cons`. The server's table says, per domain class, what each slot
  is: for an unlabelled domain slot 2 binds to nothing; for a labelled one slot 3 does. A slot
  that binds to nothing makes no call and is produced as `Connection` with no namespace entry:
  the core needs the token, the session never sees an entry (`:enoent`). Page line: steward.md
  "Two embedders and a reference", the server bullet gains "its binding table maps each
  `Shared` slot to a server, a root and a badge per domain class; a slot bound to nothing for
  that class is produced without a call".
- **N1, confirmed.** `servers/sshd` is a library (SSH1): the box binary (`servers/sshd/src/bin`),
  its manifest entry, its `ipd` scope on port 22 and its typed `login` to the steward are yours
  as "the box platform". Size is L+; say so in the report. Cases 2 to 6 reach it through a
  forwarded port as `bench-ssh-guest` does.
- **N3, `users`.** The manifest's `steward` object names the server entry: `"steward": {
  "server": "steward", "sizes": {...} }`; `init` hands `users` to that entry alone at step 6,
  never through a `handed` item (`check.rs`'s `BUDGETS` rule stands: no `handed` names `users`;
  update its comment). Page lines: init.md step 6 ("handing it the `users` budget" names the
  object), R33's status line ("the steward's half" becomes built with `init-servers` extended),
  and the manifest table's `steward` row.

## 2026-10-06: rulings from the point-1 checkpoint (architect-15, QA `STEWARD2-point1-risks`)

- **(1) The hash: `libs/sha256`, not `sha2`.** TENETS 5: reuse a crate when it is small,
  `no_std`, pure Rust and read; "otherwise we write the 50 lines". `keyd` already wrote them
  (`servers/keyd/src/sha256.rs`: one function, one constant table, no dependencies, constant
  time in the input's contents), for the reason its header gives: `sha2` arrives with six crates
  behind it, inside the process that holds every key. `init` and the steward are trusted system
  servers with the same argument. So: move `keyd`'s module to a new crate `libs/sha256`
  (`no_std`, no `unsafe`, no dependencies; its tests and the FIPS vectors move with it), have
  `keyd`, `libs/steward` (the binding hash and `key_id`) and so `init` use it, and drop `sha2`
  from `libs/steward`'s dependencies. The host-only trace crate may keep `sha2` as a
  cross-check that the two agree on the vectors. Page lines: keyd.md's sentence on its own
  SHA-256 points at `libs/sha256`; steward.md "Guards and effects" names it for the binding
  hash; the size budget: `init`'s and the steward's ELF before and after in the report (the
  steward's 371 KB should fall). `libs/sha256` joins the owned paths; `servers/keyd` only for the
  move.
- **(2) The restart: init.md rules it, the steward fails closed.** init.md "Restarts and
  reboots": if the steward dies, `init` destroys and recreates the `users` budget (every session
  logs out) and starts the steward again, so the steward always carves into an empty `users`;
  no idempotent carve, no new `init` step. The steward adds one check at start: if `users` is
  not empty (`budget_usage`: any pages, processes or children), it exits with one line (`users
  not empty`), so a wrong restart path is a visible failure, never a second set of carves.
  Page lines: steward.md "Failure and restart" says both; init.md's sentence stands, its status
  gains the case (`steward-restart`: kill the steward in a boot; `init` recreates `users`, the
  console session comes back; a second case or a host test for the not-empty exit).
- **(3) Keys in two roles across principals: `init` refuses the boot.** The rule is init.md's
  "The boot manifest": a refusal is a boot failure before any server runs, not a steward exit
  after every other server started. `init`'s manifest check gains: every key appears once across
  all principals' login and approval lists (a key may not be a login key of one principal and
  an approval key of another, nor listed twice). The core's refusal stays as defence in depth
  (the model exercises it). Page line: init.md's `principals` row and the check's host test list
  (`host:redoubt-init::a_key_in_two_roles_across_principals_is_refused`).
- Noted: `init-servers` unchanged (`steward-boot` carries the tree); the ELF size goes in the
  report with (1)'s before/after.

## 2026-10-06: rulings on the session batch (architect-15, QA `STEWARD2-session-batch`)

- **Q4: (a).** Six slots, `servers = 6`: slot 5 is the system volume (`fsd:system`, read-only;
  unlabelled data every session may read). The binding table gives the child slots 0 and 5
  under beamlet's handle names (`bootfsd`, `fsd:system`; `littlefsd:`/`erofsd:` names once FSN1
  and EROFS1 land), and slot 0 also at `/boot` in the namespace. (b) is BEAM3/BEAM4's surface:
  not here.
- **Q5: (a).** `init` appends one argument per label after the lines, `label NAME=ID`, the
  server's own grammar (not the core's: the core knows ids by design). The server validates the
  label string `sshd` sends as `init` validates names (1 to 64 bytes, the name grammar) before
  the lookup; an unknown name is the core's `NotOwner` path (no such label of that principal),
  never a panic. `sshd` keeps sending the name.
- **The departure: accepted for sessions, written as R41's Open item.** `new_connection(root,
  quota)` carries no scope, and nothing stamps a connection with a budget but the process that
  made it; so the batch makes the scope as the core asks (`CreateScope`), connects with plain
  `new_connection`, and the steward disconnects each connection by id when the session ends
  (init.md's launcher rule), which is what ends them today. Page line, steward.md R41: the
  status stays planned, and its `**Open:**` line becomes "a connection does not carry the
  revocation scope the steward makes for it: `new_connection` has no scope, and only the process
  that makes a handle stamps it. Narrowing by scope needs a means (a scope handle a server
  accepts at `new_connection` and kills by, or a disconnect-by-scope): STEWARD3 designs it
  before any lease relies on R41; until then connections end by the launcher's disconnect".
  STEWARD3's brief gets the question (I add it).
- **The image: stream it, hold one batch.** The core's `Launch` step carries tokens, not bytes;
  the bytes are the server's binding, and INIT5's rule applies to every launcher (native.md "The
  loader stub": "a launcher never holds more than one batch"): read beamlet's ELF from `/boot`
  64 pages at a time, each batch into fresh pages moved into the child, as `init` does. The
  steward's budget is sized for that (about 64 pages of image in flight plus its tables), not
  1,500. State the steward's `budget_pages` in the manifest from a measurement.

**Q5, extended (architect-15, same thread): (a') confirmed, with the grammar shaped.** After
the core's lines `init` appends the steward's own lines, parsed by the server, each `init`'s check
output naming only what the manifest holds; the core's grammar is unchanged:
- `label "NAME" id=N`, one per manifest label;
- `home "PRINCIPAL" handle=fsd:data path=/home/alice`: `handle` is the **named handle** `init`
  handed the steward for that server (the name the steward's `handle(name)` lookup takes), never
  a bare manifest string the server would have to map; `path` is cleaned and absolute, as the
  client library requires;
- `vault "PRINCIPAL" labels=[7] handle=fsd:alice-secrets`, one per label set the principal works
  under that has a labelled volume (a set with none gets no line and no slot-2 entry);
- `net "PRINCIPAL" 0.0.0.0/0:22,443 10.0.0.0/8:*`, the manifest's own prefix-and-ports form,
  which the steward passes to `ipd`'s `grant(scope)` unchanged.
Strings are quoted with the trace grammar's escapes; a malformed line is a steward start failure
naming the line number. Write `fsd:` now, matching main; FSN1 renames the handles with everything
else. Host test: each line's parse and refusal, and a round trip from `init`'s writer to the
steward's parser over the image's manifest.
- `console "PRINCIPAL"` (architect-15, same day, the orchestrator's question): the last of the
  steward's own lines, at most one, quoted with the trace grammar's escapes; absent when the
  manifest has no `console`, and then no console session (steward.md, "The console is one
  principal's session"). `init` refuses a name that is no principal before any server starts
  (init.md's `console` row); the steward still treats a line naming an unknown principal as a
  start failure naming the line, defence in depth.

## 2026-10-06: how `sshd` learns a session ended (architect-15): option (a)

The steward tells the console. When a login session ends (`Exited`, or its batch fails after
`Launch`), the steward sends one message, `ended`, on the console connection the login carried
(the copy the launcher keeps until release, init.md's rule), then releases it. `ended` is a
message of the **consol** table, so every console server has it:
- `libs/wire/tables/consol.md`, one row: `| 18 | \`ended\` | - | - |` (a `send`: no fields, no
  reply; the serving library decodes a protocol's one-way messages in the server itself).
- **`sshd`:** `ended` on a channel's badge is the session's end: exit status, EOF, close, as a
  VM's death would be. A session that sends it on its own copy of the badge ends only its own
  channel, which it could do anyway by closing; so no new authority.
- **`consoled`:** `ended` on a minted connection releases that connection (the UART session's
  `/dev/cons`); the steward's reopen of the console session follows. `init`'s own connections
  never send it; one on a root badge is refused as unknown.
- **R41:** untouched; `ended` is a notification on a connection the steward already holds, not a
  narrowing handle, and the scope question stays with STEWARD3.
Page sentences: steward.md "Authentication and sessions", the login bullet gains "When the
session ends, the steward tells its console with one `ended` message on the connection the login
carried, then releases it." sshd.md "Ending" becomes "A session's channel closes when the steward
ends the session or its VM dies, which the steward tells `sshd` with `ended` on the channel's
connection; a closed channel ends the session." consoled.md gains, where the minted connections
are described, "`ended` on a minted connection releases it: the steward says so when the console
session it started is over." (b) is refused (a held call and a second thread in `sshd` for one
bit of news); (c) is refused (unreliable, as you found).

**The restart, corrected (architect-15, same day):** init.md's "destroys and recreates the
`users` budget" cannot be: a child's class is its parent's and `budget_create` takes no class
(budgets.md "Class is trust, not order"); `users` is the kernel's, made at boot, and the dead
steward's carves are reachable by no handle anyone still holds. So (a): `init` restarts the
steward; a steward that finds `users` not empty exits at its start check (`users not empty`),
and `init`'s restart rule ends in a reboot after five tries. In M1 a steward crash costs a reboot:
fail closed, as a server that cannot stay up does. Page lines: init.md "Restarts and reboots",
the steward sentence becomes "The steward is part of the trusted base; its crash is a bug. If it
dies, `init` restarts it; a steward that finds `users` not empty exits, and its restarts end in a
reboot, which logs every session out." steward.md "Failure and restart" says the same and carries
the residual: "A steward crash ends in a reboot: nothing can empty `users` of a dead steward's
carves without destroying it, which its class forbids; a kernel call that empties a budget and
keeps it would make the restart a logout instead ([a budget emptied](../todo/empty-a-budget.md))."
The todo file `docs/todo/empty-a-budget.md` is yours to add in this package's form (one file per
follow-up the kernel pages link), linked from budgets.md's residuals too; it is a kernel ABI
follow-up, not STEWARD3's. The `steward-restart` case asserts the reboot (`init: rebooting` after
five `exited` lines) and that the console session is back after it. Case 5's session-badge
`login` is a host test, named host-only in the case description and the report (a session runs
only beamlet before BEAM3/BEAM4).
