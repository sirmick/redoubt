# B79 — the bench attaches a second writable disk: `[vault]` beside `[disk]`

Worktree /home/mcloonan/redoubt/.worktrees/B79, branch wp-B79 on main 0b3fcfd28, head 63e6db1b0, tree clean. STEWARD4 brief absent at start and finish: built to the node body.
(head in the message to the orchestrator). STEWARD4's brief (.wash/local/B79-brief-from-steward4.md)
was absent at start and at finalisation: built to the node body.

## Delivered

- tools/testbench/src/case.rs: `Boot.vault: Option<Disk>` (the same keys as `[disk]`: size_kib,
  partitions, recipe, stage, flip_version, keep); `Boot::disks()` yields (device, spec) for
  `disk0`/`disk2`; the load checks run for both disks; `[vault]` needs `[disk]`; `keep` on either
  needs distinct_across_boots; with a vault, the manifest hold is per disk.
- tools/testbench/src/qemu.rs: `VAULT_BUS = virtio-mmio-bus.4` (0x10005000, irq 5: the slot below
  the userland disk's; it is the slot tests/data/init/driver.json names `slot5`, the EMPTY slot
  dma-driver is given in init-driver-restart and init-quarantine-reboot, so a case carries `[vault]`
  or that manifest, never both; buses 0-3 stay free); the disk-making match factored into
  `make_disk(spec, path)` and run for `[disk]` at `<disk>` and `[vault]` at `<disk>.vault.img`
  (kept beside it, like the userland image); the vault's `-drive id=disk2`/`-device bus=4` follow
  the data disk's, before the userland disk's. Without `[vault]` the arguments are unchanged
  (asserted byte-for-byte).
- tools/testbench/src/disk.rs: `hold(manifest, sizes, names, device)`: with `device` given only
  volumes whose `blkd` (named by the volume's `disk`, else the manifest's one blkd, as init's
  `check::blkd`) lists that device are held. Callers without a vault pass None (unchanged).
- tools/testbench/src/main.rs: a kept disk path is taken when either disk keeps.
- tests/littlefsd-programs/src/bin/littlefsd-client.rs: `boot` takes every endpoint named and
  runs its write/read/rename/remove on each; its line now ends `at <endpoint>`. littlefsd-boot's
  expect line updated accordingly. This is test-only client code; it was the only way to have one
  reporter prove both disks (a second client's TEST PASSED would fail the case; a labelled
  writer's exit codes have no fixed order against the reporter's lines).
- Proof case: walfsd-boot (the smallest manifest-bearing walfsd case) gains `[vault]`
  (2048 KiB, 1 partition); tests/data/walfsd/two-disks.json (its own manifest: walfsd-quota, -corrupt-volume and -reboot merge servers into the one-disk boot.json and have no vault; the first sweep caught that, blkd:vault exiting code 6 there): device disk2, volumes data (disk
  blkd:data) and vault (disk blkd:vault), servers blkd:data/disk0, blkd:vault/disk2,
  walfsd:data, walfsd:vault, client handed both and run `boot walfsd:data walfsd:vault`;
  expects 7 servers, both walfsd starts, both client lines, TEST PASSED. Console (rv64):
  `started blkd:vault`, `... renamed and removed at walfsd:vault`.
- docs/testbench.md "Disks and network cards": status 34→37 (+the_vault_disk_sits_beside_the_data_disk,
  +each_disk_holds_the_volumes_on_its_device, +bench:walfsd-boot); `[vault]` in the toml block;
  the per-disk hold paragraph; a `[vault]` paragraph (R34, walfsd-boot); the slot prose became a
  devices table (data disk, card, userland disk, vault disk with bus/base/irq/manifest line).

## Host tests

- devices_sit_on_fixed_slots: extended (vault on bus 4 with net+disk; tests/data/walfsd/boot.json
  two-disks.json names disk0 and disk2 lines; image/manifest.json unchanged, its three lines still checked).
- the_vault_disk_sits_beside_the_data_disk: both files made at their sizes, the vault's GPT, the
  four added arguments and their place, the without-vault arguments identical.
- each_disk_holds_the_volumes_on_its_device.

## Gates (all exit 0)

- `q run --cores 4 -- cargo test -p testbench`: 217 passed.
- jobs.mk set (both widths): walfsd-boot, littlefsd-boot, init-boot PASS; size-budget,
  unsafe-budget, no-cruft, formatting, host-tests PASS (rv64-only cases).
- jobs.mk docs: PASS.
- `cargo +nightly fmt --all --check`: clean after formatting.
- rv64 sweep of all 40 steward-*/walfsd-* cases: 37 PASS, 3 FAIL on the shared manifest (above), fixed by the split; those three rerun PASS, host tests 217 PASS, walfsd-boot both widths PASS, docs and fmt PASS again on the final tree.

## Summaries checked

README.md, GETTING-STARTED.md, docs/README.md, docs/servers/init.md, docs/servers/blkd.md,
docs/servers/walfsd.md (Serving: status line bench:walfsd-boot unchanged, still true),
docs/servers/littlefsd.md (bench:littlefsd-boot unchanged), docs/SECURITY.md R34 row (no new
R34 test: the bench's vault is tooling; row unchanged), docs/milestones (no directory): none
mention the bench's disk count or slots besides docs/testbench.md, which is updated. No
kernel, init or steward code moved; image/manifest.json and image/boot.toml unchanged.

## Residuals / notes for STEWARD4

- A confined manifest names each volume's `disk`; the vault's blkd lists `disk2`
  (`{"name": "disk2", "base": "268455936", "irq": 5, "dma": true}`).
- `launch --system` has no vault (the image's disk holds both volumes); `--keep-disk` locking
  covers `<disk>.vault.img` only by being beside the kept disk.
