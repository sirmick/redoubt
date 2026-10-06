# VOL2 handoff (vol2-implementer), 2026-10-06

## Branch state
- Worktree /home/mcloonan/redoubt/.worktrees/VOL2, branch wp-VOL2, NOT pushed. Rebased onto main bac001578 (the EROFS1 merge).
- Head 7ff5c894c. Commits, oldest first:
  1. a1488d62a verity: root block (libs/verity RootBlock, libs/signing volume domain), plus fuzz target libs/verity/fuzz
  2. 3aef5ae59 init: pinned|signed verity. Plan::bundle_key is threaded into check::args(m, s, bundle_key); "bundle" -> that key in hex
  3. 5d86ed12f build: ed25519-compact opt-level "z" (root Cargo.toml stanza) + keyd stack_pages 3->4 (image/manifest.json, tests/data/boot-profile/manifest-unverified.json, testbench.md row 6,248/4)
  4. 7ff5c894c verityd: signed mode, packer sign/flip_version, cases verity-signed/-bad-signature/-rollback, pages. Now also image verity:system stack_pages 4->5 + testbench.md row 8,264/5 + one sentence; the message notes it.
- The panel (red + editor) is OK with notes, all folded. The orchestrator accepts on the final report.

## Pending
- The fold is DONE in 7ff5c894c (verity:system 5 pages). The rerun is in flight, background task b0jb9n2xb (output /tmp/claude-1447391350/-home-mcloonan-redoubt--worktrees-VOL2/9de2e166-8f2c-42c8-9cac-9811b8054ea2/tasks/b0jb9n2xb.output):
  - init host tests, docs
  - rv64/rv32 userland-boot and init-boot (it prints the stack verity:system and keyd peaks)
  - one more rv64 userland-boot via `jobserver all` (userland-boot pins no seed, so TESTBENCH_QEMU_SEED does nothing; it's just another random run)
- Then report the head and exits to the orchestrator (reply to 4e609d1281997fde02bb9001011e4617 / the accept thread).

## Stack peak question (orchestrator's rule: twice the largest peak over scans; don't tune the layout)
- verity:system rv64 userland-boot on 6818a9447 (signed() inlined into Volume::open): 8,264 B -> FAIL at 4 pages.
- With #[inline(never)] on signed(): 8,088 B (PASS, 1% margin). That change was dropped (no reason of its own); declared 5 pages instead.
- Main's own peak was not measured (the run was stopped). The table had 7,864 (VOL1-era).
- keyd: 6,248 rv64 / 5,760 rv32 of 4 pages.

## Full rerun on 6818a9447 (before the 5-page fold)
- host init+verityd 84 rc0; docs, fmt, size, no-cruft rc0; build-rv64/rv32 rc0
- verity-signed, bad-signature, rollback PASS both widths; init-boot PASS both; rv64 userland-boot FAIL (stack, above).

## Traps
- Env: `. /tmp/vol2-env.sh` (it puts .wash/local on PATH for jobserver). Run cases with `make -k -f /home/mcloonan/redoubt/.wash/local/jobs.mk -C <wt> RUSTSBI_PROTOTYPER=... RUSTSBI_PROTOTYPER_RV32=... BEAMLET_TOOLCHAINS=... rv64/<case>`; logs are in target/jobs/<w>-<case>.log.
- Partial staging: /tmp/vol2-stage.py. Fixups: `git commit --fixup=<sha>` then `GIT_SEQUENCE_EDITOR=: git rebase -i --autosquash bac001578`.
- An init test (the_boot_profiles_unverified_copies...) holds the boot-profile manifest copy equal to the image less the verifiers: change keyd/others in both. verity:system is not in the copy.
- Size budget: servers/init 2142, servers/verityd 604, with Size budget lines in commits 2/4.
- Report detail is in /home/mcloonan/redoubt/.wash/local/VOL2-report.md (append there).
