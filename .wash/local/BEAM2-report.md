# BEAM2 report

## Step 1 (tip 526b36e1a on main 75245a114)

Commits:
- fdfd131d7 init: each volume names its disk, and its range is minted at that disk's blkd
- 526b36e1a testbench: pack the userland disk, its objects named by their hashes, and system.index

### init's `disk` key
- `volumes[].disk` (optional string): the `servers` entry of the `blkd` serving the volume's disk.
  Required when the manifest has more than one `blkd` (`Why::NoDisk` at `volumes[i].disk`);
  refused if it names no `blkd` (`Why::Unknown` at `volumes[i].disk`).
- `check::blkd(m, v)`: the volume's own `blkd` (its `disk`, or the only one). Minting in init.rs,
  `labels.P=` per blkd (only its own disk's volumes), partition uniqueness per disk, confine's
  `users` (a blkd's range users are only the servers attaching its own volumes) all go through it.
- Host tests: `each_volume_s_range_is_minted_at_its_own_disk_s_blkd`,
  `confined_gives_each_label_set_its_own_userland_disk` (default: one attachment per label set).
  `a_volume_is_one_entry_for_one_server_at_one_blkd`: two blkds and no `disk` is now NoDisk.

### The packer
- `image/userland.toml`: `[objects]` applications kernel, stdlib, compiler, elixir, logger,
  redoubt_shell (whole, by their .app module lists); mix = userland/shell (MIX_ENV=prod);
  exclude application, gen_tcp, ram_file (embedded by the VM); docs = false;
  index target/image/system.index. One littlefs partition, 16 MiB disk.
- `tools/testbench/src/userland.rs` + `userland.exs`: strip with `beam_lib:strip/2` (Docs kept
  only with docs = true), objects `/<sha256 hex>`, `.app` resources keyed `<app>.app`.
- Measured (stripped): 564 objects (558 modules + 6 .app), 3,887,090 bytes (3.71 MiB). With
  Docs: 5,286,983 bytes (5.04 MiB). Plan's measure: idle prompt 102 modules / 1.03 MiB; seven
  apps 577 modules / 3.75 MiB. The idle prompt on host beamlet loaded 113 modules (incl. embedded).
- Determinism: two packs byte-identical (index and image), also after `rm -rf
  userland/shell/_build/prod` and a fresh compile.
- Bench: `[userland]` (recipe, flip, remove) on virtio-mmio-bus.5 (0x10006000, irq 6),
  `readonly=on`, drive id disk1; damage applied on a per-boot copy of the staged objects.
  `from = { userland_index = ... }` bundle form; `image/boot.toml` entry `system.index`;
  image manifest `public = ["system.index"]`; init-boot expects 1 public entry; image-disk carries
  the index.

### Commands (all via .wash/local/in-dev, from the worktree)
- cargo testbench init-host-tests: PASS (0)
- cargo testbench host-tests (all *host-tests matched): all PASS (0)
- cargo test -p testbench: 76 passed (0)
- cargo testbench docs: PASS; no-cruft: PASS; vendor-check: PASS
- cargo +nightly fmt --all --check: 0
- cargo build --release -p redoubt-init for riscv64gc and riscv32imac: 0
- cargo testbench --pack-disk image/userland.toml target/image/userland.img: 0

Not yet run (QEMU, needs the host): init-boot and image-disk, both widths.

### Design problem found
In a confined boot a labelled beamlet cannot be handed `bootfsd` (unlabelled, shared, not exempt
in confine.rs): R34's endpoint check refuses it, and the manifest may name `bootfsd` only once.
So under the default (one userland attachment per label set) a labelled beamlet still cannot
read `/boot/system.index`. The confined host test hands each beamlet only its own fsd:system.

### Page lines written so far
- init.md: the `volumes` row ("each volume's name, `blkd` partition, label set and disk (the
  `servers` entry of the `blkd` serving it)"); the Volumes bullet per disk with `disk`; "an `fsd`
  on that `blkd`'s disk"; "So in a confined boot each disk holds one label set's volumes, and its
  `blkd` carries that set." Status lists gain the two tests. R34 unchanged.
- testbench.md "Disks and network cards": `[userland]` in the TOML, a paragraph on the userland
  recipe, `userland_index`, `flip`/`remove`; the slot sentence names both disks and read-only.
- bootfsd.md "Serving `/boot`": "**The userland disk's table.** The image's `/boot` carries
  `system.index`, the userland disk's table, one line per object naming its module, SHA-256 and
  length, signed with the rest of the bundle ([image/README.md](../../image/README.md))."
  (R75 link added when R75 exists.)
- image/README.md and mkimage's header: userland.toml, target/image/userland.img, system.index.

## Step 2 so far (tip dd4ccce6b; history rebuilt: c263070c5 is the packer commit with crypto added)

- beamlet-redoubt: `userland.rs` (Index::parse strict, Checked<O: Objects>), `Modules::load` now
  `Result<Vec<u8>, Unloaded>` (Absent | Refused(reason)); the platform says
  `beamlet: <name> not loaded: <reason>` on the console. Reasons: "its object does not match
  system.index", "its object is missing", "its object is short", "its object could not be read".
- beamlet bin: index from /boot (exit 3 if absent/malformed), objects from handle `fsd:system`,
  start module pre-checked: refused -> `beamlet: <m> not loaded: <why>; parked`, sleeps forever;
  absent from index -> exit 2. Prints `beamlet: N objects in /boot/system.index, read from fsd:system`.
- Embedded list unchanged (the VM's own modules, loaded at VM start before any lookup):
  beamlet_io, application, gen_tcp, beamlet_tcp, beamlet_code, beamlet_kernel, beamlet_port,
  ram_file (+ logger/error_logger fallback only when the platform lacks OTP's logger).
- image/userland.toml applications now 7 (crypto added: redoubt_shell's extra_applications):
  567 objects, 3,918,951 bytes stripped (3.74 MiB); 5,336,335 with Docs (5.09 MiB).
- Beamlet cases moved to a test userland disk (tests/data/beamlet/userland.toml: io, io_lib,
  io_lib_format, unicode, crypto, lists + the four test .erl), their manifests gain disk1, blkd,
  fsd:system; `[objects]` gained `modules` and `erlang`.
- Host: beamlet-redoubt tests 3 new pass; testbench 76 pass; docs PASS; fmt 0; beamlet
  rv64+rv32 release build 0. QEMU not run yet (beamlet-boot's bound line, budgets.md's 416, will
  change).
- Page lines: boot.md R75 (after R15); SECURITY.md R75 row; beamlet.md: lookup row, "a lookup,
  not a gate" names R75, status loses "read from /boot unchecked", Natives Open closed to
  "**Open:** none." with the embedded list, parked start and confined sentences. packages.md
  paragraph. bootfsd.md and testbench.md link R75.
- Page conflict: doccheck rule C1 allows `**Open:**` only in planned sections, exactly one each.
  So the brief's Open in "beamlet on Redoubt" (built) is written as a plain sentence ("Not yet
  decided: whether a read-only volume checked against the signed bundle may be shared across
  label sets in a confined boot, instead of one attachment per label set."), and Natives
  (planned) keeps "**Open:** none.".

## Rulings applied (architect-12), tip ed2774c9d (history rebuilt: 0abf79087 packer, ed2774c9d beamlet)

- Index keyed by file name: `<file> <sha256 hex> <bytes>` (`Elixir.Enum.beam`, `elixir.app`),
  sorted byte-wise. beamlet looks up `<module>.beam` / `<app>.app` as asked; index names must end
  in `.beam` or `.app` (max 260 bytes). `exclude`, `flip`, `remove` name files.
- `--pack-disk` writes an index only when the recipe's `[objects]` declares `index`; disk.toml
  packs as before.
- Measures: whole applications (7): 567 objects = 560 modules + 7 .app, 3,918,951 bytes (3.74 MiB)
  stripped, 5,336,335 (5.09 MiB) with Docs. Idle prompt's closure (host beamlet, 113 loaded incl.
  embedded/preloaded): 104 of them on the disk, 1,050,359 bytes (1.00 MiB). Plan: 102 / 1.03 MiB
  and 577 / 3.75 MiB.
- Page lines: R75 "A module or application resource the system resolves by name, and a program
  it launches from the userland disk, is used only if its bytes hash to the entry `system.index`
  in the signed bundle gives it ..." with the `<file> <sha256 hex> <bytes>` format; bootfsd.md
  states the line format; packages.md "whole applications, every module of each one the shell's
  prompt uses"; testbench.md "Bundle files" shows the `userland_index` form and "Disks" says
  flip/remove change only the disk and when an index is written; image/README.md likewise.
- Host: beamlet-redoubt tests, testbench 76, docs PASS, fmt 0.

## beam2-implementer-2, step 1: the six owed QEMU cases (2026-10-03)

Tip 0bb5ca1e7 on main 75245a114 (d9cbb62d7, 1ac449348, 0bb5ca1e7; folded, no WIP).

First runs, tip 7bfc37ec3: init-boot PASS rv64/rv32 (0); image-disk PASS rv64/rv32 (0); the four
beamlet cases FAIL both widths (1): `beamlet: beamlet_boot:start did not start: Error undef`, then 5
restarts and `init: rebooting`. The index read and the start module's check passed.

Cause: `beam_lib:strip/2` returns its output gzipped (objects began 1f 8b). beamlet's loader takes
a module's chunks uncompressed, so `Sys::module` dropped the load error and the VM said undef. Fix:
`tools/testbench/src/userland.exs` writes `:zlib.gunzip(stripped)`, folded into the packer commit
(its app loop) and the beamlet commit (the `strip` helper). That is in the bench's packer, not in
fsd, blkd or the VM.

Second fix, expected: beamlet-boot's bound line is now
`'^init: the manifest is checked: 6 servers, bound 446 pages$'` (both widths print 446), and
docs/kernel/budgets.md:196 says "With `beamlet` and the userland disk's `blkd` and `fsd`, the bound
is 446 pages on both widths (`beamlet-boot` prints it), and 1,024 at least doubles it." (paragraph
reflowed). Folded into the beamlet commit.

Reruns, each `in-dev cargo testbench <case>`, one at a time:
- beamlet-boot, beamlet-console, beamlet-heap-flood, beamlet-budget-flood: PASS rv64+rv32, exit 0.
- init-boot, image-disk (rerun after the pack fix): PASS rv64+rv32, exit 0.

New measures (objects are now stored uncompressed): `--pack-disk image/userland.toml` exit 0, 567
objects, 7,617,285 bytes (was 3,918,951 gzipped); it fits the recipe's size_kib 16384. Not yet
re-measured: with Docs, and the idle prompt's closure (was 1,050,359 gzipped). I'll re-measure both
with the shell entry. No page quotes either size.

## beam2-implementer-3, step 1: host reruns, point 7 begun, the shell's measure (2026-10-03)

Tip 8e9d24a76 (the beamlet commit refolded) on main 75245a114.

- Host reruns: `in-dev cargo test -q -p testbench`: 76 passed (0). `cd userland/otp && in-dev
  cargo test -q -p beamlet-redoubt --features fake`: first run 1 failed (101):
  `a_module_loads_only_if_its_object_hashes_to_its_entry` still named its entries `good` etc.,
  which the file-name ruling refuses; renamed to `good.beam` etc., folded into the beamlet commit;
  rerun all pass (0). userland/otp `fmt --check` 0.
- Uncommitted, point 7: `RecipeEntry.workspace` (with a package only; tested in
  `the_image_recipe_packs_init_the_servers_and_the_manifest`, 1 passed, 0); image/boot.toml's
  `beamlet` entry (beamlet-redoubt, workspace userland/otp).
- Found: a second `blkd` cannot run: blkd.rs:56 looks up the handle `blkd` only (question sent).
- Found: the shell's prompt calls `File.cwd()`; the file server calls `prim_file:get_cwd/0`;
  `prim_file` is erts's (preloaded on BEAM, an application with `erts.app` and its ebin in the
  pinned OTP), not on the disk: `undef`, the shell dies at its first prompt. Under ruling 1
  (whole applications the closure touches) image/userland.toml now names `erts` too: 587 objects
  (579 modules + 8 .app), 7,837,721 bytes stripped and uncompressed.
- Measure (scratch case, not committed: tests/userland-measure.toml, a one-blkd manifest, 1 GiB
  RAM, beamlet budget 32,768; a temporary init patch, .wash/local/BEAM2-measure-init.patch,
  sampled the beamlet budget's pages_usage every second, reverted): the prompt answers
  `Enum.sum(1..10)` with `55` on both widths (PASS, 198 s rv64, 170 s rv32). The VM's own use
  at the prompt, peak = last: **rv64 11,877 pages (46.4 MiB), rv32 7,554 pages (29.5 MiB)**.
  Twice rv64's: 23,754; I'd give 24,576 (96 MiB) on both widths.
- Problem: at QEMU's default 256 MiB, `system` is 15,402 pages (a quarter of root). The image's
  servers other than beamlet take 9,984 (with the userland blkd and fsd), so beamlet at 24,576
  needs `system` >= ~34.6k pages: RAM >= ~560 MiB; at 1 GiB `system` is 64,540. At 8,192 pages
  the shell ran out of memory at its first line and restarted to the limit.
- Each shell boot takes ~3 min on QEMU (boot to `55`).

### architect-13's ruling (b) built: b79813ea6 blkd: receive on the endpoint its endpoint= argument names

- servers/blkd/src/args.rs: `receive_endpoint(startup) -> Result<Option<Handle>, BadArgs>`
  (exactly one `endpoint=`, a valid name); `range_labels` skips it. bin/blkd.rs: no or bad
  `endpoint=` -> BAD_ARGS (4); no handle by that name -> NO_ENDPOINT (2).
- Host test `blkd_receives_on_the_endpoint_its_argument_names_and_never_guesses` (serves on
  `blkd:system`; none, empty, `Blkd`, twice refused; a name with no handle is None).
- Manifests, scripted (each file re-parsed: only `endpoint=<receives[0]>` prepended to each blkd's
  args): image/manifest.json; tests/data/init/{bound (8 blkds), budget-handle, confined-server,
  console-forgery, consoled-handed, device-dma, device-unmatched, held-bundle-key,
  held-login-key, reboot, reporter-forged, second-keyd, servers, system-fit}.json;
  tests/data/fsd/{boot, confined, label-check, one-volume}.json; tests/data/beamlet/{boot,
  console, flood, budget-flood}.json. servers/init/tests/manifest.rs: second_disk's blkd args,
  three expected arg lists. Fuzz seeds left as corpus (init does not check blkd's args).
- Pages: blkd.md "Its endpoint." bullet before the label check (verbatim), its test listed
  (Ranges and badges, 13); init.md's sentence after R35's (verbatim). "Failure and restart"
  lists no BAD_ARGS line, so unchanged.
- Commands: `in-dev cargo test -q -p redoubt-blkd` 0; `in-dev cargo testbench init-host-tests`
  PASS 0; `in-dev cargo testbench docs` PASS 0; fmt --check 0; blkd release rv64 + rv32 0.

### Image manifest (uncommitted)
- disk1, volume `system` (disk `blkd:system`; `data` gains disk `blkd`), `blkd:system`,
  `fsd:system`, `beamlet` (24,576 pages, handed bootfsd and fsd:system, args
  `budget_pages=24576 Elixir.Redoubt.Shell`). init's fuzz ENTRIES gains `beamlet` (4,000,000).
- init's host tests then refuse the image: `SystemFit { pages, need: 34570, free: 16000 }`
  (the test machine's system models 256 MiB). Waiting on the RAM question.

### Cases 1-5 (released by the owner's ruling), uncommitted, run at memory_mib = 1024

All boot the image's own bundle (`recipe = "image/boot.toml"`, so the image's manifest with the
shell) with `[userland] recipe = "image/userland.toml"`, the image's `[disk]` and `[net]`;
userland-read-only is built as image-disk is (the image's programs + fsd-client, the image's
manifest with `client` merged by name). Each `in-dev cargo testbench <case> --arch rv64`:
- userland-boot PASS (0) 213.7 s: count line `beamlet: 587 objects in /boot/system.index, read
  from fsd:system`, banner, `/ (1)> 55`.
- userland-flipped-byte PASS (0) 221.9 s: flip `Elixir.Version.beam`; `Version.parse("1.2.3")`:
  `beamlet: Elixir.Version not loaded: its object does not match system.index` (printed 3 times:
  the VM looked the module up three times, each lookup refused, nothing loaded),
  UndefinedFunctionError, then `/ (2)> 55`.
- userland-missing-object PASS (0) 190.7 s: remove `Elixir.Version.beam`; the same with "its
  object is missing".
- userland-bad-start PASS (0) 6.9 s: flip `Elixir.Redoubt.Shell.beam`: `...; parked` after the
  index push; no restart, no reboot within the bench's grace.
- userland-read-only PASS (0) 148.2 s: fsd-client `readonly fsd:system Elixir.Version.beam`
  (handed bootfsd and fsd:system, badge 8) reads the object's name from /boot/system.index, a
  create and a write (OWRITE, and OWRITE|OTRUNC) are refused, and the object reads back the
  same length; the verdict is the system's: the shell then calls `Version.parse!("1.2.3") |>
  to_string()` -> `"1.2.3"`, which beamlet loads only if the attacked object still hashes to the
  bundle's entry.
- rv32: running.

Finding (fsd, not mine): listing a directory is quadratic. `entry_name(dir, i)` re-reads the whole
littlefs directory for each entry (servers/fsd/src/server.rs:426), so a client listing the
userland root (587 files) had not finished after 400 s, and while it ran fsd:system answered
nobody else: beamlet never reached its banner. Any client of an fsd can hold it this way with a
large directory. My first case 5 listed the root; it now attacks one object instead.

### architect-14 (confined index closed)
beamlet.md, "beamlet on Redoubt", after the parked-start sentence (verbatim): "A confined boot runs
no labelled beamlet: beamlet reads `system.index` through `bootfsd`, one instance a labelled domain
may not share with the unlabelled ones ([R34 (confined placement)](../servers/init.md#r34-confined-placement))."
The brief's confined sentence is not written. docs PASS (0). Uncommitted, folded into the beamlet
commit with the cases.
- rv32 (`--arch rv32`): userland-boot PASS (0) 128.0 s; the other four owed (host handed to K22/ABI1 at the orchestrator's word, paused).

## beam2-implementer-3, step 2: architect-14's ruling 5 built (point 7 whole), tip 1c92d78f4

Commits on 75245a114 (each final, fixups folded):
- cf29daef1 init: each volume names its disk... (+ `Size budget: servers/init:` line, 1917 -> 1937)
- 3966ad80e testbench: pack the userland disk... (erts added; message says `<file>`; init 1937 -> 1938)
- a7d98e3da beamlet: modules from the userland disk... (test names `.beam`; beamlet.md confined line)
- d056f03f9 blkd: receive on the endpoint its endpoint= argument names (blkd ceiling 1606 -> 1617)
- 1c92d78f4 image: the shell on the UART, its modules from the userland disk (init 1938 -> 1939)

Ruling 5 as built:
- RAM (a): image beamlet budget 24,576 / budget_pages=24576; memory_mib = 1024 in init-boot,
  image-disk, userland-boot, userland-bad-start, userland-read-only; init's test machine
  `system` usage(64_000). init tests rebased where the image's beamlet changed what they tested:
  `without_userland()` (no disk1/blkd:system/fsd:system/beamlet), used by tests about other rules.
- erts whole: excluded list unchanged, `application.beam`, `gen_tcp.beam`, `ram_file.beam` (no
  embedded module is an erts module). Note: 14 erts modules the VM never loads
  (vm.rs RUNTIME_MODULES: init, erl_prim_loader, prim_inet, ...) are on the disk as whole-app.
  Final: 587 objects = 579 modules + 8 .app, 7,837,721 bytes stripped; 9,310,929 with Docs.
- Cases: userland-boot carries flip `Elixir.Version.beam` and remove `Elixir.OptionParser.beam`;
  userland-flipped-byte and userland-missing-object dropped. bad-start, read-only separate.
- Page lines verbatim: budgets.md (after the 446 sentence; the next sentence's "It" became "The
  bound" so it still names the bound); image/README.md ("The image needs 1 GiB of RAM (QEMU
  `-m 1G`): the shell's budget, twice what its VM uses at the prompt, does not fit the `system`
  budget of a smaller machine"); mkimage header the same; boot.md R75 and SECURITY.md list
  bench:userland-bad-start, bench:userland-boot + the 3 host tests; testbench.md "one disk may
  carry both, one object flipped and another removed"; lists in beamlet.md and testbench.md.

Host gates at 1c92d78f4 (each `in-dev cargo testbench <case>`, exit 0 unless said): init-host-tests,
blkd-host-tests, fsd-host-tests, docs, no-cruft, unsafe-budget, vendor-check, size-budget PASS;
`cargo test -q -p testbench` 76 passed; beamlet-redoubt tests pass; fmt --check 0 (root and
userland/otp); init and blkd release rv64 + rv32 0; fsd-client rv64 + rv32 0.

Owed, needs the host: userland-boot (new form), bad-start, read-only on rv32 and userland-boot on
rv64; init-boot and image-disk both widths; userland-boot's alone and whole-run times; the
whole bench both widths; then the rebase onto 53bcd9704 and host gates again.

## Editor's and simplifier's notes on 1c92d78f4 (staged as fixup/amend commits, unfolded; one fold after Red)

Editor:
- Rewrapped to 100 columns: boot.md R75 paragraph, budgets.md:196-201, bootfsd.md's userland bullet
  (fixups to the beamlet and image commits). docs PASS.
- Size budget lines in CONTRIBUTING's form with counts: init 1917->1937 (20 lines), 1937->1938
  (1), 1938->1939 (1); blkd 1606->1627 (21: see below). a7d98e3da carried no size line; the one
  beside it is d056f03f9's (blkd), with its size-budget.toml change.

Simplifier:
1. userland.rs: `write()` returns `Names` (file -> object); `stage()` returns it; `Builder` keeps
   each staged disk (`Staged { objects, index, names }`, `Mutex<Vec<..>>`, staged once per run);
   `damaged()` looks objects up in `Names`; the second index parser is gone. This fixup sits after
   the beamlet commit in the fold (it conflicts if put on the packer commit, which the beamlet
   commit changed after); a trial fold on a scratch branch is clean and the tree identical.
2. blkd args.rs: `parse_args(args) -> Args { endpoint, labels }`, scanned once as fsd's;
   `Args::range_labels(&roots)` checks each P against the table; no endpoint (or two, or a bad
   name) is BAD_ARGS, a name with no handle NO_ENDPOINT. 1,627 lines (ceiling raised to 1,627
   with the reason line). Host test unchanged in name, now through parse_args + Startup.
3. init: `check::on_disk(m, v, s)` used by `args()`, confine's `users()`; `volumes()` asks
   whether the named volume has a receiving blkd without the expect re-find.
4. Kept the four beamlet manifests: they differ only in the start module (one line), so the
   existing `servers` merge could fold them into boot.json with no devices/volumes merge; but
   BEAM1's `every_beamlet_is_told_its_own_budget` reads JSON manifests only, and a start module
   overridden in a case's TOML would escape it. A userland recipe's `stage` is now optional
   (only `--pack-disk` needs it; the bench packs from its own staging); the beamlet recipe's
   unused line is gone; disk.rs test covers it.
5. mkimage stages the userland objects twice (once in `--pack-disk` for the disk and the index,
   and the bundle's userland_index entry again through the builder): noted, no change.

Host after the edits: testbench 76, blkd tests 29+13+7, init-host-tests PASS, size-budget PASS,
docs PASS, fmt 0, blkd rv64/rv32 0, `--pack-disk image/userland.toml` 587 objects, 7,837,721.

## Folded tip e31155894 (editor, simplifier, Red folded; on 75245a114)

- 9f9a7f566 init: each volume names its disk... (init 1917 -> 1936, 19 lines)
- 34b5bb65d testbench: pack the userland disk... (init 1936 -> 1937)
- b9a07f8df beamlet: modules from the userland disk... (simplifier's packer map lands here: it
  conflicts on the packer commit, which this one changed after)
- be50bfc62 blkd: receive on the endpoint its endpoint= argument names (blkd 1606 -> 1627;
  init 1937 -> 1951 for Red's check)
- e31155894 image: the shell on the UART... (init 1951 -> 1952)
Counts measured per commit (size-budget with the later commits' init sources backed out).

Red:
1. init's check: a `blkd`'s `endpoint=` must be exactly one and name `receives[0]`, where init
   mints its ranges; else `Why::BlkdEndpoint` ("a blkd's one endpoint= names the endpoint it
   receives on first") at `servers[i].args[k]` (or `.args` when absent). Host test
   `a_blkd_receives_where_init_mints_its_ranges` (wrong name, none, two, not the first receive,
   then accepted), listed under "The boot manifest" (18). init.md's sentence gains: "; a `blkd`'s
   must name the endpoint it receives on first, where `init` mints its volumes' ranges."
2. userland-read-only's description trimmed: "Its blkd reports the disk read-only, so fsd serves
   the volume read-only and refuses the create and the writes itself, before they reach blkd or
   the host, which attaches the disk read-only too".
3. vm.rs `locate_module`: a refused system module answers `None` like an absent one and falls
   through to the code path; unreachable today (no files on Redoubt). docs/todo/
   beamlet-refused-module-falls-through.md (in SUMMARY.md), for BEAM3/BEAM4; no code change.

Host gates at e31155894, each exit 0: size-budget, init-host-tests, blkd-host-tests,
fsd-host-tests, docs, no-cruft, unsafe-budget, vendor-check PASS; testbench 76; beamlet-redoubt;
fmt --check (root, userland/otp); init, blkd, fsd-programs release rv64 + rv32.

## beam2-implementer-4: QEMU runs, rebase onto 0d207732f, host gates (2026-10-03)

QEMU at e31155894, alone, one case at a time, each `in-dev cargo testbench <case> --arch <w>`,
exit 0, PASS: userland-boot rv64 137.7 s, rv32 142.7 s; init-boot rv64 5.3 s, rv32 5.6 s;
image-disk rv64 5.7 s, rv32 6.2 s; userland-bad-start rv32 7.5 s; userland-read-only rv32
149.5 s (it waits for the prompt: its verdict is the shell's Version call). No fixes needed.
`timeout_secs` stays 400 on all three userland cases: ruling 5 sets it at twice the whole-run
time, which the orchestrator's whole bench measures (twice alone is ~290 s).

Rebase `--onto 0d207732f 75245a114 wp-beam2`: two conflicts.
- testbench main.rs (b23275bb3): B6 split run_case into build_case -> `Built` and boot_case;
  the staged userland now travels in `Built` (`userland` field, staged in build_case after
  `prepare`, its failure returned as `Ok(Err(..))` like prepare's). The beamlet commit
  (fa3ba74d0) changes that field to `Option<userland::Staged>` with the packer map (fixup folded).
- docs/SUMMARY.md (fa3ba74d0): K22's two todo entries and BEAM2's one, all kept.

Tip d5cc7e56c on 0d207732f:
- 63adb5110 init: each volume names its disk, and its range is minted at that disk's blkd
- b23275bb3 testbench: pack the userland disk, its objects named by their hashes, and system.index
- fa3ba74d0 beamlet: modules from the userland disk, each checked against system.index
- 700b9a19a blkd: receive on the endpoint its endpoint= argument names
- d5cc7e56c image: the shell on the UART, its modules from the userland disk

Host gates at d5cc7e56c, each exit 0: size-budget, init-host-tests, blkd-host-tests,
fsd-host-tests, docs, no-cruft, unsafe-budget, vendor-check PASS; `cargo test -q -p testbench`
82 passed; beamlet-redoubt --features fake (7+7+3); `cargo +nightly fmt --check` root and
userland/otp; release builds of redoubt-init, redoubt-blkd, redoubt-fsd-programs for
riscv64gc and riscv32imac. Size ceilings unchanged (size-budget passes as rebased).
QEMU cases were not rerun after the rebase (the whole bench is the orchestrator's).

Architect's fold (orchestrator's ruling (a)): shell.md "The shell in a session" to built, partly
tested (UART console only; files and launching BEAM3's/BEAM4's, sessions the steward's; tested
bench:userland-boot); its Open on h/1's Docs chunks replaced by the rule as built and the
residual (no Docs chunk on the disk; the optional documentation package not built). Folded into
the image commit: tip 453fdcf97 on 0d207732f; docs exit 0.

## Final: tip 51426a98f on 0d207732f

Whole bench on 453fdcf97 (orchestrator's): 385 PASS, 0 FAIL, 1 SKIP (podman). Whole-run times:
userland-boot 222.6 s rv64 / 212.6 s rv32; userland-read-only 160.7 / 164.9; userland-bad-start
8.0 / 8.8. Ruling 5 folded into the image commit (which adds all three cases), with one line in
its message: userland-boot timeout_secs 400 -> 450, userland-read-only 400 -> 330;
userland-bad-start keeps 400. Docs exit 0 at 51426a98f. Commits: 63adb5110 init, b23275bb3
testbench, fa3ba74d0 beamlet, 700b9a19a blkd, 51426a98f image.
Measures: 587 objects (579 modules + 8 .app), 7,837,721 bytes stripped (9,310,929 with Docs);
excluded application.beam, gen_tcp.beam, ram_file.beam; VM at the prompt rv64 11,877 pages, rv32
7,554, budget 24,576. Residuals: docs/todo/beamlet-refused-module-falls-through.md (BEAM3/BEAM4);
shell.md's optional documentation package not built.

## Implementer 5: rebase over FSD4 (971e36840), 2026-10-03

`git rebase --onto 971e36840 0d207732f wp-beam2`. Tip **7c0c52fbe**:
c38acf089 init, f6f8584bd testbench, 18cb0bb75 beamlet, b41987052 blkd, 7c0c52fbe image.
Each conflict was resolved inside the commit it belongs to; no fix-up commits are left.

- testbench (f6f8584bd): disk.rs auto-merged. `pack_disk` keeps one staging path,
  `stage.or(p.stage)`, which the userland objects (the bench's own staging, or `--pack-disk` into
  the partition's stage) and FSD4's `generated` files both go through. New check in
  `Recipe::load`: a recipe with `objects` is one littlefs partition with a stage and nothing
  generated. Without it, objects plus `generated` and no stage would load, and `--pack-disk`
  would skip staging the objects without saying so. Asserts are in
  `a_recipe_can_generate_a_directory_of_files`. In docs/testbench.md both paragraphs are kept,
  plus "Nothing is generated beside the objects."; status count 14.
- beamlet (18cb0bb75): an objects recipe may leave its stage out, so the objects check becomes
  "one littlefs partition, nothing generated". `pack_disk` refuses a partition with no stage
  and nothing generated, which the commit's `nothing to pack` assert needs. Added assert: a
  stage-less objects recipe loads. Docs: the commit's text plus the same sentence.
- image (7c0c52fbe): docs status (16), with userland-boot, userland-read-only and FSD4's test.
  tests/fsd-programs fsd-client.rs imports `{Endpoint, sleep, time_now}`; its `list`/`reader`
  checks (FSD4) and `readonly` (BEAM2) are both kept.

Host runs at 7c0c52fbe (all through in-dev):
- cargo test -p testbench: exit 0 (83 passed)
- cargo testbench docs: exit 0 (PASS)
- cargo testbench size-budget: exit 0 (PASS)
- cargo test -p redoubt-init -p redoubt-blkd -p redoubt-fsd: exit 0
- cargo +nightly fmt --all --check: exit 0
- redoubt-fsd-programs builds on rv64, rv32 and the host.

Pending, waiting for the host: userland-boot rv64, image-disk rv64, fsd-large-directory rv64, one at a time.

### QEMU runs (the host granted by the orchestrator), one at a time

- `cargo testbench --arch rv64 userland-boot` at 7c0c52fbe: exit 0 (PASS, 150.6s)
- `cargo testbench --arch rv64 image-disk` at 7c0c52fbe: exit 0 (PASS, 5.8s)
- `cargo testbench --arch rv64 fsd-large-directory` at 7c0c52fbe: exit 1. init refused the boot:
  "servers[2].args: a blkd's one endpoint= names the endpoint it receives on first". FSD4's
  manifest tests/data/fsd/large-directory.json was written before BEAM2's blkd commit, which
  gives every blkd an `endpoint=`. Fixed inside that commit (now c8545d309):
  `"args": ["endpoint=blkd"]`. No other manifest in the tree has a blkd without `endpoint=`.
- `cargo testbench --arch rv64 fsd-large-directory` at 5089a4228: exit 0 (PASS, 1.8s)
- `cargo test -p redoubt-init -p redoubt-blkd -p redoubt-fsd` at 5089a4228: exit 0

The tree at 5089a4228 differs from 7c0c52fbe only in that manifest line, so the first two passes
still hold.

**Final tip: 5089a4228**
c38acf089 init, f6f8584bd testbench, 18cb0bb75 beamlet, c8545d309 blkd, 5089a4228 image.
