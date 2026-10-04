# BEAM2 handoff 1 (beam2-implementer, 2026-10-03)

Brief: /home/mcloonan/redoubt/.wash/local/BEAM2-implementer.md (with its "Rulings during the
build"). Detail of what was done and measured: .wash/local/BEAM2-report.md in this worktree.
Worktree /home/mcloonan/redoubt/.worktrees/beam2, branch wp-beam2, clean.

## Tip 7bfc37ec3 on main 75245a114 (three commits, each final; no WIP)

- d9cbb62d7 init: each volume names its disk, and its range is minted at that disk's blkd
  (`volumes[].disk`, Why::NoDisk, `check::blkd(m, v)`, per-disk minting/labels/partition/confine
  users; tests each_volume_s_range_is_minted_at_its_own_disk_s_blkd and
  confined_gives_each_label_set_its_own_userland_disk, the latter listed under the confinement
  check and R34 in init.md and in SECURITY.md's R34 row).
- 70e2ad9c2 testbench: pack the userland disk (tools/testbench/src/userland.rs + userland.exs;
  image/userland.toml, 7 whole applications; `[userland]` case key on virtio-mmio-bus.5
  readonly=on with flip/remove naming files; bundle form `from = { userland_index = RECIPE }`;
  image/boot.toml entry system.index; image manifest public ["system.index"]; init-boot expects
  1 public entry; image-disk carries the index; init fuzz ENTRIES has system.index).
- 7bfc37ec3 beamlet: modules from the userland disk, each checked against system.index
  (userland/otp/redoubt/src/userland.rs: Index::parse, Checked; `Modules::load -> Result<_, Unloaded>`;
  beamlet bin: index via `index_bytes`, objects via handle `fsd:system`, start module pre-checked
  and parked on refusal; R75 in boot.md + SECURITY.md; beamlet.md, packages.md, bootfsd.md,
  testbench.md lines; the four beamlet cases moved to tests/data/beamlet/userland.toml with
  blkd + fsd:system in their manifests).

History rule in use: changes are folded into the owning commit with `git commit --fixup=<sha>`
then `GIT_SEQUENCE_EDITOR=: git rebase -q -i --autosquash 75245a114` (tree must be clean).

## Owed: QEMU, never run on this branch (both widths, one at a time)

init-boot, image-disk, beamlet-boot, beamlet-console, beamlet-heap-flood, beamlet-budget-flood.
Before each: `ps -eo args | grep '[t]estbench --allow-skip'`; a run with no case name is a whole
bench: wait for the orchestrator. Expected edits after the first runs: beamlet-boot's
`'^init: the manifest is checked: 4 servers, bound 416 pages$'` (now 6 servers, bound changes) and
docs/kernel/budgets.md:196's "416 pages" quote; possibly budgets of blkd/fsd in the beamlet
manifests. If a boot fails, fold the fix into the owning commit.

## Pending with the owner

How a labelled beamlet in a confined boot gets system.index (it cannot be handed bootfsd under
R34). Detail: /home/mcloonan/redoubt/.wash/local/BEAM2-confined-index-ruling.md. Recommendation
in flight: the index on the userland disk, pinned by its hash in beamlet's manifest entry. The
index source is behind ONE function: `index_bytes(startup)` in
userland/otp/redoubt/src/bin/beamlet.rs (today: /boot/system.index through handle `bootfsd`).
Do not write beamlet.md's confined sentence or build an option until architect-13 sends the
ruling. The confined host test hands each beamlet only its own fsd:system.

## What remains of the brief

- Point 7: image manifest gains disk1 (base 268460032, irq 6, dma), blkd:system, fsd:system
  (volume system, disk "blkd:system"; data volume gets disk "blkd"), beamlet entry
  (program beamlet, handed bootfsd and fsd:system, args budget_pages=N Elixir.Redoubt.Shell,
  function `start`), with N >= 2x the VM's measured use at the prompt (measure, report).
  image/boot.toml needs a beamlet entry: RecipeEntry has no `workspace` yet (beamlet is in
  userland/otp's workspace) — add it. init-boot and image-disk then need `[userland] recipe =
  "image/userland.toml"`, and their expected lines change (servers count, public entries).
  init.md / testbench fixed-slot test: add the disk1 line check in qemu.rs
  devices_sit_on_fixed_slots once the image manifest has disk1.
- Cases (both widths): userland-boot (prompt, `Enum.sum(1..10)` -> 55, the count line
  `beamlet: N objects in /boot/system.index, read from fsd:system`), userland-flipped-byte (flip
  a file loaded on demand; mismatch line `beamlet: <name> not loaded: its object does not match
  system.index`; forbid `init: rebooting` and `exited`), userland-bad-start (flip the start
  module's file: `...; parked`; forbid `init: restarted beamlet`, `init: rebooting`),
  userland-missing-object (remove), userland-read-only (a test program handed fsd:system tries
  create and write: refused, volume unchanged). Then add them to R75's status (boot.md,
  SECURITY.md) and beamlet.md/testbench.md lists.
- Pages still to write: shell.md "The shell in a session" (modules from the userland disk with
  R75 link; status built for the UART console only, partly tested) — with the userland-boot
  case; budgets.md bound quote.
- Gates: whole bench both widths alone; host tests of beamlet-redoubt (run directly:
  `cd userland/otp && in-dev cargo test -q -p beamlet-redoubt --features fake`, no bench case runs
  them), init, fsd, testbench; Elixir differential if shell modules change; fmt (root and
  userland/otp), size and unsafe budgets, docs, no-cruft, vendor-check/vendor-build.

## Facts to reuse

- Host gates last run green: init-host-tests, all *host-tests, testbench 76, beamlet-redoubt 3
  new, docs, no-cruft, vendor-check, fmt, init and beamlet release builds rv64+rv32.
- Packing: `in-dev cargo testbench --pack-disk image/userland.toml target/image/userland.img`
  ~10 s; 567 objects (560 modules + 7 .app), 3,918,951 bytes; with Docs 5,336,335; idle prompt
  closure on disk 104 modules, 1,050,359 bytes. Byte-identical across packs and fresh compiles.
- Doc rules: `**Open:**` only in planned sections, exactly one each (C1); SECURITY.md rows must
  list the same tests as the rule's status (C7).

## What consumed context (avoid)

- Running the host shell to list loaded modules: ./shell rebuilds and `userland/otp/target/
  release/beamlet` is the Redoubt bin, not the host CLI (same bin name); the CLI is at
  userland/otp/target/cli/release/beamlet (built with CARGO_TARGET_DIR=target/cli). Not needed
  again: the closure is measured.
- Cargo output through in-dev is full of vendor/libm warnings: always grep for `^error` and our
  paths only.
- `cargo testbench host-tests` matches every *host-tests case, including model-host-tests
  (~7 min): name the case exactly.
- Whole-file diffs of testbench; read ranges instead.
