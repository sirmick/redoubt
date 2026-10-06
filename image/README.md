# Image recipes

The sources of the signed boot bundle and the disk images; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers, `beamlet` and the
  manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and
  the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
  block count into the manifest it packs, from its own pack of `userland.toml`.
- `manifest.json`: the boot manifest `init` reads, with eleven servers, including `littlefsd:data` for
  the disk's `data` volume, the userland disk's `blkd:system`, `verity:system` and `littlefsd:system`,
  and `beamlet` running the shell, `Redoubt.Shell`, on the UART console. The userland volume is
  verified: its entry's `verity` names `verity:system`, and the root and block count in this file
  are placeholders the builder replaces with the pack's ([verityd](../docs/servers/verityd.md)),
  so the signed manifest pins the disk. The image needs 512 MiB of RAM (QEMU `-m 512M`): the
  shell's budget, twice what its VM holds at its largest peak, does not fit the `system` budget of
  a smaller machine ([budgets](../docs/kernel/budgets.md)).
- `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
  `data` partition as a littlefs volume holding `target/image/stage/`, written through `littlefsd`'s own
  code, and the `image-disk` case boots a disk packed the same way.
- `userland.toml`: the userland disk, attached read-only. `./mkimage` packs it with the same
  packer into `target/image/userland.img`: each module of the applications it names, compiled by
  the pinned toolchain and stripped, as a plain file under its own name (`Elixir.Enum.beam`,
  `elixir.app`), on one verified volume, a littlefs volume followed by its hash tree. The pack
  prints the volume's root and data blocks, the ones the bundle's manifest pins.
  Two packs of the same inputs are byte-identical.
