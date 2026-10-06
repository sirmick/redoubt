# VOL1: verified volumes, a block verifier between `blkd` and `fsd`

Tier A (a new server, `init`'s manifest and checks, the packer, the bench). Size M+: the
pinned-root mode is M, and the signed-root step adds S. Needs BEAM2
(shared: `init`'s per-disk volume rules, the userland recipe and pack, `image/manifest.json`, the
bench's userland disk): start from main after BEAM2 merges.

**Ruled by the owner, 2026-10-03 ("yep, fine" to the recommendations):**
- **The shape is A,** the verifier server.
- **BEAM2 lands as built,** and VOL1 then deletes `system.index` and beamlet's check.
- **Decision 5's "the file system in between is not trusted" is reversed:** a reader trusts the
  servers that verify its volume.

The owner then asked for signing and for error checks ("can it add signing and checksum for
error"):
- **Signing** is step 2 below, a signed-root mode beside the pinned root.
- **Error detection is already complete.** Every block is checked by SHA-256 through the tree,
  so any bit error in data or tree is detected. That is stronger than a CRC, and littlefs's CRC
  on metadata still runs under it.
- **Error correction** (parity, ECC) is not proposed. A detected error fails the read; a virtio
  disk's device does its own correction.

The comparison below is kept as the record of why.

Run every cargo and bench command as `/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from
the worktree.

## For the owner: where verification lives (architect-14, 2026-10-03)

The owner: first "build signing right into littlefs and use that in beam2", then "another
server, wraps fsd, and makes it verified/signed/checksummed". Both ask for one general mechanism
instead of BEAM2's `system.index` and beamlet's per-object check. Two shapes check the same
dm-verity hash tree. In both, the tree follows the file system's blocks in the volume's range, and
its root sits in the volume's manifest entry, signed with the bundle (R15).

| | **A. `verityd`, a server between `blkd` and `fsd`** (this brief) | **B. A layer inside `fsd`** (architect-13, `verified-volumes-assessment.md`) |
| --- | --- | --- |
| Where the check runs | its own small process; `fsd` and littlefs untouched | `fsd`'s block callback, under littlefs |
| Generality | any client of `blkd`'s protocol (a later file system, a raw image, a package blob) | `fsd`'s volumes only |
| Cost per block read | one more call (`fsd` to `verityd`) and a 4 KiB copy; the hashing is the same in both | none beyond the hashing |
| Servers | one more per verified volume, so one more per label set in a confined boot (no device slot) | none |
| Trusted base of a reader | `fsd` + `verityd` | `fsd` (with its layer) |
| `init` | a `verity` key, minting at the verifier, one more kind of user in the confinement check | a `root=` argument only |

**Why not inside littlefs** (the first message): to find a signed file, `fsd` would have to parse
the volume's metadata before anything checks it. R47 bounds a parser exploit to "that volume's
data". Once a reader takes code from the volume, that bound means nothing.

**Why not a 9P-level wrapper above `fsd`** (one reading of "wraps fsd"):
- `fsd` would still parse unverified metadata, the exposure the check exists to remove.
- A partial `read` at an offset could be checked only by reading and hashing the whole file, or
  against a per-file tree that the packer and the wrapper would both have to define.
- Directory listings, `stat` and walks are metadata too, and would go unchecked or each need a
  format of their own.

Below `fsd`, every byte littlefs ever parses has been checked, and one tree covers data and
metadata alike.

**Recommendation: A, the server.** It gives the owner a general mechanism in a small process,
easy to review, and leaves `fsd`, littlefs and littlefs's C oracle untouched. Its costs (one call
per block and one process per verified volume) are bounded and measured below, and B stays
available if the measurement says the call is too dear. The owner chooses.

**What either shape reverses.** The owner's decision 5 (BEAM2) said the userland disk is
"checked object by object by whoever uses it, so the file system in between is not trusted".
Under VOL1 a reader trusts the servers that verify its volume, as it trusts `consoled` for its
console. The owner should say so explicitly when choosing.

**Order against BEAM2 (owner's choice):**
- **Recommended: land BEAM2 as built** (index, R75, its cases), **then VOL1 re-aims them.** VOL1
  deletes `system.index` and beamlet's per-object check, modules become plain `/<file>` files,
  BEAM2's flip and missing cases become `verityd`'s, and R75 is restated over R76. The index code
  is small to delete, and pausing BEAM2 holds up BEAM3-5 and the shell on the UART.
- **Alternative: pause BEAM2's held remainder** (the index, cases 1-5) and rebase it on VOL1. No
  throwaway code, but the shell on the UART waits one package.

Under either order, until VOL1 lands a confined boot runs no labelled beamlet. State that as a
limit; the BEAM2 confined-index question is moot.

## Context rules (read these first)

- **Don't read whole files.**
  - `servers/fsd/src/blkd.rs` and `volume.rs` (`Range`, `Blocks`): how `fsd` calls a range today.
  - `servers/blkd/src/server.rs`: only the request dispatch and the read-only reply.
  - `servers/init/src/check.rs`: only `volumes`, the `disk` rules and the confinement users.
  - `servers/init/src/bin/init.rs`: only where a volume's range badge is minted and handed.
  - The packer: only `--pack-disk` and the userland recipe.
- **Don't open `.wash/qa/*.md`, other reports or other briefs.**
- **Keep reports under 1900 bytes,** with detail in `.wash/local/VOL1-report.md`.

## Reading list (only these)

- `docs/servers/blkd.md`: "Ranges and badges", "Messages".
- `docs/servers/fsd.md`: "Volumes, connections and labels" (Arguments, Mounting, Read-only
  ranges), R47, R49, "Failure and restart", "Residual risks".
- `docs/servers/init.md`: the manifest table's `volumes` and `servers` rows, the Volumes bullet,
  "The confinement check", R34.
- `docs/kernel/boot.md`: R15, and R75 as BEAM2 left it.
- `docs/testbench.md`: "Disks and network cards". `libs/stride` as the model for a small shared
  crate.

## The settled design (shape A)

1. **The tree** (`libs/verity`, new: `no_std`, `forbid(unsafe_code)`, SHA-256 from the vendored
   `sha2` and nothing else; one definition shared by the packer and `verityd`).
   - A volume of N data blocks of 4096 bytes (littlefs's block, eight sectors) is followed in its
     range by its tree.
   - Level 1 holds one digest per data block, `SHA-256(0x00 ‖ block)`. Each level above holds one
     digest per block of the level below, `SHA-256(0x01 ‖ block)`. A tree block holds 128 digests,
     zero-filled after the last. Levels are stored bottom-up, the top level one block.
   - The root is `SHA-256(0x02 ‖ N as u64 LE ‖ top block)`, so it pins the block count too.
   - Geometry is overflow-checked arithmetic in one function. Host tests: a known vector, every
     level boundary (N = 1, 128, 129, 128²+1), and a single flipped bit at each level refused.
2. **The packer.** A recipe declaring `verity = true` (`image/userland.toml`) makes `--pack-disk`
   write the tree after the file system's blocks, sized into the partition. It writes the root and
   N into the staged manifest's volume entry. Two packs of the same inputs are byte-identical. No
   key and no signing: the manifest is signed with the bundle (R15).
3. **`verityd`** (`servers/verityd`, new). It holds no MMIO, interrupt or DMA, and serves one
   volume.
   - **Arguments**, all added by `init`: `endpoint=NAME`, `labels=`, `root=<64 hex>`, `blocks=N`,
     and one named handle, `volume`, its range at `blkd`.
   - **At start:**
     - It calls `info` at `blkd`.
     - It refuses a range shorter than N plus the tree (truncated).
     - It reads the top tree block and checks it against the root.
   - **If the start check fails,** `verityd` writes one line on its console naming the reason,
     answers `info` truthfully, and answers every `read` with `failed`. It stays up, so a bad
     medium is never a restart loop, and `fsd` (whose mount then fails) serves the volume as
     corrupt (R49) and stays up too. Answering `info` matters: an `fsd` that cannot size its
     range exits, and would be restarted.
   - **It serves `blkd`'s own protocol** (`libs/wire/tables/blkd.md`, unchanged) on its endpoint,
     to the badge `init` minted for the volume's `fsd`:
     - `info` gives N × 8 sectors, read-only.
     - `read` works in whole blocks. It fetches each block the request touches, hashes it, checks
       it through the tree up to a verified node, and only then copies the requested sectors out.
       A mismatch is `failed`, as `blkd` answers a device error, with one console line naming the
       block.
     - `write` is refused as `blkd` refuses one on a read-only device. `flush` answers at once.
     - The R25 read check runs against `labels=`, as `blkd`'s does.
   - **Memory** is fixed, whatever the volume's size: the top block, pinned at start; a cache of
     `TREE_CACHE` verified tree blocks (start at 32 and report the hit rate); the last verified
     data block, so sub-block reads within it hash once; and a 2-page lend at `blkd`, as `fsd`'s.
   - **Weight:** ordinary, like `fsd`'s. It does a bounded amount of work per request.
4. **`init`.**
   - **The `volume` entry's key.** A `volumes` entry may carry `verity`:
     `{ "server": NAME, "root": "<64 lowercase hex>", "blocks": "<decimal>" }`, all required.
     `server` names a `servers` entry running the `verityd` program, and no other volume names
     that entry.
   - **The volume's server** (its `fsd`) still lists the volume. `init` mints that server's
     `volume` badge (1) at the verifier's endpoint instead of at `blkd`, and mints the range badge
     at the verifier's disk's `blkd` for the verifier. The verifier gets `labels=` the volume's
     set, `root=` and `blocks=`.
   - **Refused,** naming the field:
     - a malformed root or block count;
     - a verifier entry with its own `volume`, or with any of those arguments itself;
     - a verifier named by two volumes, or by none;
     - a `handed` item at a verifier's endpoint;
     - a verifier whose labels differ from its volume's.
   - **Confinement:** the confinement check's users gain "or its range at a verifier": `fsd` is
     the verifier's user, and the verifier is `blkd`'s. R34 is unchanged in force: one verifier
     per label set, as BEAM2 gives one `blkd` and `fsd` per set.
   - **The bound:** each verifier is one more server in `Counts`, about 15 of `root`'s pages, and
     its own budget comes from `system`. Report both.
   - **The image.** The image's manifest wraps the userland volume (`verity:system`), with
     measured numbers.
5. **What it guarantees.** A reader of a verified volume sees only blocks that hash, through the
   tree, to the root the signed manifest gives. Otherwise it sees a device failure, which `fsd`
   serves as `corrupt`. `fsd` already poisons a volume on an I/O error until it is next mounted, so
   one bad block fails closed for the whole volume: later loads fail too, and the prompt keeps
   what it has. Rollback is the bundle's, as boot.md already says (no rollback protection), no
   worse. Writable volumes keep CRC and R49 only. A volume updated apart from the bundle uses
   step 2's signed root.
6. **The cost, measured.**
   - **Per block:** each block `fsd` reads is one extra call and a copy of at most 4 KiB. The
     tree's reads are about 1/128 more, mostly cached.
   - **beamlet's module loads (about 7.6 MiB, about 1,950 blocks):**
     - Report the number of `fsd` reads at `verityd` and of `verityd` reads at `blkd`.
     - Report the cache's hit rate.
     - Report the boot-to-prompt time of `userland-boot` with and without the verifier (once by
       hand, with the volume unwrapped).
   - **Lends:** `fsd`'s 2-page lend is unchanged; `fsd` is untouched.
   - **Read-ahead: not now.** A read-ahead inside `verityd` (up to `MAX_SECTORS`, 8 blocks per
     `blkd` call, hashed and cached) would cut its calls at `blkd` eightfold on sequential reads,
     with no protocol change. It is local, so it can come later if the timing asks for it. A
     larger lend from `fsd` would change `fsd`, and is not proposed.

## Step 2: the signed-root mode (after the pinned root works)

A volume that is updated apart from the bundle (M5's) cannot have its root pinned in the
manifest. It carries a signed root instead. Commit this step after the pinned-root mode passes its
cases.

1. **The root block.** It is the last block of the volume's range, after the tree, and holds:
   - a magic;
   - N;
   - a version (`u64`);
   - the root;
   - an Ed25519 signature over the preimage `redoubt_signing` builds under a new domain,
     `"redoubt.volume.v1\0"`, from N, the version and the root. Add the domain to
     `libs/signing` beside `BUNDLE_DOMAIN`, built the same way: domain, a fixed-width length, the
     bytes.

   Verify with `ed25519-compact`, the loader's and `keyd`'s crate at the same pinned version (one
   Ed25519 implementation on the box). Its layout is defined once, in `libs/verity`.
2. **The manifest.** Each volume has one mode or the other, stated by which keys its `verity`
   object carries:
   - **pinned:** `root` and `blocks`, as in point 4;
   - **signed:** `key`, either 64 hex digits or `"bundle"` (the bundle's key,
     `redoubt_signing::DEV_PUBLIC_KEY`, named rather than copied so it cannot drift), and
     `floor`, a decimal version.

   `init` refuses an entry with both modes or neither, and a malformed key or floor. It hands
   `verityd` `key=` and `floor=` in place of `root=` and `blocks=`.
3. **`verityd` at start (signed mode):**
   - It reads the root block.
   - It refuses the mount if the signature fails under the key, or if the version is below
     `floor` (a rollback).
   - It then takes N and the root from the block and goes on as in pinned mode: range length,
     top block against the root.

   A refusal behaves as the pinned mode's bad root does: one console line naming the reason,
   `info` answered, every read `failed`, and `verityd` stays up.
4. **The packer.** A recipe that names a key (`sign = { key = PATH, version = N }`, `PATH` a
   32-byte seed file on the build host) writes the root block, signed. A recipe without one writes
   no root block, and the pack prints the pinned root and N as before.
   - The bench signs with the development seed the bundle builder already uses. Report where that
     seed comes from.
   - Signing happens on the build host only; the device never holds the key (R35).
5. **What it does not do.**
   - The floor lives in the manifest, so rolling the whole bundle back rolls the floor back too.
     That is boot.md's residual (no rollback protection), unchanged.
   - Nothing on the box raises the floor yet: there is no monotonic store. A raised floor
     arrives with a new bundle.
   - **The image's userland volume stays pinned.** Only the step's cases use a signed volume.
6. **Its cases (both widths), one per refusal.** Use a small signed test volume read by a test
   program through its `fsd`, or the userland disk packed signed for the case, whichever is
   smaller to build. Name it.
   - **`verity-signed`:** a signed volume mounts and reads.
   - **`verity-bad-signature`:** one byte of the root block's version flipped after signing. The
     mount is refused, naming the signature.
   - **`verity-rollback`:** a volume signed at version 1 under `floor` 2. The mount is refused,
     naming the version.
   - **Host tests:** the root block's parse (a short or malformed block refused); a wrong key; the
     floor's edge (version equal to the floor mounts); `init`'s mode rules; the packer signing
     deterministically (Ed25519 is deterministic).

## Re-aiming BEAM2

- `system.index`, its bundle entry and `public` name, bootfsd.md's line, and the recipe's `index`
  key go.
- beamlet's index read and per-object hash go. Objects are staged as plain files
  (`/Elixir.Enum.beam`, `/elixir.app`), and `load_module` and `load_app` read them by name.
- A `corrupt` or missing file is a failed load, with one console line naming the file and the
  reason. The parked start (BEAM2 point 6) and the read-only case are kept.
- **Ruling (architect-15, 2026-10-05; QA `VOL1-absent-vs-refused`): absent and refused are told
  apart by the 9P error name, at the open and after it.** This is wire.md's planned "Error names"
  design (`docs/servers/wire.md#error-names`), not a new rule; VOL1 builds the least of it that
  the lookup needs. `libs/client` gives an `Rerror` its name: `Error::Rerror` becomes (or gains
  beside it) a variant carrying the table's name, at minimum `not_found` (`file does not exist`,
  and a walk that stopped short) against everything else; the text is never kept (native.md's
  "An `Rerror` has a name" bullet already says so). beamlet maps `not_found` to `Absent`, silent,
  as BEAM7 does (the VM's probes stay quiet); any other name at the open or on a read (`corrupt`
  from a poisoned `fsd`, a short or long file, a device error) is `Refused`, terminal, with the
  one console line naming the file and the name. So a verity refusal of a metadata block is a
  diagnosis, never an `UndefinedFunctionError`. Option A (every open failure is absent) is not
  acceptable even as an interim: it loses the line the verifier exists to give, and case 1's
  expectation ("beamlet's failed load naming the module") must hold for a flipped metadata block
  as for a data block. Option B is noise at the prompt. A host case in `libs/client` covers the
  split (a server answering `file does not exist` against one answering `corrupt`), and a
  `beamlet-redoubt` host case covers the mapping; the sweep of `Error::Rerror` matches
  (`grants.rs`, `userland.rs`, the tests in `libs/client`, `servers/fsd`, `userland/otp`) is
  yours. The full table and beamlet's `file` errors stay BEAM3's.
- **R75** (boot.md) is restated: a module or application resource the system resolves by name, and
  a program it launches from the userland disk, comes only from a verified volume (R76). Its tests
  become VOL1's cases.

## The cases (both widths)

A shell boot takes about 3 minutes under QEMU (BEAM2 ruling 5), so the cases share boots:
- **Cases 1 and 4 are one boot,** BEAM2's `userland-boot` re-aimed. Type `Enum.sum(1..10)` first
  (`55`), then the call into the flipped module. `fsd` poisons the whole volume at the first
  failed read, so nothing typed after the refusal can load.
- **Case 2 waits for no prompt.** Flip the level-1 tree block that covers the volume's first data
  blocks, so `fsd`'s mount meets it. Expected: `verityd`'s line naming the tree block, `fsd`'s
  corrupt line, beamlet parked.
- **Case 3 is its own boot.**

Every case that boots the image's manifest sets `memory_mib = 1024`.

1. **`verity-flipped-block`**: the userland disk with one data block of a module flipped after
   packing, loaded on demand. Expected: `verityd`'s line naming the block, beamlet's failed load
   naming the module, the prompt still there. Forbid `init: rebooting` and any `exited` line.
2. **`verity-flipped-tree`**: a level-1 tree block flipped, as above. `fsd`'s mount fails on it,
   and beamlet parks.
3. **`verity-wrong-root`**: the manifest's root changed by one digit. Expected: `verityd`'s refusal
   line, `fsd`'s corrupt line, beamlet's parked start. No restart, no reboot.
4. **`userland-boot`** (BEAM2's, now through `verityd`): the image's userland volume verified, the
   prompt, `55`.
5. **Host, `verityd`:**
   - a truncated range refused;
   - sub-block and multi-block reads;
   - a mismatch is `failed`;
   - writes refused;
   - the label check;
   - arbitrary media never panic (the fake range from `blkd`'s fake, or a small one of its own).
6. **Host, `init`:**
   - the `verity` key and each refusal;
   - the minting (`fsd` at the verifier, the verifier at `blkd`);
   - `confined_gives_each_label_set_its_own_verifier`: two label sets each with their own disk,
     `blkd`, `verityd` and `fsd` accepted, and an {L} `fsd` on the unlabelled verifier refused
     (R34, the endpoint).
7. **Host, the packer:** two packs byte-identical; the root the packer wrote is the one
   `libs/verity` computes over the image.

## Page lines (exact text in the report)

- **docs/servers/verityd.md**, new, in the server page's form:
  - Purpose, Interface (arguments, the tree format of point 1, messages: `blkd`'s table), and
    Authority.
  - **R76 (verified volumes)**, with point 5's statement, extended by step 2: "or, for a signed
    volume, to the root its root block gives, signed under the manifest's key at a version no
    lower than the manifest's floor".
  - The root block, and the two modes (step 2).
  - Failure and restart, and Residual risks: rollback is the bundle's; writable volumes are
    unverified; a reader trusts `fsd` and `verityd`.
  - Why: the block layer, not littlefs and not 9P; a server, not a layer in `fsd` (the comparison
    above, short).
- **SUMMARY.md, servers/README.md:** the page. **SECURITY.md:** R76's row, and R75's row
    restated.
- **init.md:**
  - the `volumes` row gains `verity`, pinned or signed;
  - the Volumes bullet gains the minting and refusals of point 4;
  - the confinement check's users sentence gains the verifier;
  - R34's host test is listed (count + 1).
- **fsd.md:**
  - Arguments: "its range at `blkd`" becomes "its range at `blkd`, or at a
    [`verityd`](verityd.md) for a verified volume".
  - Residual "littlefs does not checksum data" gains "except on a verified volume (R76)".
- **blkd.md**, "Ranges and badges": one sentence that a verified volume's range is held by its
  `verityd`, which serves the same protocol to the volume's `fsd`.
- **boot.md** R75 restated (above).
- **beamlet.md:**
  - The lookup reads plain files from the verified userland volume, and a reader of it trusts
    that volume's `fsd` and `verityd` (R76) in place of checking each object itself.
  - BEAM2's confined-boot limit line is replaced by the rule: each label set that runs beamlet
    reads its own attachment through its own `blkd`, `verityd` and `fsd`.
  - BEAM7's lookup sentence ("a name missing from `system.index` is absent, while an indexed
    object that fails verification is refused") becomes: a name the volume's `fsd` answers
    `not_found` to is absent; any other refusal at the open or on the read (`corrupt`, a short or
    long file, a device error) is refused, with one console line naming the file and the error
    name. The `load_module` row says the same in short.
- **native.md**, "An `Rerror` has a name": the status of that bullet moves from planned to
  partly built (the name is kept; `not_found` against the rest; the full table is BEAM3's), and
  the tests list gains the host case. **wire.md** "Error names": status line likewise.
- **packages.md:** the userland-disk paragraph says the same.
- **boot.md**, "Verified boot": the volume domain `"redoubt.volume.v1\0"` beside the bundle's,
  as one more domain built the same way, with `redoubt-signing`'s domain test extended.
- **testbench.md**, "Disks and network cards": the recipe's `verity` and `sign` keys, and that a case's flip
  changes the disk, never the manifest's root.
- **image/README.md:** the userland volume is verified, and its root is in the manifest.

## Owned paths

- `libs/verity/**`, `servers/verityd/**`; in `libs/signing`, the volume domain and its test
  only.
- `servers/init/**`: the `verity` key, minting, checks, tests.
- `tools/testbench/**`: the packer's tree, the cases.
- `image/**`: the recipe and the manifest's entries.
- BEAM2's index code in `userland/otp/redoubt/**`, `image/` and
  `bootfsd`'s line.
- `libs/client/src/error.rs` and the `Rerror` mapping in `libs/client`: the error name only
  (the ruling above), and the consumers of `Error::Rerror` that the change breaks. Placement
  (orchestrator, on the thread; the Architect agrees): `libs/rt/src/client.rs` `ClientError`
  gains `NotFound` (an `Rerror` whose text is `NineError::NOT_FOUND`, or a walk that stopped
  short); `Remote` stays for every other text; `libs/client` has `Error::Rerror(Name)` with
  `Name { NotFound, Other }`, which BEAM3 grows to the table. The rt change is its own commit with
  its tests; the runtime-change rule applies (every bin linking `redoubt_rt` built on both widths
  before any bench). No other `libs/rt` file.
- The pages above.

**Not yours:** `servers/fsd`, `libs/littlefs`, `servers/blkd` (report anything a boot finds), the
kernel. **Hotspot:** MEM1 also changes `init`'s manifest checks and the image manifest. Whichever
merges second rebases.

## Gates

- The whole bench on both widths, alone.
- The host tests of `verity`, `verityd`, `init` and the testbench.
- `cargo fmt --check`, the size and unsafe budgets (lines for the new crate and server), doccheck,
  no-cruft.

Report each command with its exit code, the measurements of point 6, `verityd`'s budget and
`init`'s bound against before, each case's verdict, and each page line as written.
