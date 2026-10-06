# VOL2 implementer report: the signed volume root

Branch `wp-VOL2` from main 777bba164, worktree /home/mcloonan/redoubt/.worktrees/VOL2, not pushed.

## Commits

1. `4e845930e` verity: a signed volume's root block, under the volume domain.
   - `libs/signing`: `VOLUME_DOMAIN` (`"redoubt.volume.v1\0"`), `volume_preimage(N, version, root)`
     = domain, u64_le(48), N, version, root. Test `volume_preimage_is_the_documented_bytes`;
     `domain_is_prefix_free` extended to both domains.
   - `libs/verity`: `RootBlock` (magic `RVOLROOT`, N, version, root, signature, zero fill; every
     byte checked), `parse`, `encode`, `signed`, `root_block_at`. Four host tests.
   - Fuzz target `libs/verity/fuzz` (`root_block`), excluded in the root Cargo.toml, listed in
     tests/formatting.toml, seed file; its body runs as host test
     `arbitrary_bytes_are_one_root_block_or_none`. servers/init/fuzz/Cargo.lock gains the dependency.
   - boot.md "Verified boot": the volume domain beside the bundle's, its test listed.
   - Size budget: libs/verity 130 -> 173, libs/signing 15 -> 31.
2. `be874db9c` init: a verified volume is pinned or signed, never both.
   - `Verity` holds optional root/blocks/key/floor; the check refuses both, neither or a part
     (`Why::VerityMode` at `volumes[i].verity`), a key that is neither 64 lowercase hex nor
     `bundle` (`Value` at `.key`); a non-decimal-string floor is a decode error (WrongType).
   - Args: `key=` (bundle -> DEV_PUBLIC_KEY in hex) and `floor=`.
   - Test `a_verified_volume_is_pinned_or_signed_and_never_both`; init.md row, Volumes bullet,
     status 23. Size budget: servers/init 2108 -> 2141.
3. `1065d8c41` build: `[profile.release.package.ed25519-compact] opt-level = "z"` (ruling A; the
   stanza joins my owned paths). The message carries the numbers below.
4. `3f65be2fe` verityd: the signed mode, the packer's `sign`, the cases and their pages.
   - verityd: `Mode::{Pinned, Signed}`, args `key=`/`floor=`; `Volume::open(range, mode)` reads the
     root block (the last whole block), parses it, verifies it with ed25519-compact, and holds the
     version to the floor, then uses the pinned path. Refusals: `Malformed`, `Signature`,
     `Version{version,floor}`, `RootBlockUnread`. A refused signed volume answers `info` with the
     range's blocks before the root block.
   - Host tests: `a_signed_volume_opens_from_its_root_block_at_or_above_its_floor` (the floor's
     edge), `a_root_block_that_does_not_verify_is_refused_naming_the_signature` (a wrong key; the
     version, N, root or signature flipped; malformed; a tree overlapping the root block),
     `a_root_block_below_the_floor_is_refused_naming_the_version`; the args test is extended.
   - Packer (`tools/testbench/src/disk.rs`): `sign = { key = PATH, version = N }`; the volume is the
     largest before the root block; `flip_version`; `--pack-disk` prints "signed volume ...".
     `[disk] flip_version` in case.rs/qemu.rs. Tests
     `a_signed_partition_ends_in_its_root_block_signed_deterministically` and
     `the_cases_volume_seed_is_the_development_seed`.
   - The seed: `tests/data/verity/dev-seed`, 32 bytes of 0x42 (`B`), which is the bundle builder's
     `DEV_SEED` (tools/testbench/src/build.rs); the manifest's `"key": "bundle"` verifies it.
   - Pages: verityd.md (Arguments; "The root block, and the two modes" new and built; Starting;
     Memory and cost, with the built-for-size line and the start cost; R76's extension; Residual
     rollback with the floor), testbench.md (Disks: `sign`, `flip_version`, the seed; status 23),
     SECURITY.md R76 row (the cases, signing.rs).
   - Size budget: servers/verityd 528 -> 591.

## The cases (a small signed test disk read by littlefsd-client)

A small disk was chosen over packing the userland disk signed: 4 MiB, the littlefsd stage, real
init with keyd, consoled, blkd, verity:data (verityd), littlefsd and littlefsd-client; manifest
tests/data/verity/signed.json (`key: bundle`, `floor: "2"`); disks signed-disk.toml (version 2)
and rollback-disk.toml (version 1).
- `verity-signed`: version 2 = floor 2, mounts; the client reads both files back.
- `verity-bad-signature`: `flip_version` after signing; verityd's line names the signature,
  littlefsd serves the volume as corrupt, and the client is refused at 3 attaches; no
  exit/restart/reboot.
- `verity-rollback`: version 1 under floor 2; verityd's line names version 1 and floor 2, and the
  rest is as above.

Why the verdict is the system's (rule F): verityd's and littlefsd's lines carry their own `[con N]`
consoled prefix, the client has no part in the refusal, and init's bare lines are forbidden.

## Measurement for opt-level z (icount shift=3, seed 1, guest time, temporary prints, removed)

| | s | z |
| --- | --- | --- |
| loader verify, image bundle, rv64 | 20,436,509 ticks = 2.04 s, 5,747,712 B | 2.15 s, 5,660,160 B |
| loader verify, image bundle, rv32 | 5.95 s, 7,299,072 B | 5.31 s, 6,409,728 B |
| verityd signed start, rv64 | 108 ms | 138 ms |
| verityd signed start, rv32 | 61 ms | 136 ms |

Per byte, the loader is +6.6% on rv64 (about +130 ms on the same bundle) and +1.7% on rv32. rv32
verityd loads 972,740 -> 291,264 B.

## Short gate on 3f65be2fe (through jobs.mk / jobserver)

- build-rv64 rc 0, build-rv32 rc 0.
- docs, formatting, size-budget, unsafe-budget, no-cruft: rc 0 each.
- `cargo test -p redoubt-verity -p redoubt-signing -p redoubt-verityd -p redoubt-init -p testbench`:
  rc 0, 220 passed.
- PASS on both widths: verity-signed, verity-bad-signature, verity-rollback, verity-wrong-root,
  verity-flipped-tree, bench-net-peer (and its siblings), ipc-outcomes.
- **FAIL on rv64: init-boot and userland-boot**, both "keyd: stack needs 4 pages for twice its
  6248-byte peak, declared 3". PASS on rv32.

### Finding: keyd's stack at opt-level z

ed25519-compact at "z" deepens keyd's signing stack on rv64 from a 5,264-byte peak (the
testbench.md table) to 6,248, over half of 3 pages. Proposed fix, not applied because both files
are outside my paths: image/manifest.json keyd `"stack_pages": "3"` -> `"4"`, and the testbench.md
memory table's keyd row -> 6,248 / 4, in the opt-level commit (or a commit of its own). init's bound
tests are unaffected, since the largest stack is beamlet's 17. Then rerun init-boot and
userland-boot on both widths.

## Summaries checked

- README.md, GETTING-STARTED.md: no mention of the verified volume's modes; no change.
- docs/plan (M1 progress): the verified userland line is unchanged, because the image stays
  pinned; a signed volume is a mode, not a milestone item. No change.
- image/README.md: the image's volume stays pinned; no change.
- verityd.md, init.md, boot.md, testbench.md, SECURITY.md: updated, as listed above.

## Open risks

- keyd's stack: see the finding above.
- verityd's signed start costs about 136-138 ms in guest time per signed volume.
- `pin_roots` does not refuse a signed userland recipe; init would refuse both modes, which fails
  closed.
- The whole bench is not run (not mine).

## keyd stack fold (granted), 2026-10-06

Head ff5e2c96b on 777bba164, four commits: 4e845930e verity, be874db9c init, 6a672fc2c build
(Ed25519 opt-z + keyd stack_pages 3->4 in image/manifest.json, its boot-profile-unverified copy
tests/data/boot-profile/manifest-unverified.json (held equal by init's
`the_boot_profiles_unverified_copies_are_the_image_less_its_verification`, which failed without
it), and testbench.md's memory row 6,248/4 plus one sentence), ff5e2c96b verityd.
- jobs.mk, shared: rv64 init-boot PASS (keyd 6248 of 4 pages), rv32 init-boot PASS (5760),
  rv64 userland-boot PASS 149.3 s (6248), rv32 userland-boot PASS 146.7 s (5760). These ran on a
  head whose tree differed only in the boot-profile copy, which neither case reads.
- cargo test -p redoubt-init -p testbench: rc 0, 191 passed (after the copy). docs rc 0.
