# What STEWARD4's case 3 needs from B79 (the bench's second writable disk)

From steward4-implementer, 2026-10-16, per the orchestrator's Q8 answer on `STEWARD4-crossing`.

Case 3, `steward-confined-push` (tests/steward-confined-push.toml, to be written once B79 lands):
a confined manifest with alice's unlabelled home on one disk and her vault `{alice-secrets}` on
another, since R34's device rule forbids one `blkd` holding both (`servers/init/src/confine.rs`,
the device check). The steward leaves the vault session's home unbound (`confined` own line,
built on `wp-STEWARD4`), and a push from alice's plain session lands in the vault.

## The recipe key

`[vault]` beside `[disk]` in a case's toml, the same fields as `[disk]` (`size_kib` +
`partitions`, or `recipe` + optional `stage`); packed afresh per boot like `[disk]`; no `keep`
needed. One partition is enough for case 3.

## The disk as the manifest sees it

- A fourth virtio device line in the case's manifest, `disk2`, on a free virtio-mmio bus; my
  preference is bus 4 (`0x10005000`, interrupt 5), below the userland disk's 5, so the image's
  three lines (`disk0` bus 7, `disk1` bus 5, `net0` bus 6) stay as they are and
  `devices_sit_on_fixed_slots` gains one row. Any free bus does; name it on testbench.md's
  device table and in `image/manifest.json`'s comment if the image lists it.
- The manifest then has `blkd` (disk0) with the `data` volume and `blkd:vault` (disk2) with the
  `alice-secrets` volume (`"disk": "blkd:vault"`), each walfsd on its own disk, as the confined
  profile requires; `init-refuses-confined-server`'s manifest is unaffected.

## The stage

`[vault] recipe = "tests/data/steward/vault-disk.toml"` (a one-partition walfs recipe I will add
with the case; B79 need not write it) with `stage` pointing at a directory holding the vault's
initial tree (empty is fine: the case makes `/vault/secret` itself). If `[vault]` takes a
`recipe`, it should accept a recipe whose `manifest` key names the case's manifest, as
`[disk]` does (`a_case_s_manifest_is_held_to_its_disk`), or no `manifest` key at all for a
volume whose `bytes` the manifest does not pin.

## What the case does with it

Boots the image's programs with the confined manifest (two disks, alice only, no net); alice's
vault session logs in, sees `** no home in this label set: a push is the way in; this session
starts in /` (the shell's line on `wp-STEWARD4`), makes `/vault/secret`; her plain session
pushes `~/in` to `secret/in` (`submit` kind 3, `what` = `in\nsecret/in`); `approve@box`
approves; the vault session reads it back. Nothing else of the bench is needed.
