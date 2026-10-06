# MEM1 final-head gates (mem1-implementer-5, 2026-10-05)

Final head: cfad0b83d84e768c772e504784414ed66d07cb1c, on main 6a7d2385e. Five commits:
aef72d810 client, 0eb0adfee init, 53a051649 testbench, bf9eba303 docs, cfad0b83d image.
`git status` clean. Artefacts are under the worktree's target/mem1-final/ and target/testbench/.
Not pushed.

## Six scans

Run at 80a6662a0, one invocation at a time: `cargo testbench <case> --arch <w>`. Peaks in bytes,
rv64 / rv32, against the declaration now in place.

| server | rv64 | rv32 | needs (2x peak) | declared |
| --- | ---: | ---: | ---: | ---: |
| keyd | 5,264 | 4,744 | 3 | 3 |
| consoled | 9,112 | 7,576 | 5 | 5 |
| bootfsd | 7,304 | 6,168 | 4 | 4 |
| blkd | 4,504 | 3,856 | 3 | 3 |
| netd | 4,280 | 3,424 | 3 | 3 |
| ipd | 8,040 | 6,688 | 4 | 4 |
| fsd:data | 7,176 | 5,600 | 4 | 4 |
| blkd:system | 4,504 | 3,856 | 3 | 3 |
| fsd:system | 12,680 | 9,712 | 7 | 7 (was 6) |
| beamlet | 33,240 | 27,272 | 17 | 17 (was 16) |
| read-only client | 6,616 | 5,392 | 4 | 16 (default) |

The largest peak was rv64's for every server. During boot only, fsd:system peaks at 5,928 /
4,208 and beamlet at 4,608 / 3,704.

Exit codes at 80a6662a0 (declarations 6 / 16):

| scan | rv64 | rv32 |
| --- | --- | --- |
| init-boot | 0 | 0 |
| userland-boot | 1: fsd:system needs 7, beamlet 17, as MEM2 measured | 0 |
| userland-read-only | 1: fsd:system needs 7, beamlet 17, as MEM2 measured | 0 |

Changes folded into the image commit, which gives head cfad0b83d:
- image/manifest.json: fsd:system 7, beamlet 17;
- docs/testbench.md: the table above, and the read-only client's peak 6,616;
- docs/kernel/budgets.md: bound 507 -> 508;
- servers/init/tests/manifest.rs: the bound test counts a 17-page stack;
- the commit message names the pinned compiler and the 508 bound.

Rescans at cfad0b83d: userland-boot rv64 exit 0 and userland-read-only rv64 exit 0 (fsd:system
12,680 of 7, beamlet 33,240 of 17).

## Gates at cfad0b83d, all exit 0

- `cargo +nightly fmt --all -- --check`.
- `cargo testbench host-tests`: 17 cases PASS: blkd, client, fsd, host-tests, init, ipd, littlefs,
  memory, model, net, netd, r4, rt, sshd, steward, stride and wire host tests. The stub's tests
  are in host-tests and rt-host-tests.
- `cargo testbench size-budget`: stub 362/362, libs/client 987/987, servers/init 1957/1957.
- `cargo testbench unsafe-budget`, `no-cruft`, `docs` and `formatting`.

## Whole bench

`cargo testbench`, unfiltered, no --allow-skip, at cfad0b83d: exit 1. 401 PASS, 2 FAIL, 0 SKIP.
1. memory-host-tests: `ssh::tests::the_keeper_finds_what_names_the_case`, left [2467981, 2467983],
   right [2467981]. This is known flake B9 (the keeper's /proc race). Rerun alone once
   (`cargo testbench memory-host-tests`): PASS, exit 0.
2. client-host-tests: `aio::a_server_that_breaks_its_hold_loses_the_session_at_the_margin`
   panicked at libs/client/tests/aio.rs:505 ("gave up at 1.011s", a 1-second timing margin). Not
   on the known-flake list, so not rerun, as instructed. The same case passed in the gate run
   above at the same head. MEM1 does not touch libs/client/src/aio.rs or its test.

MACHINE NOT EXCLUSIVE: after the bench I saw VOL1's QEMU guests running
(`zzvol1-measure-verified-rv64`, from .worktrees/VOL1/target/testbench/run-2546605-...). There were
4 VOL1 qemu processes while the memory-host-tests rerun was going. Load from them during the bench
is the likely cause of failure 2's 1-second margin.

Every scan in the whole bench's own memory cases passed with declarations 7 / 17.

MACHINE RELEASED.

## Host-clock cases rerun by class (jobs.mk, cfad0b83d)

Each case was run through jobs.mk at cfad0b83d: 57 names x 2 widths = 114 targets.
PASS/FAIL is the bench's own line in target/jobs/<w>-<case>.log; class and rc are what make printed.
Counts: 111 PASS, 0 FAIL, 3 n/a. The n/a are rv32 bench-init-reporter-forged, bench-ssh-guest and bench-virtio-devices: those cases are arch = ["rv64"] only, so the rv32 target runs nothing, exits 0 and leaves an empty log.
Classes as printed: alone 44 (+1 of the first 25 whose class line came from the old jobs.mk), bounded 27, net 13, shared 29.
The first 25 targets (rv64, in the pre-refinement exclusive run) each logged the bench's PASS line and nothing else, yet make printed rc=1 (one, init-console-forgery, rc=127): that is the jobserver wrapper while it was being rewritten, not the bench, whose log holds only PASS. Every target after that printed rc=0.
The 7 rv32 sshd-loopback-* targets printed (shared): their class changed in jobs.mk while the run was in progress. They are host cases that ignore the width.

```
rv64 beamlet-lookup-cli-host PASS rc=1 (alone)
rv64 beamlet-lookup-host PASS rc=1 (alone)
rv64 bench-init-reporter-forged PASS rc=1 (alone)
rv64 bench-net-peer PASS rc=1 (alone)
rv64 bench-net-peer-count PASS rc=1 (alone)
rv64 bench-net-peer-pcap-empty PASS rc=1 (alone)
rv64 bench-net-peer-twice PASS rc=1 (alone)
rv64 bench-net-self-unrefused PASS rc=1 (alone)
rv64 bench-ssh-guest PASS rc=1 (alone)
rv64 bench-ssh-loopback PASS rc=1 (alone)
rv64 bench-ssh-loopback-aborted-text PASS rc=1 (alone)
rv64 bench-ssh-loopback-deadlock PASS rc=1 (alone)
rv64 bench-ssh-loopback-exit PASS rc=1 (alone)
rv64 bench-ssh-loopback-forbid PASS rc=1 (alone)
rv64 bench-ssh-loopback-host-key PASS rc=1 (alone)
rv64 bench-ssh-loopback-openssh PASS rc=1 (alone)
rv64 bench-virtio-devices PASS rc=1 (alone)
rv64 bench-virtio-legacy-off PASS rc=1 (alone)
rv64 blkd-host-tests PASS rc=1 (alone)
rv64 client-host-tests PASS rc=1 (alone)
rv64 docs PASS rc=1 (alone)
rv64 fsd-host-tests PASS rc=1 (alone)
rv64 host-tests PASS rc=? (alone*)
rv64 image-disk PASS rc=0 (alone)
rv64 init-boot PASS rc=1 (alone)
rv64 init-console-forgery PASS rc=127 (alone)
rv64 init-host-tests PASS rc=1 (alone)
rv64 init-reboot PASS rc=0 (shared)
rv64 init-servers PASS rc=0 (shared)
rv64 ipd-host-tests PASS rc=0 (bounded)
rv64 littlefs-host-tests PASS rc=0 (bounded)
rv64 memory-host-tests PASS rc=0 (bounded)
rv64 model-host-tests PASS rc=0 (alone)
rv64 net-attacks PASS rc=0 (net)
rv64 net-host-tests PASS rc=0 (bounded)
rv64 net-pinned PASS rc=0 (net)
rv64 net-tcp PASS rc=0 (net)
rv64 netd-host-tests PASS rc=0 (bounded)
rv64 netd-restart PASS rc=0 (net)
rv64 r4-host-tests PASS rc=0 (alone)
rv64 rt-host-tests PASS rc=0 (alone)
rv64 rt-miri PASS rc=0 (alone)
rv64 sshd-host-tests PASS rc=0 (bounded)
rv64 sshd-loopback-env-refused PASS rc=0 (alone)
rv64 sshd-loopback-independent PASS rc=0 (alone)
rv64 sshd-loopback-interrupt PASS rc=0 (alone)
rv64 sshd-loopback-logins PASS rc=0 (alone)
rv64 sshd-loopback-r67 PASS rc=0 (alone)
rv64 sshd-loopback-window-change PASS rc=0 (alone)
rv64 sshd-loopback-window-change-zero PASS rc=0 (alone)
rv64 steward-host-tests PASS rc=0 (bounded)
rv64 stride-host-tests PASS rc=0 (bounded)
rv64 userland-bad-start PASS rc=0 (shared)
rv64 userland-boot PASS rc=0 (shared)
rv64 userland-read-only PASS rc=0 (shared)
rv64 vendor-check PASS rc=0 (bounded)
rv64 wire-host-tests PASS rc=0 (bounded)
rv32 beamlet-lookup-cli-host PASS rc=0 (bounded)
rv32 beamlet-lookup-host PASS rc=0 (bounded)
rv32 bench-init-reporter-forged NA rc=0 (shared)
rv32 bench-net-peer PASS rc=0 (net)
rv32 bench-net-peer-count PASS rc=0 (net)
rv32 bench-net-peer-pcap-empty PASS rc=0 (net)
rv32 bench-net-peer-twice PASS rc=0 (net)
rv32 bench-net-self-unrefused PASS rc=0 (net)
rv32 bench-ssh-guest NA rc=0 (alone)
rv32 bench-ssh-loopback PASS rc=0 (shared)
rv32 bench-ssh-loopback-aborted-text PASS rc=0 (shared)
rv32 bench-ssh-loopback-deadlock PASS rc=0 (alone)
rv32 bench-ssh-loopback-exit PASS rc=0 (shared)
rv32 bench-ssh-loopback-forbid PASS rc=0 (shared)
rv32 bench-ssh-loopback-host-key PASS rc=0 (shared)
rv32 bench-ssh-loopback-openssh PASS rc=0 (shared)
rv32 bench-virtio-devices NA rc=0 (shared)
rv32 bench-virtio-legacy-off PASS rc=0 (shared)
rv32 blkd-host-tests PASS rc=0 (bounded)
rv32 client-host-tests PASS rc=0 (alone)
rv32 docs PASS rc=0 (bounded)
rv32 fsd-host-tests PASS rc=0 (bounded)
rv32 host-tests PASS rc=0 (bounded)
rv32 image-disk PASS rc=0 (shared)
rv32 init-boot PASS rc=0 (shared)
rv32 init-console-forgery PASS rc=0 (shared)
rv32 init-host-tests PASS rc=0 (bounded)
rv32 init-reboot PASS rc=0 (shared)
rv32 init-servers PASS rc=0 (shared)
rv32 ipd-host-tests PASS rc=0 (bounded)
rv32 littlefs-host-tests PASS rc=0 (bounded)
rv32 memory-host-tests PASS rc=0 (bounded)
rv32 model-host-tests PASS rc=0 (alone)
rv32 net-attacks PASS rc=0 (net)
rv32 net-host-tests PASS rc=0 (bounded)
rv32 net-pinned PASS rc=0 (net)
rv32 net-tcp PASS rc=0 (net)
rv32 netd-host-tests PASS rc=0 (bounded)
rv32 netd-restart PASS rc=0 (net)
rv32 r4-host-tests PASS rc=0 (alone)
rv32 rt-host-tests PASS rc=0 (alone)
rv32 rt-miri PASS rc=0 (alone)
rv32 sshd-host-tests PASS rc=0 (bounded)
rv32 sshd-loopback-env-refused PASS rc=0 (shared)
rv32 sshd-loopback-independent PASS rc=0 (shared)
rv32 sshd-loopback-interrupt PASS rc=0 (shared)
rv32 sshd-loopback-logins PASS rc=0 (shared)
rv32 sshd-loopback-r67 PASS rc=0 (shared)
rv32 sshd-loopback-window-change PASS rc=0 (shared)
rv32 sshd-loopback-window-change-zero PASS rc=0 (shared)
rv32 steward-host-tests PASS rc=0 (bounded)
rv32 stride-host-tests PASS rc=0 (bounded)
rv32 userland-bad-start PASS rc=0 (shared)
rv32 userland-boot PASS rc=0 (shared)
rv32 userland-read-only PASS rc=0 (shared)
rv32 vendor-check PASS rc=0 (bounded)
rv32 wire-host-tests PASS rc=0 (bounded)
```

The cfad0b83d gate: the whole run's 401 PASS shared, of which these host-clock cases are superseded by the runs above; the whole run's 2 FAILs (B9 keeper flake, aio 1 s margin) are superseded too, since both cases PASS here (memory-host-tests bounded rc=0; client-host-tests alone rc=1-wrapper, then alone rc=0 on rv32). No failure under the class rule.

## Docs fold of the renewal notes (head 62ce93771)

Rebuilt on the recorded base 6a7d2385e, signed off, five commits:

| commit | subject |
| --- | --- |
| f5e31c7cf | client: paint each launched first-thread stack for measurement |
| cecca9120 | init: preflight each server stack before launch |
| af84564c8 | testbench: measure painted server stacks after boot |
| 3b1eb1e5b | docs: describe declared server stacks and their measurement |
| 62ce93771 | image: size first-thread stacks from six boot workloads |

The first two commits get new IDs only from the rebuild's commit timestamps; range-diff shows
them `=`. My first attempt reset onto `main`, which has moved 5 commits; I redid it on 6a7d2385e.

Applied:
- **Editor 1, init.md:70.** An image that does not fit beside its stack now makes that server's
  launch fail, which refuses the boot, or on a restart reboots the machine. The sentence links
  [restarts and reboots](#restarts-and-reboots).
- **Editor 2, testbench.md.** The dump is "beside the case's log, as
  `<case>-<arch>-smp<N>.ram`"; "in the run's directory" is gone.
- **Editor 3, tests/memory-host-tests.toml.** The description now names the ignored copied-word
  and residue cases and the out-of-range refusal. This change is in the testbench commit.
- **Editor 4, native.md.** The launch-refusal bullet is rewrapped to 100 columns.
- **Red a.**
  - testbench.md now notes that the QMP socket is in the temporary directory under a predictable
    name, created under the bench's umask, so it is private to the bench's user only under umask
    077 or on a single-user host. The note links the new follow-up page.
  - The new page, docs/todo/qmp-socket-private-dir.md, has What, Why it matters, Where and Done
    when: a private 0700 mkdtemp directory per run, with the socket inside it and a host test.
  - The page is listed in SUMMARY.md after the other follow-ups.
- **Red b, servers/README.md:320.** "Until init places it, fsd runs only under test launchers" was
  false already on main, and both the editor and red flagged it. This is a pre-existing truth
  fix. It now reads: "Volumes are kept apart by placement. The image's init runs fsd:data and
  fsd:system, one instance per volume, so one volume's data is out of another's instance only
  because init places each volume once (R47)".

Checks, through the pool:
- `jobserver share cargo testbench docs`: PASS, exit 0.
- `jobserver share cargo +nightly fmt --all -- --check`: exit 0.

`git diff --stat cfad0b83d HEAD`: docs/SUMMARY.md, docs/servers/README.md, docs/servers/init.md,
docs/testbench.md, docs/todo/qmp-socket-private-dir.md (new), docs/userland/native.md and
tests/memory-host-tests.toml. 7 files, +46/-9.

`git diff cfad0b83d HEAD -- . ':!docs' ':!tests/memory-host-tests.toml'` is empty, so every built
artefact's source is byte-identical to the benched head cfad0b83d.

The toml description is a string the bench prints only for `--list` (case.rs `Case.description`)
and never judges, so no case's behaviour changes. The whole-bench and class-rerun evidence carries
to 62ce93771.

## Rebase onto main 995781152

`git rebase --signoff 995781152` from 62ce93771 applied all five commits with no conflict,
testbench.md included: main's paragraph on a shared host and MEM1's memory-budget text sit in
different sections. Merge-base: 995781152. Final head: 431713e19faa8947b6a805d8786b84438d152b04.

| commit | subject |
| --- | --- |
| 51ea6eae6 | client: paint each launched first-thread stack for measurement |
| c68155f87 | init: preflight each server stack before launch |
| 54acbb6eb | testbench: measure painted server stacks after boot |
| 34b6474c6 | docs: describe declared server stacks and their measurement |
| 431713e19 | image: size first-thread stacks from six boot workloads |

- `git range-diff 6a7d2385e..62ce93771 995781152..HEAD`: all five commits are `=`.
- Each commit has one Signed-off-by.
- `git diff cfad0b83d HEAD -- . ':!docs' ':!tests/memory-host-tests.toml'` is EMPTY (0 bytes).
- `jobserver share cargo testbench docs`: PASS, exit 0.
- Tree clean; not pushed.
