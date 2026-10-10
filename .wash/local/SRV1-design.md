# SRV1 design: what the three file servers (and verityd) share

Read: servers/littlefsd, walfsd, erofsd (and verityd's arguments and range client), their pages,
docs/todo/file-server-arguments-and-range-client.md, libs/rt/src/server (admit.rs: `buckets`,
`own_args`). Branch wp-SRV1 off main 311b0b22b, worktree .worktrees/SRV1.

## What is shared today, by copy

| piece | littlefsd | walfsd | erofsd | verityd |
| --- | --- | --- | --- | --- |
| `parse_args` (`endpoint=`, `labels=`), `Args`, `BadArgs` | server.rs | identical | identical | own parser with more keys (`root=`, `version=`); same label and name rules |
| blkd range client (`Blkd`: `Lend`, `call`, the reply checks) | blkd.rs 109 | 6 lines differ | 87 (read-only subset, byte-offset read chunked at 64 sectors, 9-page lend) | 71 (read-only subset, 2-page lend) |
| `Range` trait, `Fault`, `Geometry`, `SECTOR` | volume.rs | same | server.rs (`sectors()`, `read(at)`) | lib.rs (`Size`) |
| `MAX_SECTORS` 64 | - | - | blkd.rs | server.rs (and servers/blkd/src/virtio.rs, the server's own) |
| quota ledger | quota.rs 224 | 223: `VOLUME` is `walfs::ROOT`, no `spare()` | - | - |
| one-volume probe (feature) | one_volume.rs 60 | 4 lines differ (the name) | - | - |

Per-format and staying: volume.rs's `Blocks` adapters (littlefs's and walfs's `BlockDevice`,
different traits), the mount rule and `MIN_BLOCKS`, server.rs, typed.rs, pack.rs, stats.rs,
walfsd's cut.rs. The servers' quota_tests (31 and 20 tests) drive the servers, not the ledger,
and stay.

## Where: a new crate `libs/fileserver` (`redoubt-fileserver`)

no_std, `#![forbid(unsafe_code)]`, depends on redoubt-rt only. Modules:
- `args`: `parse_args`, `Args`, `BadArgs`, and the two rules as functions (`label_set(list)`,
  the endpoint name via `redoubt_rt::startup::valid_name`) so verityd's parser uses them.
- `range`: `SECTOR`, `MAX_SECTORS`, `Fault`, `Geometry`, `trait Range { info, read, write,
  flush }` (sector-addressed), `Blkd::new(endpoint, lend_pages)` implementing it with `read`
  chunked at `MAX_SECTORS`, and `read_at(range, byte_offset, out)` for erofsd (and verityd if it
  wants it). The probe's `read_one` and `endpoint()` under the feature.
- `quota`: `Ledger::new(room, held, volume_id)`; `spare()` kept (littlefsd's).
- `probe` (feature `one-volume-probe`): `verdict(startup, name, range)`.
Why not rt: rt is every server's trusted base; a change there must be built against every bin
(the consumer sweep), and these four pieces are the file servers' alone. The todo page's
wording (rt::server beside `buckets=`) is the alternative; `buckets=` is the skeleton's own
argument, `endpoint=`/`labels=` are the volume servers' and verityd's.

## Tests

- The crate: unit tests of the ledger (ledger-level, derived from the ledger's own behaviour:
  mint at a live root, at a new root, over quota, disconnect returning bytes, holder/above) and
  of `parse_args` (littlefsd and walfsd have none today; erofsd 1, verityd 3: moved).
- The servers' host tests (tests/*.rs against the fake kernel with a fake blkd) keep exercising
  the client and the servers' quota behaviour unchanged.

## Size budget (code lines)

littlefsd ~-430 (blkd 109, one_volume 60, quota 224, parse_args+Args ~40), walfsd ~-430,
erofsd ~-135 (blkd 87, parse_args ~40, Fault/Range ~10), verityd ~-75 (blkd 71); the crate
~+480, one new entry; rt unchanged. Net about -590. Exact ceilings measured at the gate.

## Docs

- docs/todo/file-server-arguments-and-range-client.md deleted, its SUMMARY.md line with it.
- erofsd.md and walfsd.md residuals that link it go; servers/README.md's skeleton passage names
  the crate (the shared pieces beside the 9P skeleton); littlefsd.md, walfsd.md, erofsd.md and
  verityd.md module references follow the moves.
- The 64-sector read limit: wire tables carry no constants (the generator emits layouts only),
  so `MAX_SECTORS` is named once in the crate's `range`, and libs/wire/tables/blkd.md's prose
  cites it.

## Gates (as assigned)

The three servers' (and verityd's) host tests; every fsd/walfsd/erofsd/verity case and
userland-boot on both widths; the smoke set; docs, formatting, size-budget, unsafe-budget,
no-cruft.
