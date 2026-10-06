# PACK1: the system's boot modules in one pack, read by beamlet in one sequential read

Tier A (beamlet's platform, `mkimage`, the bench), size S. Needs BOOT1 (merged: the baseline
profile). Orthogonal to EROFS1: do not wait on it; if both are in flight, rebase onto whichever
merges first. The spec is the owner's text below, verbatim; the Architect's two rulings and
the brief follow it. Run everything natively on this host under the job pool's rules; a
profile case runs alone (docs/testbench.md, "On a shared host").

## The owner's text (2026-10-06), the spec

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

## Ruling 1: the pack is a file on the verified system volume

The pack is an entry at the root of the userland volume (`boot.pack`), packed by `mkimage`
with the module files it is made from, read by beamlet through the volume's `littlefsd` and
`verityd` like any module. Not a bundle entry. Why, from the pages:
- R75 (docs/kernel/boot.md:484-507) places the code the system resolves by name on a
  verified volume and has beamlet trust the servers that verify it; a pack of those same bytes
  on that same volume is covered by the same tree and the same signed root (R76,
  verityd.md "Purpose"), so "installed code comes only from the bundle and the profile" and
  R75 hold word for word, and the owner's "covered by verity in the first place" is literal:
  `verity-flipped-block`'s attack reaches the pack's blocks as it reaches a module's.
- The bundle holds what must exist before any volume is up (bootfsd.md:4-13, 91-94: `init`
  pushes the manifest's `public` entries to `bootfsd`, which holds them for the system's life
  and "never sees" the rest). A 1.9 MB pack there would put userland code in the bundle for
  the first time, hold it in `bootfsd`'s heap on every boot (its peak is 28 pages today,
  testbench.md's table), duplicate what the volume already carries, and make every module
  change a bundle re-sign. Nothing in bootfsd.md or boot.md changes under this ruling; say so.
- EROFS1 serves the same volume: the pack rides along unchanged when the format changes.
Pages: the pack lands on `docs/userland/beamlet.md` "beamlet on Redoubt" (what it is, the
lookup order: pack, then the volume's file, then the code path; the per-VM sentence the owner
asks for, pointing at image-cache.md), and one sentence in R75's paragraph (boot.md): "The
volume also carries the boot pack, a file the image's builder made from the same module files;
a module found in it is those bytes, and a pack entry that does not decode is a refusal naming
the module, never a search of the code path." `image/userland.toml`'s comment names the pack
and the set it holds. verityd.md and bootfsd.md: untouched.

## Ruling 2: the boot-time figure and its rule

The owner's floor (~16 s = ~9 s for 673 blocks + 6.5 s) assumes each block read once. On
littlefs a large file is read through its CTZ list, and `lfs_file_read` finds every block from
the file's head afresh at each block boundary, several pointer-word reads per block, each a
full 13.0 ms chain read verified (verityd fetches and hashes a 4 KiB block for a 4-byte word;
no LRU until EROFS1's step). The arithmetic, pre-registered so the measurement can judge it:
the pack is 1,895,790 bytes of modules plus its index, about 465 blocks (D); the root-directory
lookup once (about 63 reads); pointer reads P between about 1.5 x D (if the finds are short or
cached) and 4.5 x D (littlefs's find from the head, uncached). Floor = (D + 63 + P) x 13.0 ms
+ 6.6 s (the VM) + 0.8 s (the servers): about 17 s at the low P, about 41 s at the high.
Unverified, 6.8 ms a read: about 12 s and 25 s.

**The rule:** the target is the measured floor plus a tenth, rounded up to the next 5 s (the
scheduler's targets' rule, scheduling.md), where the floor is the prompt's `[t=N]` from
`boot-profile` at seed 1 with the pack, verified. **Set it at checkpoint 1** from the measured
littlefs read count with the pack, and report D, P and the 9P operation count beside it. If
the figure is at or under 30 s the owner's target stands as that figure (20 s at the low
arithmetic). If it is above 30 s, that is checkpoint 2 to the owner with the counts, and these
are the facts for it: the cost is littlefs's per-boundary find, not the pack's; after EROFS1
(no CTZ, one lookup) the same pack reads in about D blocks, a prompt near 14 s. One lever is
allowed before that, measured and reported: reading the pack from its last block to its first
(each 9P read at a descending offset), which makes every find from the head one or two hops;
keep it only if the counts show it, and the page then says why the read runs backwards.

**The page sentence** (beamlet.md's boot-time paragraph, after BOOT1's measured sentences):
"With the boot pack the image reaches its prompt in N s of guest time verified (N1 to N2 over
seeds 1 to 5) and M s unverified: the VM reads the pack once, D blocks and P pointer reads
through littlefs's chain, then decodes each of its 96 modules when first called, with no
lookup; a module outside the pack still costs one. The VM's own work and console, 6.6 s, and
the servers' start, 0.8 s, are what remain beside the read. The target is T s verified, the
measured floor plus a tenth rounded up to 5 s; `boot-profile` bounds the prompt's stamp by it."
Numbers from the measurement; no package IDs, dates or promises.

## The pack's bytes: held in one allocation, released when every entry has been decoded

BEAM6 settled the "second copy" question: the loader keeps no copy of a module's file bytes
(`Lookup::Found` dies at the end of `load`; `.wash/local/BEAM6-report.md`, "Loader copies: none
kept"; beamlet.md "What the VM holds at its prompt"). Do not re-decide it. For the pack: one
allocation, read once, decoded in place by slice (no copy arises); a slice of one allocation
cannot be freed alone, and the pack is the modules the prompt loads, so count the entries
decoded and free the whole allocation when the count reaches the index's; a module in the pack
never called keeps it, bounded by the pack's size. `beamlet-footprint`'s scan at the prompt
then returns to BEAM6's figure, or rises by at most the pack (about 465 pages); report which,
and move testbench.md's row only if it rose.

## The pack, exactly

- `mkimage` writes `boot.pack` into the userland volume: a magic and a version word; the entry
  count; the index, entries sorted by module name (name length and bytes, offset, length); the
  module bytes. Byte-for-byte deterministic from its inputs; a host test packs twice and
  compares. The set is the modules `boot-profile` loads to the prompt, named in
  `image/userland.toml` (a list, with the comment saying how it was measured and that a module
  missing from it only costs a lookup). `mkimage` refuses a name not among the packed modules
  and asserts each entry's bytes equal the module file it also writes.
- beamlet (`userland/otp/redoubt`): at platform start, open and read `boot.pack` whole in
  9P reads of `MSIZE` (ascending, or descending if the lever above is kept), charged to the
  VM; check magic, version, count, every entry's name validity and `offset + length` within
  the file and non-overlapping; a failure is one console line naming the pack and the error,
  and the VM waits without exiting, as it does for a volume that does not attach (beamlet.md
  "beamlet on Redoubt", the tampered-disk paragraph). `load_module` consults the index first;
  a hit decodes from the slice; a miss is today's per-file path. An entry that does not decode
  is `Refused` naming the module (R75's refusal, never a fall-through). Absent pack
  (`not_found`): today's behaviour, one console line saying the pack is absent.
- `boot-stats`: `beamlet: boot pack read B bytes, E entries [t=N]`, and the loads line counts
  pack hits apart from volume reads.

## Cases (both widths unless a case is rv64-only today)

- `boot-profile`, `boot-profile-unverified`: the pack's line expected; the prompt's stamp
  bounded by the target once set; descriptions say the pack.
- `pack-outside-module`: a module not in the pack (one `userland.toml` leaves out) loads from
  the volume and runs; `pack-bad-entry`: a pack with a truncated entry, one with a wrong
  length, one whose name the index does not match (packed by a test helper, the volume
  unverified so the pack reaches beamlet): the VM's start fails cleanly with the named line,
  no reboot loop; and the verified volume case: a flipped block inside the pack is
  `verity-flipped-block`'s outcome (`verityd` names the block, the module's load is refused).
- `beamlet-footprint` at the prompt, both widths, the scan's rise bounded as above;
  `userland-boot` and the other smoke cases unchanged in substance.

## Owned paths

`userland/otp/redoubt/src` (the platform's lookup and the pack reader), `image/mkimage` and
its packer code for the pack, `image/userland.toml`, `tests/boot-profile*.toml`,
`tests/pack-*.toml` and their programs or helpers, `tests/data` for the bad packs; the pages:
`docs/userland/beamlet.md`, `docs/kernel/boot.md` (the R75 sentence), `image/README.md`,
`docs/plan/m1-separation.md`, `docs/testbench.md` (the row, if it moves). **Not yours:**
`userland/otp/vm` (the decoded representation, the loader's checks, lazy loading: BEAM8 is in
it), `servers/littlefsd`, `servers/verityd`, `servers/bootfsd`, `libs/littlefs`, the bundle,
the shell.

## The short gate

Both builds (rv64, rv32); host tests of `beamlet-redoubt`, the packer (`mkimage`'s crate),
and `beamlet-vm` if a signature it exports moves; the docs checker, `cargo fmt --check`, the
size budget, the `unsafe` ratchet unchanged, the no-cruft gate; own cases on both widths:
`boot-profile`, `boot-profile-unverified`, `pack-outside-module`, `pack-bad-entry`,
`beamlet-footprint`, `beamlet-boot`, `verity-flipped-block`, `verity-flipped-tree`,
`verity-wrong-root`, `userland-read-only`; and the smoke set (`userland-boot`, `init-boot`,
`bench-net-peer`, `ipc-outcomes`). The whole bench is the train's.

## Not here (the owner's list, and the rulings')

Page sharing between VMs, a steward byte cache, a pre-decoded snapshot, a per-profile pack
(image-cache.md, BEAM6, M5); the volume's format (EROFS1); verityd's LRU (EROFS1's step); the
VM's decoded form (BEAM8); the loader's checks and lazy-load semantics; a bundle entry.

## One target, for the format the image ships (ruled before checkpoint 1)

EROFS1's step 3 measures `boot-profile` at 14.9 s verified on rv64 (11.1 s unverified) against
littlefs's 1,016.7 s, and is about to merge. The pack is per VM and format-agnostic, so the
page states **one target: for the format the image ships** when PACK1 lands, by the rule above
(floor plus a tenth, rounded up to 5 s). If EROFS1 has merged, PACK1 rebases onto it, the
target is the EROFS figure, and the littlefs number is reported, not put on the page (a page
states what is). If PACK1 lands first, the target is littlefs's and EROFS1's merge re-measures
and replaces it, replaced not amended. Checkpoint 1 reports the number on both formats and what
each gain removes: EROFS removes the directory walks (the 1,000 s); the pack removes the 96
lookups' per-file 9P traffic (walk, open, read, clunk per module) in favour of one file's
sequential read, which on EROFS is about D block reads with no CTZ walk. On EROFS the pack's
gain is therefore the per-file remainder of 14.9 s less that one read: a few seconds, not an
order of magnitude; say so plainly in the report, so the owner sees the two gains compose and
how much the second is worth once the first is in. The owner's "under 30 s on littlefs" is met
or missed on littlefs as ruling 2 says; it is not the EROFS target.

## Checkpoints

1. After the pack reads on one width: the measured prompt, the littlefs read count with D and
   P, the 9P count, the figure the rule gives; the Architect confirms the target. The
   placement is ruled above and needs no checkpoint.
2. Only if the figure is above 30 s: to the owner, with the counts and the facts in ruling 2.
