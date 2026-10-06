# B19 report: a bench case starts in a second; timer-driven boots move to icount

Branch wp-B19, head cc6f3f3f0, on main 86c76a9f0 (rebased; clean). 8 commits:

- f1de95863 testbench: a run stages each userland disk in a directory named for its recipe's whole path
- 59c695a97 testbench: --prebuild builds every case's pieces once; --prebuilt runs a case from them with no cargo
- e78f52ae3 scripts: jobs.mk runs each case by its exact name, from target/prebuilt when it is there
- 16a2ae882 / fef9161f1 / e564b34a7 / 784dc91b6 tests: 98 cases to icount, in four groups (23 + 24 + 16 + 35)
- cc6f3f3f0 docs: which boot cases run in guest time, how their deadlines are set, and jobs.mk's prebuilt path

Paths: tools/testbench/src/{main.rs, prebuilt.rs (new), build.rs, ssh.rs}, scripts/jobs.mk,
docs/testbench.md, 98 tests/*.toml.

## 1. Measured start (before any change; temporary trace, not committed)

ms from `q run` to each stage, warm build cache:

| stage | ipc-outcomes alone | 16 at once (same case) | userland-boot alone |
| --- | --- | --- | --- |
| lease | 38 | 34-50 | - |
| bench main (cargo run's lock + check) | 88 | 260-420 | +50 |
| cases loaded / QEMU probe | 123 / 130 | 297-470 / 304-476 | |
| cargo: kernel, loader, programs | 27 + 28 + 26 | 93-281 + 172-184 + 78-125 | 13 builds, 1.37 s (init's 1.0 s) |
| userland stage + pack, manifest pin | - | - | 1.13 s |
| bundle | 19 | 19 | 0.13 s |
| QEMU spawn / first console line | 232 / 263 | 714-981 / 746-1013 | 2.83 / 2.87 s |

Cold (fresh build dir) userland-boot: 62 s before QEMU, beamlet-redoubt 37.5 s of it.
Train 4 (q log `ran` minus the bench's reported time, rv64, 192 boot cases): median 2.3 s,
p10 0.5, p90 28.5; the beamlet cases ~86 s each (all waiting on one cold beamlet build behind
cargo's build-dir lock), kernel-containment 356 s. Also found: a jobs.mk case target passed a
substring filter, so rv64/budget ran 17 results, rv64/timeouts also timeouts-tcg, rv64/ipc the
ipc-* cases.

## 2. Build once

- `cargo testbench --prebuild DIR [--arch W] [filter]`: every boot and build case of each width
  packed into DIR/<arch>/ (cargo's copies, bundles, userland disks) with index.json, the bench
  binary copied to DIR/testbench and Redoubt's host sshd to DIR/redoubt-sshd-host. Widths in
  turn in one process (two userland stages at once race `mix compile` in the shared _build: seen
  as a 4-module stage). Each width built aside and renamed into place. A failed build is kept as
  the case's result. Warm: ~45-50 s a width alone, ~100 s beside load; cold both: 270 s.
- `DIR/testbench --prebuilt DIR ...`: boot/build cases take their pieces from DIR, no cargo, no
  packing. The index holds a fingerprint of the tree (HEAD, `git diff HEAD --binary`, untracked
  non-ignored files' names and bytes, 20 ms); a mismatch is refused, loudly (it voided one of my
  own runs when I committed mid-run: every later case "built from another tree").
- `--exact`: the filter is a whole case name; refused if none.
- jobs.mk: `prebuilt` target (one 8-core job); case targets use `--exact`, and the prebuilt
  binary when target/prebuilt/<w>/index.json exists, else `cargo testbench`.
- Bug fixed on the way (with its test): userland disks were staged by recipe file stem, and three
  recipes are named userland.toml; a run staging two booted the wrong disk. Pre-existing in any
  multi-case run.

## 3. icount audit

158 boot cases had no icount. 98 moved to `shift=3,sleep=off` (passed both widths in 5 rounds
before the commits, then again in the final gate); 60 stay, each with a reason. Now 134 of 194
boot cases carry icount (131 sleep=off, 2 shift=3 disk cases, 1 deliberate bad option).
Deadlines of moved cases: max(10, ceil10(4 x slowest pass on either width)), only ever lowered;
three kept their deadline because their comments require it (dma-destroy-quarantine 120,
dma-reset-quarantine 90: above the program's own waits; map-fixed-tables 120: a regression's
minute of mapping). Rule stated once on the page, with "never raised without a measurement".
In-guest waits: for moved cases they are guest time and cost nothing idle; for unmoved cases
noted in the table (aio-many-reads-two's 20 s receive) and left.

Not moved, by reason: a disk or userland disk 38; host sockets 8; host-typed [[input]] 4
(uart-irq rv32 lost a 'pq' burst under icount); spinning harts 3 (all-together smp=2 36-42 s vs
3 s, ipc smp=4 8-10 s vs 2.5 s, smp-boot smp=4 "runner 4 counted 0" under icount while it passes
on main without); host-time twins 4 (asid-cost-host, sched-latency-tcg, timeouts-tcg,
smp-evict-mttcg); residuals 3: redoubt-ipc fails under icount both widths ("abandoned = 189, want
256"; passes on main without) - follow-up; sum-clear and lend-untouched-page smp=4 fail on main
WITHOUT icount too (train 4 and alone here), so they move once fixed.

## 4. Docs

docs/testbench.md: "How to use it" (--exact, --prebuild, --prebuilt), a "Building once" section
(status: built, 4 host tests named), "Which cases run in guest time" (the rule, counts, reasons,
residuals, the deadline rule), the shared-host paragraph ("60 of them"), the jobs.mk paragraph
(prebuilt, exact). Summaries checked: README.md (no bench internals: no change), GETTING-STARTED.md
(names `q` and `jobs.mk rv64/<case>` only, still right: no change), docs/kernel/*.md and
docs/userland/beamlet.md mentions of virtual time (existing icount cases only: no change), scripts/jobs.mk
header (updated). CONTRIBUTING.md: no change.

## Gates (head cc6f3f3f0 unless said)

- `q run --cores 4 -- cargo test -p testbench`: rc 0, 141 passed (on 83d3f342c: the same code, before a docs-only change).
- `make prebuilt`: rc 0, 205 rv64 + 191 rv32 cases, 0 failed builds.
- docs, no-cruft, formatting, size-budget, unsafe-budget (jobs.mk, on cc6f3f3f0): all rc 0.
- Moved cases both widths: 5 rounds pre-commit all pass; final gate (light3, on cc6f3f3f0) 196 of
  196 targets rc 0. rv32/return-lent-unmapped smp=4 ran out of its 10 s deadline once beside a
  full machine (light2), passed alone on the quiet cores (0.4 s) and in light3: deadline-only, per
  the page's rule.
- Smoke set from target/prebuilt, both widths: userland-boot, init-boot, bench-net-peer,
  ipc-outcomes: rc 0.
- Light set (light3, cases-rv64 + cases-rv32 minus model-host-tests and worst-walk, which the
  orchestrator excluded; machine near idle): 484 targets; everything but kernel-containment done
  in 3 min 39 s (15:25:01-15:28:40, `q log`); kernel-containment (heavy tail) PASS rv64 865.9 s, rv32 737.8 s; whole run 887 s, make rc 2 from the main failures below.
- Light case wall vs reported (light3, 356 single-boot boot cases): median 0.1 s, p90 0.2 s,
  max 1.2 s; 353 of 356 within 1 s.
- Failures in the gate, all main's, rerun alone on the quiet cores and still failing, none
  touched by B19: aio-many-reads-two (both), sched-exit-churn (both), sched-timer-flood (both),
  sched-carve-return rv32, sched-cluster-old-control rv32, sum-clear (both), lend-untouched-page
  smp=4 (both) - all failed in train 4 too; sched-share: NEW on main 86c76a9f0 (weight 300 got
  549 of 1000; in light3 rv64 passed and rv32 failed), reproduced on a clean `git archive main` copy; passed in train 4.
- Not run: model-host-tests and worst-walk (killed on the orchestrator's instruction: the old
  serial case B18 rewrites; K24's livelock). Quiet set (host-clock cases) not run: B19 does not
  touch them.

## Risks and follow-ups

- A run keeps the 8 newest run directories: a busy make prunes failed cases' console logs before
  anyone reads them (pre-existing; e.g. keep runs with failures).
- jobs.mk runs host-tests cases under both rv64/ and rv32/: each runs twice (B18's kind).
- A disk recipe is still packed per boot (~10 cases); could be packed once in the prebuild.
- Concurrent userland stages of a fresh tree race in the Mix project's _build (pre-existing for
  parallel `cargo testbench` runs; the prebuild avoids it by doing widths in turn).
- redoubt-ipc under icount, and sum-clear's instruction-fault report under icount: kernel-side looks.

## Audit table (every boot case without icount before B19)

| case | measures | outcome |
| --- | --- | --- |
| aio-many-reads-two | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) ; its reader's 20 s receive timeout is an in-guest bound, left as it is (fails on main at 20.7 s) |
| aio-many-reads | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| all-together | see outcome | not moved: passes, but smp=2 takes 36-42 s under icount against 3 s (harts round-robin on one host thread, spinning on the kernel lock) |
| asid-cost-host | see outcome | not moved: host time is what it records (the twin of asid-cost) |
| asid-reuse-stale | guest time or nothing | moved; slowest pass 17.5 s; timeout_secs 120 -> 70 |
| beamlet-boot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| beamlet-budget-flood | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| beamlet-console | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| beamlet-footprint | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| beamlet-heap-flood | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| bench-attack-forgery | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| bench-bundle-file | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| bench-cbo-self-unrefused | guest time or nothing | moved; slowest pass 0.8 s; timeout_secs 30 -> 10 |
| bench-console-after-expect | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 60 -> 10 |
| bench-debug-assertions-off | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 60 -> 10 |
| bench-debug-assertions | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 60 -> 10 |
| bench-init-reporter-forged | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| bench-net-peer-count | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| bench-net-peer-pcap-empty | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| bench-net-peer-twice | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| bench-net-self-unrefused | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| bench-poweroff-missing | guest time or nothing | moved; slowest pass 3.0 s; timeout_secs 3 -> 3 |
| bench-reporter-mismatch | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| bench-ssh-guest | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| bench-virtio-devices | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| bench-virtio-legacy-off | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 60 -> 10 |
| boot-stack-reservation | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| budget-carve-attack | guest time or nothing | moved; slowest pass 0.3 s; timeout_secs 30 -> 10 |
| budget-destroy-attack | guest time or nothing | moved; slowest pass 1.1 s; timeout_secs 30 -> 10 |
| budget-destroy-kills | guest time or nothing | moved; slowest pass 1.1 s; timeout_secs 30 -> 10 |
| budget-forge-attack | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| budget-mem-churn | guest time or nothing | moved; slowest pass 1.0 s; timeout_secs 30 -> 10 |
| budget-syscall-attack | guest time or nothing | moved; slowest pass 0.9 s; timeout_secs 30 -> 10 |
| budget-table-attack | guest time or nothing | moved; slowest pass 2.0 s; timeout_secs 30 -> 10 |
| budget | guest time or nothing | moved; slowest pass 4.4 s; timeout_secs 30 -> 20 |
| bundle-mapped | guest time or nothing | moved; slowest pass 0.9 s; timeout_secs 30 -> 10 |
| cbo-user-fault | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| destroy-keeps-notices-creator | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 60 -> 10 |
| destroy-keeps-notices | guest time or nothing | moved; slowest pass 0.8 s; timeout_secs 60 -> 10 |
| device-exec-refused | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 30 -> 10 |
| device-info-attack | guest time or nothing | moved; slowest pass 1.4 s; timeout_secs 30 -> 10 |
| device | guest time or nothing | moved; slowest pass 1.5 s; timeout_secs 30 -> 10 |
| dma-destroy-quarantine | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 120 -> 120 |
| dma-reset-quarantine | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 90 -> 90 |
| dma-reset-reuse | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| dma-rules | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| ending-pumps-once | guest time or nothing | moved; slowest pass 2.0 s; timeout_secs 60 -> 10 |
| endpoint-destroy-open-calls | guest time or nothing | moved; slowest pass 1.1 s; timeout_secs 60 -> 10 |
| erofs-corrupt | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| erofs-read-only | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| handle-chain-attack | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 60 -> 10 |
| handle-chain-fault | guest time or nothing | moved; slowest pass 1.4 s; timeout_secs 60 -> 10 |
| heap-cap | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| image-disk | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| init-boot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| init-console-forgery | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| init-driver-restart | guest time or nothing | moved; slowest pass 2.8 s; timeout_secs 60 -> 20 |
| init-handed-revoked | guest time or nothing | moved; slowest pass 2.8 s; timeout_secs 60 -> 20 |
| init-quarantine-reboot | guest time or nothing | moved; slowest pass 2.8 s; timeout_secs 60 -> 20 |
| init-reboot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| init-refuses-bound | guest time or nothing | moved; slowest pass 1.1 s; timeout_secs 30 -> 10 |
| init-refuses-budget-handle | guest time or nothing | moved; slowest pass 0.3 s; timeout_secs 30 -> 10 |
| init-refuses-confined-server | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-consoled-handed | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-device-dma | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-device-unmatched | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-held-bundle-key | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-held-login-key | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-public-manifest | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 30 -> 10 |
| init-refuses-second-keyd | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-stack | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-refuses-system-fit | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| init-restart | guest time or nothing | moved; slowest pass 2.8 s; timeout_secs 60 -> 20 |
| init-rollback | guest time or nothing | moved; slowest pass 3.3 s; timeout_secs 60 -> 20 |
| init-servers | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| ipc-outcomes | guest time or nothing | moved; slowest pass 3.3 s; timeout_secs 60 -> 20 |
| ipc | see outcome | not moved: passes, but smp=4 takes 8-10 s under icount against 2.5 s (same cause) |
| irq-attack | see outcome | not moved: host-typed [[input]] on the UART (uart-irq lost a burst under icount on rv32) |
| irq-first-receive | guest time or nothing | moved; slowest pass 0.3 s; timeout_secs 120 -> 10 |
| kernel-half-attack | guest time or nothing | moved; slowest pass 0.8 s; timeout_secs 60 -> 10 |
| kernel-wx | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 60 -> 10 |
| launcher-orphan | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 60 -> 10 |
| legacy-gone | see outcome | not moved: host-typed [[input]] on the UART |
| lend-untouched-page | see outcome | not moved: smp=4 fails "4 started, 1 ran user code" under icount and on main without it (train 4, and alone here); moves once it passes |
| lender-touches-lent | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 60 -> 10 |
| littlefsd-boot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-confined-labelled | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-corrupt-volume | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-label-check | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-large-directory | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-one-volume | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-quota | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-reboot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| littlefsd-restart | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| loader-rejects-kernel-address | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 20 -> 10 |
| loader-rejects-kernel-entry | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 20 -> 10 |
| loader-rejects-truncated-elf | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| logsrv-badge-forgery | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| map-anon-placement | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| map-fixed-attack | guest time or nothing | moved; slowest pass 1.8 s; timeout_secs 30 -> 10 |
| map-fixed-tables-rv32 | guest time or nothing | moved; slowest pass 2.1 s; timeout_secs 60 -> 10 |
| map-fixed-tables | guest time or nothing | moved; slowest pass 3.2 s; timeout_secs 120 -> 120 |
| mem-attack | guest time or nothing | moved; slowest pass 0.3 s; timeout_secs 60 -> 10 |
| move-borrowed-page | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| net-attacks | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| net-tcp | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| netd-restart | host clock | not moved: host sockets (forward/peer/dial/poke), wall-time dials |
| ninep-newconn-discard | guest time or nothing | moved; slowest pass 1.2 s; timeout_secs 60 -> 10 |
| pack-bad-truncated | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| pack-bad-wrong-length | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| pack-bad-wrong-name | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| pack-outside-module | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| page-table-reclaim | guest time or nothing | moved; slowest pass 7.5 s; timeout_secs 60 -> 30 |
| pages-exhaustion | guest time or nothing | moved; slowest pass 0.9 s; timeout_secs 60 -> 10 |
| panic-in-print | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 20 -> 10 |
| pid-pinning-attack | guest time or nothing | moved; slowest pass 0.7 s; timeout_secs 60 -> 10 |
| pid-reuse-authority | see outcome | not moved: host-typed [[input]] on the UART |
| process-attack | guest time or nothing | moved; slowest pass 1.7 s; timeout_secs 60 -> 10 |
| process-chain-fault | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 60 -> 10 |
| process-fill | guest time or nothing | moved; slowest pass 9.0 s; timeout_secs 120 -> 40 |
| process-lifecycle | guest time or nothing | moved; slowest pass 4.9 s; timeout_secs 60 -> 20 |
| process-map-untouched-attack | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| process-review | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 30 -> 10 |
| process | guest time or nothing | moved; slowest pass 1.9 s; timeout_secs 60 -> 10 |
| programs-unknown-budget-attack | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| programs-unknown-program-attack | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| receive-bad-record | guest time or nothing | moved; slowest pass 1.8 s; timeout_secs 60 -> 10 |
| redoubt-dead | guest time or nothing | moved; slowest pass 2.6 s; timeout_secs 30 -> 20 |
| redoubt-ipc-attack | guest time or nothing | moved; slowest pass 3.3 s; timeout_secs 60 -> 20 |
| redoubt-ipc | see outcome | not moved: fails under icount both widths: "[ipc] FAIL: abandoned = 189 (rv32 242), want 256"; follow-up |
| redoubt-revoke | guest time or nothing | moved; slowest pass 2.6 s; timeout_secs 30 -> 20 |
| redoubt-tight | guest time or nothing | moved; slowest pass 1.7 s; timeout_secs 30 -> 10 |
| return-lent-unmapped | guest time or nothing | moved; slowest pass 0.5 s; timeout_secs 30 -> 10 |
| rng | guest time or nothing | moved; slowest pass 0.8 s; timeout_secs 60 -> 10 |
| rustsbi-boot | guest time or nothing | moved; slowest pass 1.7 s; timeout_secs 60 -> 10 |
| sched-latency-tcg | see outcome | not moved: reference run in real time (twin of sched-latency) |
| smp-boot | see outcome | not moved: smp=4 fails under icount: "runner 4 counted 0" (round-robin harts) |
| smp-evict-mttcg | see outcome | not moved: needs multi-threaded TCG, which icount excludes (twin of smp-evict) |
| stub-launch | guest time or nothing | moved; slowest pass 1.0 s; timeout_secs 60 -> 10 |
| sum-clear | see outcome | not moved: fails on main without icount too (train 4, and alone here: no KERNEL FAILURE line; under icount the load reports as an instruction fault of PID 2); moves once it passes |
| syscall-attack | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| thread-limit | guest time or nothing | moved; slowest pass 2.6 s; timeout_secs 60 -> 20 |
| timeouts-tcg | see outcome | not moved: reference run in real time (twin of timeouts) |
| touch-beyond-ram | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| uaf-lent-page | guest time or nothing | moved; slowest pass 0.2 s; timeout_secs 30 -> 10 |
| uart-irq | see outcome | not moved: host-typed [[input]]: under icount rv32 never received 'pq' (timed out) |
| userland-bad-start | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| userland-boot | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| userland-read-only | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| verified-boot-rejects-bare-archive | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| verified-boot-rejects-tamper | guest time or nothing | moved; slowest pass 0.1 s; timeout_secs 20 -> 10 |
| verity-bad-signature | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| verity-flipped-tree | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| verity-rollback | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| verity-signed | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| verity-wrong-root | host device | not moved: a disk (page rule: blkd waits on the host virtio completion; sleep=off poisons the volume) |
| write-only-attack | guest time or nothing | moved; slowest pass 1.5 s; timeout_secs 30 -> 10 |
| wx | guest time or nothing | moved; slowest pass 1.6 s; timeout_secs 60 -> 10 |
