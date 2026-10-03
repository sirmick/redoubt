# Image recipes

The sources of the signed boot bundle and the disk images; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers, `system.index` and
  the manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`,
  and the `init-boot` case boots the same bundle.
- `manifest.json`: the boot manifest `init` reads, with the six servers and `fsd:data` for the
  disk's `data` volume; `bootfsd` serves `system.index` at `/boot`.
- `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
  `data` partition as a littlefs volume holding `target/image/stage/`, written through `fsd`'s own
  code, and the `image-disk` case boots a disk packed the same way.
- `userland.toml`: the userland disk, attached read-only. `./mkimage` packs it with the same
  packer into `target/image/userland.img`: each module of the applications it names, compiled by
  the pinned toolchain and stripped, as one object named by the SHA-256 of its bytes, on one
  littlefs volume. The same step writes `target/image/system.index`, one line per object,
  `<file> <sha256 hex> <bytes>` (`Elixir.Enum.beam`, `elixir.app`), sorted, which `boot.toml`
  signs into the bundle. `--pack-disk` writes an index only for a recipe that declares one, so
  `disk.toml` packs as it always did.
  Two packs of the same inputs are byte-identical, the disk and the index.
