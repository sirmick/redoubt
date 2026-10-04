# FSD2: the brief's gaps (Architect's ruling)

## Q1: rule 9 covers every pair, not only the first. Answer: (a)

Rule 9 says "the mount walk records every directory's pairs". A tail pair that is another
directory's head, or two chains that join, is a pair named twice. The fix:

- `libs/littlefs`'s `read_dir_at` reports each pair it reads. This joins FSD2's owned paths, as
  rule 3 already allows for the CTZ function.
- The mount walk records them all.
- A chain that loops is corrupt too. The walk stays bounded as the mount walk is: no pair is
  visited twice.

Tests:
- littlefs: `read_dir_at` reports every pair of a directory split across three or more pairs, and
  the differential oracle agrees.
- fsd: forged images, each served as corrupt:
  - A's tail pair is B's head;
  - two chains join at one pair;
  - a chain loops back to its own head.
- fsd: a real volume with split directories still mounts.

## Q2: counting pairs, and the split allowance. Answer: (a), with the allowance

- **Counting.** `read_dir` returns its pair count, as `read_dir_at` does. A change that commits
  to a directory recounts that directory's pairs before and after. The recount is bounded by the
  directory's pairs and charged to the nearest live root.
- **The allowance.** A change that commits to a directory needs room for its own growth plus
  the most pairs one commit can add to a directory.
  - Derive that number from littlefs's split and compact code, and put it in a named constant
    (`SPLIT_PAIRS`) with its reasons. Expect 1, which is 8 KiB.
  - Without the allowance, each root can overshoot after the fact, and N roots overshoot rule
    7's reserve, which is sized for one operation. That breaks "no promise the disk cannot keep".
  - A quota under the allowance can read and remove only.
- **Removes are exempt.** A remove is always allowed (rule 5: a root over quota may still
  remove). Check in littlefs's code whether a remove's commit can ever add a pair. If it can,
  report it and ask before going on.

### Q2, re-ruled: no allowance; a split is made only with room (supersedes the allowance above)

`SPLIT_PAIRS` does not hold. One commit can add up to about 5 pairs (`FSD2-split-pairs.md`). But
littlefs never *needs* a new pair to commit: `compact` (fs.rs:541) splits only to keep half a
block free. When `new_pair` answers `NoSpace`, it puts the entries back and writes one pair
(fs.rs:568). So the fix is a gate, not an allowance:

- **The gate.**
  - `libs/littlefs` gets a limit on new metadata pairs for the current operation, for example a
    `pair_room` that fsd sets before each operation.
  - fsd sets it to the room of the root the change is charged to, divided by a pair's size.
  - Every `new_pair` (a split, or `mkdir`'s own pair) takes one from it, and answers `NoSpace`
    when it is 0.
  - So a split happens only when the root has room for it. Otherwise compaction keeps the
    entries in the pairs the directory already has.
- **A commit fails only if its entries do not fit in one block.** That is then a real
  `no space`, refused as rule 2 says.
- **The recount charges what was actually added,** which the gate keeps within room.
- **Removes need nothing special.** Their compaction falls back the same way, and they never
  fail on it.
- **No quota is too small** to create in. A root near its quota only compacts more often. That
  costs speed, and costs no promise.

Tests:
- A root at its quota keeps creating while its directory's entries fit in a block. Its count
  never passes its quota, and littlefs keeps one pair.
- With room, the same directory splits, and the split is charged.
- A `mkdir` with no room for its pair is `no space` and changes nothing.

Page line, replacing the "A root holds what lies under it" append above:
> A directory is split into another metadata pair only when the root above it has room for one;
> otherwise littlefs keeps it in the pairs it has, so a split never takes a root past its quota.

### Rule 7's reserve under the gate

**A commit gets pair room only in a directory charged to the change's own root. Every other
directory it touches gets `pair_room = 0`.**

- The other directories are the volume root's id-counter commit, and a rename's source directory
  when it lies under another root.
- With `pair_room = 0` they are compacted in the pairs they have, by littlefs's own fallback.
- Neither grows its live entries: the counter is a fixed-size attribute rewritten, and a rename
  only takes an entry out of the source. So each fits wherever it fit before.

**So no split ever comes out of the reserve, and rule 7's reserve covers only blocks littlefs
allocates outside any root in one operation.**

- Derive that from littlefs's code, as the brief says: an allocation no root is charged for.
- If the superblock pair is the only one, the reserve is 0 pairs beyond it, and the named
  constant says why.
- The formula of two allocations per split plus extra is moot.
- The test stays: the volume never reports `NoSpace` while every root is within its quota.

Page line (fsd.md "Quotas"), the bullet "No promise the disk cannot keep" becomes:
> - **No promise the disk cannot keep.** The volume root's quota is the usable blocks less a fixed
>   reserve for the blocks littlefs takes outside any root, and carved quotas never exceed it. A
>   commit splits a directory only into room its change's own root has; any other directory it
>   touches (the id counter's, a rename's source) is compacted in the pairs it already has.

Added test: one commit can add 7 or 8 pairs (`FSD2-split-sim.py`), so `pair_room` is counted
down at each `new_pair`, not checked once a commit. A commit that would split into 3 or more pairs,
with room for exactly 1, makes 1 split and keeps the rest; the recount equals the room used.

### Rule 5 amended (red's BLOCK): a parent holds the larger of a child's quota and its count

The hole: a mint moved the whole count under the new root out of the parent's held, but raised
the parent's reserve only by the new root's quota. Minting at quota 0 over files, or above a
live root, freed room in the parent. The fix:

- **The charge.** A live root R charges its parent `charge(R) = max(quota(R), held(R) +
  reserve(R))`. A parent's reserve is the sum of its children's charges.
  - So minting never changes the parent's total: a mint over files moves `count` from held to
    reserve, plus `quota - count` if the quota is larger.
  - The carve check is then "parent's held, less what moves, plus its reserve, less the charges
    of the roots below R that become R's, plus `charge(R)`, is at most its quota".
- **Keeping it current.** When R's held or reserve changes, recompute `charge(R)` and pass the
  difference up while it is not zero. The depth is bounded by live roots.
  - Within its quota a root's charge stays its quota.
  - A root over its quota can only shrink (rule 5), and its charge shrinks with it, which is the
    room that removes really free.
- **Disconnect is unchanged.** R's held and reserve return to the parent, in place of its charge.
- **Refusing a mint over files is not the fix.** It would end the quota-0 read-only share of a
  directory that already has files, which rule 5 allows.

Tests:
- `a_root_minted_over_files_counts_them` now asserts that the parent's room is unchanged by the
  mint, at quota 0 and at a quota below the count.
- New, `minting_over_files_frees_no_room`: Bob at B with quota Q writes Q bytes to B/x, mints a
  quota-0 connection at B/x and keeps it, and a write of one more block to B/y is `no space`.
- New, `minting_above_a_live_root_frees_no_room`: R minted at quota 0 above a live C leaves P's
  room unchanged.

Page lines (fsd.md "Quotas"):
- **"A root holds what lies under it":** "less what lies under the live roots minted below it,
  whose quotas it holds in reserve instead" becomes "less what lies under the live roots minted
  below it; for each of those it holds in reserve the larger of that root's quota and what that
  root holds, so minting a root never frees room".
- **"Nothing is stored":** append "A root minted over more than its quota can read and remove
  only, until it is under."

Red's two minor findings, fixed in this round:
- **A failed copy whose cleanup also fails** is charged for what it left. Recount the
  destination's blocks after the failure; don't assume zero.
- **An inline file counted twice** over-charges, which fails closed. Fix it if the fix is local;
  otherwise state it in the report and leave it.

## Q3: a live root's own directory is never removed or renamed over. Answer: refuse

This follows rule 6: removing it would end that root's connections, and carry its count away.
- A 9P remove of a live root's directory (empty or not, by any connection, its own root fid
  included) is `refused`.
- So is a rename whose target is a live root's directory.

## The implementer's reading of minting: confirmed

Minting a root R above an existing live root C makes R C's nearest live root. C's quota then
moves from the old parent P's reserve to R's.

- P's carve check counts that move: P's held, less R's count, plus P's reserve, less C's quota,
  plus R's quota, must be at most P's quota.
- R's held is its walk less C's subtree, and its reserve is C's quota. If that is over R's
  quota, R can read and remove only (rule 5).
- At R's last disconnect, its held and its reserve (C's quota included) return to P.

## Page lines (fsd.md, in the commit that makes each true)

- **"Quotas", the "A root holds what lies under it" bullet:** append:
  > A change that commits to a directory also needs room for the metadata pairs one commit can
  > add to it, so a split never takes a root past its quota; a quota smaller than that can only
  > read and remove.
- **"Quotas", the rename bullet:** it becomes:
  > - **A rename or remove never ends a live root.** Moving a live root or a directory holding
  >   one, removing a live root's directory, or renaming over it is refused: it would end that
  >   root's connections and carry its count away. A rename between two roots' parts of the tree
  >   moves the bytes and needs room in the second.
- **"Mounting":** the brief's insertion ", and no directory names a metadata pair another
  names" becomes ", and no metadata pair is named twice, within one directory's chain or across
  two".
- **"Residual risks":** the brief's bullet becomes:
  > - **A refused rename or remove says a live root is there.** A connection that tries to move
  >   or remove a directory learns whether some connection is rooted at or under it.
