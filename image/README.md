# Image recipes

The sources of the signed boot bundle and the disk images; what they produce goes under
`target/image/`. The bundle is on [boot](../docs/kernel/boot.md) and [init](../docs/servers/init.md).

- `boot.toml`: the bundle's entries in order, the kernel, `init`, the servers (the steward and
  `sshd` among them), `beamlet`, `piped`, `consrelay`, `crossing` and the manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and
  the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
  block count into the manifest it packs, from its own pack of `userland.toml`.
- `manifest.json`: the boot manifest `init` reads, with thirteen servers, including `walfsd:data`
  for the disk's `data` volume, `walfsd:alice-secrets` for alice's labelled one, the userland
  disk's `blkd:system`, `verity:system` and `erofsd:system`, the steward and `sshd`. Its principals
  are alice (who owns `alice-secrets` and works under `{}` and `{alice-secrets}`), bob, and
  `agent`, the principal a sponsor-run agent logs in as, **sponsored by alice**: it has a home
  of its own, bob's quota, no labels and no network scope, and no budget of its own: its session
  is a lease of alice's, carved from her unlabelled share for `MAX_LEASE`, which her sessions
  can end and are told the end of ([agents](../docs/userland/agents.md);
  [the steward](../docs/servers/steward.md#leases)). All three have login test keys, and alice and bob
  approval keys (`approve@box`), from `tests/keys/`, so the manifest must never ship. The budgets, in pages: a session is 11,392 and
  its budget's own cost 1, and each label set's share is its principal's pages over its label
  sets, less 1. Alice's `{}` share must hold at once her console session, one SSH login, the
  agent's sponsored session and one lease of the steward's `agent` size (5,248 pages, 2
  processes: the agent VM's heap peak on the echo path, 1,993 pages on rv64 and 2,083 on rv32
  as `beamlet-agent-footprint` measures it once the agent has read its input and written its
  result, doubled, with the 44 pages it holds outside the heap and room for one sub-agent's
  budget, `sub_agent`'s 1,024 and its own page, rounded up to 128; and a `quota` of 1 MiB, the
  bytes of the sponsor's home a lease's `/project` root is carved with, of alice's 8 MiB
  `home_quota`, given back when the lease ends):
  three sessions are 34,179 and the lease 5,249, 39,428 in all, so alice's 81,920 give each of
  her two label sets 40,959, with 1,531 over in `{}` and three sessions' room in her vault's. Bob's 32,768 give his one set 32,767, two sessions
  with 9,981 over. Together the two top budgets are 114,688 pages, 114,690 with their own, of
  the about 192,700 pages `users` holds on the 1 GiB machine (the kernel's `Budgets:` line at
  boot: 192,723 at one rv64 boot, 192,576 and 192,600 at rv32 ones, the figure moving a little
  with the pack), a margin of about 78,000 pages. A session holds 10 processes, its VM, its
  console's relay, `piped` and up to 7 stages of a pipeline, and a lease 2, its VM and one
  sub-agent's process: alice's 64 give each label set 32, three sessions and a lease, and bob's
  20 two sessions. The cap of live contexts follows: alice three per set (the console's session
  and the agent's count among her `{}` ones), bob two; what one account may have open at
  `walfsd` at once is that cap, the console session, a lease and one spare for a context
  detaching while another attaches, 6 for alice, and the server's constant is the largest such
  sum over the tree's manifests, 6 ([walfsd](../docs/servers/walfsd.md#serving)); the two
  `walfsd` entries' 1,280 pages are that page's entry rule. The connections the steward mints
  for her sessions and leases are hers to hold by the same rule, at every server: at the system
  volume two per session and one per lease, 11 at most, which is `erofsd`'s cap per bucket
  ([erofsd](../docs/servers/erofsd.md)). The shell,
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
