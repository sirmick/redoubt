# VOL2: the signed root; a verified volume that is updated apart from the bundle

Tier A (`verityd`, `init`'s manifest checks, the packer). Size S. Needs VOL1 (the pinned-root
mode, `verityd`, the packer's tree). Don't start until VOL1 is merged.

This is VOL1's step 2, lifted from its brief and ruled as written there (the owner, 2026-10-03:
signed roots are step 2, Ed25519 under its own domain, with a version floor). Read VOL1's report
first for what landed and under which names.

Run everything natively on this host, under the job pool's rules (docs/testbench.md "On a shared
host").

## Context rules (read these first)

- **Don't read whole files.** `libs/verity` and `servers/verityd` by symbol; VOL1's brief only
  its "Step 2" section and "Page lines".
- **Don't open `.wash/qa/*.md`.**
- **Pipe bench output;** boot logs through `grep` or `tail`.
- **Read a file right before you Write it,** and prefer Edit.
- **Reports under 1900 bytes,** detail in `.wash/local/VOL2-report.md`.

## Reading list (only these)

- `.wash/local/VOL1-implementer.md`: "Step 2: the signed-root mode" and the verityd.md page
  lines; `.wash/local/VOL1-report.md`.
- `docs/servers/verityd.md` (VOL1's page) whole; `docs/kernel/boot.md` "Verified boot" (the
  domains, `redoubt_signing`); `docs/servers/init.md` "The boot manifest" (the `verity` object).
- `libs/signing/src/lib.rs` (`BUNDLE_DOMAIN`, the preimage builder, `DEV_PUBLIC_KEY`);
  `libs/verity/src/lib.rs`; `servers/verityd/src/main.rs` by symbol; the packer's tree in
  `tools/testbench/src/disk.rs`.

## The design (VOL1's step 2, verbatim where it rules)

1. **The root block** is the last block of the volume's range, after the tree: a magic, N, a
   version (`u64`), the root, and an Ed25519 signature over the preimage `redoubt_signing`
   builds under the new domain `"redoubt.volume.v1\0"` from N, the version and the root. The
   domain sits beside `BUNDLE_DOMAIN` in `libs/signing`, built the same way (domain, a
   fixed-width length, the bytes), and the domain test is extended. Verify with
   `ed25519-compact` at the pinned version: one Ed25519 on the box. The block's layout is
   defined once, in `libs/verity`.
2. **The manifest.** A volume's `verity` object is pinned (`root`, `blocks`) or signed (`key`:
   64 hex digits or `"bundle"` for `redoubt_signing::DEV_PUBLIC_KEY`, named not copied; `floor`:
   a decimal version). `init` refuses both modes, neither, a malformed key or floor, and hands
   `verityd` `key=` and `floor=` in place of `root=` and `blocks=`.
3. **`verityd` in signed mode** reads the root block, refuses the mount if the signature fails
   under the key or the version is below `floor`, then takes N and the root from the block and
   continues as pinned mode does. A refusal is one console line naming the reason, `info`
   answered, every read `failed`, `verityd` up.
4. **The packer.** `sign = { key = PATH, version = N }` in a recipe writes the signed root
   block (a 32-byte seed file on the build host; the bench uses the development seed the bundle
   builder uses: report where it comes from). Without `sign`, no root block, the pinned root
   printed as before. Signing is on the build host only; the device never holds a key (R35).
5. **What it does not do.** The floor is the manifest's, so a bundle rollback rolls it back
   (boot.md's residual, unchanged); nothing on the box raises the floor (no monotonic store
   until M5); the image's userland volume stays pinned: only this package's cases use a signed
   volume.

### The rule it extends

R76 (verified volumes): "…or, for a signed volume, to the root its root block gives, signed
under the manifest's key at a version no lower than the manifest's floor" (VOL1's page line,
written then as step 2's extension; make it true).

## The cases (both widths; system verdicts)

A small signed test volume read by a test program through its `fsd`, or the userland disk
packed signed for the case, whichever is smaller to build (name it in the report).

1. **`verity-signed`**: a signed volume mounts and reads.
2. **`verity-bad-signature`**: one byte of the root block's version flipped after signing; the
   mount is refused, `verityd`'s line naming the signature; `fsd` serves corrupt; no reboot.
3. **`verity-rollback`**: a volume signed at version 1 under `floor` 2; refused, the line naming
   the version.
4. **Host:** the root block's parse (short, malformed refused; fuzz target); a wrong key; the
   floor's edge (version equal to the floor mounts); `init`'s mode rules (both, neither, bad
   key, bad floor); the packer signs deterministically; the signing domain test.

## Page lines (exact text in the report)

- **verityd.md:** "The root block, and the two modes" from planned to built; R76's extension
  sentence stands as built; Interface: `key=`, `floor=`; status lines with the cases.
- **init.md:** the `verity` object's two modes in the manifest table and the refusals.
- **boot.md** "Verified boot": the volume domain listed as built beside the bundle's.
- **testbench.md** "Disks and network cards": the recipe's `sign` key.
- **SECURITY.md:** R76's row gains the cases.

## Owned paths

- `libs/verity/**`, `servers/verityd/**`, `libs/signing` (the domain and its test),
  `servers/init` (the `verity` mode checks), `tools/testbench/src/disk.rs` (the packer's
  `sign`), the cases and pages above.

**Not yours:** `fsd`, `blkd`, the kernel, the image's userland volume (stays pinned).

## Gates

The whole bench on both widths under the pool's rules; the host tests of `verity`, `signing`
and `init`; fmt; the unsafe ratchet; the size budget; doccheck. Report each command with its
exit code.

## Not here

A monotonic floor on the box; signing on the device; a signed userland image.

## Checkpoint

After `verity-signed` is green on one width, one progress line with the branch.
