# FSD2: `fsd`, a byte quota per attach root

Tier A (it is R48, and it meters what principals who do not trust each other share). Size M. It
needs FSD1 merged. Everything it builds runs on the host. Run every cargo and bench command as
`/home/mcloonan/redoubt/.wash/local/in-dev <command>`, from the worktree.

The file server step is three packages:
- FSD1 (merged): the server on one volume, its labels, typed operations, remove, mounting, ids.
- **FSD2** (this one): quotas per attach root (R48), and one hostile-image fix in the mount check.
- FSD3: `fsd` under `init`: range badges, `blkd`'s range labels, the image's disk, restart, and
  the bench cases.

Do none of FSD3's work here: no `init`, no `blkd`, no `image/`, no bench boot.

## Context rules (read these first; context ran out four times on INIT2)

- **Don't read whole files.** Run `grep -n`, then Read a range.
  - `servers/fsd/src/server.rs` is the file you change most. Read its node, mount and
    `minted`/`disconnected` parts, not all of it.
  - `libs/rt/src/server/ninep.rs`: only `FileServer::minted` and `disconnected` (their doc
    comments say what the skeleton guarantees), about 30 lines.
  - `libs/littlefs/src/file.rs`: only `flush` (it rewrites a file's tail) and the CTZ size
    arithmetic (`grep -n "fn ctz\|skips"`).
- **Don't open `.wash/qa/*.md`, other packages' reports or other briefs.** If you must open a QA
  file, read it only up to its checkpoint comment: `sed '/wash-qa-checkpoint/q'`.
- **Pipe bench output.** Read logs only through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Keep reports under 1900 bytes,** with detail in `.wash/local/FSD2-report.md`.
- **If you hand off, keep the handoff short** and end it with "what consumed my context".

## Reading list (only these)

- `docs/servers/fsd.md`: "Volumes, connections and labels" (the "A fid is a path and an id" and
  "Mounting" bullets), "Quotas", R48, and "Residual risks".
- `docs/servers/serving.md`: "Minted connections" (its prose) and R26's first paragraph.
- `docs/servers/wire.md`: the `ninep_common` paragraph (`new_connection(root, quota)`) and the
  error-name table's `no space` row.
- `libs/wire/tables/fsd.md`.

## What is settled (cite these; reopen none)

- **R48** as fsd.md states it: a quota per attach root, carved from the granter's, never charged
  to a parent root; quota 0 can read and remove but not create or grow; the volume root's quota is
  the usable blocks less a fixed reserve.
- **Fids are a path and an id** (FSD1), and a volume `fsd` did not write is corrupt.
- **The skeleton's hooks:** `minted` may refuse before any handle exists; `disconnected` is called
  for every connection `minted` accepted, including one whose mint failed after it. The serving
  library holds no byte counters.

## The rules this brief settles

1. **A root holds what lies under it.** A *live root* is a directory with at least one connection
   minted there and not yet disconnected; the volume root is always live, and every base
   connection (attached, not minted) is at it. A live root *holds* every byte under its directory
   except what lies under the live roots below it; for those it holds their quotas in *reserve*
   instead. Counted as fsd.md says: whole blocks for a file stored in blocks (its skip-list
   included), the byte length of an inline file, and each directory's metadata pairs.
2. **Room.** A root may grow while held + reserve stays within its quota. Every change is charged
   to the nearest live root above the entry it changes, whichever connection made it: a create of
   a file or directory, a write, a truncate, `copy_file`, a remove (a credit), and a rename (rule 6).
   A refusal is the 9P `no space` and the typed `no_space` (rule 8), and changes nothing.
3. **A rewrite counts what it writes.** littlefs's `flush` rewrites a file from the first block
   written to its end before the commit frees the old blocks. So a write needs room for those
   blocks as well as any growth, until it commits. Derive the count from littlefs's own CTZ
   arithmetic (expose one function from `libs/littlefs` if it has none, used by both the check and
   the walk), never a second copy of it.
4. **Nothing is stored.** When the first connection is minted at a directory, `fsd` walks that
   directory once, bounded as the mount walk is, to count what it holds. It keeps one record per
   live root, keyed by the directory's id, while the root is live. The medium holds no counter to
   trust or to lose in a power cut, and a restarted `fsd` counts afresh at the next mint. Records
   are bounded by connections, which admission bounds (R26).
5. **Minting.** `minted(caller, badge, root, quota)`:
   - The parent is the nearest live root above `root`. Its room must take the carve: (held, less
     what now moves to the new root) + reserve + `quota` ≤ its quota; otherwise `refused`. On
     success the parent's held drops by the new root's count and its reserve rises by `quota`.
   - Several connections minted at one directory share its record, whose quota is the sum of
     theirs; each carve is checked on its own.
   - A connection minted at its granter's own root is that root: it carves nothing, and a quota
     other than 0 is `refused`.
   - A new root may already hold more than its quota (files were there). It can then read and
     remove only, until it is under.
   - `disconnected(badge)` undoes the carve: the record's quota falls by that connection's; when
     the last connection at the directory goes, the record goes, its held returns to the parent's
     held and its reserve to the parent's reserve. A parent may then be over its quota, and grows
     no further until it is under. That is fail closed, and the parent's own doing.
6. **Renames.** A rename that would move a live root, or a directory holding one, is `refused`:
   it would end that root's connections (fids do not follow renames) and carry its count away. A
   rename from one live root's part of the tree to another's moves the bytes between them and
   needs room in the second.
7. **The volume root's quota** is the volume's blocks, less those littlefs holds for itself (the
   superblock pair), less a fixed reserve: the most blocks littlefs can need beyond what the roots
   hold to finish any one operation (a metadata pair split, a compaction). Derive the number from
   littlefs's code, put it in a named constant with its reasons, and show a test that the volume
   never reports `NoSpace` while every root is within its quota.
8. **Typed errors.** fsd's error table gains row 9, `no_space`. A typed operation that meets
   `NoSpace` or a quota answers `no_space`; `too_large` stays for `FileTooBig` and an attribute
   over 1022 bytes. Regenerate whatever the wire generator produces from the row.
9. **The hostile-image fix (FSD1's review).** Two empty directories naming one metadata pair pass
   FSD1's mount check: a later create shows under both, and removing one frees a pair the other
   still names. On a shared volume that would put one root's file under another root. The mount
   walk records every directory's pairs, and a pair named twice makes the volume corrupt. Add a
   forged-image test that fails without it.

### Page lines (exact; each in the commit that makes it true)

**fsd.md, "Quotas":** status becomes built and tested (FSD2's host tests). Replace the two
paragraphs and the three bullets before "The attack tests" with:

> Each root a connection is minted at has its own **byte quota**, set by whoever granted it in
> `new_connection`'s `quota` field and carved from the room of the live root above it. `fsd`
> records it in the skeleton's `minted` hook, which refuses a quota that room does not have
> (`refused`), and gives it back when the connection is disconnected. A change that would take a
> root past its quota is refused with `no space`. So Bob filling the `data` volume cannot make
> Alice's saves fail ([R48 (a quota per attach root)](#r48-a-quota-per-attach-root)). The serving
> library holds no byte counters; `fsd` is the only server that meters bytes.
>
> - **A root holds what lies under it:** whole blocks for a file stored in blocks, the byte length
>   of an inline file, and each directory's metadata pairs, less what lies under the live roots
>   minted below it, whose quotas it holds in reserve instead. A change is charged to the nearest
>   live root above it, whichever connection made it. The volume root is always live, and every
>   attached connection is at it.
> - **Nothing is stored.** `fsd` counts a root by walking its directory when the first connection
>   is minted there, and keeps the count while one is live, so the medium holds no counter to
>   trust or to lose in a power cut. Connections minted at one directory share its count, and its
>   quota is the sum of theirs; one minted at its granter's own root is that root and carves
>   nothing.
> - **A rewrite counts what it writes.** littlefs rewrites a file from the first block written to
>   its end before it commits, so a write needs room for those blocks as well as any growth.
>   `copy_file` writes new blocks and is charged in full.
> - **No promise the disk cannot keep.** The volume root's quota is the usable blocks less a fixed
>   reserve for metadata, and carved quotas never exceed it.
> - **A quota of 0 means nothing:** the connection can read and remove, but not create or grow. A
>   quota is never charged to a parent root, which would reopen a shared pool.
> - **A rename never moves a live root.** Moving one, or a directory holding one, is refused: it
>   would end that root's connections and carry its count away. A rename between two roots' parts
>   of the tree moves the bytes and needs room in the second.

**fsd.md, R48:** status built and tested, with the tests; keep its text.

**fsd.md, "Mounting" bullet:** after "no two the same, and the id counter is above the highest",
insert ", and no directory names a metadata pair another names".

**fsd.md, "Residual risks"**, add:
> - **A refused rename says a live root is below.** A connection that tries to move a directory
>   learns whether some connection is rooted at or under it.

and append to "A shared `fsd` is shared state.": " A mint walks its root's directory once, so
minting in a loop costs `fsd` that walk each time."

**libs/wire/tables/fsd.md:** row `| 9 | \`no_space\` |`.

## The tests (host, in `fsd-host-tests`; name each in its section's status list)

- R48's two attack tests: a write past one root's quota is refused while another root still
  writes; a root with quota 0 cannot create, but can read and remove.
- Carving: a mint the parent's room cannot take is `refused` and mints nothing; disconnect gives
  the reserve back; two connections at one directory share it; one at the granter's own root with
  a quota is refused.
- A root minted over existing files counts them at the mint.
- A rewrite at the start of a large file is refused without room for its tail, and succeeds with
  it; a refused write leaves the file as it was.
- A rename moving a live root is refused; one between two roots moves the bytes.
- The volume never answers a real `NoSpace` while every root is within its quota (fill every root
  to its quota with random operations, then check).
- The aliasing image is corrupt at mount.
- The typed `no_space`.

Mutation-check the room check, the rewrite count and the reserve on disconnect; each must fail a
test. Say how.

**littlefs:** if you change `libs/littlefs`, run its differential oracle against the C library by
hand (`libs/littlefs/diff/`, outside the gates), report the result, and remove its `target/`.

## Owned paths

- `servers/fsd/**`.
- `libs/littlefs`: only the CTZ size function rule 3 needs, with a test.
- `libs/wire/tables/fsd.md` and the generator's output for row 9.
- fsd.md's lines above.

**Not yours:** `libs/rt` (the hooks exist; change nothing), `servers/init`, `servers/blkd`,
`image/`, `tools/testbench`, `libs/client/src` (call it; tell me if its error mapping needs the new
row by hand).

## Gates

- `fsd-host-tests`, `littlefs-host-tests` and `client-host-tests`.
- The whole bench on both widths, alone.
- `cargo fmt --check`; the size budget (give `fsd`'s lines); the unsafe ratchet; doccheck.

Report each command with its exit code, each rule above with its code and test, and each page
line as written.

## Checkpoint

When rules 1, 2 and 5 pass R48's two attack tests, send one progress line with the branch.
