# B35 report: bootfsd reserves each entry's size once, not by doubling from the first chunk

Branch wp-B35, worktree /home/mcloonan/redoubt/.worktrees/B35, head b95d0fda2 (81497bcf8 with the red's P2 folded), ONE commit (19 files)
on main 0abd7b1cb. Tier A (init, bootfsd). Design: option B as ruled on my checkpoint; the
orchestrator's three conditions answered below.

## What was built

- **init** (check.rs `args`, now given the bundle's `(name, len)`; init.rs passes them from its
  entries): bootfsd's argument list carries each public entry as `LENGTH:NAME`, the length from
  the bundle init checked the manifest against.
- **bootfsd** (`BootFs::new`, `parse_entry`): each argument split at the FIRST colon (a name may
  hold one); the length canonical: digits only, no leading zero but `0` itself, within `usize`
  (`SetupError::BadLength` otherwise); the name checked by `path::valid_name` exactly as before
  (`BadName`); duplicates, more than `MAX_ENTRIES`, and lengths whose total passes `MAX_BYTES`
  (`checked_add`, `TooLarge`) stop the server before it serves. Each entry's buffer is
  `try_reserve_exact`ed once (`NoMemory` if not); `add` refuses a chunk past the declared length
  (the per-add `MAX_BYTES` check is implied and gone); **a seal while any entry is short of its
  length is REFUSED** (the choice: the table has only `refused`, init already stops the boot on a
  failed seal, and `/boot` stays empty rather than serve a short file) and the server stays
  unsealed, so nothing is visible.
- **Cap**: bootfsd's heap peak measured by the memory scan after the change: 987 pages on rv32
  (userland-boot; init-boot 971), 762 on rv64 (userland-boot; init-boot 746); main's was 1,540
  (the 4 MiB doubling of one-page adds), BOOT2's 60 KiB adds gave 2,884 on rv32. By B32's rule
  (twice the largest peak, rounded up to 128) the cap falls 3,080 -> 2,048 in image/manifest.json
  and the two test manifests that publish what the image does; the four BEAM4 natives cases
  publish three entries more (beamlet-hello, beamlet-caller, beamlet-session.args: ~1,026 pages on
  rv32), so theirs is 2,176 (the red's P2); testbench.md's memory table row carries 987 / 2,048 and
  the note that the cap follows each manifest's own published total.
- **Pages**: bootfsd.md "The list" (the form, the reservation, why), `add`, `seal`, "Started by
  init"; init.md step 5. Size ceilings: servers/bootfsd 228 -> 244 and servers/init 2422 -> 2424,
  each with its `Size budget:` line in the commit.

## Tests

- bootfsd host (server_tests.rs): `an_entry_is_a_canonical_length_a_colon_and_a_name` (leading
  zeros `007`/`00`, empty length, no colon, `1a`, `-1`, a 23-digit overflow -> BadLength; `1:`,
  `1:.`, `1:..`, `1:a/b`, `1:a\0b` -> BadName; `3:erofsd:system` parses with the colon in the
  name; `0:empty` reserves 0; MAX_BYTES exactly fine, MAX_BYTES + 1 and usize::MAX + 1 ->
  TooLarge); `an_entry_is_reserved_once_for_its_length` (**the image just over a power of two**:
  2^16 + 1 bytes pushed in 32 KiB and in 60 KiB chunks, capacity == length before and after the
  seal); `a_chunk_past_the_length_and_a_seal_before_it_are_refused` (the short seal refused,
  nothing visible, then filled and sealed); the bytes bound at the list. The whole-program tests
  (tests/bootfsd.rs) and the 9P vectors run with sized lists. init: `bootfsd_is_given_the_public_list_after_its_buckets`
  expects `100:trace`; the volume test `4000000:beamlet`; every other `args` caller passes `ENTRIES`.
- Results: cargo test -p redoubt-bootfsd 13 + 3 + 1 + 1, -p redoubt-init 59 + 18 + 1, all pass.

## Gates (q / jobs.mk, scratch .tmp/B35/)

On the final tree (identical code to the head; the commit followed): PASS both widths userland-boot,
init-boot, boot-profile, beamlet-boot, under the 2,048 cap (the scan judges cap >= 2 x peak).
On the head 81497bcf8 (prebuilt rc 0): PASS size-budget (with the two raised ceilings committed),
docs, formatting; cargo test -p redoubt-bootfsd 13 + 3 + 1 + 1 and -p redoubt-init 59 + 18 + 1,
all pass. The bootfsd peaks above are the measurement the orchestrator asked for.

## Documentation check

bootfsd.md, init.md, testbench.md (the table) updated; budgets.md states no bootfsd cap;
serving.md unaffected; README/GETTING-STARTED no claim. R46 (only the public list) unchanged: the
sizes come with the names from the launcher; a client can neither size nor fill an entry.

## Red's P2 folded (b95d0fda2)

The four natives manifests' cap 2,176, the table's note. Gates on b95d0fda2: PASS rv64 and rv32
beamlet-natives, beamlet-serve, beamlet-launch, beamlet-natives-attack; PASS docs.
