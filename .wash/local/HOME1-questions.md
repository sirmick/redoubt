# HOME1: two design gaps before code

Worktree .worktrees/HOME1, branch wp-HOME1 off origin/main 44d970ab4; nothing written.

Where things are (verified):
- The steward mints a session's slots in `servers/steward/src/bin/steward.rs` `Machine::fresh`
  (line ~187): `server.new_connection(lend, root, 0)` on the volume server it was handed
  (unattached `Nine`), once per session per slot.
- The `home`/`vault` lines are parsed in `servers/steward/src/own.rs` (`Own::line`), not
  libs/steward manifest.rs (the core's lines; own.rs uses its `tokens`/`list`/`u64_of`), and
  written by `servers/init/src/check.rs` `steward_own_lines`.
- The manifest's `principals[].home` is a string `"VOLUME:/path"`; `volumes[]` have name,
  partition, labels, disk, verity: no size. init's check never sees a partition's size (blkd
  reads the GPT; disk.toml makes equal partitions of a 64 MiB disk, so ~32 MiB each, and walfs's
  room is the data region's blocks x 4096, less than the partition).
- walfsd's quota (libs/fileserver/src/quota.rs `Ledger::mint`): a quota is carved from the live
  root above; a mint that doesn't fit is refused. **A second mint at an already-live root adds
  its quota** (`sum = quota(i) + quota`): a root's quota is the sum of its connections'. A mint
  through a connection at its own root with quota 0 carves nothing and shares that root's quota.
- A dead steward's connections are disconnected by init at its exit, at every depth below
  (steward.md, "Failure and restart"), so connections the steward holds are restart-safe.

## Gap 1: per-session minting multiplies the quota

Minting `new_connection(home, Q)` per session gives /home/alice Q x (alice's live sessions): two
sessions, 16 MiB; with CTX contexts, more. "A quota per principal's home" then holds per
session, and init's sum-of-quotas check bounds nothing about concurrency; walfsd refuses the
mint once the volume's root is full, so a later login gets no home (Refused, session fails).

- (a) Recommended: the steward holds one home connection per principal, minted with Q at the
  principal's first session and kept for the steward's life (released by init at its exit), and
  gives each session `new_connection(held, "", 0)`: the same root, sharing Q. Same for each
  vault label set (Q per principal x label set). Quota is per principal whatever its sessions.
- (b) Per session, as the spec's wording reads: each session carves Q; document that a
  principal's home holds Q per live session, and a login is refused when the volume can't carve.

## Gap 2: init cannot see a volume's room

- (a) Recommended: a `bytes` field on each `volumes[]` entry a quota lands on (walfs/littlefs,
  not the verified system volume); init refuses when the quotas carved on it (homes on that
  volume; vault quotas of the label set whose volume it is) sum past it. The image gives data and
  alice-secrets their partition size; the disk packer (or a bench check) holds disk.toml's
  partitions to it, so the declaration cannot lie upward unnoticed. walfsd's mint refusal stays
  the real bound (data region < partition).
- (b) No size in the manifest: init checks only that each quota is nonzero and fits u64; the
  over-commit refusal is walfsd's at mint time, and the "init refuses over-commit" case becomes
  "the steward's session gets no home" instead.

Manifest shape I'd use either way: `home: {"at": "data:/home/alice", "quota": "8388608"}` (the
probe's) or keep `home` a string and add `home_quota`; and `label_sets[].quota`. The probe's
object form changes every manifest copy's `home` (tests/data has many); a sibling field changes
fewer. Which do you want?
