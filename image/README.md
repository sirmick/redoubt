# Image recipes

The sources of the signed boot bundle and the disk images; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers (the steward and
  `sshd` among them), `beamlet`, `piped` and the manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and
  the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
  block count into the manifest it packs, from its own pack of `userland.toml`.
- `manifest.json`: the boot manifest `init` reads, with thirteen servers, including `walfsd:data`
  for the disk's `data` volume, `walfsd:alice-secrets` for alice's labelled one, the userland
  disk's `blkd:system`, `verity:system` and `erofsd:system`, the steward and `sshd`. Its principals
  are alice (who owns `alice-secrets` and works under `{}` and `{alice-secrets}`), bob, and
  `agent`, alice's agent in name only for now: the principal a sponsor-run agent logs in as,
  with a home of its own and bob's quota, no labels and no network scope, which leases convert
  when the steward has them ([agents](../docs/userland/agents.md)). All three have test keys from
  `tests/keys/`, so the manifest must never ship. The budgets, in pages: a session is 11,392 and
  its budget's own cost 1, so two sessions are 22,786, and each label set's share is its
  principal's pages over its label sets, less 1. Alice's 47,624 give each of her two label sets
  23,811: two sessions, her console session and one SSH login under `{}`, with 1,025 over. Bob's
  32,768 give his one set 32,767, two sessions with 9,981 over, and agent's 24,576 give its one
  set 24,575, two sessions (one login and a second context) with 1,789 over. Together the three
  top budgets are 104,968 pages, 104,971 with their own, of the about 192,700 pages `users`
  holds on the 1 GiB machine (the kernel's `Budgets:` line at boot: 192,723 at one rv64 boot,
  192,576 and 192,600 at rv32 ones, the figure moving a little with the pack), a margin of
  about 88,000 pages, so alice's and bob's did not have to shrink for agent's. A session holds
  10 processes, its VM, its console's relay, `piped` and up to 7 stages of a pipeline, and
  alice's 40, bob's 20 and agent's 20 give each label set two sessions of them. The shell,
  `Redoubt.Shell`, is started by the
  steward as the console principal's session (`console: "alice"`) on the UART, and as each SSH
  login's session on port 22 ([steward](../docs/servers/steward.md#authentication-and-sessions)).
  `beamlet` and `piped`, the server a session starts for its pipes
  ([piped](../docs/servers/piped.md)), are the `public` entries, read from `/boot`. The userland volume is
  verified: its entry's `verity` names `verity:system`, and the root and block count in this file
  are placeholders the builder replaces with the pack's ([verityd](../docs/servers/verityd.md)),
  so the signed manifest pins the disk. The image needs 1 GiB of RAM (QEMU `-m 1G`): the
  principals' budgets, each holding sessions of twice what a VM holds at its largest peak, do not
  fit the `users` budget of a smaller machine ([budgets](../docs/kernel/budgets.md)).
- `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
  `data` partition as a walfs volume holding `target/image/stage/` (with the principals' homes,
  `home/alice`, `home/bob` and `home/agent`, which `./mkimage` makes), written by `libs/walfs` itself
  ([walfsd](../docs/servers/walfsd.md#the-packer)), and the `alice-secrets` partition, a walfs
  volume too, holding `target/image/vault/`; the `image-disk` case boots a disk packed the same way.
  The image holds no littlefs volume; `littlefsd` stays in the bundle for a flash medium and the
  cases that ask for one.
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
