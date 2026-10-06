# VOL1 step 1 (pinned root): implementer report, before the machine

Branch `wp-VOL1` from main 37ae709ed. No cargo, fmt, test or QEMU was run (machine hold). Nothing
here has been compiled. Step 2 (signed root) is not started.

## Commits

1. `aedc0f88f` verity: the hash tree (`libs/verity`): geometry in one overflow-checked function,
   `leaf`/`node`/`root` with prefixes 0x00/0x01/0x02, `build`, `Geometry::largest`. Host tests:
   known vectors (computed apart, with Python hashlib), level boundaries 1/128/129/128²/128²+1,
   overflow, a flipped bit at each level, the root pins N.
2. `f2bd9b145` verityd: `servers/verityd` (lib: args, volume, server, blkd client; bin),
   `tests/verity-host-tests.toml`, `docs/servers/verityd.md` (R76), SUMMARY, servers/README,
   SECURITY R76 row, size and unsafe budgets.
3. `698237332` init: the `verity` key, its refusals, `check::range` (where `volume` is minted),
   the args, the startup block, `Counts`, confinement users and the endpoint check; init.md;
   SECURITY R34 row.
4. `271c112c8` userland: the packer's tree (`verity = true`), the builder pins the root in the
   manifest it signs (`[[file]] verity = RECIPE`, `build::pin_roots`), the run's one pack is the
   booted disk, post-pack damage (`flip`, `flip_tree`, `wrong_root`); image recipe and manifest;
   beamlet reads plain files; system.index, its entry, `public` and beamlet's check deleted;
   cases; pages.

## Cases (both widths, none run yet)

- `userland-boot`: cases 1 (verity-flipped-block) and 4 in one boot. Types `NoSuch.call()`
  first, so the error's own modules load before the volume is poisoned, then `Enum.sum(1..10)`
  (55), then `Version.parse("1.2.3")` (flipped). Expects verityd's block line, beamlet's
  "not loaded: its file could not be read", UndefinedFunctionError.
- `verity-flipped-tree` (case 2), `verity-wrong-root` (case 3): verityd's line, fsd's corrupt
  line, then beamlet's "fsd:system did not attach: ...; parked".
- Re-aimed: `userland-bad-start` (start module's block flipped after the pack),
  `userland-read-only`, `image-disk`, `init-boot` (11 servers, 0 public entries, 6 badges,
  9 consoles). The beamlet-* cases keep their own unverified recipe; only system.index went.

## Host tests written

- verity: 7 tests in `libs/verity/src/tests.rs`.
- verityd: args, truncated, wrong root/top, sub- and multi-block reads, cache and last block,
  mismatch said once naming the block, failed block never kept, writes refused, labels, badge,
  noise never panics.
- init: `a_verified_volume_s_server_reads_through_its_verifier`,
  `a_verified_volume_s_key_and_verifier_are_refused_naming_the_field`,
  `a_verifier_costs_init_one_server_and_its_range`,
  `confined_gives_each_label_set_its_own_verifier`; image-manifest tests updated (bound 523).
- testbench: `a_verified_partition_is_its_volume_then_its_tree` (two packs identical, root
  equals libs/verity's over the image, the flips), `the_manifest_pins_the_packs_root`, userland
  and qemu tests rewritten.
- beamlet: `a_module_is_its_file_and_a_failed_read_is_refused`.

## Decisions within the brief (say if wrong)

- **Missing file (question sent):** option A. An open that fails is Absent (silent). A read that
  fails after the open is Refused, with one line. libs/client keeps no Rerror text.
- **A verifier carries no argument at all.** That is a superset of "any of those arguments".
  verityd refuses everything else, `buckets=` included, so any argument would only make it exit.
- **The image manifest in git holds a placeholder root (zeros) and blocks "1".** The builder
  overwrites both from its own pack. A manifest that was not staged fails closed, as a wrong
  root.
- **verityd says a block's line before it replies**, so case lines come in order.
- **Paths outside the listed owned ones:** `tests/fsd-programs/src/bin/fsd-client.rs` (BEAM2's
  index lookup in `readonly`), `mkimage` comments, README.md and the M1 plan line.
- **verityd's budget:** 256 pages, ordinary weight 100. Estimated: 128 KiB cache, 32 KiB
  scratch, 8 KiB blocks, a 2-page lend. To be measured.
- **init's bound for the image:** 508 → 523 pages (+15: two endpoints, a process, a block with
  tables, a watcher), and the handle table stays at one page.

## Needs the machine (in order, after a grant)

1. `cargo fmt`. Then `cargo test -p redoubt-verity -p redoubt-verityd -p redoubt-init -p
   testbench`, and beamlet's `userland` test in `userland/otp`.
2. Update `servers/init/fuzz/Cargo.lock`: init now depends on redoubt-verity and sha2. Not
   hand-edited.
3. The size budget numbers (libs/verity 118, servers/verityd 538, servers/init 2078 are
   estimates), unsafe budget, doccheck, no-cruft, rv32 build of verityd.
4. Bench cases, one width at a time: userland-boot, userland-bad-start, verity-flipped-tree,
   verity-wrong-root, userland-read-only, image-disk, init-boot, the beamlet-* cases.
5. Measurements (point 6): the `stats` feature prints the counts every 1024 reads. Boot-to-prompt
   of userland-boot with and without the verifier (by hand).

## Affected summaries checked

- README.md: updated ("modules read from a verified userland volume").
- GETTING-STARTED.md: no change. It names `cargo testbench userland-boot`, still the case.
- docs/plan/m1-separation.md (M1 progress): beamlet line updated to verityd.
- docs/servers/README.md: verityd in the start graph, the start order and the authority table.
- fsd.md: Arguments, and the Residual "littlefs does not checksum data, except on a verified
  volume".
- init.md: volumes row, the Verified volumes bullet, confinement users, R34 (10), status lists.
- blkd.md: one bullet, "Ranges and badges".
- boot.md: R75 restated over R76.
- beamlet.md: load_module row, handles, parked start, confined rule, modules paragraph, status.
- packages.md: the userland paragraph.
- bootfsd.md: the system.index bullet removed.
- testbench.md: the disks section, the [userland] keys, the file example.
- image/README.md: updated. SECURITY.md: R76 row added, R75 restated, R34 test added.

## Ruling C (absent vs refused), 2026-10-05

Commits: `537d95e32` rt (alone): `ClientError::NotFound` for `file does not exist` and a short
walk, `Remote` for the rest; the module comment restated; `libs/rt/tests/echo.rs` asserts both
NotFound forms; `servers/bootfsd/tests/bootfsd.rs:139` now NotFound. libs/client maps both to
`Rerror` in this commit only, so it builds. `fb18c5c98` libs/client + beamlet + sweep + pages.

- Shape: `redoubt_client::Error::Rerror(Name)`, `pub enum Name { NotFound, Other }`, exported.
- Sweep: grants.rs (`Rerror(_)`), file.rs settle (both variants free the fid); libs/client tests:
  launch.rs 283/329 Other, file.rs 60 NotFound, 61-63 Other, 158/160/190 NotFound, 179 Other,
  console.rs 130 Other; servers/fsd/tests/fsd.rs 230/237/324/418-420/433 Other; fsd-client
  298/332/396 and restart-client 97 `Rerror(_)`; init-programs lib.rs 147 `Rerror(Name::Other)`.
  Not changed: consoled (walks answer `not a directory`), ipd tests and tests/net (no
  `file does not exist` on those paths; their patterns are non-exhaustive and still compile).
- beamlet: `userland::unread(e, at_open)`. `not_found` at the open is Absent, silent. Anything else
  is `Refused("its file could not be read: <name>")`, with names not_found/other/disconnected/failed.
- Tests: `host:redoubt-client::an_rerror_keeps_its_name_not_found_against_the_rest`,
  `host:beamlet-redoubt::not_found_at_the_open_is_absent_and_every_other_error_is_refused_by_name`;
  fsd's corrupt attach asserts `Rerror(Name::Other)`.
- Case 1 and metadata: fsd's mount check reads every metadata pair, so a flipped metadata block
  fails at the mount. That is `verity-flipped-tree`'s path: the corrupt line, then beamlet parks.
  `userland-boot` is widened instead. After Version's block poisons the volume, a module never
  loaded before, `OptionParser`, fails at its open in fsd. The case expects
  `beamlet: Elixir.OptionParser not loaded: its file could not be read: other`. `NoSuch.call()`
  stays silent.
- Pages:
  - native.md: a new section "An `Rerror` has a name", built and partly tested. The planned
    bullet becomes "Every error name". doccheck allows only built or planned, so the built part
    has its own heading.
  - wire.md "Error names": built and partly tested (only not_found against the rest; the table,
    servers by name, the drift check and the file errors not built). Its `**Open:**` line goes.
  - beamlet.md: the load_module row and the status. boot.md R75: the lookup sentence names the
    error, and the status adds the beamlet test. SECURITY R75 row: the same test.
- The machine run must also build every redoubt_rt bin on both widths and run the sweep sites'
  host tests (runtime-change rule).

## Host grant run (2026-10-05). Every command under `taskset -c 8-23 nice -n 10`

| Command | Exit | Result |
| --- | --- | --- |
| `cargo +nightly fmt --all` | 0 | reformatted 16 files, folded into their owning commits |
| `cargo test -p redoubt-verity -p redoubt-verityd` | 0 | 7 + 11 passed |
| `cargo test -p redoubt-init -p redoubt-rt -p redoubt-client -p redoubt-fsd -p redoubt-bootfsd` | 0 (after 1 fix) | 318 passed, 0 failed |
| `cargo +nightly build` in servers/init/fuzz (after `cargo update -w`) | 0 | the lock only gains the new crates, at the root lock's versions |
| `cargo test -p testbench` | 0 (after 1 fix) | 88 passed |
| `cargo test -p beamlet-redoubt --features fake` (userland/otp) | 0 | 16 passed (userland 2) |
| `./build --arch rv64 --programs`, `--arch rv32` | 0, 0 | the warning is pre-existing (kernel-half-attack) |
| `cargo build --release --target {rv64,rv32}` of the 10 servers, stub, init/fsd programs, net tests and net client | 0, 0 | no warnings in our crates |
| `cargo build --release --target {rv64,rv32} -p beamlet-redoubt --bin beamlet` | 0, 0 | |
| `cargo testbench size-budget` | 0 | verity 118, verityd 540, init 2077 (lowered), rt 3436 (+2), client 971 (+5) |
| `cargo testbench unsafe-budget`, `docs`, `no-cruft` | 0, 0, 0 | |
| `cargo +nightly fmt --all --check`; `git diff --check 37ae709ed HEAD` | 0; 0 | |
| `testbench --pack-disk image/userland.toml` twice | 0 | 587 objects, 7,837,769 bytes; N = 4054 blocks, 2 levels; root 508da36d…; both packs byte-identical |

Compile and test fixes (no semantic change):
- init check.rs: a closure returning borrowed `&str` became a nested `fn sorted`.
- disk.rs test: the removed `index` key dropped from a recipe string.
- doccheck C5/C3: the first citations of R47, R76, R75, R15, R49 and R25, and "M1 (separation
  and containment)", on verityd.md, fsd.md, testbench.md, native.md, wire.md and SECURITY.md.

Rewritten branch (fixups folded): abafa290d verity, 86043786d verityd, 2c47d85ce init,
8b66eecac userland, edf262f33 rt (`Size budget: libs/rt`), 74f609cb4 client
(`Size budget: libs/client`). Expect a force-push of wp-VOL1 if it was pushed.

Still needs QEMU: every boot case (userland-boot, userland-bad-start, verity-flipped-tree,
verity-wrong-root, userland-read-only, image-disk, init-boot, beamlet-*), both widths, then the
whole bench; point-6 measurements (the `stats` feature, boot-to-prompt with and without the
verifier); verityd's 256-page budget seen in use.

## Rebase onto main 6a7d2385e (BEAM7), 2026-10-05

Base 6a7d2385e, head e23f74a14. Six commits, each signed off: e26273223 verity, 0aa5dea9a
verityd, 5c9c53ea9 init, 2d1294c36 userland, a2125004c rt, e23f74a14 client.

Conflicts and how each was resolved (BEAM7's content kept, VOL1's added on top):
- **userland commit:**
  - boot.md R75 keeps BEAM7's rule: absence permits the code path, a refusal never searches it,
    and an app spec makes one attempt. Under R76, the absent and refused cases are a file the
    volume lacks and one that does not read whole. The status keeps BEAM7's two beamlet-vm tests
    and its two lookup tests, drops the three index tests, and adds the verity cases.
  - beamlet.md: the status merged the same way, and BEAM7's Found/Absent/Refused load_module row
    is restated for the volume. BEAM7's sentence in "a lookup, not a gate" is restated: absent is
    a file the volume lacks, refused one that does not read whole.
  - SECURITY.md R75 row: VOL1's rule, BEAM7's "never falls through to the code path", BEAM7's
    enforcing files (vm.rs, info.rs, lib.rs) and the merged test list.
  - case.rs: BEAM7's HostTests test kept beside VOL1's recipe test.
  - The todo file's deletion was dropped: main already deleted it, with its SUMMARY entry.
  - BEAM7's `userland/otp/redoubt/tests/lookup.rs` used the deleted index API. It is re-aimed to
    `Disk`/`Files` under the same two test names (found, absent and refused for modules and apps;
    each name read once; the diagnostic).
- **client commit:** the same three pages gain the error name. A `not_found` answer is absent;
  any other refusal is refused, naming the error. Statuses add
  `not_found_at_the_open_is_absent_and_every_other_error_is_refused_by_name`. lookup.rs uses
  `Unread::Failed(..)`, and its diagnostic now ends ": other".
- No conflict in vm.rs and none in beamlet's lib.rs: BEAM7's `Lookup` reaches `Disk` through
  `Unloaded`, unchanged.
- Editor's note 3: native.md now says `other` (`Name::NotFound` and `Name::Other`), and the status
  reads "told apart from `other`".

Range-diff (37ae709ed..74f609cb4 against 6a7d2385e..e23f74a14): six to six, same subjects.
- Commits 1-3, 5: Cargo.lock context only.
- Commit 4: SECURITY, boot.md, beamlet.md, testbench.md context, case.rs, the userland files,
  and the new tests/lookup.rs.
- Commit 6: SECURITY, boot.md, wire.md, beamlet.md, native.md, userland.rs, and tests/lookup.rs.

Checks, run through `machine host`:
- `cargo +nightly fmt --all -- --check`: 0.
- userland/otp `cargo test -p beamlet-redoubt -p beamlet-vm --features beamlet-redoubt/fake`: 0,
  73 passed, including lookup 2 and userland 2.
- `cargo test -p redoubt-client`: the first run gave 101. aio's
  `a_server_that_breaks_its_hold_loses_the_session_at_the_margin` gave up at 1.011 s with the
  load average at 18. It is AIO1's timing test, unchanged by VOL1. Three solo runs passed 9/9,
  and the whole suite then passed 36/36.
- `cargo testbench docs`: 0 (PASS).

## Fix round 1 (red 2-3, simplifier 1-5), 2026-10-05

Base 6a7d2385e, head 61638faa9. Seven commits, all signed off: 53a943ff4 verity, ea89e02fe
verityd, 6cf0f6d2a init, f6ebd4131 userland, 16a12b5ec rt (not_found), 5f158f34f client,
61638faa9 rt (say, new).

- red 2, applied as docs (no rt code change):
  - native.md "An `Rerror` has a name" and wire.md "Error names": a multi-name walk that stops
    short on a refusal after the first name reads as not_found, since 9P drops the reason; a
    caller needing the split walks one name at a time, as beamlet's lookup does.
  - native.md's planned section ("Dropped files, error names and generated calls") gets the
    split as its Open item, for BEAM3. wire.md's section is built and may not hold an Open line.
- red 3, applied: userland-boot forbids `beamlet: Elixir.NoSuch`.
- simplifier 1, applied:
  - The `stats` feature, `Said::Counts` and the served counter are deleted.
  - `Counts` stays, because the cache test reads it. `Volume::counts` and `Verityd::counts` are
    `#[cfg(test)]`.
  - Point 6 is measured by hand with a local, uncommitted counter print.
- simplifier 2, applied: `redoubt_rt::start::say` beside `note_console`, used by fsd and verityd,
  in a commit of its own. Size lines: libs/rt 3436 → 3446 (+10, with its Size budget line),
  fsd 1405 → 1395, verityd 494.
- simplifier 3, applied: `redoubt_verity::from_hex`, with a host test; init's root check and
  verityd's `root=` parse both use it, and verityd's own hex parser is deleted.
- simplifier 4, applied: `Geometry::level` and `total_blocks` are deleted, and their tests use
  `node(0, 0)` and `total_sectors`. `total_sectors` keeps its one caller, verityd's start check.
- simplifier 5, applied: the handles `debug_assert` in `Serving::handle` (the parameter is now
  `_handles`, the codec's guarantee a comment), and the bounds `debug_assert` in `read`, which the
  slice indexing already enforces.

Size lines now: verity 130 (new crate), verityd 494, init 2076, rt 3446, fsd 1395, client 971.

Gates, through `machine host`:
- fmt --check 0.
- `cargo test` for verity, verityd, init, fsd, rt and testbench: 376 passed, 0 failed.
- Release builds on rv64 and rv32 of every redoubt_rt bin, with test-programs and beamlet: 0.
- size-budget, unsafe-budget, docs, no-cruft: 0.
- `git diff --check`: 0.

## QEMU cases on 61638faa9 (shared pool), 2026-10-05

jobs.mk declares its case targets .PHONY, and GNU make never searches pattern rules for phony
targets, so `make -f jobs.mk rv64/<case>` ran nothing ("Nothing to be done", rc 0). The cases ran
through /tmp/vol1-jobs.mk: the same recipe as a static pattern rule, the shared pool,
`jobserver take`, no -j.

| Case | rv64 | rv32 |
| --- | --- | --- |
| userland-bad-start | PASS | PASS |
| verity-flipped-tree | PASS | PASS |
| verity-wrong-root | PASS | PASS |
| userland-read-only | PASS 205.7 s | PASS 195.4 s |
| image-disk | PASS | PASS |
| init-boot | PASS | PASS |
| beamlet-boot, beamlet-console, beamlet-heap-flood, beamlet-budget-flood | PASS | PASS |
| userland-boot | FAIL 450 s | FAIL 450 s |

**userland-boot is a finding, not timing.** Everything VOL1 claims held on both widths, in order:
- NoSuch.call() stayed silent: UndefinedFunctionError, and no beamlet line.
- 55 printed.
- `verityd: block 765 does not match the tree`.
- `beamlet: Elixir.Version not loaded: its file could not be read: other`.
- The prompt came back.
- `beamlet: Elixir.OptionParser not loaded: ... other`.

The expected `UndefinedFunctionError ... Version.parse/1` never printed. Once fsd serves the volume
as corrupt, printing that error needs modules not yet loaded, and they now fail too: eval_bits,
Inspect.List and Inspect.Tuple; logger_backend, sys and error_logger for the crash report. The
shell prints "** (EXIT) the evaluation ended: a reason that could not be shown". That expectation
came from BEAM2's case. Brief case 1 asks only for verityd's line, beamlet's line naming the module,
and the prompt still there.

Proposed, not applied: drop the UndefinedFunctionError line after the flip, keep the Version and
OptionParser lines, and add a final input `1 + 1` expecting `> 2$` (prompt alive). Also forbid
`could not be read` before the flip. Awaiting a go.

## Point 6, measured (rv64, alone in my queue, shared host pool)

A local, uncommitted counter print in verityd, every 16384 reads, and two temporary cases. Both are
reverted and deleted; the tree is clean.
- **Boot to the prompt and `Enum.sum(1..10)` = 55:**
  - verified (image manifest, verity:system): 172.9 s;
  - unverified (the same manifest without verity, the volume attached to fsd directly): 102.9 s.
  - The verifier costs +70 s (+68 %).
- **Counts at 65,536 reads served** (just after the 55): 51,312 data blocks checked = 51,331 reads
  at blkd (the 19 more are tree blocks); level-1 hits 51,294 of 51,312 (99.96 %).
- **Where it goes:** littlefs reads a block in pieces and alternates between blocks, so the
  last-block buffer saves only 22 % of fsd's reads. Each block is fetched and hashed about 26
  times over the ~1,950 blocks the boot loads. The tree cache is not the cost: one extra
  verityd-to-blkd call and a SHA-256 per fsd read is.
- **A first try printing every 128 reads** (~600 console lines) never reached the prompt in 450 s:
  console output dominates.
- **Recommendation (brief point 6, "read-ahead ... can come later if the timing asks for it"; it
  does):** keep a few checked data blocks, LRU, beside the tree cache, so a block fsd reads in
  pieces is hashed once, or read ahead up to 8 blocks per blkd call. Both are local to verityd and
  change no protocol. Not done without a go.
- **Lends:** fsd's 2-page lend is unchanged. verityd's 256-page budget held: no exit, no fault.

Remains: the userland-boot ruling, then the serial whole bench on both widths.

## Renewal fold-up (vol1-implementer-2), 2026-10-05

Head 4e5ef2173 on main 6a7d2385e, seven commits, tree clean, not pushed.
- **userland-boot change in the userland commit** (ca45446dc) as ruled: UndefinedFunctionError for
  Version dropped, input `1 + 1` expecting `> 2`, reason in the commit body. The client commit
  (6bfcd86dc) adds the `: other` suffix, the OptionParser line and NoSuch forbid on top. The case
  text says "prints 2": "answers 2" tripped doccheck C11 (read as a QA reference).
- **P3s:** Counts under cfg(test) (verityd commit); SECTORS_PER_BLOCK in the verity, verityd and
  disk.rs tests (three literal 8s were left in verityd's cache test: fixed). The wraps rustfmt
  asked for are folded into each owner, including commit 6's tests/userland.rs.
- **Size budget:** servers/verityd 494 -> 488 (exact), in the rt commit that last set it.
- **Point 6:** already on verityd.md "Memory and cost" (measured bullet) with the planned section
  "A cache of checked data blocks" and its one Open item; checked, unchanged.

Run through the pool on this head's content:
- cargo test -p redoubt-verity -p redoubt-verityd -p testbench: rc 0 (90, 11, 8 passed).
- cargo testbench formatting rc 0, unsafe-budget rc 0, no-cruft rc 0, size-budget rc 0 (488 of
  494 before the ratchet).
- cargo testbench docs: rc 1 on C11 before the reword; the rerun is queued.
- userland-boot rv64/rv32: queued, never started (make killed at its 1 h limit, logs empty).

**Pool deadlock, 15:38 onward:** SCHED1's `jobserver all ... host-tests` (pid 2607174, child
`flock -x 5`, the gate) and MEM1's `jobserver all ... init-console-forgery` (pid 2619767, child
`flock -x 4`, the lock) wait on each other; every take/share is behind them. The gate file dates
from 16:35, after both started: likely the script was edited under a running bash. Not touched.

**Completed on 4e5ef2173 (pool cleared by the orchestrator):** docs rc 0 (after the C11 reword),
size-budget rc 0 (verityd 488 of 488), beamlet-redoubt --test userland rc 0 (2 passed).
userland-boot through jobs.mk, shared class: rv64 PASS 241.4 s, rv32 PASS 245.2 s (make rc 0).

## Whole bench on 2f7025638 (base main 46cb2f0ca), 2026-10-05

Rebase: `git rebase --signoff 46cb2f0ca`, clean; range-diff 6a7d2385e..4e5ef2173 vs 46cb2f0ca..2f7025638
is `=` for all seven commits.

Builds (jobs.mk build-rv64/build-rv32, `./build --arch <w> --programs`): rc 0, rc 0.

Cases through jobs.mk, no -j. The first `make cases-rv64 cases-rv32` (17:43) hit my tool's 2-hour
background cap at 19:43. The targets with no rc line were rerun by name in smaller makes, the
alone-class ones one make at a time. Logs: /tmp/vol1-bench*.log, /tmp/vol1-b2-*.log, target/jobs/.

| Width | Targets | rc 0 | rc != 0 | alone | bounded | net | shared |
| --- | --- | --- | --- | --- | --- | --- | --- |
| rv64 | 228 | 228 | 0 | 20 | 17 | 9 | 182 |
| rv32 | 228 | 227 | 1 | 7 | 17 | 9 | 195 |

No SKIP lines. rv32 ran 13 ssh-loopback/sshd-loopback cases as shared: jobs.mk reclassified them
between the two runs.

**The one failure:** target rv32/aio-many-reads (shared). The name is a substring filter, so the
target also ran aio-many-reads-two, which failed:
`FAIL aio-many-reads-two [rv32, smp=1] 21.4s forbidden output /aio-reader TEST FAILED/: aio-reader
TEST FAILED: receive: Timeout`. It is guest-reported (the reader's own line), beside other work.
- Its own target rv32/aio-many-reads-two passed in 1.8 s (shared).
- Rerun alone (`jobserver all cargo testbench --arch rv32 aio-many-reads`): both PASS, 1.5 s.
- aio is AIO1's, untouched by VOL1. Not fixed; for the orchestrator to rule.

## Final fold, 2026-10-05

Head d0aeb4f54 on main 995781152 (`git rebase --signoff`), seven commits. All in the userland commit
9e80389d0: verityd.md Open names no package ("a follow-up, with the signed root or on its own");
body says "as the case did before"; userland-boot forbids '%Version\{'.
`git diff 2f7025638 HEAD -- . ':!docs' ':!tests/userland-boot.toml'` is empty (0 bytes). The diff
stat vs 2f7025638 is 9 files +201 -127: main's three docs commits plus VOL1's 2 doc lines and 2 case lines.
jobs.mk: docs rc 0; rv64/userland-boot PASS 204.8 s, rv32/userland-boot PASS 186.8 s (shared).

## Rebases onto MEM1 (27e74ec3b) and MEM2 (3aafc7a83), 2026-10-06

Final head 9ca8c282d on 3aafc7a83, seven commits; `git merge-tree` onto main cca35a0e4 is clean.

Conflicts, each kept both sides:
- init's manifest.rs comment table and init.md rows: VOL1's `verity`, main's `stack_pages`/`heap_pages`.
- Cargo.lock: testbench's deps.
- The init bound test: VOL1's numbers with beamlet's stack of 17.
- image/README.md: "eleven servers".
- client tests' imports.
- Size ceilings, from the size-budget case's counts: init 2097, client 999, rt 3516.

Fixed after the rebase:
- init.md's status count is 22.
- VOL1's test helper `verified_volume` takes the image's `verity:system` entry (its declarations), so the verifier's cost test is still one server and its range (16 pages).

**Declarations, from six scans** (init-boot, userland-boot, userland-read-only on both widths):

| Server | Stack peaks (bytes) | stack_pages | Heap peaks (pages) | heap_pages |
| --- | --- | ---: | --- | ---: |
| `verity:system` | 5,120 to 7,864 (rv64 userland-boot) | 4 = ceil(2 × 7,864 / 4096) | 44 to 47 (rv64 userland-boot) | 94 = 2 × 47 |
| `fsd:system` | 12,696 (unchanged rule: 7) | 7 | 19 (rv64 userland-boot and read-only) | 38, was 32 |

`fsd:system`'s heap peak rose from MEM2's 16 to 19: it reads the plain-file volume through verityd.
With the cap at 32, rv64 userland-boot and userland-read-only failed the scan on that line alone.
The testbench.md table gains the verity:system row and fsd:system's 19/38, with the reason.

Results on f873f6a15 (9ca8c282d differs only in servers/init/tests/manifest.rs, a host test):
- build-rv64 and build-rv32 rc 0; docs, formatting, size-budget, unsafe-budget, no-cruft rc 0.
- memory, verity and client host tests rc 0; init host tests rc 0 on 9ca8c282d (50 passed).
- Shared, both widths: init-boot, userland-boot, userland-read-only, init-refuses-stack,
  verity-flipped-tree, verity-wrong-root, image-disk all PASS.
- The one exception, rv32 init-boot: "beamlet: no heap record found" beside other work; alone on
  9ca8c282d, PASS in 7.2 s.
