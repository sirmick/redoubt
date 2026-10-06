# PACK1: the owner's package text (2026-10-06)

Owner (2026-10-06): the system beam pack is M1 work. Add a package and start it.

Node. PACK1, parent M1, template package, Tier A (beamlet's platform, mkimage, the bench), size
S. Title: "The system's boot modules in one pack: beamlet reads them in one sequential read, not
96 lookups." Needs BOOT1 (the baseline profile). It does not need EROFS1: it is orthogonal to
which read-only format serves the system volume, and must not wait on EROFS1's merge. If both
are in flight, PACK1 rebases onto whichever merges first.

Why. BOOT1's profile: the image reaches its prompt in 1,016 s guest time verified (535 s
unverified), 99 % in the VM's 96 lazy module loads, which make 77,710 block reads of 673
distinct blocks because 97 % of reads re-walk the volume's root directory. One sequential read
of those blocks is ~9 s at today's per-read cost; the VM's own work and console are 6.5 s.
EROFS1 removes the directory walks but still pays per-file traffic (~55-60 s estimated). The
pack removes the lookups themselves, and the two gains compose.

What it is.
- mkimage emits one pack: the boot module set (the modules tests/boot-profile.toml loads to the
  prompt, today 96, from the apps image/userland.toml lists) concatenated as raw .beam bytes
  behind a small index (name, offset, length). Deterministic from its inputs: same module
  bytes, same pack, byte for byte; sorted by module name. Reproducible builds are an M5 goal
  and this must not undercut them.
- The pack lives inside the signed root: either an entry on the verified system volume (verity
  covers it, same as every module file) or an entry in the bundle served by bootfsd. The
  architect picks; the constraint is that trust is unchanged: R75 (verified userland) and
  "installed code comes only from the bundle and the profile" hold exactly as before, and the
  pack is never written at run time.
- beamlet reads the pack once at VM start, in one read into memory charged to the VM, and
  locate_module consults the pack before the code path. A module not in the pack falls through
  to today's per-file load, unchanged. No change to the VM's decoded representation, loader
  checks, or lazy-load semantics: a module is still decoded when first called. The pack's bytes
  are released once decoded, or kept only if the architect shows keeping them is cheaper than
  the VM's second copy (BEAM6 step 2's "no second copy of a module's chunks" is the same
  question: coordinate, don't duplicate).
- The pack is per VM. No sharing of pages between VMs, no steward byte cache, no pre-decoded
  snapshot, no per-profile pack: those are decided elsewhere (docs/beyond/image-cache.md, BEAM6,
  M5's pkg use). Say so in the docs so nobody re-opens them here.

Verdict. bench:boot-profile and bench:boot-profile-unverified rerun with the pack: the number
lands in docs/userland/beamlet.md's boot-time paragraph beside BOOT1's baseline, and the 9P
operation and block-read counts with it. Target: prompt under 30 s guest time verified on
littlefs (the floor is ~16 s; the architect sets the exact figure from the profile at
checkpoint 1). A case that asserts a module outside the pack still loads from the volume. A
case that a pack with a bad entry (truncated, wrong length, a name the index doesn't match)
fails the VM's start cleanly and is covered by verity in the first place. Memory:
bench:beamlet-footprint's scan must not rise by more than the pack's own size at the prompt,
and the budget row in docs/testbench.md moves if it does. Short gate plus the beamlet, verity
and boot-profile cases; one whole bench on its train.

Docs that move. docs/userland/beamlet.md (#beamlet-on-redoubt: the pack, the lookup order, the
boot-time paragraph), docs/kernel/boot.md or the volume page depending on where the pack
lives, image/userland.toml's comment, docs/plan/m1-separation.md#remaining-work and #progress.
Status lines and tested-by lists as the book requires.

Checkpoints. Checkpoint 1 after the architect's placement decision (volume vs bundle) with the
measured floor; checkpoint 2 only if the target is missed.
