# INIT4 report (init4-implementer-2), tip b0db86111, base 387f1639e (main after RT1 and B7)

The simplifier's review, which overwrote this file once, is in INIT4-simplifier.md.

## Commits
- a3503ba6d net-tcp boots the real init, with the card on its fixed slot
- 993a032c5 slirp offers the guest no IPv6
- aa7f54b7e the peer's self-checks boot the real init
- 10b0afebb net-pinned boots the real init
- 6f8864b5c virtio-probe reads each device's slot and transport version ("1 for legacy" folded in)
- 81e64588a each net case's forbidden addresses are read from its manifest
- cd481def4 the attacks on netd and ipd boot the real init
- 75e81e62c netd-restart, a faulted netd restarted by init (WIP folded; Unsafe and Size budget
  lines; documented as needing the feature build)
- fb38551cf the net rig goes (step 9, and every grant, each named in the body)

## Cases (cargo testbench --arch A CASE, at d3538d345; later commits change docs, test names and the netd-restart judge only)
Old program: the rig binary. New programs: init, keyd, consoled, netd, ipd, net-judge, net-client
(tests/data/net/*.json), except where noted.

| case | old | rv64 | rv32 |
|---|---|---|---|
| net-tcp | net-rig | PASS | PASS |
| bench-net-peer | net-rig-peer | PASS | PASS |
| bench-net-peer-twice | net-rig-twice | PASS | PASS |
| bench-net-peer-count | net-rig-twice | PASS | PASS |
| bench-net-peer-pcap-empty | net-rig-twice | PASS | PASS |
| bench-net-self-unrefused (must fail) | net-rig-unrefused | PASS | PASS |
| net-pinned | net-rig-pinned | PASS | PASS |
| net-attacks | net-rig-attacks | PASS | PASS |
| bench-virtio-legacy-off | net-rig-probe, now virtio-probe as the tester | PASS | PASS |
| bench-virtio-devices | unchanged (virtio-probe) | PASS | rv64 only |
| netd-restart | new | PASS (at b0db86111) | PASS (at b0db86111) |

netd-restart failed before the features piece; it now passes with the bench's own feature build.

## Gates (in-dev, from the worktree), exit 0 at the tip
docs, size-budget, unsafe-budget, ipd-host-tests. At d3538d345: net-host-tests, netd-host-tests,
host-tests (testbench). Also nightly fmt --check on the touched crates, and git diff --check. The
whole bench has not been run.

## Deleted
tests/net/src/rig.rs, build.rs, the seven net-rig*.rs binaries (932 lines). SELF_ARGS goes, and
so do the stub dependency and the client's rig handle and exit path. Branch total: +1860 −1256.

## Grants used (rig commit)
- SECURITY.md:233 bullet removed.
- R26 row: test names renamed.
- R53 row (line 159): residual becomes "blkd is trusted without an IOMMU".
- R70 row (line 161): residual becomes "QEMU's UART neither sticks nor lies: only a fake one on
  the host does".
- netd.rs and ipd.rs module docs.
- ipd tests args.rs and sizing.rs: renamed to every_scope_and_the_milestone_{parse,fit}, with
  their citations in ipd.md and serving.md; RIG/rig become EVERY_SCOPE/every_scope.
- tests/{ipd,netd}-host-tests.toml comments.
- blkd.md:298, bootfsd.md:140 and keyd.md:311: the "does not boot in the bench" bullets are
  deleted whole (the Architect's form, A).
- servers/README.md:320: the residual narrows to fsd (Architect's A).

## Judgement calls
- consoled.md:20 is unchanged: it is still true, since no boot case reads /dev/cons.
- bench:netd-restart went into netd.md's "Started by init" tested list, beside the partly-tested
  prose, and not into R57's list. R57 is about netd's own resets, while a fault's reset is the
  kernel's.
- The seven manifests stay as plain data. A manifest is a [[file]] read verbatim and the JSON has
  no include, so a shared base would need new testbench mechanism.

## Residuals
- The whole bench has not been run.

## Next
On the word, rebase over B7 and RT1, then:
- add `features` to Program::Package and build.rs, with the feature build in its own out dir;
- set netd's features = ["restart-probe"] in netd-restart.toml;
- set netd.md's status back to tested;
- rerun netd-restart on both widths.

## Review notes taken
- Red (1), attacks commit: the labelled caller's claim is now "its connect reaches nothing (its
  peer counts 0)", in both cases' descriptions, the judge and the commit body.
- Red (2), dropped as ruled. There is no check that a call through the old handle is refused,
  because the design keeps that handle alive. netd's endpoint is init's and outlives the
  instance; ipd and the judge reach the new instance through the same handle, and echo's
  connection is ipd's socket, which does not restart. What the old instance held of its own
  dies with it, by the kernel: its DMA pages, its ipd handle and its budget. INIT3's cases prove
  that mechanism.
- Red (3), netd-restart commit: after the second echo the judge asks netd again. The new expect
  line is 'judge ok: netd's instance is unchanged since its restart', so netd restarted once.
  It passes on both widths with the feature forced on locally (Cargo.toml reverted).
- Red (4), netd-restart commit: netd.md gains a "restart probe" paragraph. With the feature,
  netd answers the instance call before checking the caller's badge and labels.
- Architect B: waits for the rebase. netd.md:169, init.md:430 and testbench.md's example line
  change in the netd-restart commit then.

## Rebases and the features piece
- Onto RT1 (57f4cf01f): the only conflict was in the rig commit. RT1 had added panic_handler!
  to the seven deleted net-rig*.rs binaries, so they stay deleted.
- Onto B7 (387f1639e): clean.
- The netd-restart commit now carries:
  - `features` on Program::Package (case.rs, with host test a_package_program_takes_features);
  - build.rs: a build with features copies its binaries to run/cargo-<features>, apart from
    run/cargo. B7's fixture test interleaved_builds_each_pack_their_own_binary also checks that
    one run's plain build and feature build differ;
  - netd-restart.toml: netd has features = ["restart-probe"];
  - Architect B: netd.md:169, init.md "Restarts and reboots" (10), and testbench.md's example.
- netd-restart passes on rv64 and rv32 with no local forcing. The feature build lands in
  run-*/cargo-restart-probe, and the fault line appears.
- Commit hashes in the list above are from before the rebases. The current branch, oldest
  first, is 9 commits ending eeba0e260 (attacks), 5a764fdf0 (netd-restart), b0db86111 (rig).
