# SRV1 report: what the volume servers share, in libs/fileserver

Branch wp-SRV1, worktree /home/mcloonan/redoubt/.worktrees/SRV1, head cea5bf2ec on main 311b0b22b,
clean, never pushed. Four commits, one per shared piece, each moving every server at once:

| commit | piece | size budget (code lines) |
| --- | --- | --- |
| 07cbfaacf | args: `parse_args`/`Args`/`BadArgs`, `label_set`, `number`; littlefsd, walfsd, erofsd, verityd | fileserver new 49; erofsd 515→479, littlefsd 1513→1477, walfsd 1155→1119, verityd 604→590 |
| 8d245cfbd | range: `Range`/`Fault`/`Geometry`/`Blkd`, `read_at`; `SECTOR`/`MAX_SECTORS` in libs/wire's blkd module; the four servers' blkd.rs deleted | fileserver 199, wire 3675→3678, erofsd 410, littlefsd 1376, walfsd 1018, verityd 523 |
| 1a2ecc06a | quota: `Ledger::new(room, held, volume)`; both quota.rs deleted | fileserver 345, littlefsd 1235, walfsd 881 |
| cea5bf2ec | probe (feature one-volume-probe); both one_volume.rs deleted; todo page deleted | fileserver 390, littlefsd 1190, walfsd 836 |

Net: −435 code lines (servers −828, wire +3, fileserver +390). Each raise carries a
`Size budget:` line. Commit 1 also adds libs/fileserver's unsafe budget (0) and the new crate to
the two other lockfiles naming littlefsd (userland/otp/Cargo.lock, libs/littlefs/diff/Cargo.lock):
the first gate run found both missing; they were folded into commit 1, not left as fix-ups.

## Tests

Host tests, `q run --cores 6 -- cargo test -p redoubt-fileserver -p redoubt-littlefsd -p redoubt-walfsd
-p redoubt-erofsd -p redoubt-verityd -p redoubt-blkd -p redoubt-wire`: exit 0, 0 warnings. Also with
`--features redoubt-littlefsd/one-volume-probe,redoubt-walfsd/one-volume-probe`: exit 0.

| crate | main 311b0b22b | head |
| --- | --- | --- |
| littlefsd | lib 53 + tests 8 | 53 + 8 |
| walfsd | lib 30 + tests 4 | 30 + 4 |
| verityd | lib 15 | 15 |
| erofsd | lib 13 + tests 4 | 12 + 4 (its parser test moved to the crate) |
| fileserver (new) | — | 9: 2 args, 1 range, 6 quota |

The servers' quota_tests.rs (littlefsd 31, walfsd 20 tests) are unchanged, re-homed from
`mod tests` in the deleted quota.rs to `mod quota_tests` at the end of each server.rs. The ledger
tests are new and ledger-level: a mint at a new root, past the room above, at the granter's own
root (with a non-zero volume id), two connections at one root and the last disconnect, a root
left over its quota by a disconnect, and `under`.

## Gates (head cea5bf2ec, tree clean)

- `make -f scripts/jobs.mk build-rv64 build-rv32`: 0; `prebuilt`: 0.
- Every case `ls tests | grep -E '^(littlefsd|walfsd|erofs|verity|fsd)'` (35; the three `*-programs`
  are directories, not cases), fileserver-host-tests, userland-boot and the smoke set (init-boot,
  bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page at smp 1 and 4), rv64/ and rv32/
  each: 78 targets, all exit 0, 73 PASS lines, none FAIL or SKIP (7 host-tests cases have no rv32
  target). No rerun needed. Logs: /home/mcloonan/redoubt/.tmp/SRV1/case-*.log, gate.log.
- `q run -- cargo testbench --exact <gate>`: formatting 0, docs 0, size-budget 0, unsafe-budget 0,
  no-cruft 0.

## Docs and summaries checked

- Changed: docs/servers/serving.md ("Beside the skeleton" paragraph names the crate's pieces and
  their tests; residual "Admission counts objects" now names walfsd's quotas too);
  littlefsd.md (Quotas: littlefsd and walfsd meter bytes with one ledger, the crate's);
  walfsd.md (Quotas: the one shared ledger; residual on copies removed); erofsd.md (status test
  name moved, client residual gone); blkd.md (where the limits live); SECURITY.md R48 row
  (enforced in libs/fileserver/src/quota.rs); docs/servers/README.md (Naming: the crate);
  docs/SUMMARY.md and docs/todo/file-server-arguments-and-range-client.md (deleted: every
  "Done when" item is met, with the crate the design chose instead of redoubt_rt::server; deleted
  rather than reworded).
- Checked, no change: README.md, GETTING-STARTED.md, docs/plan/m1-separation.md (describes the
  servers' behaviour, unchanged), docs/testbench.md, verityd.md (its argument rules unchanged; no
  module paths named), SECURITY.md R47 row (bins still enforce; the probe is test-only).

## Open risks

- No design problems found. The probe needed no name parameter: the name it says is the
  endpoint name the bin passes, as before.
- `Ledger::roots()` is now unconditionally public (servers' cfg(test) audits use it); it panics
  without memory, documented "serving never calls it".

Next: review.

## Review round 1 (steward red, simplifier), head 0bc07b8dc on main 3fef5d657

Five commits now: aba9f74ca args, 4521bfd03 range, 6eb96afc9 quota, 6780d3f9d probe, 0bc07b8dc
program (new: the bins' shared start and exit codes need args, range and probe, so it follows
them rather than folding into one). Fixups were folded with autosquash, never left.

- Red P2: `Blkd::read` refuses a length that is not whole sectors as Fault, then is `read_at` of
  whole sectors (simplifier 6). Host test libs/fileserver/tests/blkd.rs,
  `reads_split_at_the_lend_and_part_of_a_sector_is_a_fault` (fake blkd on the fake kernel: a
  20-sector read is 2 calls at 2 pages; 1, 511, 513 and 1124 bytes are Fault and ask nothing;
  read_at across a boundary is 1 call; overflow and past-the-range are Fault).
- Red note: beamlet.md's EROFS figures now say they counted calls to blkd in whole sectors, and
  that boot-stats now counts reads asked of the range with the bytes asked.
- 1 taken: range::Memory (feature test-support; littlefsd's packer takes it through a host-only
  target dependency, the tests as dev-dependency) replaces littlefsd's, walfsd's, erofsd's and
  pack.rs's. Cost: the size budget counts a feature-gated module, so libs/fileserver gains about
  110 lines while only pack.rs's ~30 left a counted server file (the test copies were uncounted).
- 2, 3, 4 taken: program::{BAD_ARGS, NO_VOLUME, NO_RANDOM, start}; start(startup, pages, fits)
  reads buckets/args/endpoint, checks fits(buckets), opens the range and, under
  one-volume-probe, runs the probe; verityd re-exports BAD_ARGS and NO_VOLUME; receive_endpoint
  gone.
- 5 rejected: redoubt_client::typed::call would save Blkd::call's ~25 lines but makes every
  volume server link redoubt-client (budget 1088 lines, and it depends on the loader stub
  crate), and the "no handles in the reply" refusal would still need code of its own.
- 7 taken: quota() returns room for the volume root first.

Size budget per commit (wire, fileserver, erofsd, littlefsd, walfsd, verityd):
args 3675 49 479 1477 1119 590; range 3678 309 410 1346 1018 523; quota 3678 458 410 1205 881
523; probe 3678 503 410 1160 836 523; program 3678 537 401 1142 821 522. From main: servers
−1012, wire +3, fileserver +537, net −472.

Gates on 0bc07b8dc: host tests (7 crates) exit 0, 0 warnings, counts littlefsd 53+8, walfsd 30+4,
erofsd 12+4, verityd 15, fileserver 9+1; also with the probe and boot-stats features, 0.
formatting, docs, size-budget, unsafe-budget, no-cruft 0. build-rv64/rv32 0, prebuilt 0; the
same 78 case targets as before, all exit 0, 73 PASS, none FAIL or SKIP, no reruns.
