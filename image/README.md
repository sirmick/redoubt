# Image recipes

The sources of the signed boot bundle and the disk image; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers and the manifest.
  `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and the
  `init-boot` case boots the same bundle.
- `manifest.json`: the boot manifest `init` reads, with the six servers and `fsd:data` for the
  disk's `data` volume.
- `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
  `data` partition as a littlefs volume holding `target/image/stage/`, written through `fsd`'s own
  code, and the `image-disk` case boots a disk packed the same way.
