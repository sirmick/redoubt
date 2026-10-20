# READ1: the Files cap derivation (phase 2, step 1)

Rule: `files = n + w × (n + 1)` per server, where

- `n` is the connections one account may hold at the server, every one of which attaches a root
  fid for its life: a fixed per-connection cost charged to the bucket total. It is the same `n`
  as the `state` cap's derivation (the tree's largest: erofsd `2 × sessions + leases`, the home
  and `/boot` servers `sessions + leases`), so roots never fill a bucket: `roots ≤ state < files`.
- `w` is the largest working need of one connection: fids walked, opened and clunked beside the
  root. The share is over working fids, floored at one.
- `w × (n + 1)` and not the sum over the mix (hubs × 3 + leases × 2): with a working pool of
  `w (n + 1)` divided by `holders + 1 = n + 1`, the hub's second working fid is admitted when
  every other connection holds one (held 1 < 2); the sum form's integer share would give the
  hub one and refuse its second, the data volume's `emfile` again.

| server | w | n (latency manifest) | files before → after | image's own mix | bucket bytes before → after |
| --- | --- | --- | --- | --- | --- |
| erofsd (system) | 1: the sync client uses one fid per open; the shell's hub is at the data volume | 28 = 2 × 5 + 18 | 32 → 57 | n = 17 → 35 | 235,520 → 286,720 |
| walfsd / littlefsd | 2: the hub's root + cwd + a working fid | 23 = 5 + 18 | 32 → 71 | n = 12 → 38 | 1,617,408 → 1,697,280 |
| bootfsd (/boot) | 1 | 23 | 32 → 47 | | 165,888 → 196,608 |
| consoled | its own derivation (`CONSOLE_FIDS × 3`), unchanged | | | | |

Entries by the entry rule (`every_manifest_entry_holds_its_buckets_at_their_caps`):

- erofsd: none moves (the image's six buckets 1,720,320 B = 420 pages of the entry's 870).
- bootfsd: none moves.
- walfsd 4 buckets with its heap cap of 64: 6,789,120 B = 1,658 + 64 + 128 = 1,850 → **2,048**
  (was 1,792): `beamlet-footprint`, `beamlet-compile-footprint`, `pipe`, `steward/idle`;
  littlefsd 4 buckets (no heap cap) 1,658 + 128 = 1,786 → 1,792 unchanged.
- walfsd 5 buckets 2,072 + 192 = 2,264 → 2,304 unchanged; 6 buckets 2,487 + 192 = 2,679 → 2,816
  unchanged (the image's `walfsd:data`); 9 buckets 3,730 + 192 = 3,922 → **4,096** (was 3,840,
  `six-principals`).

Supported N after: every connection holds its root and at least one working fid at both volumes,
the hub its two: the latency manifest's 16 leases beside two sessions (her pages' carve; 18 by
quota), from 13 at the system volume and 7 at the data volume; the image's 7 leases (pages) with
margin, from 7 at the data volume's edge.

Held by: the bench test asserting `files ≥ state + w (state + 1)` per server beside STEWARD7's
state/in_flight test, and an admit.rs test of root-out, the floor and the hub's second fid.

Question: at the system volume `w = 1` gives exactly one working fid per connection when all hold
one; a second concurrent open on one connection is refused until another closes (the client's
bounded retry rides it). `w = 2` there would cost 29 × 2 KiB more per bucket (erofsd 345,088 B;
the image's entry still holds its six buckets).
