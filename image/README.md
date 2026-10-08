# Image recipes

The sources of the signed boot bundle and the disk images; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers (the steward and
  `sshd` among them), `beamlet` and the manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and
  the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
  block count into the manifest it packs, from its own pack of `userland.toml`.
- `manifest.json`: the boot manifest `init` reads, with thirteen servers, including `walfsd:data`
  for the disk's `data` volume, `littlefsd:alice-secrets` for alice's labelled one, the userland
  disk's `blkd:system`, `verity:system` and `erofsd:system`, the steward and `sshd`. Its principals
  are alice (who owns `alice-secrets` and works under `{}` and `{alice-secrets}`) and bob, with
  their test keys from `tests/keys/`, so it must never ship. Alice's 44,040 pages give each of her
  label sets room for two sessions of 11,009 pages (a session's limit and its budget's cost): her
  console session and one SSH login under `{}`. The shell, `Redoubt.Shell`, is started by the
  steward as the console principal's session (`console: "alice"`) on the UART, and as each SSH
  login's session on port 22 ([steward](../docs/servers/steward.md#authentication-and-sessions)).
  `beamlet` is the one `public` entry, read from `/boot`. The userland volume is
  verified: its entry's `verity` names `verity:system`, and the root and block count in this file
  are placeholders the builder replaces with the pack's ([verityd](../docs/servers/verityd.md)),
  so the signed manifest pins the disk. The image needs 1 GiB of RAM (QEMU `-m 1G`): the
  principals' budgets, each holding sessions of twice what a VM holds at its largest peak, do not
  fit the `users` budget of a smaller machine ([budgets](../docs/kernel/budgets.md)).
- `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
  `data` partition as a walfs volume holding `target/image/stage/` (with the principals' homes,
  `home/alice` and `home/bob`, which `./mkimage` makes), written by `libs/walfs` itself
  ([walfsd](../docs/servers/walfsd.md#the-packer)), and the `alice-secrets` partition as a littlefs
  volume holding `target/image/vault/`, written through `littlefsd`'s own code; the `image-disk`
  case boots a disk packed the same way.
- `userland.toml`: the userland disk, attached read-only. `./mkimage` packs it with the same
  packer into `target/image/userland.img`: each module of the applications it names, compiled by
  the pinned toolchain and stripped, as a plain file under its own name (`Elixir.Enum.beam`,
  `elixir.app`), and the boot pack, `boot.pack`: the files the shell's prompt loads, which the
  recipe's `pack` list names, again in one file that beamlet reads whole at start
  ([beamlet on Redoubt](../docs/userland/beamlet.md#beamlet-on-redoubt)); all on one verified
  volume, an EROFS volume written by Redoubt's own writer ([erofsd](../docs/servers/erofsd.md))
  and followed by its hash tree. The packer prints the volume's root and data blocks, the ones the
  bundle's manifest pins.
  Two packs of the same inputs are byte-identical.
