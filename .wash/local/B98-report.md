# B98 report: walfsd's mount fails under load, and the box reboots

Branch `wp-B98`, base d66839252 (train 89), head ecaeae89f (round 1: 417d9b226). Tier A, reviewer servers-red. Never pushed.

## Cause (thread B98-cause, ruled)

It is blkd's `REQUEST_TIMEOUT_US` (10 s) missed by an honest, slow virtual disk. Under QEMU's
default cache, a guest's flush is an fdatasync on the image file, and that queues behind the host's
writeback. The case disks live under `$REDOUBT_TMP`, on the root ext4 every build shares. These
cases run without icount, so blkd's 10 s is 10 s of host time.

No byte was wrong: walfsd says `Io: blkd answered Failed`, never `Corrupt`. So it is not a kernel
or driver finding.

Evidence:
- **B95's own logs** (.tmp/B98/notes-b95-logs.md): the failing run took 11.3 s, so the mount took
  about 10 s, and three passing runs were 7-10 s slow.
- **Natural load** (.tmp/B98/load/): 0 failures in 20 runs. Host PSI io peaked at 2.8 %.
- **Injected host I/O pressure** (.tmp/B98/inject/, cause-numbers.md): a pinned
  `dd 2 GB conv=fsync` loop. beamlet-files went 7/10 on each width. Four consoles say
  `blkd: the device is marked broken: a flush: no completion in 10000376-10002228 us (the bound is
  10000000), and nothing since`, and walfsd says `why it does not mount: formatting the blank range:
  Io: blkd answered Failed`. The timeouts fall inside the writer's 11.7 s and 13.9 s fsync passes,
  at PSI 53-78 % with up to 2 GB in writeback.

The Architect ruled on B98-cause:
1. blkd's rule stands: a missed bound is a broken device.
2. The bench owns it: case disks are attached cache=unsafe, and ./launch keeps writeback.
3. FILE_WAIT_US does not move.
No bound moved.

## Delivered (3 commits)

**abdf96713 blkd: say once why the device is marked broken.**
- `Disk` keeps a `Broke`: the request (read or write with its sectors and first sector, or a
  flush) and the error. `DeviceError::Io` now names its `Lie` (`UsedIndex{want,got}`,
  `UsedEntry{id,len}`, `Status(u8)`). A timeout carries the wait by blkd's clock, plus whether the
  used ring had moved when blkd looked again.
- The bin says `blkd: the device is marked broken: <request>: <why>` once, after the reply, and on
  the `NO_PARTITIONS` exit.
- New test `a_broken_device_says_once_what_broke_it`.
- Size budget servers/blkd 1656→1725, with its `Size budget:` line.

**ebb9720f0 walfsd: say why a volume does not mount.**
- `Mounted::Corrupt(Unmounted{step, error, why})`. The step is one of Blank, BlankReadOnly,
  Format, Mount or Count. The range's reason is kept only when the error is Io.
- After the corrupt line it says `walfsd: why it does not mount: <step>: <error>[: <reason>]`.
- New test `a_mount_that_fails_says_why`; the noise test now asserts `mounting: Corrupt`.
- walfsd-corrupt-volume expects the reason line and forbids blkd's broken line.
- Size budget servers/walfsd 856→901, with its line.

**417d9b226 testbench: a case's disks cost the host no fsync.**
- `virtio_devices` takes a `Flush`: `Free` (cache=unsafe, the bench, main.rs) or `Host` (QEMU's
  default, ./launch). The read-only userland disk is unchanged.
- New test `a_cases_disks_cost_the_host_no_fsync_and_launchs_keep_the_default`; the vault test's
  expected drive string is updated.
- testbench.md's disk section states the setting and the numbers. blkd.md has a new residual: on a
  virtualised disk the bound measures the host too.
- beamlet-files forbids `blkd: the device is marked broken`.

**Console-line contracts.** `tests/*.toml` was grepped before any line was added:
- **walfsd's corrupt line:** unchanged. The reason is a new line after it, and it does not start
  `walfsd: the volume does not mount`, which walfsd-flipped-block forbids.
- **blkd's new line:** forbidden by walfsd-corrupt-volume and beamlet-files.
- **walfsd's reason line:** expected by walfsd-corrupt-volume.

## Gates (rc)

Host tests on the head 417d9b226, `q run --cores 4 -- cargo test -p redoubt-blkd -p redoubt-walfsd
-p testbench`: rc=0, 322 passed, 0 failed.

`jobs.mk set CASES="docs formatting size-budget unsafe-budget no-cruft"` on the head: rc=0, all
PASS. Before the rebase, size-budget failed until each ceiling was raised in its own commit.

Both builds: `jobs.mk prebuilt` at 417d9b226 succeeded (rc=0) for rv64 and rv32, every case's
binaries. The unsafe count is unchanged (no unsafe added; unsafe-budget passes).

All of the following on 417d9b226 unless stated:

| Run | Result |
| --- | --- |
| walfsd-corrupt-volume and beamlet-files, both widths (pre-rebase 7526ba6ae) | rc=0 |
| The Architect's gate, window 1, beside the pinned writer | rc=0 on every run: beamlet-files 20/20, walfsd-power-loss 20/20, rv64 launch-keep-disk 10/10, steward-home-quota 20/20. No blkd or walfsd line. |
| Smoke set (window 2): userland-boot, init-boot, bench-net-peer, ipc-outcomes, sum-clear, lend-untouched-page (also at 4 harts), both widths | 14/14 PASS, rc=0 |
| Disk subset (window 2): 42 cases (names starting walfsd-, littlefsd-, erofs- or verity-, plus image-disk, bench-virtio-devices, blkd-fairness, launch-keep-disk, dma-reset-reuse, init-reboot, reboot-verifies-bundle, userland-boot and userland-read-only), both widths | 78 boots PASS plus the host-test cases, rc=0 on all 79 jobs. The only reason line was walfsd-corrupt-volume's expected `mounting: Corrupt`. |

The other ~100 disk-carrying cases are left to the train's whole bench, by agreement with the
orchestrator. Logs are in .tmp/B98/window/{nocache,cache,smoke,subset}/.

## steward-home-quota after B95 (the Architect's verification)

On main-with-B95 without cache=unsafe (ebb9720f0), beside the writer, it failed 20/20 at 610 s.
The fill ends `{:wrote, {N, {:error, :etimedout}}}` with N from 0 to 1792 KiB.
- **17/20:** no eio afterwards. stat and read succeed, so B95 holds.
- **3/20:** blkd broke on a flush, and walfsd said `volume poisoned: open: blkd answered Failed`;
  the eio follows from that poisoning.
- **0/20:** eio without a poisoned line. No client-side node is needed.

With cache=unsafe it passed 20/20.

## Summaries checked

- **docs/servers/blkd.md, "Device lies" and the residuals:** updated (the line; the
  virtualised-disk residual).
- **docs/servers/walfsd.md, "Failure and restart":** updated (the reason line and its test).
- **docs/testbench.md, "Disks and network cards":** updated (cache=unsafe, why, the numbers, and
  ./launch undecided).
- **docs/SECURITY.md:** no change. R51-R53 claim the DMA region, a lie as a failure, and
  partitions; none of those changes.
- **README.md, GETTING-STARTED.md, the milestone progress:** no change. This adds diagnostics and a
  bench setting to built features, with no status claim. A grep of docs for cache settings found
  only the card host's own FLUSH = fdatasync (docs/beyond/card-host.md), which is a different
  backend.

## Not done, and risks

- **File reading:** I wrote or read in full disk.rs, virtio.rs, both bins and volume.rs. Of
  queue.rs, blkd's server.rs, device.rs, regressions.rs, walfsd's server.rs and server_tests.rs,
  qemu.rs, main.rs and launch.rs, and the three pages, I read the changed regions and their
  surroundings, not the whole files: about 11K lines in total.
- **The bound itself:** on real hardware, a flush over 10 s still breaks blkd for good, by the
  ruling, and FILE_WAIT_US still gives a program etimedout under host writeback. That is ruled to
  stay.
- **./launch:** how it should flush is left undecided, and testbench.md says so.
- **Scratch worktrees:** .tmp/B98/wt and .tmp/B98/wt-nocache are git worktrees, kept for review;
  remove them with `git worktree remove` when the package merges.

## Fold round 1 (servers-red P1, the Architect's ruling (a), and the P3s)

The head is ecaeae89f; the range-diff is in .wash/local/B98-range-diff-r1.txt. Commit 1 changed,
commit 2 is unchanged, and commit 3 changed.

- **Launch cases now get cache=unsafe too.** `launch::command` takes a `qemu::Flush`: the bench
  (main.rs) passes Free and ./launch passes Host.
  - Host test `a_launch_case_differs_from_launch_only_in_its_disks_cache`: the two command lines
    differ only in the two writable drives' `,cache=unsafe`.
  - testbench.md states the one difference where it describes `launch = true`, launch-system and
    the idle measure, and in the disks section. Each of launch-keep-disk, launch-system and
    launch-idle says it in one comment.
- **Flood bounds:** blkd-fairness, walfsd-flood-turn and vault-flood-turn each say their 4,000 ms
  came from worsts measured under writeback, to be re-measured at the next train, unmoved until
  then.
- **Broke.waited's span:** `Queue::waited_from` is an `Option`, set when a wait begins and cleared
  when a completion is accepted. `break_on` uses it only for a Timeout, with a `debug_assert` that
  a timeout has a wait. Size budget servers/blkd is now 1729 (+4), and its commit's line is
  updated.

### Gates on ecaeae89f

| Gate | Result |
| --- | --- |
| docs, formatting, size-budget, unsafe-budget, no-cruft | rc=0 |
| Host tests, blkd/walfsd/testbench | 323 passed |
| prebuilt, rv64+rv32 | rc=0 |
| Beside the writer (PSI up to 82 %): launch-keep-disk | 20/20 |
| Beside the writer: launch-system | 20/20 |
| Beside the writer: launch-idle | 12/20; the 8 failures are all interrupts_per_hart (3.83-5.31/s against the 3.8 ceiling) |
| launch-idle alone | 6/6; its ceilings held |

No blkd or walfsd line appeared. The Architect ruled launch-idle's misses beside the writer no
verdict: it is a host-clock case and passed every alone run, the idle window does no disk I/O, and
no control run is needed. The interrupt rate rising with host load was added to B101. servers-red
renewed OK on ecaeae89f, conditional on these runs. Logs are in
.tmp/B98/window/{launch,idlealone}/.
