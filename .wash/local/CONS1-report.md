# CONS1 report: typed parking, consol size/resize, resize to the shell

Branch wp-CONS1, worktree .worktrees/CONS1, head 6eae76e90 on origin/main 76fd2b9bd. Not pushed.
Design: .wash/local/CONS1-design.md. Tier A (rt, consoled, sshd): the steward red reviews.

## Commits
1. b7e441a45 rt: a typed call that must wait is handed back and parked, as a 9P read is
   (TypedServer::waits, typed::serve_parking/reply; NineServer::serve_parking's own returns
   Option<Request>; 15 callers).
2. 12123a82b consoled, sshd: consol's size and resize, on the console's own endpoint
   (libs/rt server::consol; consoled `size=COLS,ROWS`; sshd per-channel resize count).
   Orchestrator's option B is folded in: with no `size=`, consoled refuses `size` and
   `resize` as malformed, so the console's size is unknown, as the image's is. consoled's
   heap cap is 22 -> 24 in image/manifest.json and the three copies (boot-profile-unverified,
   beamlet-footprint, steward/vault-launch). Measured peaks: 12 pages with CONS1 before B,
   10-11 after.
3. 600204c00 beamlet: Platform::console_resized; the Redoubt resize thread; fixture consol.
4. d1e3e5588 shell: the driver handles {:beamlet_console_resize, {c, r}}; a screen gets {:resize}.
5. 6eae76e90 tests: consol-size and steward-ssh-resize, on both widths (the WIP commit,
   reworded; consol-client.rs is rustfmt-clean).

## Tests and gates (final tree, 6eae76e90)
- Host, through q: `cargo test -p redoubt-rt -p redoubt-consoled -p redoubt-sshd -p redoubt-client
  -p redoubt-init-programs -p redoubt-ipd`: rc 0, 295 passed, 0 failed. In userland/otp,
  `cargo test -p beamlet-redoubt --features fake -p beamlet-vm`: rc 0, 168 passed, 0 failed.
  New: host:redoubt-consoled::a_console_with_no_size_refuses_consol. Without B it would see
  status 0 and a size.
- `cargo test --workspace --no-run` (the rt contract sweep): rc 0. prebuilt built every case's
  pieces on both widths: rc 0 (rv64 239, rv32 225, 0 failed).
- `./test-shell` (whole): rc 0, every stage passed (it includes mix format).
- `make -k -f scripts/jobs.mk set CASES=<59 cases>` (.tmp/CONS1/cases.txt: scripts/shell-cases,
  every steward-* and sshd-*, consol-size, the *-build cases of rt, client, consoled and ipd,
  the host tests of rt, client, ipd and init, rt-miri, size-budget, unsafe-budget, docs and
  formatting): 98 of 99 jobs rc 0. That includes consol-size, steward-ssh-resize,
  userland-boot, userland-read-only and beamlet-footprint on rv64 and rv32, plus size-budget,
  unsafe-budget, docs, formatting and rt-miri.
- The one failure, rv64/steward-model-host-tests: properties::steward_policy "ran past its
  deadline of 35 s". It failed twice, the second time alone, at load average 14-16 from other
  work. CONS1 changes nothing in its dependency closure (model -> libs/steward -> libs/sys and
  libs/sha256; the diff touches none of these), so it is host load, not this package. Rerun it
  on a quiet machine or in the train.
- Footprint (cap 10,989): beamlet 5,462 rv64 / 5,279 rv32 pages.
- Not run: the whole bench, which is the train's.

## Summaries checked
- docs/plan/m2-usable-shell.md: lines 73, 119 and 209 already say what CONS1 builds. No change.
- README.md, GETTING-STARTED.md, docs/userland/README.md and docs/servers/README.md make no
  claim about the console's size or resize. No change.
- consoled.md (consol section, status +1 host test), shell.md (the pager: unknown size on the
  UART, known on SSH; its status says no case pages over SSH; the console's size: the UART
  without size= is unknown), beamlet.md (the console_size row: an unsized consoled refuses),
  sshd.md and serving.md (status lines name the bench cases).

## Open risks
- The pager now takes the screen over SSH, where the size is known. That is host-tested only;
  no machine case pages over SSH (shell.md's status says so).
- consoled refuses the VM's resize for a multiplexed session (in_flight 2 per bucket), by
  design. Only sshd delivers resizes.

## Round 2: rebased onto 7659ce688, red's two P2s folded (head 78367d8ed)
- Rebase onto 7659ce688 (RCMD1, HOME1): no conflicts. consoled's heap_pages is 24 in all four
  manifests, beside HOME1's changes.
- P2 (b): in sshd, `Chan::consol_size()` is None once the session has ended. The slot's serve
  then refuses any consol call as malformed. A resize parked before the end still gets its one
  final answer (resize_due). The VM's next call is refused, so its resize thread ends.
  Test: consol_size_is_the_pty_s_and_a_resize_is_due_when_it_changes now ends the channel with a
  waiter due and asserts the next call is refused (consol_size() == None). sshd.md says so.
- P2 docs: consoled.md "What a waiter costs" now says a multiplexed session holds one InFlight of
  the connection's share. On consoled (share 1) the VM's resize is refused. On sshd the session
  and one waiter fill the share of two, and the VM's reads and writes wait inside its session.
  shell.md's sentence gives that reason; beamlet.md's console_resized row adds sshd's refusal
  after the end.
- Gates on 78367d8ed:
  - cargo test --workspace --no-run rc 0. Host rt/consoled/sshd/client/init-programs 244/0
    rc 0; beamlet-redoubt(fake)+beamlet-vm 168/0 rc 0. ./test-shell rc 0.
  - prebuilt rc 0 (rv64 241, rv32 227 cases, 0 failed).
  - jobs.mk set, 52 cases on both widths (.tmp/CONS1/cases3.txt: the shell-cases beamlet set,
    every steward-*, sshd-* and userland-*, consol-size, size-budget, unsafe-budget, docs,
    formatting, rt/client host tests): 89 of 89 jobs rc 0.
  - rv64/steward-model-host-tests alone: PASS, 29.4 s. Earlier fails were host load.
  - beamlet-footprint 5,462 rv64 / 5,279 rv32 of 10,989.
