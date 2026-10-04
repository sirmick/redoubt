# Verified read-only volumes: assessment (architect-13, 2026-10-03)

The owner, thinking aloud: give littlefs checksums and signing, and use that instead of one-off
integrity schemes (BEAM2's `system.index` and beamlet's per-object check, then M5's volumes).

## Recommendation

**Yes to one mechanism, at the block layer, not as a Merkle file inside littlefs.** A verified
volume is a read-only `fsd` volume whose every block is checked against a hash tree before
littlefs sees it, the shape of dm-verity. The tree sits in the volume's range after the file
system's blocks. Its root reaches `fsd` from the volume's manifest entry, which the bundle signs
(R15). littlefs, its on-disk format and its differential oracle against the C reference are
untouched: the layer is `fsd`'s block-device callback, between `blkd` and littlefs.

**Why not a manifest file inside littlefs.** To find a file, `fsd` must first parse littlefs
metadata, so a crafted disk would reach the parser before any check. R47 (fsd.md:357-359) bounds a
parser exploit to "that volume's data and nothing else". Once a consumer trusts `fsd` for code, a
parser exploit on the code volume *is* code in the consumer, and the bound means nothing. At the
block layer, littlefs never parses an unverified byte. For a verified volume, the untrusted-medium
surface shrinks to one small hash-tree check (tenet 1: one obvious way, in one place). R49 still
governs writable volumes.

## (1) The trust change

Integrity moves from the consumer (beamlet checks bytes against the index) to `fsd`'s verity
layer: the consumer trusts its `fsd`, as it already trusts `consoled` for its console. This is
acceptable:
- Tenet 2: `fsd` checks its input before parsing it.
- Tenet 1: one mechanism replaces per-consumer ones. beamlet loses its index parser and hash
  check; every later consumer of the volume (BEAM4's programs, M5) gets integrity for free.
- R34: each label set reads its own per-label-set `fsd`, so a compromised `fsd` reaches only its
  own label set's view.

It does reverse a stated part of the owner's decision 5 (BEAM2 brief): "checked object by object
by whoever uses it, so the file system in between is not trusted". The owner should say so
explicitly. What it costs: `fsd`'s verity layer joins the trusted base of every reader of a
verified volume.

## (2) Rollback

- **Bundle-paired volumes (the userland disk):** the root is pinned in the bundle's manifest, so
  the volume's rollback is the bundle's. That is boot.md's existing residual "no rollback
  protection", no worse.
- **M5's volumes updated apart from the bundle:** a signed root (Ed25519 over root and version) in
  the volume's header, the public key and a version floor in the manifest entry. This is the same
  layer plus one signature check at mount, built when M5 needs it. It is still bounded by the
  bundle's own rollback.

## (3) BEAM2

**Replaced:**
- `system.index`: the bundle entry, `/boot`'s public entry, bootfsd.md's line and the image
  manifest's `public`.
- beamlet's index read and its per-object check.
- Objects named by hash: modules can be plain `/<module>.beam` files, since the tree pins the whole
  volume.
- The flip and remove cases become `fsd`'s: a flipped data or tree block is `corrupt`, and a wrong
  root refuses the mount.
- The confined-index owner question disappears: no `bootfsd` dependency, and no index.

**Kept:**
- The packer and recipe, deterministic, now also writing the tree and the root.
- The second disk, its `blkd`/`fsd`, the `disk` key and `endpoint=`.
- The image manifest and the shell on the UART.
- R34's one attachment per label set.
- beamlet parking on a failed start module: `fsd`'s `corrupt` surfaces as a failed load.

## (4) The packer

The bundle-paired form needs **no signing**. The pack step computes the tree, writes it after the
file system's blocks, and writes the root into the staged manifest entry (`root=<hex>`, an
argument `init` passes as opaque). The manifest is already signed with the bundle (R15). Signing
with a key comes only with M5's form, on the build host, as the bundle is signed today
(R35: the device never holds that key).

## (5) Size, tier, order

**VOL1**, Tier A (fsd, the packer), size M:
- `fsd`'s verity layer on its block reads, read-only enforced, root from its arguments, verified
  nodes cached within its budget.
- The packer's tree.
- Cases: a flipped data block, a flipped tree block, a wrong root, a truncated range.
- littlefs untouched; SHA-256 already vendored; no Ed25519 now.

Writable volumes keep CRC and R49: a live tree on writes needs atomic tree-and-root updates
alongside littlefs's copy-on-write, and a signing key on the device to re-sign the root. That is
the hard part, and not proposed.

**Order, recommended: land BEAM2 as built, then VOL1 replaces.** BEAM2 is at its QEMU cases with
the index built. The index and check are small to delete, and pausing BEAM2 blocks BEAM3-5. Until
VOL1, a confined boot gives no labelled domain beamlet: state it as a limit and take the open
confined-index question off the owner's list.

**Alternative:** pause BEAM2's index-centric remainder, cut VOL1, and rebase BEAM2 onto it. No
throwaway code, but it delays the shell on the UART by one package.
