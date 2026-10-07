# WFS2 report: walfsd serves the SSD's writable volumes

## Branches and commits
wp-WFS2 (worktree .worktrees/WFS2), commits 1-4, on main 29238720c (main has since moved to
425a0d71c, scripts only):
1. 1340f9cff walfs: a volume's data blocks and inode count (two read-only accessors and their test, a_volume_reports_its_data_blocks_and_inode_count, listed on walfsd.md's format status; no format change)
2. 0ebd86fc7 walfsd: one walfs volume over 9P, its quotas in bytes, its sync blkd's flush
3. b7e1ff12c image: the data volume is walfs, served by walfsd:data (incl. init, BEAM3's beamlet-files rename)
4. 97cb5ddb3 walfsd: its cases on the machine, and its page's serving and rules built

wfs2-on-steward2 (local branch, same worktree), on wp-STEWARD2's tip 62d60cb5d: commits 1-4
cherry-picked (d69e3da75, 171304b36, be553fe92, 884346685; conflicts resolved for STEWARD2's
image, see below), then commit 5 609b2c864 "image: alice's labelled volume is walfs too, and the
steward's lines name walfsd". Commit 4 there carries tests/data/walfsd/confined.json in STEWARD2's
label_sets schema, and commit 3's Size budget line reads servers/init 2371 to 2372. When STEWARD2
merges, rebase these five onto main and make them wp-WFS2.

## Decisions (each stated on walfsd.md)
- Quota (R48), in bytes, littlefsd's ledger copied (sharing would link littlefs into walfsd).
  Room R = data blocks x 4096. Each entry holds a share S = ceil(R / (inode_count - 2)) plus 4096
  per data block its size reaches (holes included) and per indirect block; a directory holds its
  own blocks too. N.S <= R keeps inodes, and blocks <= R/4096 keeps blocks: no quota promises
  what the volume lacks (host test the_volume_never_runs_out_while_every_root_is_within_its_quota).
  S is about 64 KiB at the packer's density, so walfsd-quota's roots are 256 KiB, not 64 KiB.
  Needs walfs's data_blocks()/inode_count() (commit 1, my option (a); no answer received).
- Sync: blkd already has `flush` (opcode 4; BLK_FLUSH required at negotiation). walfsd's
  BlockDevice::sync is it, four a transaction (every_transaction_is_four_flushes). Nothing added
  to blkd. QEMU's drive uses the default writeback cache: a guest flush is a host fdatasync.
- Corrupt: a hash failure is `corrupt` for the request that read it (the page's "What is
  corrupt"; walfsd-flipped-block: the other file reads, the volume writes). A volume that does not
  mount is served as corrupt. An I/O error poisons until restart. (The brief's point 1 said every
  read fails; its walfsd-flipped-block says other files read: the page rules, so the latter.)
- Node: path + inode + generation; qid path = inode, version = generation's low 32 bits (a write
  does not move it: residual on the page).
- Typed ops: littlefsd's table (rename, copy_file, set_attr, get_attr). No remove_attr or time
  operation exists on the wire; mtime is walfs's, 0 until a clock reaches walfsd. Attr types 0-15
  refused as on littlefsd; values <= 254 bytes (too_large), area overflow no_space.
- Mirrored cases run littlefsd-client (a client of either server); walfsd-client covers quota,
  cut and flipped.
- Reads per data block, sequential, host-measured (a_sequential_read_costs_...): 10.8 in 4 KiB
  reads, 2.9 in 32 KiB reads. About 9 block reads a request (path lookup and open, with hashes),
  and ~1.8 a data block (its hash block alternates with the inode table's in walfs's one-block
  hash cache). No cache added.
- Packer: `damage = { what = "flip", path }` for a walfs partition (tools/testbench/src/disk.rs),
  outside the brief's owned paths, as erofs's damage is; tested in the walfs packer test.

## Memory (walfsd:data, then walfsd:alice-secrets in commit 5: one server's declarations)
Image boots never write the data volume: init-boot, userland-boot, beamlet-footprint rv64 peaks
stack 16,040 bytes, heap 8-9 pages. Write path: six memory-scanned runs of walfsd-quota (memory =
true added for the runs, not committed), rv64 x3: stack 28,616, heap 15; rv32 x3: stack 27,320,
heap 15. Declared stack_pages 14 (2 x 28,616 rounded up), heap_pages 64 (> 2 x 15, and above a
32-block transaction, 32 pages, over that peak). On the STEWARD2 base with sessions: walfsd:data
heap <= 11 of 64, stack 16,040 of 14 pages; walfsd:alice-secrets heap <= 9, stack 16,040.
testbench.md's table rows: `walfsd:data` | 28,616 | 14 | 15 | 64 (and alice-secrets in commit 5).

## Gates
wp-WFS2 head 97cb5ddb3 (all via q / jobs.mk):
- cargo test -p redoubt-walfsd: 0 (30 lib + 8 program tests); -p redoubt-init: 0 (54 manifest);
  -p testbench: 0 (142); -p walfs --release: 0.
- size-budget 0, unsafe-budget 0 (walfsd 0 unsafe), no-cruft 0, formatting 0, docs 0,
  build-rv64 0, build-rv32 0.
- prebuilt 0, then 48/48 case runs rc 0 on both widths: walfsd-{boot, one-volume, quota,
  corrupt-volume, flipped-block, reboot, restart, power-loss, label-check, confined-labelled},
  init-boot, image-disk, userland-boot, userland-read-only, beamlet-footprint, beamlet-files,
  boot-profile, boot-profile-unverified, userland-bad-start, smoke (init-boot, userland-boot,
  ipc-outcomes, bench-net-peer), littlefsd-boot, littlefsd-quota, walfs-host-tests.
- walfsd-power-loss --sweep 1..10: rv64 10/10, rv32 10/10 (cuts at block writes 1-18; found
  before, written and renamed each seen).
- Not run: init-refuses-* on wp-WFS2 (they ran on the STEWARD2 base: all pass), the whole bench
  (the train's).
wfs2-on-steward2 ((folded into 884346685)): init + testbench host tests 0; size, unsafe, no-cruft, formatting,
docs 0; 80 case runs (all steward-*, all init-refuses-*, the walfsd cases, the image cases,
smoke) on both widths: 72 pass at once, walfsd-confined-labelled x2 passed after the fixup. Six
fail: rv32 userland-boot, rv32 userland-read-only, rv64+rv32 boot-profile, rv32
boot-profile-unverified (`sshd exited, code 3`: NOT_STARTED, keyd/ipd), rv32
steward-sub-budget-flood (a 10 MB allocation fails in the vault session). They fail identically
alone (q --quiet), and identically on wp-STEWARD2's own tip 62d60cb5d with WFS2 absent: STEWARD2's,
not WFS2's.

## Summaries checked
Updated: docs/servers/walfsd.md (Purpose, Serving, Quotas, Authority, R47-R50 as parts "on walfs
volumes", Failure and restart, Residual risks), SECURITY.md (R47-R50 rows), servers/README.md
(graphs, tiers, holdings, naming, residual), init.md (manifest volume sentence, name example,
weights, start order, boot figure), littlefsd.md (Purpose), image/README.md, testbench.md (memory
table), userland/files.md (intro, figure, residual), plan/m1-separation.md (built list, not
built), and in commit 5 steward.md, sessions.md, init.md's worked configuration, m1's R2 row.
Checked, no change: README.md, GETTING-STARTED.md, SUMMARY.md (walfsd already listed),
blkd.md (flush already documented), erofsd.md, testbench.md line 671 (littlefsd-reboot's own
example), userland/beamlet.md and userland/otp (bind examples name a handle, not the image),
libs/rt, libs/wire examples (valid names), steward's own tests (handle strings, not formats).

## Open risks
- The quota's share makes small files dear (about 64 KiB each at the packer's density); the
  quota's granularity is on the page as a residual.
- walfsd's per-request path lookup costs about nine block reads; no cache (measure-first).
- qid version = generation: a client caching by qid misses writes (residual).

## Page lines
The exact text is the diffs: `git diff main wp-WFS2 -- docs image/README.md` below, and commit 5's
`git show 609b2c864 -- docs image/README.md` after it.

### Commits 1-4
```diff
diff --git a/docs/SECURITY.md b/docs/SECURITY.md
index 5201bc580..234fb34bd 100644
--- a/docs/SECURITY.md
+++ b/docs/SECURITY.md
@@ -153,10 +153,10 @@ lines planned. Side channels cross every box and are listed below.*
 | A badge signs with one key, for one purpose, over a digest `keyd` computes itself | [R44 (one key, one purpose, keyd's own digest)](servers/keyd.md#r44-one-key-one-purpose-keyds-own-digest) | `servers/keyd/src/keys.rs`, `servers/keyd/src/ssh.rs` | bench:r4-host-tests, host:redoubt-keyd::a_badge_signs_only_its_own_key_and_only_its_own_purpose, host:redoubt-keyd::a_relayed_ssh_user_auth_blob_is_never_what_gets_signed, host:redoubt-keyd::parts_cannot_be_slid_into_each_other, host:redoubt-keyd::the_hash_matches_an_independent_implementation, host:redoubt-keyd::a_granted_capability_names_the_same_key_and_dies_with_release | built | `keyd` sees the SSH shared secret; an `ssh_host` badge speaks as the box ([more](servers/keyd.md#residual-risks)) |
 | Signing takes the same time whatever the key | [R45 (constant-time signing)](servers/keyd.md#r45-constant-time-signing) | `servers/keyd/src/sha256.rs`, `servers/keyd/src/keys.rs`, the Ed25519 crate keyd links, at the version pinned in `Cargo.lock` | — | built, partly tested | The timing tests are ignored in an ordinary test run ([more](servers/keyd.md#residual-risks)) |
 | `/boot` shows exactly the public list's entries, fixed at the seal | [R46 (only the public list)](servers/bootfsd.md#r46-only-the-public-list) | `servers/bootfsd/src/server.rs` | host:redoubt-bootfsd::a_session_reads_the_public_entries_and_sees_nothing_else, host:redoubt-bootfsd::a_walk_to_an_unpublished_name_is_the_same_as_to_one_that_never_existed, host:redoubt-bootfsd::setup_is_refused_after_seal_and_from_every_minted_connection, host:redoubt-bootfsd::nothing_is_visible_before_seal, host:redoubt-bootfsd::every_way_of_writing_is_refused, host:redoubt-bootfsd::a_client_cannot_publish_into_boot | built | Whatever is public is public to everyone ([more](servers/bootfsd.md#residual-risks)) |
-| Each `littlefsd` or `erofsd` instance serves one volume and holds only its block range | [R47 (one volume per instance)](servers/littlefsd.md#r47-one-volume-per-instance) | `servers/init/src/check.rs`, `servers/init/src/bin/init.rs`, `servers/littlefsd/src/bin/littlefsd.rs`, `servers/erofsd/src/bin/erofsd.rs` | bench:littlefsd-one-volume, host:redoubt-init::an_erofsd_entry_is_a_volume_server_as_a_littlefsd_one_is, host:redoubt-init::no_server_is_handed_a_badge_at_blkd | built | A shared `littlefsd` is shared state ([more](servers/littlefsd.md#residual-risks)) |
-| Every connection's root has a byte quota carved from its granter's | [R48 (a quota per attach root)](servers/littlefsd.md#r48-a-quota-per-attach-root) | `servers/littlefsd/src/quota.rs`, `servers/littlefsd/src/server.rs` | bench:littlefsd-quota, host:redoubt-littlefsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes, host:redoubt-littlefsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove | built | A refused rename or remove says a live root is there ([more](servers/littlefsd.md#residual-risks)) |
-| Whatever bytes the medium holds, littlefs and erofs refuse them as corrupt rather than crashing | [R49 (a hostile medium is corrupt, not a crash)](servers/littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash) | `libs/littlefs/src/mdir.rs`, `libs/littlefs/src/fs.rs`, `libs/erofs/src/read.rs`, `servers/erofsd/src/server.rs` | bench:erofs-corrupt, fuzz:erofs/image, fuzz:littlefs/image, fuzz:littlefs/mutate, host:littlefs::corrupted_bytes_never_panic, host:littlefs::noise_never_panics, host:littlefs::tail_list_cycle_is_refused, host:littlefs::skip_list_pointing_at_itself_terminates, host:littlefs::forged_file_sizes_do_not_amplify_allocation, host:littlefs::stale_handle_after_pair_drop_does_not_erase_another_files_data | built | Data is not checksummed ([more](servers/littlefsd.md#residual-risks)) |
-| A power cut at any block write leaves every metadata change done or not done | [R50 (power loss leaves before or after)](servers/littlefsd.md#r50-power-loss-leaves-before-or-after) | `libs/littlefs/src/fs.rs`, `libs/littlefs/src/mdir.rs` | host:littlefs::crash_at_every_write_small_blocks, host:littlefs::crash_at_every_write_random_workloads, host:littlefs::crash_at_every_write_torn_erases, host:littlefs::crash_during_repair | built | Attributes and data are two commits ([more](servers/littlefsd.md#residual-risks)) |
+| Each `walfsd`, `littlefsd` or `erofsd` instance serves one volume and holds only its block range | [R47 (one volume per instance)](servers/littlefsd.md#r47-one-volume-per-instance) | `servers/init/src/check.rs`, `servers/init/src/bin/init.rs`, `servers/littlefsd/src/bin/littlefsd.rs`, `servers/erofsd/src/bin/erofsd.rs`, `servers/walfsd/src/bin/walfsd.rs` | bench:littlefsd-one-volume, bench:walfsd-one-volume, host:redoubt-init::a_walfsd_entry_is_a_volume_server_as_a_littlefsd_one_is, host:redoubt-init::an_erofsd_entry_is_a_volume_server_as_a_littlefsd_one_is, host:redoubt-init::no_server_is_handed_a_badge_at_blkd | built | A shared `littlefsd` is shared state ([more](servers/littlefsd.md#residual-risks)) |
+| Every connection's root has a byte quota carved from its granter's | [R48 (a quota per attach root)](servers/littlefsd.md#r48-a-quota-per-attach-root) | `servers/littlefsd/src/quota.rs`, `servers/littlefsd/src/server.rs`, `servers/walfsd/src/quota.rs`, `servers/walfsd/src/server.rs` | bench:littlefsd-quota, bench:walfsd-quota, host:redoubt-littlefsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove, host:redoubt-littlefsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes, host:redoubt-walfsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove, host:redoubt-walfsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes | built | A refused rename or remove says a live root is there ([more](servers/littlefsd.md#residual-risks)) |
+| Whatever bytes the medium holds, littlefs, erofs and walfs refuse them as corrupt rather than crashing | [R49 (a hostile medium is corrupt, not a crash)](servers/littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash) | `libs/littlefs/src/mdir.rs`, `libs/littlefs/src/fs.rs`, `libs/erofs/src/read.rs`, `servers/erofsd/src/server.rs`, `libs/walfs/src/layout.rs`, `libs/walfs/src/fs.rs`, `servers/walfsd/src/server.rs` | bench:erofs-corrupt, bench:walfsd-corrupt-volume, bench:walfsd-flipped-block, fuzz:erofs/image, fuzz:littlefs/image, fuzz:littlefs/mutate, fuzz:walfs/image, fuzz:walfs/mutate, host:littlefs::corrupted_bytes_never_panic, host:littlefs::forged_file_sizes_do_not_amplify_allocation, host:littlefs::noise_never_panics, host:littlefs::skip_list_pointing_at_itself_terminates, host:littlefs::stale_handle_after_pair_drop_does_not_erase_another_files_data, host:littlefs::tail_list_cycle_is_refused, host:redoubt-walfsd::a_flipped_bit_in_a_file_is_corrupt_for_that_file_alone, host:redoubt-walfsd::noise_is_never_formatted_and_never_mounted, host:walfs::every_flipped_bit_is_corrupt_where_it_is_read, host:walfs::noise_never_panics | built | Data is not checksummed ([more](servers/littlefsd.md#residual-risks)) |
+| A power cut at any block write leaves every metadata change done or not done, and on walfs every operation | [R50 (power loss leaves before or after)](servers/littlefsd.md#r50-power-loss-leaves-before-or-after) | `libs/littlefs/src/fs.rs`, `libs/littlefs/src/mdir.rs`, `libs/walfs/src/fs.rs`, `servers/walfsd/src/volume.rs` | bench:walfsd-power-loss, host:littlefs::crash_at_every_write_random_workloads, host:littlefs::crash_at_every_write_small_blocks, host:littlefs::crash_at_every_write_torn_erases, host:littlefs::crash_during_repair, host:redoubt-walfsd::a_cut_at_every_write_of_a_write_and_a_rename_leaves_before_or_after, host:walfs::crash_at_every_write_fixed_workload, host:walfs::crash_at_every_write_random_workloads, host:walfs::crash_during_recovery | built | Attributes and data are two commits ([more](servers/littlefsd.md#residual-risks)) |
 | `blkd` points the device only at its own DMA region | [R51 (DMA stays in its region)](servers/blkd.md#r51-dma-stays-in-its-region) | `servers/blkd/src/queue.rs`, `servers/blkd/src/virtio.rs` | host:redoubt-blkd::a_broken_device_is_never_handed_a_clients_write_payload, host:redoubt-blkd::a_hundred_thousand_random_liars_never_panic_and_never_stray, host:redoubt-blkd::impossible_requests_never_reach_the_device | built | `blkd` is trusted without an IOMMU ([more](servers/blkd.md#residual-risks)) |
 | A lying disk makes a request fail, never corrupts `blkd` or mixes clients' bytes | [R52 (a lie is a failure, never corruption)](servers/blkd.md#r52-a-lie-is-a-failure-never-corruption) | `servers/blkd/src/transport.rs`, `servers/blkd/src/queue.rs` | fuzz:redoubt-blkd/device, host:redoubt-blkd::rewriting_the_rings_changes_nothing_the_driver_believes, host:redoubt-blkd::a_device_that_lies_about_a_completion_is_refused_and_never_spoken_to_again, host:redoubt-blkd::a_lying_device_becomes_failed_and_stays_failed, host:redoubt-blkd::a_hundred_thousand_random_liars_never_panic_and_never_stray, host:redoubt-blkd::one_clients_read_never_carries_anothers_bytes | built | Wrong bytes are not detected ([more](servers/blkd.md#residual-risks)) |
 | A client's badge names one partition, and overlapping partition tables are refused | [R53 (a filesystem sees only its partition)](servers/blkd.md#r53-a-filesystem-sees-only-its-partition) | `servers/blkd/src/gpt.rs`, `servers/blkd/src/range.rs` | fuzz:redoubt-blkd/gpt, host:redoubt-blkd::a_range_cannot_name_a_sector_outside_itself, host:redoubt-blkd::hostile_partition_entries_are_refused, host:redoubt-blkd::a_badge_that_names_no_range_is_refused, host:redoubt-blkd::a_gap_in_the_table_does_not_renumber_the_volumes_after_it | built | `blkd` is trusted without an IOMMU ([more](servers/blkd.md#residual-risks)) |
diff --git a/docs/plan/m1-separation.md b/docs/plan/m1-separation.md
index 63d1c5d82..e7ad67a1f 100644
--- a/docs/plan/m1-separation.md
+++ b/docs/plan/m1-separation.md
@@ -145,9 +145,10 @@ Built and attack-tested today:
   ([littlefsd](../servers/littlefsd.md#littlefs)).
 - **The file server:** `littlefsd` over `blkd`, placed by `init`, with one volume per instance, quotas
   and typed operations ([littlefsd](../servers/littlefsd.md)).
-- **walfs**, the format for the SSD's writable volumes, on the host: its library against a model,
-  a power cut at every write and hostile volumes, and the bench's packer; its server, `walfsd`,
-  remains planned ([walfsd](../servers/walfsd.md)).
+- **walfs**, the format for the SSD's writable volumes: its library against a model, a power cut
+  at every write and hostile volumes, the bench's packer, and its server, `walfsd`, serving the
+  image's data volume under `init`, with a power cut on the machine before or after
+  ([walfsd](../servers/walfsd.md)).
 - **`bootfsd`, `consoled` and `keyd`**, attacked in host tests and booted under `init`
   ([bootfsd](../servers/bootfsd.md), [consoled](../servers/consoled.md), [keyd](../servers/keyd.md)).
 - **Launching:** the startup block and the loader stub ([init](../servers/init.md#the-startup-block)).
@@ -181,4 +182,4 @@ Built and attack-tested today:
   [SSH sessions](../testbench.md#sessions-and-the-loopback-server)).
 
 Not built: native launching on Redoubt, the steward server,
-`sshd` on the box, `walfsd`, sessions and the agent.
+`sshd` on the box, sessions and the agent.
diff --git a/docs/servers/README.md b/docs/servers/README.md
index 427b677a1..a6e0f4fca 100644
--- a/docs/servers/README.md
+++ b/docs/servers/README.md
@@ -34,9 +34,9 @@ flowchart TD
     I -.-> BF[bootfsd<br/>/boot]
     I -.-> BL[blkd<br/>the disk]
     I -.-> VD[verityd<br/>one per verified volume]
-    I -.-> FS[littlefsd:volume<br/>one per flash volume, and the data volume]
+    I -.-> FS[littlefsd:volume<br/>one per flash volume]
     I -.-> EF[erofsd:volume<br/>one per read-only volume]
-    I -.-> WF[walfsd:volume<br/>planned: one per writable SSD volume]
+    I -.-> WF[walfsd:volume<br/>one per writable SSD volume]
     I -.-> ND[netd<br/>the network card]
     I -.-> IP[ipd:network<br/>TCP/IP]
     I -.-> KD[keyd<br/>keys]
@@ -49,7 +49,7 @@ flowchart TD
 M1 (separation and containment).*
 
 `init` starts the drivers and the servers that need no principal first (`consoled`, `bootfsd`,
-`blkd`, each `verityd`, each `littlefsd` and `erofsd`, `netd`, each `ipd`, `keyd`), then the steward
+`blkd`, each `verityd`, each `littlefsd`, `erofsd` and `walfsd`, `netd`, each `ipd`, `keyd`), then the steward
 and `sshd`.
 It keeps each server's receive right, so a restarted server receives on the same endpoint (Restarts
 and crash blame, below). The servers planned for later milestones join the same graph:
@@ -68,7 +68,7 @@ Status: planned · M1 (separation and containment)
 | --- | --- | --- | --- |
 | TCB | firmware, loader, kernel; `blkd` and `netd` while there is no IOMMU | everything | the whole machine |
 | Trusted system servers | `init`, the steward, `keyd`, `sshd` | crossing principals: logins, keys, approvals, launching | every principal |
-| Shared servers | `consoled`, `bootfsd`, each `littlefsd` and `erofsd`, each `ipd`; later the resolver and `gatewayd` | serving many principals and keeping them apart by badge and label | the principals that server serves |
+| Shared servers | `consoled`, `bootfsd`, each `littlefsd`, `erofsd` and `walfsd`, each `ipd`; later the resolver and `gatewayd` | serving many principals and keeping them apart by badge and label | the principals that server serves |
 | Per-principal code | sessions, agents, native programs | nothing beyond their own capabilities | that principal's own capabilities |
 
 - **The DMA drivers are TCB.** A driver that holds a DMA-flagged device handle can point a bus
@@ -79,7 +79,8 @@ Status: planned · M1 (separation and containment)
   userland: a compromised session VM holds exactly its principal's capabilities, like a native
   program.
 - **A shared server is split by network or medium,** so one parser bug does not reach every
-  principal: one `littlefsd` or `erofsd` per volume, one `ipd` per network or trust domain.
+  principal: one `littlefsd`, `erofsd` or `walfsd` per volume, one `ipd` per network or trust
+  domain.
 - **Server work is paid by the server's weight,** not the caller's; no time is donated. Each
   shared server therefore bounds the work one request can cause and admits by caps
   ([scheduling](../kernel/scheduling.md#residual-risks), [serving](serving.md)).
@@ -228,7 +229,7 @@ Status: planned · M1 (separation and containment)
 | Server | Receives on | Holds | Never holds |
 | --- | --- | --- | --- |
 | `init` | the exit endpoint of every server | `root`, `system` and `users`; every device object and the Reset right; every server's receive right; the bundle's pages | network, user data, keys |
-| steward | its own endpoint | `users`; a connection to each `littlefsd` and `ipd`; a `keyd` grant for the `audit` purpose | any key; a budget of a server |
+| steward | its own endpoint | `users`; a connection to each writable volume's server and each `ipd`; a `keyd` grant for the `audit` purpose | any key; a budget of a server |
 | `keyd` | its own endpoint | the keys the manifest names | a key a person logs in or approves with; the bundle key |
 | `sshd` | its own endpoint | the network through `ipd`; a `keyd` badge for the host key; the steward's endpoint | any login key |
 | `consoled` | its own endpoint | the UART's MMIO and IRQ handles | anything else |
@@ -257,13 +258,14 @@ flowchart LR
     I -. passes seeds .-> KD[keyd]
     FS[littlefsd:volume] -. range badge .-> BL
     EF[erofsd:volume] -. range badge .-> BL
-    WF[walfsd:volume<br/>planned] -. range badge .-> BL
+    WF[walfsd:volume] -. range badge .-> BL
     IP[ipd:network] -. netif connection .-> ND
     ST -. audit grant .-> KD
     SS[sshd] -. host-key badge .-> KD
     SS -. login and sessions .-> ST
     SS -. connections .-> IP
     ST -. connections .-> FS
+    ST -. connections .-> WF
     ST -. scoped grants .-> IP
 ```
 *Figure: the capabilities each server holds. An edge from `init` is a handle it places; any other
@@ -278,9 +280,9 @@ and so stays a co-holder ([devices](../kernel/devices.md#which-process-gets-whic
 Status: built · tested: bench:init-boot, bench:userland-boot
 
 A file server is named for the format it serves, and its endpoints for the volumes: `erofsd`
-serves EROFS and `erofsd:system` is the system volume; `littlefsd` serves littlefs and
-`littlefsd:data` is the data volume; `walfsd` serves walfs. The name says what parser stands
-between a client and the medium, which is what
+serves EROFS and `erofsd:system` is the system volume; `walfsd` serves walfs and `walfsd:data`
+is the data volume; `littlefsd` serves littlefs. The name says what parser stands between a
+client and the medium, which is what
 [R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance) bounds. Servers that
 serve no format keep their role's name (`blkd`, `bootfsd`, `verityd`).
 
@@ -338,7 +340,7 @@ inside `gatewayd` until the web stack needs `tlsd` beyond M5.
   server's share of the CPU from its other callers, never more
   ([scheduling](../kernel/scheduling.md#residual-risks)).
 - **The DMA drivers are TCB** while there is no IOMMU ([devices](../kernel/devices.md#residual-risks)).
-- **Volumes are kept apart by placement.** The image's `init` runs `littlefsd:data` and `erofsd:system`,
+- **Volumes are kept apart by placement.** The image's `init` runs `walfsd:data` and `erofsd:system`,
   one instance per volume, so one volume's data is out of another's instance only because `init`
   places each volume once ([R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance)).
 
diff --git a/docs/servers/init.md b/docs/servers/init.md
index 5d579e8ac..5f2d58ac5 100644
--- a/docs/servers/init.md
+++ b/docs/servers/init.md
@@ -54,7 +54,7 @@ and `init`'s only input. Its entries:
 | `devices` | each device's name, its register base and its interrupt number (either may be absent, not both), and whether it may do DMA |
 | `labels` | each label's name, owner principal and 64-bit id |
 | `volumes` | each volume's name, `blkd` partition, label set and disk (the `servers` entry of the `blkd` serving it), and for a verified volume `verity`: its verifier (the `servers` entry of a [`verityd`](verityd.md)) and one mode, pinned, the root and data blocks it pins, `{ "server", "root": 64 lowercase hex digits, "blocks": a decimal string }`, or signed, the key its root block is signed under and the lowest version it may carry, `{ "server", "key": 64 lowercase hex digits or "bundle", "floor": a decimal string }` |
-| `servers` | each server's name, program (a bundle entry), budget (pages, processes, weight), the devices it gets (each a `devices` name and the name the program looks it up by), volume (its range badge, minted by `init`, and its label ids as `labels=`; a volume's server has `program` `littlefsd`, or `erofsd` for a read-only volume, and no other key says the format), the endpoints it receives on, the endpoints it is handed (each an endpoint name and the root badge `init` mints for it: a decimal string below `FIRST_MINTED_BADGE`, never used twice at one endpoint), arguments, and its stack in pages (`stack_pages`, 16 if absent, at most 128), and its heap cap in pages (`heap_pages`, none if absent) |
+| `servers` | each server's name, program (a bundle entry), budget (pages, processes, weight), the devices it gets (each a `devices` name and the name the program looks it up by), volume (its range badge, minted by `init`, and its label ids as `labels=`; a volume's server has `program` `walfsd` or `littlefsd` for a writable volume, or `erofsd` for a read-only one, and no other key says the format), the endpoints it receives on, the endpoints it is handed (each an endpoint name and the root badge `init` mints for it: a decimal string below `FIRST_MINTED_BADGE`, never used twice at one endpoint), arguments, and its stack in pages (`stack_pages`, 16 if absent, at most 128), and its heap cap in pages (`heap_pages`, none if absent) |
 | `public` | the bundle entries `bootfsd` serves at `/boot`, by exact name |
 | `principals` | each principal's name, SSH public keys (`ssh-ed25519` only) for login and approval, budget, account, owned labels, the label sets it works under (each with a fixed sub-budget: pages, processes, weight), home (volume and path), and network scope (IP prefixes and ports) |
 | `confined` | optional; a boolean at the top level ([confinement](#the-confinement-check)) |
@@ -66,7 +66,7 @@ and `init`'s only input. Its entries:
   startup block's `u32`; a larger one is the wrong type. A wrong type, an unknown member or a
   repeated one is an error, and an error refuses the boot.
 - **Names.** Every name (device, label, volume, server, endpoint, principal) is 1 to 64 bytes of
-  `[a-z0-9_:+-]`, starting with a letter (`littlefsd:data`, `alice+secrets`), compared byte for byte.
+  `[a-z0-9_:+-]`, starting with a letter (`walfsd:data`, `alice+secrets`), compared byte for byte.
   Names become endpoint names, volume names and 9P paths, so no empty name, NUL, U+FEFF or control
   character may reach them. The startup block applies the same rule (`valid_name`).
 - **Stacks.** A server's `stack_pages` is the size of its first thread's stack, charged to its
@@ -106,7 +106,7 @@ and `init`'s only input. Its entries:
   carries the smallest badge from 1 that no `handed` item there uses. A manifest names each of
   these programs at most once, `keyd` exactly once: a second would run beside the one `init`
   calls, unchecked, and a second `keyd` could hold keys `init` never asked about (R35). A program
-  that may run more than once (`blkd`, one per disk; `littlefsd` or `erofsd`, one per volume) is told the endpoint
+  that may run more than once (`blkd`, one per disk; `walfsd`, `littlefsd` or `erofsd`, one per volume) is told the endpoint
   it receives on by its argument `endpoint=NAME`, which the manifest gives it; a `blkd`'s must
   name the endpoint it receives on first, where `init` mints its volumes' ranges.
 - **Volumes.** A `volumes` entry is one GPT entry of its disk, which no other entry names on
@@ -148,7 +148,7 @@ and `init`'s only input. Its entries:
   the manifest's weights are the whole scheduling policy. `init`, the steward and the drivers
   (`consoled`, `blkd`, `netd`) get weights an order of magnitude above a session's (1000 against
   a principal's 100), so they are served promptly without running ahead of the queue; the servers
-  that work for principals (`bootfsd`, `littlefsd`, `ipd`, `keyd`, `sshd`) get ordinary weights and
+  that work for principals (`bootfsd`, the file servers, `ipd`, `keyd`, `sshd`) get ordinary weights and
   bound the work of one request. The weights carve the `system` budget like every other limit.
   The kernel sizes `system`, not the manifest, and `init` refuses a manifest whose servers' pages,
   processes or weights add up to more than `system` holds
@@ -164,9 +164,9 @@ and `init`'s only input. Its entries:
   principal's account and keys.
 
 ```json
-{ "servers": [ { "name": "littlefsd:data", "program": "littlefsd", "volume": "data",
+{ "servers": [ { "name": "walfsd:data", "program": "walfsd", "volume": "data",
                  "budget": { "pages": "4096", "processes": 1, "weight": 100 },
-                 "receives": ["littlefsd:data"], "args": ["endpoint=littlefsd:data", "buckets=4"] } ],
+                 "receives": ["walfsd:data"], "args": ["endpoint=walfsd:data", "buckets=4"] } ],
   "principals": [ { "name": "alice", "account": "1001", "labels": ["alice-secrets"],
                     "ssh_keys": ["ssh-ed25519 AAAA..."], "home": "data:/home/alice",
                     "net": [ { "prefix": "0.0.0.0/0", "ports": [22, 443] } ] } ] }
@@ -220,8 +220,8 @@ steward ([steward](steward.md)). The refusal is a boot failure, not a warning
 
 The domains compared are each `servers` entry, under its `labels` (`{}` if none), and each
 principal's label sets. A server's users are the servers handed one of its endpoints, or a
-volume's range at it (a `littlefsd` on that `blkd`'s disk, or a verified volume's `verityd`; the `littlefsd`
-attaching a verified volume at its `verityd`, which counts at the verifier's first endpoint too),
+volume's range at it (a volume's server on that `blkd`'s disk, or a verified volume's `verityd`; the
+server attaching a verified volume at its `verityd`, which counts at the verifier's first endpoint too),
 and, for a shared server (one that takes
 `buckets=N`), every principal domain with the server's own label set. Only such a domain may later
 be granted a connection there, since the steward grants within a label set by this same rule, so
@@ -292,8 +292,9 @@ Reset right. The loader maps the bundle into it, read-only
    the child ([consoled](consoled.md#started-by-init)). The check refuses a manifest that hands
    any server an endpoint `consoled` receives on: a root badge there writes bare lines, and only
    `init` holds one. Without a `consoled` entry, `init` keeps the UART;
-5. starts the rest of the drivers and the servers below the steward: `bootfsd`, `blkd`, `littlefsd`
-   and `erofsd` (one per volume), `netd` and `ipd`, then pushes the `public` entries to `bootfsd`;
+5. starts the rest of the drivers and the servers below the steward: `bootfsd`, `blkd`, each
+   volume's `walfsd`, `littlefsd` or `erofsd`, `netd` and `ipd`, then pushes the `public` entries to
+   `bootfsd`;
 6. starts the steward, handing it the `users` budget, and `sshd`.
 
 Each server runs in a budget of its own, carved from `system`, and is started through the loader
@@ -319,13 +320,13 @@ sequenceDiagram
     I->>KD: holds(each login, approval and bundle key)
     KD->>I: no (a yes stops the boot)
     I->>S: launch through the stub:<br/>consoled, then bootfsd, blkd, netd, ipd
-    I-->>S: launch littlefsd, one per volume
+    I->>S: launch walfsd, littlefsd or erofsd,<br/>one per volume
     I-->>ST: launch, with the users budget
     I-->>SH: launch, with keyd's host-key badge
     SH-->>ST: a login: whose key is this?
     ST-->>ST: carve the session budget,<br/>launch the first session
 ```
-*Figure: the boot from the loader to the first session. Dashed: planned (`littlefsd`, the steward and `sshd`).*
+*Figure: the boot from the loader to the first session. Dashed: planned (the steward and `sshd`).*
 
 The attack tests: a manifest whose servers do not fit in `system`, or whose device entries do not
 match the kernel's device objects, is refused before any server runs. The verdict is `init`'s
diff --git a/docs/servers/littlefsd.md b/docs/servers/littlefsd.md
index 3cf8d7044..7dafb94c4 100644
--- a/docs/servers/littlefsd.md
+++ b/docs/servers/littlefsd.md
@@ -15,8 +15,9 @@ bytes per attach root so one principal filling a shared volume cannot make anoth
 littlefs was chosen for a published format, an independent second implementation to test against,
 power-loss safety by design, and a size that can be read. littlefs is a file system for
 **writable** volumes only: its format is built for a written medium (commits, power loss). A
-read-only volume is EROFS, served by [`erofsd`](erofsd.md). A writable volume on flash, and
-the data volume, are littlefs; the SSD's writable volumes are walfs ([walfsd](walfsd.md)).
+read-only volume is EROFS, served by [`erofsd`](erofsd.md). The SSD's writable volumes, the
+image's data volume among them, are walfs ([walfsd](walfsd.md)); `littlefsd` serves littlefs for
+a flash medium, and for the cases that ask for a littlefs volume.
 
 ## Interface
 
diff --git a/docs/servers/walfsd.md b/docs/servers/walfsd.md
index 4e89625dc..929328b34 100644
--- a/docs/servers/walfsd.md
+++ b/docs/servers/walfsd.md
@@ -2,7 +2,7 @@
 
 ## Purpose
 
-`walfsd` is to serve a **writable volume** on the SSD in walfs, a write-ahead-log file system of
+`walfsd` serves each **writable volume** on the SSD in walfs, a write-ahead-log file system of
 Redoubt's own: a superblock, a log of whole blocks, a table of inodes, a block bitmap, and a
 SHA-256 for every block, checked on every read. littlefs ([littlefsd](littlefsd.md)) is built
 for raw flash, with wear levelling and erase units an SSD does not need, and it checksums no
@@ -362,43 +362,238 @@ and a `write` for each file, each the transactions it takes. It is deterministic
 
 ### Serving
 
-Status: planned · M1 (separation and containment)
+<details><summary>Status: built · tested (37)</summary>
+
+- bench:walfsd-boot
+- bench:walfsd-confined-labelled
+- bench:walfsd-corrupt-volume
+- bench:walfsd-flipped-block
+- bench:walfsd-label-check
+- bench:walfsd-reboot
+- host:redoubt-init::a_walfsd_entry_is_a_volume_server_as_a_littlefsd_one_is
+- host:redoubt-walfsd::a_blank_range_is_formatted_and_only_a_blank_one
+- host:redoubt-walfsd::a_cut_at_every_write_of_a_write_and_a_rename_leaves_before_or_after
+- host:redoubt-walfsd::a_device_that_fails_makes_the_volume_corrupt_until_it_is_mounted_again
+- host:redoubt-walfsd::a_failing_range_answers_corrupt
+- host:redoubt-walfsd::a_flipped_bit_in_a_file_is_corrupt_for_that_file_alone
+- host:redoubt-walfsd::a_range_of_noise_is_served_as_corrupt
+- host:redoubt-walfsd::a_range_too_small_is_no_volume
+- host:redoubt-walfsd::a_read_only_range_is_never_written
+- host:redoubt-walfsd::a_read_only_range_is_served_read_only
+- host:redoubt-walfsd::a_read_only_volume_refuses_every_typed_change
+- host:redoubt-walfsd::a_removed_files_other_fids_get_removed
+- host:redoubt-walfsd::a_rename_over_a_file_removes_it
+- host:redoubt-walfsd::a_sequential_read_costs_a_few_block_reads_per_request_and_per_block
+- host:redoubt-walfsd::a_strangers_fid_is_not_found
+- host:redoubt-walfsd::arguments_are_an_endpoint_and_labels_and_nothing_else
+- host:redoubt-walfsd::arguments_it_does_not_understand_stop_it_before_serving
+- host:redoubt-walfsd::attach_walk_open_read_write
+- host:redoubt-walfsd::attributes_set_and_get_with_the_reserved_types_refused
+- host:redoubt-walfsd::copy_file_copies_and_counts_the_bytes
+- host:redoubt-walfsd::every_transaction_is_four_flushes
+- host:redoubt-walfsd::fids_are_bounded_and_disconnect_frees_them
+- host:redoubt-walfsd::files_and_directories_survive_a_remount
+- host:redoubt-walfsd::files_survive_a_restart
+- host:redoubt-walfsd::listing_a_directory_reads_it_once_per_window
+- host:redoubt-walfsd::noise_is_never_formatted_and_never_mounted
+- host:redoubt-walfsd::rename_moves_within_the_volume_and_keeps_the_inode
+- host:redoubt-walfsd::the_client_library_works_against_walfsd
+- host:redoubt-walfsd::the_conformance_vectors_run_against_walfsd
+- host:redoubt-walfsd::the_volumes_labels_are_checked_on_every_request
+- host:redoubt-walfsd::typed_operations_check_the_volumes_labels
 
-`walfsd` will serve walfs as `littlefsd` serves littlefs: one instance per volume, under a
-`blkd` range, with the same 9P face, labels and typed operations.
+</details>
 
-**Open:** what a quota counts (blocks or bytes) and which attribute types `walfsd` serves.
+`walfsd` serves walfs as `littlefsd` serves littlefs
+([littlefsd](littlefsd.md#volumes-connections-and-labels)), with the same 9P face, labels and
+typed operations, so a client names no format:
+
+- **One instance per volume.** A `walfsd` holds one block-range handle, `volume`, its partition at
+  [`blkd`](blkd.md), and no MMIO, interrupt or DMA. `init` starts it as it starts `littlefsd` and
+  `erofsd`: a `servers` entry whose `program` is `walfsd` and whose `volume` names the volume, with
+  the arguments `endpoint=NAME` (`walfsd:data`), `labels=ID[,ID...]` and `buckets=N`
+  ([init](init.md#the-boot-manifest)). A block is eight of `blkd`'s sectors, so the volume's block
+  count is its range's sectors divided by 8; a range of fewer than 40 blocks, the smallest volume
+  at the packer's inode density, is no volume and `walfsd` exits.
+- **Mounting.** A range whose first two blocks (the superblock and the log's header) are all zero
+  has never been written, and `walfsd` formats it with an inode for every 16 blocks, as the packer
+  does. Any other range is mounted, which recovers a transaction left in the log
+  ([The log](#the-log)) and finishes the orphan list; one that does not mount is served as
+  corrupt: every attach is refused with `corrupt`, `walfsd` says so on its console, and it stays
+  up, so a damaged or hostile medium never becomes a restart loop. `walfsd` never formats a range
+  that holds anything. A range `blkd` reports read-only is served read-only: every change is
+  refused before it reaches `blkd`, and a blank one is not formatted.
+- **`sync` is `blkd`'s `flush`.** walfs's device `sync`, between each of a transaction's four
+  steps ([The log](#the-log)), is a `flush` on the range, which returns only when the device's own
+  flush has completed ([blkd](blkd.md#messages)): four flushes a transaction, and nothing written
+  is acknowledged before them.
+- **A fid is a path, an inode and a generation.** A node is the path its client walked, built only
+  from names clients walked or created, and the inode and generation the file had there
+  ([Inodes](#inodes)). Every request finds the path again and checks the pair: an entry gone, or
+  another file in its place, even in the same inode at a later generation, is `removed`. So a
+  remove or a rename over a file ends every other fid on it, and fids do not follow renames, as on
+  `littlefsd`. A qid's path is the inode and its version the generation's low 32 bits, so a qid
+  names one file for the volume's life, and survives a reboot.
+- **A remove ends the file at once.** No walfs handle outlives a request, so a removed file's
+  blocks are freed in the same call, and nothing removed holds space or quota.
+- **One transaction an operation.** Each of 9P's `create`, `write` (one transaction while it
+  fits one, [Atomicity](#atomicity)), `remove` and an open that truncates, and each typed
+  `rename` and `set_attr`, is the walfs operation of the same name; a power cut leaves it before
+  or after ([R50 (power loss leaves before or after), on walfs volumes](#r50-power-loss-leaves-before-or-after-on-walfs-volumes)).
+- **Damage is corrupt where it is read.** A block that fails its hash, or any structure the format
+  calls corrupt ([What is corrupt](#what-is-corrupt)), fails the request that read it with
+  `corrupt` (the `Rerror` text for 9P, the table's `corrupt` for a typed operation), and nothing
+  else: other files read and the volume still writes. An I/O error from `blkd` poisons the volume
+  until `walfsd` starts again, as on `littlefsd`.
+- **Labels are per volume.** Each volume has one label set, and `walfsd` reports it as every
+  node's, so the skeleton's label check runs on every request
+  ([R25 (the label check)](serving.md#r25-the-label-check)); there are no per-file labels, owners
+  or permission bits.
+- **Typed operations** are `littlefsd`'s table, served alike: `rename`, `copy_file`, `set_attr`
+  and `get_attr` ([littlefsd](littlefsd.md#typed-operations)). Attributes live in the inode's
+  256-byte area ([User attributes](#user-attributes)): types 16 to 255 are the user's and 0 to 15
+  are refused, as on `littlefsd`, so a client sees one contract; a value is at most 254 bytes
+  (`too_large` above), and attributes that no longer fit the area are `no_space`. `copy_file`
+  writes a new file a block at a time and removes it if it does not finish. Times: a file's mtime
+  is walfs's, 0 until a clock reaches `walfsd`.
+- **Listings** are served from a window of up to 64 entries filled by one pass over the
+  directory, as on `littlefsd`, so listing n entries costs about n / 64 passes.
+- **Admission** is the serving library's, with `littlefsd`'s caps per bucket
+  ([R26 (admission fairness)](serving.md#r26-admission-fairness)).
+- **Memory** is the format's ([Memory](#memory)), a node per fid (its path), the listing window,
+  and the quota's records; the image declares the heap the memory scan measured
+  ([testbench](../testbench.md#the-memory-budget)).
+- **Reads.** Each request finds its file by path and opens it, about nine block reads with their
+  hash blocks, then reads its data blocks, each about two block reads, since a data block's hash
+  block takes turns with the inode table's in the one hash block walfs keeps: 10.8 block reads per
+  data block in 4 KiB reads, 2.9 in 32 KiB reads. `walfsd` keeps no block cache.
+
+### Quotas
+
+<details><summary>Status: built · tested (9)</summary>
+
+- bench:walfsd-quota
+- host:redoubt-walfsd::a_copy_past_the_quota_is_no_space
+- host:redoubt-walfsd::a_mint_the_room_cannot_take_is_refused_and_disconnect_gives_it_back
+- host:redoubt-walfsd::a_rename_between_two_roots_moves_the_bytes_and_never_ends_a_live_root
+- host:redoubt-walfsd::a_root_minted_over_files_counts_them
+- host:redoubt-walfsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove
+- host:redoubt-walfsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes
+- host:redoubt-walfsd::file_bytes_counts_data_and_indirect_blocks
+- host:redoubt-walfsd::the_volume_never_runs_out_while_every_root_is_within_its_quota
 
-## Authority
+</details>
 
-Status: planned · M1 (separation and containment)
+A quota is in **bytes**, carved at `new_connection` from the room of the live root above, and
+kept by `littlefsd`'s ledger, rules and refusals: a root's quota is the sum of its connections',
+a change is charged to the nearest live root above it, a root minted over more than its quota
+can read and remove only, nothing is stored on the medium, and a rename or remove never ends a
+live root ([littlefsd](littlefsd.md#quotas)). What walfs counts:
+
+- **The volume root's room** is the data region's bytes: its blocks times 4096.
+- **An entry holds** its share, ⌈room / (`inode_count` − 2)⌉ bytes (inodes 0 and 1 are never an
+  entry's), and 4096 bytes for each data block its size reaches, holes included, and each
+  indirect block that would map them; a directory holds its own blocks too, and what lies under
+  it. So entries that fit a quota never take more inodes than the volume has, nor blocks: no
+  quota is a promise the volume cannot keep, in blocks or inodes.
+- **A change is refused before its transaction begins.** A write needs the bytes its new size
+  adds; a create, its share and a block its directory may gain; a copy, the whole file; a rename
+  between two roots, what moves less what it replaces, and the block. What the change then made
+  is charged: the file's size, the directory's blocks, as they are after it.
+
+With the packer's inode for every 16 blocks, the share is about 64 KiB, so a root of quota Q holds
+about Q / 64 KiB entries, fewer if they hold data
+([Residual risks](#residual-risks)).
+
+## Authority
 
-`walfsd` will hold its own endpoint, one `blkd` range and the connections it mints, as
-`littlefsd` does.
+Status: built · tested: bench:walfsd-one-volume
 
-**Open:** none.
+`walfsd` holds its endpoint, its one block-range handle at `blkd`, the console `init` gave it,
+and the connections it mints, as `littlefsd` does. It holds no device, no budget handle and no
+connection to any other file server. What a client may reach is the subtree its connection is
+rooted at, under the volume's labels and its root's quota.
 
 ## Security properties
 
-Status: planned · M1 (separation and containment)
+### R47 (one volume per instance), on walfs volumes
 
-`walfsd` is to keep, for its volumes, what `littlefsd` keeps today:
-[R47 (one volume per instance)](littlefsd.md#r47-one-volume-per-instance),
-[R48 (a quota per attach root)](littlefsd.md#r48-a-quota-per-attach-root),
-[R49 (a hostile medium is corrupt, not a crash)](littlefsd.md#r49-a-hostile-medium-is-corrupt-not-a-crash)
-and [R50 (power loss leaves before or after)](littlefsd.md#r50-power-loss-leaves-before-or-after),
-each restated here when the server is built. The format above is what R49 and R50 will rest on.
+<details><summary>Status: built · tested (2)</summary>
 
-**Open:** none beyond Serving's.
+- bench:walfsd-one-volume
+- host:redoubt-init::a_walfsd_entry_is_a_volume_server_as_a_littlefsd_one_is
 
-## Failure and restart
+</details>
+
+Each `walfsd` instance serves one volume and holds only its block range, placed by `init` as a
+`littlefsd`'s is: a parser exploit through a crafted volume or request reaches that volume and
+nothing else.
+
+### R48 (a quota per attach root), on walfs volumes
+
+<details><summary>Status: built · tested (3)</summary>
+
+- bench:walfsd-quota
+- host:redoubt-walfsd::a_root_with_quota_0_cannot_create_but_can_read_and_remove
+- host:redoubt-walfsd::a_write_past_one_roots_quota_is_refused_while_another_still_writes
+
+</details>
+
+Every connection's root has a byte quota carved from its granter's, and no change takes a root
+past it ([Quotas](#quotas)); an entry's share keeps inodes, as well as blocks, within what the
+quotas promise, so one principal filling a shared volume cannot make another's creates or writes
+fail.
+
+### R49 (a hostile medium is corrupt, not a crash), on walfs volumes
+
+<details><summary>Status: built · tested (8)</summary>
+
+- bench:walfsd-corrupt-volume
+- bench:walfsd-flipped-block
+- fuzz:walfs/image
+- fuzz:walfs/mutate
+- host:redoubt-walfsd::a_flipped_bit_in_a_file_is_corrupt_for_that_file_alone
+- host:redoubt-walfsd::noise_is_never_formatted_and_never_mounted
+- host:walfs::every_flipped_bit_is_corrupt_where_it_is_read
+- host:walfs::noise_never_panics
+
+</details>
+
+Whatever bytes the medium holds, walfs refuses them as corrupt where they are read rather than
+panicking, looping or allocating past the volume ([What is corrupt](#what-is-corrupt)), and
+`walfsd` answers `corrupt` for that request alone; a volume that does not mount is served as
+corrupt, and `walfsd` stays up. A block of a file changed on the medium is found by its hash, so
+unlike littlefs's, a walfs volume's data is checked too.
+
+### R50 (power loss leaves before or after), on walfs volumes
+
+<details><summary>Status: built · tested (5)</summary>
 
-Status: planned · M1 (separation and containment)
+- bench:walfsd-power-loss
+- host:redoubt-walfsd::a_cut_at_every_write_of_a_write_and_a_rename_leaves_before_or_after
+- host:walfs::crash_at_every_write_fixed_workload
+- host:walfs::crash_at_every_write_random_workloads
+- host:walfs::crash_during_recovery
+
+</details>
+
+On a device that keeps `blkd`'s `flush`, a power cut at any block write leaves each operation,
+data and metadata together, as before it or after it ([Atomicity](#atomicity)), and the next mount
+recovers the log. The bench cuts `walfsd` itself after a block write drawn from its seed, inside a
+write of three blocks and a rename, and the restarted instance serves the file as before or after
+each, with the volume check finding nothing.
+
+## Failure and restart
 
-A cut transaction is recovered at mount, so a restarted `walfsd` serves the volume as of its
-last committed transaction; a corrupt volume will be served as corrupt, not an exit.
+Status: built · tested: bench:walfsd-restart, bench:walfsd-power-loss, bench:walfsd-corrupt-volume
 
-**Open:** none.
+- **`walfsd` crashes:** its clients' calls get `Dead`, `init` restarts it on the same endpoint
+  ([init](init.md#restarts-and-reboots)), and its mount recovers a cut transaction from the log,
+  so the volume is as of its last committed transaction. Clients ask for fresh connections.
+- **The medium is corrupt:** a volume that does not mount is served as corrupt, never exited on;
+  a damaged block fails the requests that read it.
+- **An I/O error from `blkd`** poisons the volume until `walfsd` starts again, whose mount
+  recovers what the error left.
 
 ## Residual risks
 
@@ -409,11 +604,21 @@ last committed transaction; a corrupt volume will be served as corrupt, not an e
   fails reads as a commit torn by power loss, so the transaction it held is lost rather than
   refused as corrupt. The blocks it would have written are left as they were, each still
   checked by its slot.
-- **A directory lookup is linear** in the directory's size.
+- **A directory lookup is linear** in the directory's size, and `walfsd` finds a file by its path
+  on every request: about nine block reads before the first data block.
 - **No wear levelling:** an SSD levels its own wear; walfs on raw flash would wear its log and
   hash region first. littlefs stays the format for raw flash.
 - **A large write is several transactions.** Each is before or after, so a power cut inside a
   write of more than one transaction's blocks leaves a prefix of it.
+- **A quota is counted coarsely.** An entry costs its share, about 64 KiB at the packer's
+  density, whatever it holds, and a file every block its size reaches, holes included, so a
+  quota holds fewer small or sparse files than its bytes suggest; and once every inode's share
+  is carved, the volume's last directory blocks are out of every quota's reach.
+- **A qid's version is the generation,** not a count of writes: a write does not move it, so a
+  client caching a file by its qid does not see the change. mtime is 0 until a clock reaches
+  `walfsd`.
+- **A shared `walfsd` is shared state,** as a shared `littlefsd` is
+  ([littlefsd](littlefsd.md#residual-risks)).
 
 ## Why
 
diff --git a/docs/testbench.md b/docs/testbench.md
index 796a50ccd..f4bfa4f0f 100644
--- a/docs/testbench.md
+++ b/docs/testbench.md
@@ -1233,7 +1233,11 @@ read-only case also scans its additional client from the merged manifest. `beaml
 rv64 with the boot pack read before its VM starts and its console on the hub; twice it needs its
 18 pages. `consoled`'s row is from the runs once it serves beamlet's console as a multiplexed
 session: its stack peak from rv64 `userland-read-only`, its heap peak from rv32
-`beamlet-footprint`.
+`beamlet-footprint`. `walfsd:data`'s row is from six runs of `walfsd-quota`, three on each width,
+which writes through it, as the image's memory cases do not (their boots peak at 16,040 bytes of
+stack and 9 heap pages); its heap cap is also above what a transaction of the format's 32 blocks
+adds to that case's peak, 32 pages, since no case writes that many at once
+([walfsd](servers/walfsd.md#memory)).
 `erofsd:system`'s row and `verity:system`'s heap, which holds 4 checked data blocks, are from the
 six runs with the userland volume on EROFS. `verity:system`'s stack is from rv64 `userland-boot`
 once it also checks a signed volume's root block, which it does not use there but whose code lies
@@ -1247,7 +1251,7 @@ in its start path.
 | `blkd` | 4,504 | 3 | 17 | 34 |
 | `netd` | 4,280 | 3 | 2 | 4 |
 | `ipd` | 8,040 | 4 | 4 | 8 |
-| `littlefsd:data` | 7,176 | 4 | 9 | 18 |
+| `walfsd:data` | 28,616 | 14 | 15 | 64 |
 | `blkd:system` | 4,504 | 3 | 17 | 34 |
 | `verity:system` | 8,264 | 5 | 50 | 100 |
 | `erofsd:system` | 9,704 | 5 | 12 | 24 |
diff --git a/docs/userland/files.md b/docs/userland/files.md
index b933256b9..21003b981 100644
--- a/docs/userland/files.md
+++ b/docs/userland/files.md
@@ -2,8 +2,8 @@
 
 A file on Redoubt is something a server serves over 9P, reached through a connection in the
 session's namespace. Elixir's `File`, `IO` and `Path` work unchanged on top: beamlet's file
-natives speak 9P to the file server, one instance per volume, which keeps the data in littlefs on
-a block device. There are no permission bits, no owners, no symlinks and no hard links. Access is
+natives speak 9P to the file server, one instance per volume, which keeps the data in walfs on
+the SSD (`walfsd`), or in littlefs on a flash medium (`littlefsd`). There are no permission bits, no owners, no symlinks and no hard links. Access is
 by capability (holding the connection) and by label (the volume's labels against the caller's);
 a **bind** puts a connection the session already holds at another path.
 
@@ -74,14 +74,15 @@ flowchart LR
     F["File, IO<br/>(Elixir)"] -.-> FM[":file, file_io_server<br/>(OTP, unchanged)"]
     FM -.-> PF["prim_file natives<br/>(beamlet)"]
     PF -.-> C["the 9P client<br/>(beamlet's Platform)"]
-    C -.->|"9P over call and lend"| LFSD["littlefsd, one per volume"]
-    LFSD -.-> LFS["littlefs"]
+    C -.->|"9P over call and lend"| LFSD["walfsd, one per volume"]
+    LFSD -.-> LFS["walfs"]
     LFS -.->|"typed calls"| B["blkd"]
     B -.-> D["virtio block device"]
 ```
 *Figure: the file I/O path from `File` to the block device. The links are built; they are drawn
 dashed until Elixir's `File` runs over them in a session. The servers are
-[the file server](../servers/littlefsd.md)'s and [blkd](../servers/blkd.md)'s pages.*
+[the file server](../servers/walfsd.md)'s and [blkd](../servers/blkd.md)'s pages; `littlefsd`
+serves the same 9P and typed calls over littlefs.*
 
 `File.read!/1` becomes OTP's `file` module, whose `prim_file` calls beamlet implements over its
 9P client ([beamlet](beamlet.md#the-platform-boundary);
@@ -95,11 +96,11 @@ neither inside nor above one is `:enoent` ([sessions](sessions.md#namespaces)).
 - **Everything moves in pieces**: a read asks at most what one answer carries (the 64 KiB `msize`
   less its header), a write at most a page, each one request on the VM's hub
   ([asynchronous underneath](beamlet.md#asynchronous-underneath-synchronous-on-top)), so only the
-  Erlang process that asked waits for it. `littlefsd`'s `rename` is a typed call, which no hub
+  Erlang process that asked waits for it. The file server's `rename` is a typed call, which no hub
   carries: it is made on the VM's thread.
 - **Plain 9P2000**, with no Unix extensions: a `stat` has a name, a length, a modification time
   and a qid (the server's identity and version for the file), and nothing else. Custom
-  per-file metadata is the file server's typed `set_attr` and `get_attr`, kept in littlefs
+  per-file metadata is the file server's typed `set_attr` and `get_attr`, kept in the volume's
   attributes.
 - **An error is a Redoubt error first.** The file server refuses with its own reasons
   (`not_found`, `refused`, `exists`, `not_dir`, `removed` for a fid whose file was removed,
@@ -110,7 +111,7 @@ neither inside nor above one is `:enoent` ([sessions](sessions.md#namespaces)).
 
 | Operation | What happens |
 | --- | --- |
-| `File.stat`, `:file.read_file_info` | what 9P and the file server have: the type (from the qid), the size and the modification time (0 until `littlefsd` keeps one: the residual below); `access`, `mode`, `uid`, `gid`, `links`, `inode` and `major_device` are `:undefined`, since no server says what a connection may do; attributes through `get_attr` |
+| `File.stat`, `:file.read_file_info` | what 9P and the file server have: the type (from the qid), the size and the modification time (0 until the file server keeps one: the residual below); `access`, `mode`, `uid`, `gid`, `links`, `inode` and `major_device` are `:undefined`, since no server says what a connection may do; attributes through `get_attr` |
 | `File.ls` | a read of a directory fid; entries the caller may not read are left out |
 | `File.rm` of an open file | succeeds: an "in use" refusal would tell one client about another |
 | `File.chmod`, `File.chown` | `{:error, :enotsup}`: there are no mode or owner bits, and access is by capability |
@@ -131,7 +132,8 @@ bench:beamlet-files sees `mode` undefined in a boot.
 Residuals, each a departure from the table, until the file server serves what it needs:
 - **No times are set and none is stored.** The 9P skeleton refuses `Twstat`, so `File.write_stat`
   with times, and a cut at a position (`:file.truncate/1`) other than an open's, are
-  `{:error, :enotsup}`; and `littlefsd` keeps no modification time, so it reads as 0 (1970).
+  `{:error, :enotsup}`; and no file server has a clock to keep a modification time by, so it
+  reads as 0 (1970).
 
 ### Copying, moving, removing and binds
 
@@ -161,7 +163,7 @@ into its startup block, so a session's binds reach a child only if the session p
 Status: planned · M1 (separation and containment)
 
 Labels are per volume: each volume has its own file server instance and its own label set, fixed
-when the volume is set up ([the file server](../servers/littlefsd.md)). The file server checks every
+when the volume is set up ([the file server](../servers/walfsd.md)). The file server checks every
 request against the caller's label set, which the kernel stamps on the message
 ([R14 (unforgeable sender)](../kernel/ipc.md#r14-unforgeable-sender)), by the servers' label rule
 ([labels](../servers/README.md#labels)):
diff --git a/image/README.md b/image/README.md
index 234ddad03..572f7e198 100644
--- a/image/README.md
+++ b/image/README.md
@@ -7,7 +7,7 @@ The sources of the signed boot bundle and the disk images; what they produce goe
   manifest. `./mkimage` packs it with the bench's builder into `target/image/redoubt.bundle`, and
   the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
   block count into the manifest it packs, from its own pack of `userland.toml`.
-- `manifest.json`: the boot manifest `init` reads, with eleven servers, including `littlefsd:data` for
+- `manifest.json`: the boot manifest `init` reads, with eleven servers, including `walfsd:data` for
   the disk's `data` volume, the userland disk's `blkd:system`, `verity:system` and `erofsd:system`,
   and `beamlet` running the shell, `Redoubt.Shell`, on the UART console. The userland volume is
   verified: its entry's `verity` names `verity:system`, and the root and block count in this file
@@ -16,8 +16,10 @@ The sources of the signed boot bundle and the disk images; what they produce goe
   shell's budget, twice what its VM holds at its largest peak, does not fit the `system` budget of
   a smaller machine ([budgets](../docs/kernel/budgets.md)).
 - `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
-  `data` partition as a littlefs volume holding `target/image/stage/`, written through `littlefsd`'s own
-  code, and the `image-disk` case boots a disk packed the same way.
+  `data` partition as a walfs volume holding `target/image/stage/`, written by `libs/walfs` itself
+  ([walfsd](../docs/servers/walfsd.md#the-packer)), and the `image-disk` case boots a disk packed
+  the same way. The image holds no littlefs volume; `littlefsd` stays in the bundle for a flash
+  medium and the cases that ask for one.
 - `userland.toml`: the userland disk, attached read-only. `./mkimage` packs it with the same
   packer into `target/image/userland.img`: each module of the applications it names, compiled by
   the pinned toolchain and stripped, as a plain file under its own name (`Elixir.Enum.beam`,
```

### Commit 5
```diff
commit 609b2c864d4f708aa4e6966775e56d52c71afba3
Author: Michael <sirmick@gmail.com>
Date:   Tue Oct 6 23:02:11 2026 -0700

    image: alice's labelled volume is walfs too, and the steward's lines name walfsd
    
    The image's `alice-secrets` partition is packed as walfs and served by `walfsd:alice-secrets`,
    program `walfsd`, with walfsd:data's declarations; the steward is handed both walfsd endpoints, so
    the lines init writes it read `home "alice" handle=walfsd:data path=/home/alice` and `vault
    "alice" labels=[7] handle=walfsd:alice-secrets`. Nothing in the steward names a format: this is a
    rename of the manifest's handles. The image now has no littlefs volume, so the cases booting its
    manifest run no littlefsd; the steward's cases run on the walfs volumes with their verdicts
    unchanged. The pages' worked configuration, namespace example and memory table name walfsd.
    
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>

diff --git a/docs/plan/m1-separation.md b/docs/plan/m1-separation.md
index 6581bd707..cc45feafd 100644
--- a/docs/plan/m1-separation.md
+++ b/docs/plan/m1-separation.md
@@ -49,7 +49,7 @@ Bob, attacking Alice.
 | A lender destroyed while `littlefsd` holds its lent pages, and `littlefsd` survives | [R3 (lends and abandoned calls)](../kernel/ipc.md#r3-lends-and-abandoned-calls) | `uaf-lent-page`, `process-lifecycle` for the kernel; with `littlefsd` not yet |
 | Crash blame: Bob crashes `littlefsd` three times while Alice is busy; every session and lease of Bob's with that label set ends and he cannot log straight back in; Alice is unaffected, also when `littlefsd` panics rather than faults and when the crashing thread holds her calls open too; a crash from a `send` while a bystander's call is parked blames nobody; a vault session's crashes do not end its owner's unlabelled session | [R21 (crash blame)](../kernel/processes.md#r21-crash-blame), [R40 (blame by label set)](../servers/steward.md#r40-blame-by-label-set) | `process`, `process-attack` for the kernel's blame; the steward's not yet |
 | Pinned open calls: 64 lent calls parked at `ipd` with short timeouts, and `ipd` still takes `netd`'s frames and frees the abandoned calls; SSH sessions survive | [R28 (parked-call accounting)](../servers/serving.md#r28-parked-call-accounting), [R4a (open calls)](../kernel/ipc.md#r4a-open-calls) | `net-pinned`; with SSH not yet |
-| System fairness: a busy `littlefsd:data` does not fill `blkd`'s `WAIT_CAP` for `littlefsd:alice-secrets` | R2 | not yet |
+| System fairness: a busy `walfsd:data` does not fill `blkd`'s `WAIT_CAP` for `walfsd:alice-secrets` | R2 | not yet |
 | Server CPU: expensive requests to a server delay other users only by that server's weight | R12 | `sched-server-busy`, `sched-large-weight` |
 | Shared pools: filling the `data` volume does not fail Alice's saves; flooding `littlefsd` with handles does not grow its table | [R48 (a quota per attach root)](../servers/littlefsd.md#r48-a-quota-per-attach-root) | not yet |
 | Server authority: no server's startup block holds its budget, a manifest granting one is refused, and no server can destroy a session | [R33 (no server holds a system budget)](../servers/init.md#r33-no-server-holds-a-system-budget) | `init-refuses-budget-handle`; that no server can destroy a session is the steward's, not yet |
diff --git a/docs/servers/init.md b/docs/servers/init.md
index e1ea95f5b..b52ab41a1 100644
--- a/docs/servers/init.md
+++ b/docs/servers/init.md
@@ -610,7 +610,7 @@ Alice and Bob each log in over SSH; Alice has a vault label `alice-secrets` and
 ```
 kernel
 └── init                                              root
-    ├── consoled bootfsd blkd littlefsd:data littlefsd:alice-secrets  system
+    ├── consoled bootfsd blkd walfsd:data walfsd:alice-secrets  system
     │   netd ipd:lan keyd steward sshd
     ├── session alice-1                               users/alice/{}/session-1
     ├── agent alice/researcher [lease 2 h]            users/alice/{}/researcher
@@ -621,21 +621,21 @@ kernel
 
 | Name | Alice's session | Bob's session | Enforced by |
 | --- | --- | --- | --- |
-| `/` | `littlefsd:data` at `/home/alice`, read-write | `littlefsd:data` at `/home/bob`, read-write | `littlefsd` (badge) |
+| `/` | `walfsd:data` at `/home/alice`, read-write | `walfsd:data` at `/home/bob`, read-write | `walfsd` (badge) |
 | `/dev/cons` | her SSH channel | his | `sshd` (badge, channel labels) |
 | `/net` | `ipd:lan`, connect out to ports 22 and 443, not the box's own addresses | `ipd:lan`, connect out to 443 | `ipd` (badge) |
 | `powerbox`, `budget` | hers | his | the steward, the kernel |
 
 - Weights: Alice 100, Bob 100; the agent 20, carved from Alice's, sharing her account. `init`,
   the steward and the drivers are 1000 each in the same queue.
-- The vault session reads and writes `littlefsd:alice-secrets`, reads (never writes) her home on the
-  unlabelled `littlefsd:data`, which is how data enters the vault, has no `/net`, and prints only to its
+- The vault session reads and writes `walfsd:alice-secrets`, reads (never writes) her home on the
+  unlabelled `walfsd:data`, which is how data enters the vault, has no `/net`, and prints only to its
   own channel.
 - The agent has its own principal, `/work` only and no `/net`; its escalations wait for Alice's
   approval, and the lease's end destroys its budget and everything it passed on.
 - No session or lease holds a `keyd` grant in M1 (separation and containment): `keyd`'s purposes are
   the host key and audit signing.
-- Bob crashing `littlefsd:data` three times is blamed on his account each time: his sessions end and he
+- Bob crashing `walfsd:data` three times is blamed on his account each time: his sessions end and he
   is locked out for a while; Alice is not affected.
 
 **Open:** none.
diff --git a/docs/servers/steward.md b/docs/servers/steward.md
index 67045bb28..252e29546 100644
--- a/docs/servers/steward.md
+++ b/docs/servers/steward.md
@@ -276,8 +276,8 @@ so the steward still holds no key.
   it runs each batch's steps in order, stopping at the first that fails, and reports the batch
   as one `Done`. Its binding table maps each `Shared` slot to a server, a root and a badge per
   domain class; a slot bound to nothing for that class is produced without a call. The slots, in
-  order: `bootfsd` at `/boot`; the home volume's `littlefsd` at the home's path; the label set's
-  volume's `littlefsd` at `/vault` (a vault session only); `ipd` at `/net`, granted the
+  order: `bootfsd` at `/boot`; the home volume's server (`walfsd`) at the home's path; the label
+  set's volume's at `/vault` (a vault session only); `ipd` at `/net`, granted the
   principal's scope (an unlabelled session only); the console at `/dev/cons` (`sshd`'s channel,
   or `consoled` for the console principal's session); and the system volume's `erofsd`. The
   child finds the first and last also under beamlet's handle names, `bootfsd` and
@@ -467,7 +467,7 @@ sequenceDiagram
     participant SH as sshd
     participant KD as keyd
     participant ST as steward
-    participant F as littlefsd, ipd, consoled
+    participant F as walfsd, ipd, consoled
     participant S as session
     C->>SH: SSH, user alice+secrets, key K
     SH->>KD: sign the exchange (host key)
@@ -675,8 +675,8 @@ sequenceDiagram
     participant P as Alice (owner)
     participant ST as steward
     participant R as reader budget {alice-secrets}
-    participant V as littlefsd:alice-secrets
-    participant U as littlefsd:data
+    participant V as walfsd:alice-secrets
+    participant U as walfsd:data
     Note over P,U: planned
     P-->>ST: declassify(item)
     ST-->>R: create (exact labels, deadline), call
@@ -804,7 +804,7 @@ The steward keeps each principal's package records and never parses a package
 Status: planned · M5 (persist, install, share)
 
 A **project** is a principal sponsored by several members, with its own budget, volume
-(`littlefsd:project-x`), package directory and profile, and optionally a label. Membership is
+(`walfsd:project-x`), package directory and profile, and optionally a label. Membership is
 capabilities minted into a revocation scope per member; removing a member destroys the scope. A
 labelled project is worked on in project vault sessions (`ssh alice+project-x@box`), and
 declassifying one of its items needs a project owner's approval. No kernel mechanism is involved.
diff --git a/docs/testbench.md b/docs/testbench.md
index b714af469..1037ab9e7 100644
--- a/docs/testbench.md
+++ b/docs/testbench.md
@@ -1241,10 +1241,10 @@ so at most 46 a connection, and its cap is twice 8 pages and four connections, 3
 stack is a page over twice its peak. `consoled`'s row is from the runs once it serves each
 session's console as a multiplexed session. The read-only case also scans its additional client
 from the merged manifest.
-`walfsd:data`'s row is from six runs of `walfsd-quota`, three on each width, which writes
-through it, as the image's memory cases do not (their boots peak at 16,040 bytes of stack and 9
-heap pages); its heap cap is also above what a transaction of the format's 32 blocks adds to
-that case's peak, 32 pages, since no case writes that many at once
+The two `walfsd` rows are one server's, from six runs of `walfsd-quota`, three on each width,
+which writes through it, as the image's memory cases do not (their boots peak at 16,040 bytes of
+stack and 9 heap pages); its heap cap is also above what a transaction of the format's 32 blocks
+adds to that case's peak, 32 pages, since no case writes that many at once
 ([walfsd](servers/walfsd.md#memory)).
 
 | Image server | Largest stack peak (bytes) | Declared stack (pages) | Largest heap peak (pages) | Heap cap (pages) |
@@ -1256,7 +1256,7 @@ that case's peak, 32 pages, since no case writes that many at once
 | `netd` | 4,296 | 3 | 2 | 4 |
 | `ipd` | 13,672 | 7 | 37 | 74 |
 | `walfsd:data` | 28,616 | 14 | 15 | 64 |
-| `littlefsd:alice-secrets` | 7,192 | 4 | 9 | 18 |
+| `walfsd:alice-secrets` | 28,616 | 14 | 15 | 64 |
 | `blkd:system` | 4,520 | 3 | 17 | 34 |
 | `verity:system` | 8,264 | 5 | 50 | 100 |
 | `erofsd:system` | 9,704 | 5 | 13 | 26 |
diff --git a/docs/userland/sessions.md b/docs/userland/sessions.md
index 3313a4877..d5803f865 100644
--- a/docs/userland/sessions.md
+++ b/docs/userland/sessions.md
@@ -29,7 +29,7 @@ At the prompt, the session's namespace is a table you can print:
 
 ```elixir
 /home/alice (1)> ns()
-/home/alice  littlefsd:home     (Alice's home volume)
+/home/alice  walfsd:data       (Alice's home volume)
 /dev/cons    sshd         (this SSH channel)
 /boot        bootfsd      (the boot bundle, read-only)
 /net         ipd          (the hosts and ports this session may reach)
@@ -98,7 +98,7 @@ and a session holds only what its principal was granted.
 
 ### Vault sessions
 
-Status: built · partly tested: a vault session's own reads and writes wait for beamlet's file platform (BEAM3); the label check they meet at `littlefsd` is attacked on its own · tested: bench:steward-vault-session, bench:steward-sub-budget-flood, bench:littlefsd-label-check, host:redoubt-steward-server::a_vault_login_carves_from_the_vault_s_sub_budget_and_has_no_network
+Status: built · partly tested: a vault session's own reads and writes wait for beamlet's file platform (BEAM3); the label check they meet at `walfsd` is attacked on its own · tested: bench:steward-vault-session, bench:steward-sub-budget-flood, bench:walfsd-label-check, host:redoubt-steward-server::a_vault_login_carves_from_the_vault_s_sub_budget_and_has_no_network
 
 A **vault session** carries one of its principal's labels: `ssh alice+tax@box` starts a session
 whose budget has Alice's `tax` label. A budget's labels are fixed when it is created and only grow
@@ -146,7 +146,7 @@ flowchart LR
             sy["erofsd:system"]
         end
     end
-    h --> FS["littlefsd, Alice's home volume"]
+    h --> FS["walfsd, Alice's home volume"]
     c --> SH["sshd, this SSH channel"]
     b --> BF["bootfsd, read-only"]
     n --> IP["ipd, a scope of hosts and ports"]
@@ -207,7 +207,7 @@ startup block and resolves paths against it
   `etc/passwd`. A name longer than 255 bytes or holding a NUL, and a path deeper than 64
   components, are refused.
 - A named handle's name is lower-case ASCII letters, digits and `_:+-`, starting with a letter,
-  at most 64 bytes (`littlefsd:data`, `alice+secrets`).
+  at most 64 bytes (`walfsd:data`, `alice+secrets`).
 - A program started with an empty namespace fails cleanly: the echo client exits with its error
   code, and the echo server with `NO_ENDPOINT`, instead of reaching anything.
 
diff --git a/image/README.md b/image/README.md
index 507789587..e7cd0cd6e 100644
--- a/image/README.md
+++ b/image/README.md
@@ -8,7 +8,7 @@ The sources of the signed boot bundle and the disk images; what they produce goe
   the `init-boot` case boots the same bundle. The builder writes the userland volume's root and
   block count into the manifest it packs, from its own pack of `userland.toml`.
 - `manifest.json`: the boot manifest `init` reads, with thirteen servers, including `walfsd:data`
-  for the disk's `data` volume, `littlefsd:alice-secrets` for alice's labelled one, the userland
+  for the disk's `data` volume, `walfsd:alice-secrets` for alice's labelled one, the userland
   disk's `blkd:system`, `verity:system` and `erofsd:system`, the steward and `sshd`. Its principals
   are alice (who owns `alice-secrets` and works under `{}` and `{alice-secrets}`) and bob, with
   their test keys from `tests/keys/`, so it must never ship. Alice's 43,528 pages give each of her
@@ -25,9 +25,10 @@ The sources of the signed boot bundle and the disk images; what they produce goe
 - `disk.toml`: the disk image. `./mkimage` packs it into `target/image/disk.img`: a GPT, then the
   `data` partition as a walfs volume holding `target/image/stage/` (with the principals' homes,
   `home/alice` and `home/bob`, which `./mkimage` makes), written by `libs/walfs` itself
-  ([walfsd](../docs/servers/walfsd.md#the-packer)), and the `alice-secrets` partition holding
-  `target/image/vault/`, written through `littlefsd`'s own code; the `image-disk` case boots a disk
-  packed the same way.
+  ([walfsd](../docs/servers/walfsd.md#the-packer)), and the `alice-secrets` partition, a walfs
+  volume too, holding `target/image/vault/`; the `image-disk` case boots a disk packed the same way.
+  The image holds no littlefs volume; `littlefsd` stays in the bundle for a flash medium and the
+  cases that ask for one.
 - `userland.toml`: the userland disk, attached read-only. `./mkimage` packs it with the same
   packer into `target/image/userland.img`: each module of the applications it names, compiled by
   the pinned toolchain and stripped, as a plain file under its own name (`Elixir.Enum.beam`,
```
